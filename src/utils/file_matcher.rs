//! File matching and indexing for `ProPresenter` library files.
//!
//! Provides fuzzy search with persistent caching of:
//! - File index (avoids cold-start rescans)
//! - Selection history (previously matched files rank higher)

// Allow unwrap for compile-time constant regex patterns in lazy_static blocks
#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use rayon::prelude::*;
use regex::Regex;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::error::{Error, Result};
use crate::planning_center::types::Category;

/// Cache file name
const CACHE_FILE: &str = "library_cache.json";

/// Get the application cache directory, creating it if needed.
/// Uses `~/Library/Application Support/proflow/` on macOS (via `dirs::data_dir`).
fn cache_dir() -> Option<PathBuf> {
    let dir = dirs::data_dir()?.join("proflow");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Get the cache file path. Falls back to a dotfile next to the library
/// if the platform data directory is unavailable.
fn cache_path(library_path: &Path) -> PathBuf {
    cache_dir().map_or_else(
        || library_path.join(".proflow_cache.json"),
        |d| d.join(CACHE_FILE),
    )
}

/// A file entry representing a `ProPresenter` file
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::pub_underscore_fields)]
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
    #[serde(rename = "relative_path")]
    pub _relative_path: String,
    /// Absolute path on disk
    pub full_path: PathBuf,
}

impl FileEntry {
    /// Compute lowercase variants (call after deserializing)
    fn compute_lowercase(&mut self) {
        self.file_name_lower = self.file_name.to_lowercase();
        self.normalized_lower = self.normalized_name.to_lowercase();
    }
}

/// Persistent cache data
#[derive(Debug, Default, Serialize, Deserialize)]
struct CacheData {
    /// Library path this cache was built from
    library_path: PathBuf,
    /// When the cache was last built
    #[serde(with = "humantime_serde")]
    built_at: Option<SystemTime>,
    /// Cached file entries
    entries: Vec<FileEntry>,
    /// Item ID → file path selections
    selections: HashMap<String, String>,
    /// File path → selection count
    frequency: HashMap<String, u32>,
    /// Item ID → editor state (content, cursor, etc.)
    #[serde(default)]
    editor_states: HashMap<String, crate::app::EditorState>,
    /// Item ID → completion status
    #[serde(default)]
    item_completion: HashMap<String, bool>,
    /// Item ID → ignored status
    #[serde(default)]
    item_ignored: HashMap<String, bool>,
}

mod humantime_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    // serde requires `&Option<T>` for field-level `serialize_with`; we cannot
    // change the signature to `Option<&SystemTime>`.
    #[allow(clippy::ref_option)]
    pub fn serialize<S: Serializer>(time: &Option<SystemTime>, s: S) -> Result<S::Ok, S::Error> {
        match time {
            Some(t) => t.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO).as_secs().serialize(s),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<SystemTime>, D::Error> {
        let secs: Option<u64> = Option::deserialize(d)?;
        Ok(secs.map(|s| UNIX_EPOCH + Duration::from_secs(s)))
    }
}

/// Index of `ProPresenter` files with persistent caching
pub struct FileIndex {
    /// All indexed files
    pub entries: Vec<FileEntry>,
    /// Item ID → file path selections (persisted)
    pub item_file_selections: HashMap<String, String>,
    /// File path → selection count (persisted)
    pub selection_frequency: HashMap<String, u32>,
    /// Item ID → editor state (persisted)
    pub editor_states: HashMap<String, crate::app::EditorState>,
    /// Item ID → completion status (persisted)
    pub item_completion: HashMap<String, bool>,
    /// Item ID → ignored status (persisted)
    pub item_ignored: HashMap<String, bool>,
    /// Library path for cache persistence
    library_path: PathBuf,
}

impl FileIndex {
    /// Build or load a file index for the given library path
    pub fn build(library_path: &Path) -> Result<Self> {
        if !library_path.is_dir() {
            return Err(Error::Library(format!(
                "Library path does not exist or is not a directory: {}",
                library_path.display()
            )));
        }

        let cp = cache_path(library_path);

        // Try to load from cache
        if let Some(mut index) = Self::load_cache(&cp, library_path) {
            // Recompute lowercase fields
            for entry in &mut index.entries {
                entry.compute_lowercase();
            }
            return Ok(index);
        }

        // Build fresh index
        let start = Instant::now();
        let entries: Vec<FileEntry> = WalkDir::new(library_path)
            .follow_links(true)
            .into_iter()
            .filter_map(std::result::Result::ok)
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "pro"))
            .filter_map(|entry| {
                let stem = entry.path().file_stem()?.to_str()?;
                let normalized = normalize_name(stem);
                let relative_path = entry.path()
                    .strip_prefix(library_path)
                    .unwrap_or_else(|_| entry.path())
                    .to_string_lossy()
                    .to_string();
                
                Some(FileEntry {
                    file_name: stem.to_string(),
                    normalized_name: normalized.clone(),
                    file_name_lower: stem.to_lowercase(),
                    normalized_lower: normalized.to_lowercase(),
                    display_name: stem.to_string(),
                    _relative_path: relative_path,
                    full_path: entry.path().to_path_buf(),
                })
            })
            .collect();

        let count = entries.len();
        let elapsed = start.elapsed();
        tracing::info!("Indexed {count} files in {elapsed:?}");

        let index = Self {
            entries,
            item_file_selections: HashMap::new(),
            selection_frequency: HashMap::new(),
            editor_states: HashMap::new(),
            item_completion: HashMap::new(),
            item_ignored: HashMap::new(),
            library_path: library_path.to_path_buf(),
        };

        // Save cache (ignore errors)
        let _ = index.save_cache(&cp);

        Ok(index)
    }

    /// Try to load index from cache file
    fn load_cache(cache_path: &Path, library_path: &Path) -> Option<Self> {
        let data = std::fs::read_to_string(cache_path).ok()?;
        let cache: CacheData = serde_json::from_str(&data).ok()?;

        // Validate cache is for the same library
        if cache.library_path != library_path {
            return None;
        }

        // Check if cache is stale (library modified after cache built)
        if let (Some(built_at), Ok(meta)) = (cache.built_at, std::fs::metadata(library_path)) {
            if let Ok(modified) = meta.modified() {
                if modified > built_at {
                    return None; // Cache is stale
                }
            }
        }

        Some(Self {
            entries: cache.entries,
            item_file_selections: cache.selections,
            selection_frequency: cache.frequency,
            editor_states: cache.editor_states,
            item_completion: cache.item_completion,
            item_ignored: cache.item_ignored,
            library_path: library_path.to_path_buf(),
        })
    }

    /// Save index to cache file
    fn save_cache(&self, cache_path: &Path) -> Result<()> {
        let cache = CacheData {
            library_path: self.library_path.clone(),
            built_at: Some(SystemTime::now()),
            entries: self.entries.clone(),
            selections: self.item_file_selections.clone(),
            frequency: self.selection_frequency.clone(),
            editor_states: self.editor_states.clone(),
            item_completion: self.item_completion.clone(),
            item_ignored: self.item_ignored.clone(),
        };

        let json = serde_json::to_string_pretty(&cache)
            .map_err(|e| Error::Msg(format!("Failed to serialize cache: {e}")))?;
        std::fs::write(cache_path, json)?;
        Ok(())
    }

    /// Persist current selections to cache
    pub fn persist(&self) {
        let cp = cache_path(&self.library_path);
        let _ = self.save_cache(&cp);
    }

    /// Record a file selection for an item
    pub fn record_selection(&mut self, item_id: &str, file_path: &Path) {
        let path_str = file_path.to_string_lossy().to_string();
        self.item_file_selections.insert(item_id.to_string(), path_str.clone());
        *self.selection_frequency.entry(path_str).or_insert(0) += 1;

        // Persist after each selection
        self.persist();
    }

    /// Get the previously selected file for an item
    pub fn get_selection_for_item(&self, item_id: &str) -> Option<&String> {
        self.item_file_selections.get(item_id)
    }

    /// Save editor state for an item
    pub fn save_editor_state(&mut self, item_id: &str, state: &crate::app::EditorState) {
        self.editor_states.insert(item_id.to_string(), state.clone());
        self.persist();
    }

    /// Get editor state for an item
    pub fn get_editor_state(&self, item_id: &str) -> Option<&crate::app::EditorState> {
        self.editor_states.get(item_id)
    }

    /// Save item completion status
    pub fn save_item_completion(&mut self, item_id: &str, completed: bool) {
        self.item_completion.insert(item_id.to_string(), completed);
        self.persist();
    }

    /// Get item completion status
    pub fn get_item_completion(&self, item_id: &str) -> Option<bool> {
        self.item_completion.get(item_id).copied()
    }

    /// Save item ignored status
    pub fn save_item_ignored(&mut self, item_id: &str, ignored: bool) {
        self.item_ignored.insert(item_id.to_string(), ignored);
        self.persist();
    }

    /// Get item ignored status
    pub fn get_item_ignored(&self, item_id: &str) -> Option<bool> {
        self.item_ignored.get(item_id).copied()
    }

    /// Add a newly exported file to the index, skipping duplicates.
    pub fn add_entry(&mut self, full_path: &Path) {
        // Dedup: skip if already indexed
        if self.entries.iter().any(|e| e.full_path == full_path) {
            return;
        }

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
            _relative_path: relative,
            full_path: full_path.to_path_buf(),
        });

        self.persist();
    }

    /// Find matching files for a search query
    pub fn find_matches(&self, query: impl AsRef<str>, max_results: usize) -> Vec<FileEntry> {
        let query_str = query.as_ref().trim();
        if query_str.is_empty() {
            return Vec::new();
        }
        
        let query_lower = query_str.to_lowercase();
        let normalized_query = normalize_name(query_str);
        let effective = if normalized_query.is_empty() { query_str } else { &normalized_query };
        let effective_lower = effective.to_lowercase();
        
        let matcher = SkimMatcherV2::default();
        let hymn_number = extract_hymn_number(query_str);
        let composite_parts = parse_composite_query(effective);
        let tokens = tokenize_query(&effective_lower);

        // Score all entries in parallel
        let mut scored: Vec<(i64, &FileEntry)> = self.entries.par_iter()
            .filter_map(|entry| {
                let score = self.score_entry(
                    &matcher, entry, effective, &effective_lower,
                    &query_lower, hymn_number.as_deref(), &composite_parts, &tokens,
                )?;
                Some((score, entry))
            })
            .collect();
        
        // Sort by score descending
        scored.par_sort_unstable_by(|a, b| b.0.cmp(&a.0));

        // Apply adaptive threshold filtering
        let filtered = apply_threshold_filter(scored, 5);

        filtered.into_iter()
            .take(max_results)
            .map(|(_, e)| e.clone())
            .collect()
    }

    /// Score a single entry against the query
    #[allow(clippy::too_many_arguments)]
    fn score_entry(
        &self,
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
        let fuzzy = matcher.fuzzy_match(&entry.normalized_name, term).unwrap_or(0)
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
        } else if query_lower.contains(&entry.normalized_lower) && entry.normalized_lower.len() > 5 {
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
            if entry.normalized_name.eq_ignore_ascii_case(last_part) || 
               entry.file_name.eq_ignore_ascii_case(last_part) {
                score = score.max(20000);
                quality = 3;
            } else if entry.normalized_lower.starts_with(&last_lower) ||
                      entry.file_name_lower.starts_with(&last_lower) {
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
                if token_score > 3000 { quality = quality.max(2); }
                else if token_score > 1000 { quality = quality.max(1); }
            }
        }

        // Hymn number matching
        if let Some(num) = hymn_number {
            if entry.file_name_lower.contains(&format!("#{num}")) ||
               entry.file_name_lower.contains(&format!(" {num} ")) ||
               entry.file_name_lower.contains(&format!("-{num}")) {
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

        // Frequency bonus (previously selected files rank higher)
        let path_str = entry.full_path.to_string_lossy();
        #[allow(clippy::cast_possible_wrap)]
        let freq_bonus = i64::from(self.selection_frequency.get(path_str.as_ref()).copied().unwrap_or(0)) * 500;
        score += freq_bonus;

        // Filter out completely irrelevant matches
        if quality > 0 || score > 300 {
            Some(score.max(10))
        } else {
            None
        }
    }
}

/// Normalize a filename by removing common prefixes and patterns
pub fn normalize_name(name: &str) -> String {
    use std::sync::LazyLock;

    static RE_BRACKETS: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\s*\[[^\]]+\]\s*").unwrap());
    static RE_HASH_NUM: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\s*#\d+\s*").unwrap());
    static RE_HYMN_NUM: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\s*(?i)hymn\s+(?:#?\d+\s*|)").unwrap());
    static RE_ANTHEM: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\s*(?i)anthem\s*[:|-]?\s*").unwrap());
    static RE_LEADING_NUM: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\s*\d+[\.:\-\s]+").unwrap());
    static RE_PUNCTUATION: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"[,;:\(\)\[\]'!?]").unwrap());
    static RE_SPACES: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\s+").unwrap());

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

/// Extract hymn number from text like "#123" or "Hymn 123"
fn extract_hymn_number(text: &str) -> Option<String> {
    if let Some(pos) = text.find('#') {
        let start = pos + 1;
        let end = text[start..].find(|c: char| !c.is_ascii_digit())
            .map_or(text.len(), |p| p + start);
        if start < end {
            return Some(text[start..end].to_string());
        }
    }

        let lower = text.to_lowercase();
        if let Some(pos) = lower.find("hymn") {
        let after = &text[pos + 4..];
        if let Some(digit_start) = after.find(|c: char| c.is_ascii_digit()) {
            let num_part = &after[digit_start..];
            let end = num_part.find(|c: char| !c.is_ascii_digit()).unwrap_or(num_part.len());
                if end > 0 {
                return Some(num_part[..end].to_string());
            }
        }
    }

        let trimmed = text.trim();
    if trimmed.starts_with(|c: char| c.is_ascii_digit()) {
        let end = trimmed.find(|c: char| !c.is_ascii_digit()).unwrap_or(trimmed.len());
            if end > 0 {
                return Some(trimmed[..end].to_string());
        }
    }

    None
}

/// Parse composite query parts (split by / or "and")
fn parse_composite_query(query: &str) -> Vec<&str> {
    if query.contains('/') {
        query.split('/').map(str::trim).filter(|s| !s.is_empty()).collect()
    } else if query.to_lowercase().contains(" and ") {
        query.split(" and ").map(str::trim).filter(|s| !s.is_empty()).collect()
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
    
    let mut score = matcher.fuzzy_match(&entry.normalized_name, token)
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
        if entry.normalized_lower.contains(&format!(" {token} ")) ||
           entry.normalized_lower.starts_with(&format!("{token} ")) ||
           entry.normalized_lower.ends_with(&format!(" {token}")) ||
           entry.normalized_lower == token {
            score += 2000;
        }
    } else if entry.file_name_lower.contains(token) {
        score += 2000 + boost;
    }
    
    (score > 0).then_some(score)
}

/// Apply adaptive threshold filtering to results
fn apply_threshold_filter(results: Vec<(i64, &FileEntry)>, min_desired: usize) -> Vec<(i64, &FileEntry)> {
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
        let filtered: Vec<_> = results.iter()
            .filter(|(s, _)| *s >= threshold)
            .copied()
            .collect();

        if filtered.len() >= min_desired {
            return filtered;
        }
    }

    results
}

/// Find matching files for multiple items (batch operation)
pub fn find_matches_for_items<'a, I>(
    items: I,
    library_path: &Path,
    max_results: usize,
) -> HashMap<String, Vec<String>> 
where
    I: Iterator<Item = (&'a String, &'a Category)>,
{
    let Ok(index) = FileIndex::build(library_path) else {
        return HashMap::new();
    };

    items.map(|(title, _)| {
        let matches: Vec<String> = index.find_matches(title, max_results)
            .into_iter()
            .map(|e| e.file_name)
                .collect();
        (title.clone(), matches)
    }).collect()
    }
    