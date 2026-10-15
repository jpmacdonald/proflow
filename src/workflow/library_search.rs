//! Library file search and song matching utilities.

mod filename;
pub(super) use filename::{resolve_exact_library_file, ExactLibraryFileMatch};
mod song_roles;
use song_roles::{file_song_role, infer_requested_song_role, role_compatibility_score, SongRole};

use std::collections::HashSet;

use propresenter::library::{normalize_name, LibraryCatalog, LibraryEntry};

/// Standard library search — accepts fuzzy matches with word overlap.
pub(super) fn search_index(index: Option<&LibraryCatalog>, query: &str) -> Option<String> {
    let idx = index?;
    let matches = idx.find_matches(query, 1);
    let entry = matches.first()?;

    let name_lower = entry.file_name().to_lowercase();
    let query_lower = query.to_lowercase();

    if name_lower == query_lower
        || name_lower.contains(&query_lower)
        || query_lower.contains(&name_lower)
    {
        return Some(entry.full_path().to_string_lossy().to_string());
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
        return Some(entry.full_path().to_string_lossy().to_string());
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SongLibraryMatch {
    Resolved(String),
    Candidate(String),
    /// More than one library file carries the song's exact title.
    Ambiguous(Vec<String>),
    /// The song title and the item title each name a different library file.
    ConflictingTitles(Vec<String>),
    Missing,
}

pub(super) fn resolve_song_library_match(
    index: Option<&LibraryCatalog>,
    song_title: &str,
    item_title: &str,
    stripped_title: &str,
    bare_title: &str,
) -> SongLibraryMatch {
    // An attached song title owns identity. The item title can expose a conflict,
    // but a shortened item title cannot replace an unmatched qualified song title.
    // Search normalization, role inference, and path quality only rank suggestions.
    let exact_song_paths = exact_song_title_paths(index, song_title);
    let exact_item_paths = exact_song_title_paths(index, item_title);
    match exact_song_paths.as_slice() {
        [path] if exact_item_paths.iter().any(|item_path| item_path != path) => {
            let mut paths = vec![path.clone()];
            for item_path in exact_item_paths {
                push_unique_path(&mut paths, item_path);
            }
            return SongLibraryMatch::ConflictingTitles(paths);
        }
        [path] => return SongLibraryMatch::Resolved(path.clone()),
        [] => {}
        _ => return SongLibraryMatch::Ambiguous(exact_song_paths),
    }

    let candidates = song_search_queries(song_title, stripped_title, bare_title);
    let requested_role = infer_requested_song_role(song_title, item_title);
    let mut strict_matches = Vec::new();
    for (query, query_penalty) in &candidates {
        strict_matches.extend(search_index_strict_matches(
            index,
            query,
            requested_role,
            *query_penalty,
        ));
    }
    if let Some(candidate) = preferred_song_match(&strict_matches) {
        return SongLibraryMatch::Candidate(candidate.path);
    }

    let mut loose_matches = Vec::new();
    for (query, _) in &candidates {
        if let Some(path) = search_index(index, query) {
            push_unique_path(&mut loose_matches, path);
        }
    }
    if let Some(path) = preferred_song_path(loose_matches) {
        return SongLibraryMatch::Candidate(path);
    }

    SongLibraryMatch::Missing
}

/// Identity normalization preserves every title word and qualification. Only
/// letter case and repeated whitespace are insignificant at this boundary.
fn exact_song_title_paths(index: Option<&LibraryCatalog>, title: &str) -> Vec<String> {
    let identity = |value: &str| {
        value
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    let key = identity(title);
    let Some(index) = index.filter(|_| !key.is_empty()) else {
        return Vec::new();
    };
    index
        .entries()
        .iter()
        .filter(|entry| identity(entry.file_name()) == key)
        .map(|entry| entry.full_path().to_string_lossy().into_owned())
        .collect()
}

fn push_unique_path(paths: &mut Vec<String>, path: String) {
    if !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

fn preferred_song_path(paths: Vec<String>) -> Option<String> {
    paths.into_iter().max_by_key(|path| path_penalty(path))
}

fn path_penalty(path: &str) -> i32 {
    let stem = std::path::Path::new(path)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(path)
        .to_lowercase();

    let mut score = 0;
    if stem.contains("conflicted copy") {
        score -= 100;
    }
    if stem.ends_with("-1") || stem.ends_with(" copy") {
        score -= 25;
    }
    score
}

fn song_search_queries(
    song_title: &str,
    stripped_title: &str,
    bare_title: &str,
) -> Vec<(String, i32)> {
    let mut variants = Vec::new();

    for candidate in [song_title, stripped_title, bare_title] {
        let literal = candidate.trim().to_string();
        if !literal.is_empty() {
            push_unique_variant(&mut variants, literal, 0);
        }

        let normalized = normalize_song_query(candidate);
        if normalized.is_empty() {
            continue;
        }

        push_unique_variant(&mut variants, normalized.clone(), 0);

        for prefix in SONG_BRACKET_PREFIXES {
            push_unique_variant(&mut variants, format!("{prefix} {normalized}"), -10);
        }

        let without_speaker = super::classify_matching::strip_speaker(candidate);
        if without_speaker != candidate.trim() {
            push_unique_variant(&mut variants, without_speaker.clone(), -20);
            let normalized_without_speaker = normalize_song_query(&without_speaker);
            push_unique_variant(&mut variants, normalized_without_speaker.clone(), -20);
            for prefix in SONG_BRACKET_PREFIXES {
                push_unique_variant(
                    &mut variants,
                    format!("{prefix} {normalized_without_speaker}"),
                    -30,
                );
            }
        }
    }

    variants
}

fn push_unique_variant(variants: &mut Vec<(String, i32)>, value: String, query_penalty: i32) {
    let key = value.to_lowercase();
    if key.trim().is_empty() {
        return;
    }
    if let Some((_, existing_penalty)) = variants
        .iter_mut()
        .find(|(existing, _)| existing.to_lowercase() == key)
    {
        *existing_penalty = (*existing_penalty).max(query_penalty);
    } else {
        variants.push((value, query_penalty));
    }
}

fn normalize_song_query(query: &str) -> String {
    normalize_name(query)
}

/// Strict library search for songs — requires containment match, not just
/// word overlap. Prevents false positives like "Have Thine Own Way" matching
/// "We Have Come at Christ's Own Bidding".
#[cfg(test)]
pub(super) fn search_index_strict(index: Option<&LibraryCatalog>, query: &str) -> Option<String> {
    preferred_song_match(&search_index_strict_matches(
        index,
        query,
        SongRole::Unknown,
        0,
    ))
    .map(|candidate| candidate.path)
}

fn search_index_strict_matches(
    index: Option<&LibraryCatalog>,
    query: &str,
    requested_role: SongRole,
    query_penalty: i32,
) -> Vec<ScoredSongMatch> {
    let Some(idx) = index else {
        return Vec::new();
    };

    let mut matches = Vec::new();
    for entry in idx.entries() {
        if let Some(title_score) = strict_title_score(entry, query) {
            // Generated prefixes and speaker-stripped fallbacks remain useful
            // candidates but cannot equal direct title evidence.
            push_scored_song_match(
                &mut matches,
                entry,
                title_score + query_penalty,
                requested_role,
            );
        }
    }

    matches
}

fn preferred_song_match(matches: &[ScoredSongMatch]) -> Option<ScoredSongMatch> {
    matches
        .iter()
        .max_by(|left, right| {
            left.title_score
                .cmp(&right.title_score)
                .then(left.role_score.cmp(&right.role_score))
                .then(left.path_score.cmp(&right.path_score))
                .then_with(|| right.path.cmp(&left.path))
        })
        .cloned()
}

fn push_scored_song_match(
    matches: &mut Vec<ScoredSongMatch>,
    entry: &LibraryEntry,
    title_score: i32,
    requested_role: SongRole,
) {
    let path = entry.full_path().to_string_lossy().to_string();
    let candidate = ScoredSongMatch {
        role_score: role_compatibility_score(requested_role, file_song_role(entry.file_name())),
        path_score: path_penalty(&path),
        title_score,
        path,
    };

    if let Some(existing) = matches
        .iter_mut()
        .find(|existing| existing.path == candidate.path)
    {
        if candidate.better_than(existing) {
            *existing = candidate;
        }
    } else {
        matches.push(candidate);
    }
}

fn strict_title_score(entry: &LibraryEntry, query: &str) -> Option<i32> {
    let normalized_query = normalize_song_query(query);
    if normalized_query.is_empty() {
        return None;
    }

    let query_lower = query.trim().to_lowercase();
    let normalized_lower = normalized_query.to_lowercase();
    let name_lower = entry.file_name().to_lowercase();
    let normalized_name = entry.normalized_name().to_lowercase();
    let name_norm_lower = normalize_name(entry.file_name()).to_lowercase();

    let mut score = None;
    update_score(
        &mut score,
        title_score_for_pair(&normalized_name, &normalized_lower, true),
    );
    update_score(
        &mut score,
        title_score_for_pair(&name_lower, &query_lower, false),
    );
    update_score(
        &mut score,
        title_score_for_pair(&name_norm_lower, &normalized_lower, true),
    );
    score
}

fn title_score_for_pair(file_name: &str, query: &str, normalized: bool) -> Option<i32> {
    if query.is_empty() {
        return None;
    }
    if file_name == query {
        return Some(if normalized { 100 } else { 110 });
    }
    if file_name.starts_with(query) {
        return Some(90);
    }
    if file_name.contains(query) {
        return Some(80);
    }
    if is_strong_token_reorder_match(file_name, query) {
        return Some(75);
    }
    if is_substantial_reverse_match(file_name, query) {
        return Some(60);
    }
    None
}

fn update_score(score: &mut Option<i32>, candidate: Option<i32>) {
    if let Some(candidate) = candidate {
        *score = Some(score.map_or(candidate, |score| score.max(candidate)));
    }
}

fn is_substantial_reverse_match(file_name: &str, query: &str) -> bool {
    if file_name.len() < 10 || !query.contains(file_name) {
        return false;
    }

    let file_tokens = significant_tokens(file_name);
    if file_tokens.len() < 2 {
        return false;
    }

    let query_tokens = significant_tokens(query);
    if query_tokens.is_empty() {
        return false;
    }

    let file_token_count = file_tokens.len();
    let overlap = file_tokens
        .iter()
        .filter(|token| query_tokens.contains(token))
        .count();

    overlap == file_token_count && file_token_count * 2 >= query_tokens.len()
}

fn is_strong_token_reorder_match(file_name: &str, query: &str) -> bool {
    let file_tokens = significant_tokens(file_name);
    let query_tokens = significant_tokens(query);
    if file_tokens.len() < 2 || query_tokens.len() < 3 {
        return false;
    }

    let file_token_set: HashSet<&str> = file_tokens.iter().copied().collect();
    let query_token_set: HashSet<&str> = query_tokens.iter().copied().collect();
    let overlap = query_token_set
        .iter()
        .filter(|token| file_token_set.contains(**token))
        .count();

    overlap == query_token_set.len() && overlap >= 3
}

fn significant_tokens(value: &str) -> Vec<&str> {
    const STOP_WORDS: &[&str] = &[
        "and", "the", "of", "to", "in", "by", "not", "our", "your", "with", "hymn", "anthem",
        "choir",
    ];

    value
        .split_whitespace()
        .filter(|token| token.len() > 2 && !STOP_WORDS.contains(token))
        .collect()
}

#[cfg(test)]
mod tests;

#[derive(Debug, Clone)]
struct ScoredSongMatch {
    path: String,
    title_score: i32,
    role_score: i32,
    path_score: i32,
}

impl ScoredSongMatch {
    fn better_than(&self, other: &Self) -> bool {
        (
            self.title_score,
            self.role_score,
            self.path_score,
            &self.path,
        ) > (
            other.title_score,
            other.role_score,
            other.path_score,
            &other.path,
        )
    }
}
