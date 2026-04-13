//! Library file search and song matching utilities.

use std::collections::HashSet;

use crate::utils::file_index::{normalize_name, FileIndex};

/// Standard library search — accepts fuzzy matches with word overlap.
pub(super) fn search_index(index: Option<&FileIndex>, query: &str) -> Option<String> {
    let idx = index?;
    let matches = idx.find_matches(query, 1);
    let entry = matches.first()?;

    let name_lower = entry.file_name.to_lowercase();
    let query_lower = query.to_lowercase();

    if name_lower == query_lower
        || name_lower.contains(&query_lower)
        || query_lower.contains(&name_lower)
    {
        return Some(entry.full_path.to_string_lossy().to_string());
    }

    // Word overlap
    let query_words: HashSet<&str> = query_lower
        .split_whitespace()
        .filter(|w| w.len() > 2)
        .collect();
    let name_words: HashSet<&str> = name_lower
        .split_whitespace()
        .filter(|w| w.len() > 2)
        .collect();
    let overlap = query_words.intersection(&name_words).count();
    if overlap >= 2 || (overlap == 1 && query_words.len() == 1) {
        return Some(entry.full_path.to_string_lossy().to_string());
    }

    None
}

/// Strip `#NNN ` hymn number prefix from a title.
pub(super) fn strip_hymn_number(title: &str) -> String {
    let trimmed = title.trim();
    if let Some(rest) = trimmed.strip_prefix('#') {
        if let Some(space) = rest.find(' ') {
            if rest[..space].chars().all(|c| c.is_ascii_digit()) {
                return rest[space..].trim().to_string();
            }
        }
    }
    trimmed.to_string()
}

const SONG_BRACKET_PREFIXES: &[&str] = &[
    "[Hymn]",
    "[Anthem]",
    "[Youth Choir]",
    "[Choir]",
    "[Special Music]",
    "[Solo]",
    "[Duet]",
    "[Prelude]",
    "[Postlude]",
];

#[derive(Debug, Clone)]
pub(super) struct SongLibraryMatch {
    pub path: Option<String>,
    pub uncertain: bool,
}

impl SongLibraryMatch {
    pub const fn exact(path: String) -> Self {
        Self {
            path: Some(path),
            uncertain: false,
        }
    }

    pub const fn uncertain(path: String) -> Self {
        Self {
            path: Some(path),
            uncertain: true,
        }
    }

    pub const fn none() -> Self {
        Self {
            path: None,
            uncertain: false,
        }
    }
}

pub(super) fn resolve_song_library_match(
    index: Option<&FileIndex>,
    song_title: &str,
    stripped_title: &str,
    bare_title: &str,
) -> SongLibraryMatch {
    let candidates = song_search_queries(song_title, stripped_title, bare_title);

    for query in &candidates {
        if let Some(path) = search_index_strict(index, query) {
            return SongLibraryMatch::exact(path);
        }
    }

    for query in &candidates {
        if let Some(path) = search_index(index, query) {
            return SongLibraryMatch::uncertain(path);
        }
    }

    SongLibraryMatch::none()
}

fn song_search_queries(song_title: &str, stripped_title: &str, bare_title: &str) -> Vec<String> {
    let mut variants = Vec::new();
    let mut seen = HashSet::new();

    for candidate in [song_title, stripped_title, bare_title] {
        let normalized = normalize_song_query(candidate);
        if normalized.is_empty() {
            continue;
        }

        push_unique_variant(&mut variants, &mut seen, normalized.clone());

        for prefix in SONG_BRACKET_PREFIXES {
            push_unique_variant(&mut variants, &mut seen, format!("{prefix} {normalized}"));
        }
    }

    variants
}

fn push_unique_variant(variants: &mut Vec<String>, seen: &mut HashSet<String>, value: String) {
    let key = value.to_lowercase();
    if !key.trim().is_empty() && seen.insert(key) {
        variants.push(value);
    }
}

fn normalize_song_query(query: &str) -> String {
    normalize_name(&super::classify::strip_speaker(query))
}

/// Strict library search for songs — requires containment match, not just
/// word overlap. Prevents false positives like "Have Thine Own Way" matching
/// "We Have Come at Christ's Own Bidding".
pub(super) fn search_index_strict(index: Option<&FileIndex>, query: &str) -> Option<String> {
    let idx = index?;
    let normalized_query = normalize_song_query(query);
    let search_term = if normalized_query.is_empty() {
        query.trim()
    } else {
        normalized_query.as_str()
    };
    let matches = idx.find_matches(search_term, 3);

    let query_lower = query.trim().to_lowercase();
    let normalized_lower = normalized_query.to_lowercase();

    for entry in &matches {
        let name_lower = entry.file_name.to_lowercase();
        let normalized_name = entry.normalized_name.to_lowercase();

        if !normalized_lower.is_empty()
            && (normalized_name == normalized_lower
                || normalized_name.contains(&normalized_lower)
                || normalized_lower.contains(&normalized_name))
        {
            return Some(entry.full_path.to_string_lossy().to_string());
        }

        if name_lower == query_lower
            || name_lower.contains(&query_lower)
            || query_lower.contains(&name_lower)
        {
            return Some(entry.full_path.to_string_lossy().to_string());
        }

        // Retry with normalized names and punctuation stripped from the raw file name.
        let name_norm = normalize_name(&entry.file_name);
        let query_norm = normalize_song_query(query);
        if !query_norm.is_empty()
            && (name_norm.eq_ignore_ascii_case(&query_norm)
                || name_norm
                    .to_lowercase()
                    .contains(&query_norm.to_lowercase())
                || query_norm
                    .to_lowercase()
                    .contains(&name_norm.to_lowercase()))
        {
            return Some(entry.full_path.to_string_lossy().to_string());
        }
    }

    None
}
