//! File matching and indexing for `ProPresenter` library files.
//!
//! The index is rebuilt once at runtime startup so an invalid cache cannot turn
//! a library read failure into a misleading "not found" result.

// Allow unwrap for compile-time constant regex patterns in lazy_static blocks
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::time::Instant;

use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use rayon::prelude::*;
use regex::Regex;
use serde::Serialize;
use uuid::Uuid;
use walkdir::WalkDir;

use crate::error::{Error, Result};
use crate::propresenter::deserialize::{detect_presentation_file_format, PresentationFileFormat};
use crate::propresenter::generated::rv_data;
use crate::propresenter::resolution::inspect_presentation_size;
use crate::propresenter::PresentationSizeStatus;
use prost::Message;

fn is_pro_path(path: &Path) -> bool {
    path.extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pro"))
}

/// A file entry representing a `ProPresenter` file
#[derive(Debug, Clone, Serialize)]
pub struct FileEntry {
    /// Original file name without extension
    pub file_name: String,
    /// Name after stripping prefixes/numbering
    pub normalized_name: String,
    /// Lowercase variant of `file_name` (not serialized)
    #[serde(skip)]
    pub file_name_lower: String,
    /// Lowercase variant of `normalized_name` (not serialized)
    #[serde(skip)]
    pub normalized_lower: String,
    /// Human-readable display name
    pub display_name: String,
    /// Path relative to the library root
    pub relative_path: String,
    /// Absolute path on disk
    pub full_path: PathBuf,
    /// Named native arrangements available for playlist selection.
    pub arrangements: Vec<IndexedArrangement>,
    /// Uniformity and dimensions of native presentation slides.
    pub presentation_size: PresentationSizeStatus,
}

/// Native arrangement metadata needed before a playlist build is approved.
///
/// A complete arrangement has a nonempty native name and a parseable UUID.
/// Incomplete entries remain visible so classification can require review
/// instead of treating corrupt metadata as though the arrangement did not
/// exist.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum IndexedArrangement {
    /// Arrangement can be selected by its exact native name.
    Complete {
        /// Exact native arrangement name.
        name: String,
    },
    /// Arrangement has an empty name or a missing/malformed UUID.
    Incomplete {
        /// Exact native arrangement name, which may be empty when the entry is malformed.
        name: String,
    },
}

impl IndexedArrangement {
    /// Exact arrangement name stored in the native presentation.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Complete { name } | Self::Incomplete { name } => name,
        }
    }

    /// Whether the native arrangement carries the identity required by a
    /// playlist selection.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        matches!(self, Self::Complete { .. })
    }
}

/// In-memory index of `ProPresenter` files.
pub struct FileIndex {
    /// All indexed files
    pub entries: Vec<FileEntry>,
    /// Library root used for relative paths of newly generated files.
    library_path: PathBuf,
}

impl FileIndex {
    /// Return the indexed metadata for one exact library path.
    #[must_use]
    pub fn entry_at(&self, path: &Path) -> Option<&FileEntry> {
        self.entries.iter().find(|entry| entry.full_path == path)
    }

    /// Build or load a file index for the given library path
    pub fn build(library_path: &Path) -> Result<Self> {
        if !library_path.is_dir() {
            return Err(Error::Library(format!(
                "Library path does not exist or is not a directory: {}",
                library_path.display()
            )));
        }

        let start = Instant::now();
        let mut entries = Vec::new();
        for entry in WalkDir::new(library_path).follow_links(false) {
            let entry = entry.map_err(|error| {
                Error::Library(format!(
                    "Failed to traverse ProPresenter library {}: {error}",
                    library_path.display()
                ))
            })?;
            if !entry.file_type().is_file() || !is_pro_path(entry.path()) {
                continue;
            }

            let path = entry.path();
            let data = std::fs::read(path).map_err(|error| {
                Error::Library(format!(
                    "Failed to read ProPresenter file {}: {error}",
                    path.display()
                ))
            })?;
            let format = detect_presentation_file_format(&data);
            if format != PresentationFileFormat::NativePresentation {
                tracing::warn!(
                    path = %path.display(),
                    %format,
                    "excluding unsupported .pro file from library index"
                );
                continue;
            }
            let metadata = decode_presentation_metadata(&data, path)?;

            let stem = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .ok_or_else(|| {
                    Error::Library(format!(
                        "ProPresenter filename is not valid UTF-8: {}",
                        path.display()
                    ))
                })?;
            let normalized = normalize_name(stem);
            let relative_path = path
                .strip_prefix(library_path)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string();
            entries.push(FileEntry {
                file_name: stem.to_string(),
                normalized_name: normalized.clone(),
                file_name_lower: stem.to_lowercase(),
                normalized_lower: normalized.to_lowercase(),
                display_name: stem.to_string(),
                relative_path,
                full_path: path.to_path_buf(),
                arrangements: metadata.arrangements,
                presentation_size: metadata.presentation_size,
            });
        }

        let count = entries.len();
        let elapsed = start.elapsed();
        tracing::info!("Indexed {count} files in {elapsed:?}");

        Ok(Self {
            entries,
            library_path: library_path.to_path_buf(),
        })
    }

    /// Add a newly exported file to the index, skipping duplicates.
    pub fn add_entry(&mut self, full_path: &Path) {
        // Dedup: skip if already indexed
        if self.entries.iter().any(|e| e.full_path == full_path) {
            return;
        }

        let data = match std::fs::read(full_path) {
            Ok(data) => data,
            Err(error) => {
                tracing::warn!(
                    path = %full_path.display(),
                    %error,
                    "not adding unreadable .pro file to library index"
                );
                return;
            }
        };
        let format = detect_presentation_file_format(&data);
        if format != PresentationFileFormat::NativePresentation {
            tracing::warn!(
                path = %full_path.display(),
                %format,
                "not adding unsupported .pro file to library index"
            );
            return;
        }
        let presentation_metadata = match decode_presentation_metadata(&data, full_path) {
            Ok(metadata) => metadata,
            Err(error) => {
                tracing::warn!(
                    path = %full_path.display(),
                    %error,
                    "not adding undecodable .pro file to library index"
                );
                return;
            }
        };

        let Some(stem) = full_path.file_stem().and_then(|s| s.to_str()) else {
            return;
        };

        let normalized = normalize_name(stem);
        let relative = full_path
            .strip_prefix(&self.library_path)
            .unwrap_or(full_path)
            .to_string_lossy()
            .to_string();

        self.entries.push(FileEntry {
            file_name: stem.to_string(),
            normalized_name: normalized.clone(),
            file_name_lower: stem.to_lowercase(),
            normalized_lower: normalized.to_lowercase(),
            display_name: stem.to_string(),
            relative_path: relative,
            full_path: full_path.to_path_buf(),
            arrangements: presentation_metadata.arrangements,
            presentation_size: presentation_metadata.presentation_size,
        });
    }

    /// Find matching files for a search query
    pub fn find_matches(&self, query: impl AsRef<str>, max_results: usize) -> Vec<FileEntry> {
        let query_str = query.as_ref().trim();
        if query_str.is_empty() {
            return Vec::new();
        }

        let query_lower = query_str.to_lowercase();
        let normalized_query = normalize_name(query_str);
        let effective = if normalized_query.is_empty() {
            query_str
        } else {
            &normalized_query
        };
        let effective_lower = effective.to_lowercase();

        let matcher = SkimMatcherV2::default();
        let hymn_number = extract_hymn_number(query_str);
        let composite_parts = parse_composite_query(effective);
        let tokens = tokenize_query(&effective_lower);

        // Score all entries in parallel
        let mut scored: Vec<(i64, &FileEntry)> = self
            .entries
            .par_iter()
            .filter_map(|entry| {
                let score = Self::score_entry(
                    &matcher,
                    entry,
                    effective,
                    &effective_lower,
                    &query_lower,
                    hymn_number.as_deref(),
                    &composite_parts,
                    &tokens,
                )?;
                Some((score, entry))
            })
            .collect();

        // Sort by score descending
        scored.par_sort_unstable_by(|a, b| b.0.cmp(&a.0));

        // Apply adaptive threshold filtering
        let filtered = apply_threshold_filter(scored, 5);

        filtered
            .into_iter()
            .take(max_results)
            .map(|(_, e)| e.clone())
            .collect()
    }

    /// Score a single entry against the query
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn score_entry(
        matcher: &SkimMatcherV2,
        entry: &FileEntry,
        term: &str,
        term_lower: &str,
        query_lower: &str,
        hymn_number: Option<&str>,
        composite_parts: &[&str],
        tokens: &[&str],
    ) -> Option<i64> {
        let mut score = 0i64;
        let mut quality = 0u8; // 0=none, 1=weak, 2=moderate, 3=strong

        // Fuzzy match score
        let fuzzy = matcher
            .fuzzy_match(&entry.normalized_name, term)
            .unwrap_or(0)
            .max(matcher.fuzzy_match(&entry.file_name, term).unwrap_or(0));
        score = score.max(fuzzy);

        // **KEY FIX**: Check if filename is contained within the query (reverse containment)
        // This catches cases like query="Prayer and the Lord's Prayer (Hope)" matching file="Prayer and The Lord's Prayer"
        if query_lower.contains(&entry.file_name_lower) {
            // The full filename appears in the query — very strong match.
            // Filenames are always short enough that truncation is not a concern.
            #[allow(clippy::cast_possible_wrap)]
            let len_bonus = (entry.file_name_lower.len() as i64) * 100;
            score = score.max(25000 + len_bonus);
            quality = 3;
        } else if query_lower.contains(&entry.normalized_lower) && entry.normalized_lower.len() > 5
        {
            #[allow(clippy::cast_possible_wrap)]
            let len_bonus = (entry.normalized_lower.len() as i64) * 100;
            score = score.max(22000 + len_bonus);
            quality = 3;
        }

        // Exact/prefix/contains matching with boosts
        if entry.normalized_name.eq_ignore_ascii_case(term) {
            score = score.max(20000);
            quality = 3;
        } else if entry.file_name.eq_ignore_ascii_case(term) {
            score = score.max(19000);
            quality = 3;
        } else if entry.normalized_lower.starts_with(term_lower) {
            score = score.max(15000);
            quality = quality.max(2);
        } else if entry.file_name_lower.starts_with(term_lower) {
            score = score.max(14000);
            quality = quality.max(2);
        } else if entry.normalized_lower.contains(term_lower) {
            score = score.max(if term_lower.len() <= 2 { 800 } else { 8000 });
            quality = quality.max(1);
        } else if entry.file_name_lower.contains(term_lower) {
            score = score.max(if term_lower.len() <= 2 { 600 } else { 6000 });
            quality = quality.max(1);
        }

        // Composite query handling (e.g., "Prayer/Lord's Prayer")
        if let Some(last_part) = composite_parts.last() {
            let last_lower = last_part.to_lowercase();
            if entry.normalized_name.eq_ignore_ascii_case(last_part)
                || entry.file_name.eq_ignore_ascii_case(last_part)
            {
                score = score.max(20000);
                quality = 3;
            } else if entry.normalized_lower.starts_with(&last_lower)
                || entry.file_name_lower.starts_with(&last_lower)
            {
                score = score.max(15000);
                quality = 3;
            } else if entry.normalized_lower.contains(&last_lower) {
                score = score.max(6000);
                quality = quality.max(2);
            }
        }

        // Token-based matching
        for &token in tokens {
            if let Some(token_score) = score_token(matcher, entry, token) {
                score = score.max(token_score);
                if token_score > 3000 {
                    quality = quality.max(2);
                } else if token_score > 1000 {
                    quality = quality.max(1);
                }
            }
        }

        // Hymn number matching
        if let Some(num) = hymn_number {
            if entry.file_name_lower.contains(&format!("#{num}"))
                || entry.file_name_lower.contains(&format!(" {num} "))
                || entry.file_name_lower.contains(&format!("-{num}"))
            {
                score = score.max(9000);
                quality = 3;
            }
        }

        // Liturgical matching (only if we don't already have a strong match)
        if quality < 3
            && (query_lower.contains("lord's prayer") || query_lower.contains("our father"))
        {
            if entry.normalized_lower.contains("lord's prayer")
                || entry.file_name_lower.contains("lord's prayer")
            {
                score = score.max(10000);
                quality = quality.max(2);
            } else if entry.normalized_lower.contains("our father")
                || entry.file_name_lower.contains("our father")
            {
                score = score.max(8000);
                quality = quality.max(2);
            }
        }

        // Filter out completely irrelevant matches
        if quality > 0 || score > 300 {
            Some(score.max(10))
        } else {
            None
        }
    }
}

struct IndexedPresentationMetadata {
    arrangements: Vec<IndexedArrangement>,
    presentation_size: PresentationSizeStatus,
}

fn decode_presentation_metadata(data: &[u8], path: &Path) -> Result<IndexedPresentationMetadata> {
    let presentation = rv_data::Presentation::decode(data).map_err(|error| {
        Error::Library(format!(
            "Failed to decode ProPresenter file {} after native format detection: {error}",
            path.display()
        ))
    })?;

    let arrangements = presentation
        .arrangements
        .iter()
        .map(|arrangement| {
            let complete = !arrangement.name.trim().is_empty()
                && arrangement
                    .uuid
                    .as_ref()
                    .is_some_and(|uuid| Uuid::parse_str(&uuid.string).is_ok());
            if complete {
                IndexedArrangement::Complete {
                    name: arrangement.name.clone(),
                }
            } else {
                IndexedArrangement::Incomplete {
                    name: arrangement.name.clone(),
                }
            }
        })
        .collect();

    Ok(IndexedPresentationMetadata {
        arrangements,
        presentation_size: inspect_presentation_size(&presentation),
    })
}

/// Normalize a filename by removing common prefixes and patterns
pub fn normalize_name(name: &str) -> String {
    use std::sync::LazyLock;

    static RE_BRACKETS: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\s*\[[^\]]+\]\s*").unwrap());
    static RE_HASH_NUM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*#\d+\s*").unwrap());
    static RE_HYMN_NUM: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\s*(?i)hymn\s+(?:#?\d+\s*|)").unwrap());
    static RE_ANTHEM: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\s*(?i)anthem\s*[:|-]?\s*").unwrap());
    static RE_LEADING_NUM: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\s*\d+[\.:\-\s]+").unwrap());
    static RE_PUNCTUATION: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"[,;:\(\)\[\]'!?]").unwrap());
    static RE_SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());

    let mut s = RE_BRACKETS.replace(name, "").to_string();
    s = RE_HASH_NUM.replace(&s, "").to_string();
    s = RE_HYMN_NUM.replace(&s, "").to_string();
    s = RE_ANTHEM.replace(&s, "").to_string();
    s = RE_LEADING_NUM.replace(&s, "").to_string();
    s = RE_PUNCTUATION.replace_all(&s, " ").to_string();
    s = RE_SPACES.replace_all(&s, " ").to_string();
    s.trim().to_string()
}

/// Get the default `ProPresenter` library path
pub fn get_default_library_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let path = home.join("Documents/ProPresenter/Libraries/Default");
    path.is_dir().then_some(path)
}

/// Parse composite query parts (split by / or "and")
fn parse_composite_query(query: &str) -> Vec<&str> {
    if query.contains('/') {
        query
            .split('/')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect()
    } else if query.to_lowercase().contains(" and ") {
        query
            .split(" and ")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect()
    } else {
        Vec::new()
    }
}

/// Tokenize query into searchable terms
fn tokenize_query(query_lower: &str) -> Vec<&str> {
    const STOP_WORDS: &[&str] = &["and", "the", "of", "to", "in"];
    let tokens: Vec<&str> = query_lower
        .split_whitespace()
        .filter(|t| t.len() > 1 && !STOP_WORDS.contains(t))
        .collect();

    if tokens.is_empty() {
        vec![query_lower]
    } else {
        tokens
    }
}

/// Score a single token match
fn score_token(matcher: &SkimMatcherV2, entry: &FileEntry, token: &str) -> Option<i64> {
    const SKIP_WORDS: &[&str] = &["my", "me", "we", "us", "it", "is", "am", "be"];
    if token.len() <= 2 && SKIP_WORDS.contains(&token) {
        return None;
    }

    let mut score = matcher
        .fuzzy_match(&entry.normalized_name, token)
        .or_else(|| matcher.fuzzy_match(&entry.file_name, token))
        .unwrap_or(0);

    let boost = match token.len() {
        1..=2 => 50,
        3..=4 => 200,
        _ => 400,
    };

    if entry.normalized_lower.contains(token) {
        score += 3000 + boost;
        // Word boundary bonus
        if entry.normalized_lower.contains(&format!(" {token} "))
            || entry.normalized_lower.starts_with(&format!("{token} "))
            || entry.normalized_lower.ends_with(&format!(" {token}"))
            || entry.normalized_lower == token
        {
            score += 2000;
        }
    } else if entry.file_name_lower.contains(token) {
        score += 2000 + boost;
    }

    (score > 0).then_some(score)
}

/// Apply adaptive threshold filtering to results
fn apply_threshold_filter(
    results: Vec<(i64, &FileEntry)>,
    min_desired: usize,
) -> Vec<(i64, &FileEntry)> {
    if results.len() <= min_desired {
        return results;
    }

    let top_score = results.first().map_or(0, |(s, _)| *s);
    let threshold = match top_score {
        s if s > 10000 => 500,
        s if s > 5000 => 300,
        _ => 100,
    };

    if results.len() > min_desired * 2 {
        let filtered: Vec<_> = results
            .iter()
            .filter(|(s, _)| *s >= threshold)
            .copied()
            .collect();

        if filtered.len() >= min_desired {
            return filtered;
        }
    }

    results
}

/// Extract an optional catalog number used to boost hymn-library matches.
fn extract_hymn_number(text: &str) -> Option<String> {
    for (index, _) in text.match_indices('#') {
        if let Some(number) = leading_digits(&text[index + 1..]) {
            return Some(number);
        }
    }

    let lower = text.to_ascii_lowercase();
    if let Some(index) = lower.find("hymn") {
        let rest = &text[index + "hymn".len()..];
        if rest.chars().next().is_some_and(char::is_whitespace) {
            let rest = rest.trim_start();
            if let Some(number) = leading_digits(rest.strip_prefix('#').unwrap_or(rest)) {
                return Some(number);
            }
        }
    }

    leading_digits(text.trim_start())
}

fn leading_digits(value: &str) -> Option<String> {
    let number = value
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    (!number.is_empty()).then_some(number)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use std::fs;

    use tempfile::tempdir;

    use crate::propresenter::generated::rv_data;
    use prost::Message;

    fn native_presentation(name: &str) -> Vec<u8> {
        rv_data::Presentation {
            uuid: Some(rv_data::Uuid {
                string: format!("{name}-id"),
            }),
            name: name.to_string(),
            ..Default::default()
        }
        .encode_to_vec()
    }

    fn native_presentation_with_arrangements(name: &str) -> Vec<u8> {
        rv_data::Presentation {
            uuid: Some(rv_data::Uuid {
                string: format!("{name}-id"),
            }),
            name: name.to_string(),
            arrangements: vec![
                rv_data::presentation::Arrangement {
                    uuid: Some(rv_data::Uuid {
                        string: "550e8400-e29b-41d4-a716-446655440000".to_string(),
                    }),
                    name: "Christmas Eve".to_string(),
                    ..Default::default()
                },
                rv_data::presentation::Arrangement {
                    uuid: Some(rv_data::Uuid {
                        string: "not-a-uuid".to_string(),
                    }),
                    name: "Broken".to_string(),
                    ..Default::default()
                },
            ],
            cues: vec![rv_data::Cue {
                actions: vec![rv_data::Action {
                    action_type_data: Some(rv_data::action::ActionTypeData::Slide(
                        rv_data::action::SlideType {
                            slide: Some(rv_data::action::slide_type::Slide::Presentation(
                                rv_data::PresentationSlide {
                                    base_slide: Some(rv_data::Slide {
                                        size: Some(rv_data::graphics::Size {
                                            width: 1920.0,
                                            height: 1080.0,
                                        }),
                                        ..rv_data::Slide::default()
                                    }),
                                    ..rv_data::PresentationSlide::default()
                                },
                            )),
                        },
                    )),
                    ..rv_data::Action::default()
                }],
                ..rv_data::Cue::default()
            }],
            ..Default::default()
        }
        .encode_to_vec()
    }

    #[test]
    fn extracts_hymn_number_for_search_scoring() {
        assert_eq!(
            extract_hymn_number("#510 Jesus Shall Reign").as_deref(),
            Some("510")
        );
        assert_eq!(
            extract_hymn_number("Hymn #42 Amazing Grace").as_deref(),
            Some("42")
        );
        assert_eq!(extract_hymn_number("hymn 7").as_deref(), Some("7"));
        assert_eq!(extract_hymn_number("510 Title").as_deref(), Some("510"));
        assert_eq!(extract_hymn_number("Call to Worship"), None);
    }

    #[test]
    fn index_excludes_non_presentation_files_with_pro_extension() {
        let directory = tempdir().expect("create library dir");
        fs::write(
            directory.path().join("Native.pro"),
            native_presentation("Native"),
        )
        .expect("write native presentation");
        fs::write(directory.path().join("Archive.pro"), b"PK\x03\x04archive")
            .expect("write ZIP fixture");
        fs::write(directory.path().join("Fixture.pro"), b"{\"slides\":[]}")
            .expect("write JSON fixture");
        fs::write(
            directory.path().join("Playlist.pro"),
            rv_data::Playlist {
                uuid: Some(rv_data::Uuid {
                    string: "playlist-id".to_string(),
                }),
                name: "Playlist".to_string(),
                r#type: rv_data::playlist::Type::Playlist as i32,
                ..Default::default()
            }
            .encode_to_vec(),
        )
        .expect("write playlist fixture");
        fs::write(directory.path().join("Marker.pro"), b"MOCKPRESENTATION")
            .expect("write marker fixture");
        fs::write(directory.path().join("Unknown.pro"), [0xff, 0x00])
            .expect("write binary fixture");

        let index = FileIndex::build(directory.path()).expect("build library index");

        assert_eq!(index.entries.len(), 1);
        assert_eq!(index.entries[0].file_name, "Native");
    }

    #[test]
    fn index_records_complete_and_incomplete_native_arrangements() {
        let directory = tempdir().expect("create library dir");
        fs::write(
            directory.path().join("Song.pro"),
            native_presentation_with_arrangements("Song"),
        )
        .expect("write native presentation");

        let index = FileIndex::build(directory.path()).expect("build library index");

        assert_eq!(
            index.entries[0].arrangements,
            vec![
                IndexedArrangement::Complete {
                    name: "Christmas Eve".to_string(),
                },
                IndexedArrangement::Incomplete {
                    name: "Broken".to_string(),
                },
            ]
        );
        assert_eq!(
            index.entries[0].presentation_size,
            PresentationSizeStatus::Uniform {
                size: crate::propresenter::PresentationSize::new(1920, 1080)
                    .expect("valid full HD size"),
            }
        );
    }
}
