//! Library file search and song matching utilities.

use std::collections::HashSet;

use crate::propresenter::library::{normalize_name, LibraryCatalog, LibraryEntry};

/// Result of resolving a configured `library_file` target.
///
/// Explicit targets use a deliberately narrow filename policy: surrounding
/// whitespace and a trailing `.pro` extension are ignored, comparison is
/// case-insensitive, and every other character must match. A filename that
/// occurs more than once in the library is ambiguous rather than arbitrarily
/// selecting one path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ExactLibraryFileMatch {
    Unique(String),
    Missing,
    Ambiguous,
}

/// Resolve a configured library filename exactly and uniquely.
pub(super) fn resolve_exact_library_file(
    index: Option<&LibraryCatalog>,
    requested_file: &str,
) -> ExactLibraryFileMatch {
    let Some(index) = index else {
        return ExactLibraryFileMatch::Missing;
    };
    let Some(requested_key) = exact_filename_key(requested_file) else {
        return ExactLibraryFileMatch::Missing;
    };

    let mut paths = index
        .entries()
        .iter()
        .filter(|entry| exact_filename_key(entry.file_name()).as_ref() == Some(&requested_key))
        .map(|entry| entry.full_path().to_string_lossy().to_string());
    let Some(path) = paths.next() else {
        return ExactLibraryFileMatch::Missing;
    };
    if paths.next().is_some() {
        ExactLibraryFileMatch::Ambiguous
    } else {
        ExactLibraryFileMatch::Unique(path)
    }
}

fn exact_filename_key(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.contains(['/', '\\']) {
        return None;
    }
    let normalized = trimmed.to_lowercase();
    let stem = normalized
        .strip_suffix(".pro")
        .unwrap_or(&normalized)
        .trim();
    (!stem.is_empty()).then(|| stem.to_string())
}

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
    Missing,
}

pub(super) fn resolve_song_library_match(
    index: Option<&LibraryCatalog>,
    song_title: &str,
    item_title: &str,
    stripped_title: &str,
    bare_title: &str,
) -> SongLibraryMatch {
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
    if let Some(preferred) = preferred_song_match(&strict_matches) {
        return if preferred.ambiguous || !preferred.candidate.is_confident() {
            SongLibraryMatch::Candidate(preferred.candidate.path)
        } else {
            SongLibraryMatch::Resolved(preferred.candidate.path)
        };
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
    .map(|preferred| preferred.candidate.path)
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

fn preferred_song_match(matches: &[ScoredSongMatch]) -> Option<PreferredSongMatch> {
    let candidate = matches.iter().max_by(|left, right| {
        left.title_score
            .cmp(&right.title_score)
            .then(left.role_score.cmp(&right.role_score))
            .then(left.path_score.cmp(&right.path_score))
            .then_with(|| right.path.cmp(&left.path))
    })?;
    let best_score = candidate.semantic_score();
    let ambiguous = matches
        .iter()
        .any(|other| other.path != candidate.path && other.semantic_score() == best_score);
    Some(PreferredSongMatch {
        candidate: candidate.clone(),
        ambiguous,
    })
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SongRole {
    Unknown,
    Hymn,
    Special,
}

#[derive(Debug, Clone)]
struct ScoredSongMatch {
    path: String,
    title_score: i32,
    role_score: i32,
    path_score: i32,
}

struct PreferredSongMatch {
    candidate: ScoredSongMatch,
    ambiguous: bool,
}

impl ScoredSongMatch {
    const fn is_confident(&self) -> bool {
        self.title_score >= 100 && self.role_score >= 0
    }

    const fn semantic_score(&self) -> (i32, i32, i32) {
        (self.title_score, self.role_score, self.path_score)
    }

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

fn infer_requested_song_role(song_title: &str, item_title: &str) -> SongRole {
    let titles = [song_title, item_title].map(|title| title.trim().to_lowercase());
    if titles.iter().any(|title| is_hymn_song_role(title)) {
        return SongRole::Hymn;
    }
    if titles.iter().any(|title| is_special_song_role(title)) {
        return SongRole::Special;
    }
    SongRole::Unknown
}

fn is_hymn_song_role(title: &str) -> bool {
    let title = title.trim_start();
    title.starts_with('#')
        || title.starts_with("[hymn]")
        || title.contains("gtg #")
        || title.contains("hymn #")
        || starts_with_role_word(title, "hymn")
}

fn is_special_song_role(title: &str) -> bool {
    const BRACKETED: &[&str] = &[
        "[anthem]",
        "[youth choir]",
        "[choir]",
        "[special music]",
        "[solo]",
        "[duet]",
        "[prelude]",
        "[postlude]",
    ];
    const PLAIN: &[&str] = &[
        "anthem",
        "youth choir",
        "choir",
        "special music",
        "solo",
        "duet",
        "prelude",
        "postlude",
        "offertory",
        "handbell",
        "organ prelude",
        "organ postlude",
    ];
    let title = title.trim_start();
    BRACKETED.iter().any(|prefix| title.starts_with(prefix))
        || PLAIN
            .iter()
            .any(|prefix| starts_with_role_word(title, prefix))
}

fn starts_with_role_word(title: &str, prefix: &str) -> bool {
    title.strip_prefix(prefix).is_some_and(|suffix| {
        suffix.is_empty()
            || suffix.chars().next().is_some_and(|character| {
                character.is_whitespace() || matches!(character, ':' | '-')
            })
    })
}

fn file_song_role(file_name: &str) -> SongRole {
    let lower = file_name.trim().to_lowercase();
    if is_hymn_song_role(&lower) {
        return SongRole::Hymn;
    }
    if is_special_song_role(&lower) {
        return SongRole::Special;
    }
    SongRole::Unknown
}

const fn role_compatibility_score(requested_role: SongRole, file_role: SongRole) -> i32 {
    match (requested_role, file_role) {
        (SongRole::Unknown, _) | (_, SongRole::Unknown) => 0,
        (SongRole::Hymn, SongRole::Hymn) | (SongRole::Special, SongRole::Special) => 20,
        (SongRole::Hymn, SongRole::Special) | (SongRole::Special, SongRole::Hymn) => -20,
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
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::propresenter::generated::rv_data;
    use prost::Message;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    fn write_presentation(path: &Path) {
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .expect("fixture path has a UTF-8 stem");
        let presentation = rv_data::Presentation {
            uuid: Some(rv_data::Uuid {
                string: format!("{name}-id"),
            }),
            name: name.to_string(),
            ..Default::default()
        };
        fs::write(path, presentation.encode_to_vec()).expect("write presentation fixture");
    }

    fn build_index(files: &[&str]) -> (tempfile::TempDir, LibraryCatalog) {
        let dir = tempdir().expect("temp dir");
        for file in files {
            write_presentation(&dir.path().join(file));
        }
        let index = LibraryCatalog::build(dir.path()).expect("index fixture library");
        (dir, index)
    }

    #[test]
    fn explicit_filename_resolution_only_normalizes_case_whitespace_and_extension() {
        let (_dir, index) = build_index(&["Call to Worship.pro", "Call to Worship Extended.pro"]);

        let resolved = resolve_exact_library_file(Some(&index), "  call to worship.PRO  ");
        assert!(
            matches!(resolved, ExactLibraryFileMatch::Unique(path) if path.ends_with("Call to Worship.pro"))
        );
        assert_eq!(
            resolve_exact_library_file(Some(&index), "Call to Wors"),
            ExactLibraryFileMatch::Missing
        );
    }

    #[test]
    fn explicit_filename_resolution_rejects_duplicate_filenames() {
        let dir = tempdir().expect("temp dir");
        let nested = dir.path().join("nested");
        fs::create_dir(&nested).expect("create nested library folder");
        write_presentation(&dir.path().join("Announcements.pro"));
        write_presentation(&nested.join("announcements.pro"));
        let index = LibraryCatalog::build(dir.path()).expect("index fixture library");

        assert_eq!(
            resolve_exact_library_file(Some(&index), "Announcements.pro"),
            ExactLibraryFileMatch::Ambiguous
        );
    }

    #[test]
    fn strict_song_search_rejects_short_reverse_containment() {
        let (_dir, index) = build_index(&["By Faith.pro", "Praise.pro", "Our God.pro"]);

        assert!(search_index_strict(
            Some(&index),
            "#399 We Walk by Faith and Not by Sight (to AZMON-466)",
        )
        .is_none());
        assert!(search_index_strict(Some(&index), "O Praise The Name (Anastasis)").is_none());
        assert!(search_index_strict(Some(&index), "#210 Our God, Our Help in Ages Past").is_none());
    }

    #[test]
    fn strict_song_search_allows_substantial_reverse_containment() {
        let (_dir, index) = build_index(&["Rock of Ages.pro", "Amazing Grace.pro"]);

        assert!(search_index_strict(Some(&index), "GTG #438 Rock of Ages").is_some());
        assert!(search_index_strict(Some(&index), "#649 Amazing Grace").is_some());
    }

    #[test]
    fn strict_song_search_allows_reordered_hymn_catalog_tokens() {
        let (_dir, index) = build_index(&["[Hymn] Rock of Ages GTG #438.pro"]);

        assert!(search_index_strict(Some(&index), "GTG #438 Rock of Ages").is_some());
    }

    #[test]
    fn song_resolution_prefers_direct_title_over_bracket_classifier() {
        let (_dir, index) = build_index(&[
            "#210 Our God, Our Help in Ages Past.pro",
            "[Hymn] Our God, Our Help in Ages Past.pro",
        ]);

        let resolved = resolve_song_library_match(
            Some(&index),
            "#210 Our God, Our Help in Ages Past",
            "#210 Our God, Our Help in Ages Past",
            "#210 Our God, Our Help in Ages Past",
            "Our God, Our Help in Ages Past",
        );

        assert!(matches!(
            resolved,
            SongLibraryMatch::Resolved(path)
                if path.contains("#210 Our God, Our Help in Ages Past.pro")
        ));
    }

    #[test]
    fn song_resolution_prefers_exact_untagged_worship_title_over_tagged_partial() {
        let (_dir, index) = build_index(&[
            "This Is Amazing Grace.pro",
            "[Anthem] Amazing Grace.pro",
            "[Hymn] Amazing Grace.pro",
        ]);

        let resolved = resolve_song_library_match(
            Some(&index),
            "This Is Amazing Grace",
            "This Is Amazing Grace",
            "This Is Amazing Grace",
            "This Is Amazing Grace",
        );

        assert!(matches!(
            resolved,
            SongLibraryMatch::Resolved(path) if path.contains("This Is Amazing Grace.pro")
        ));
    }

    #[test]
    fn song_resolution_requires_review_when_best_native_files_tie() {
        let (_dir, index) =
            build_index(&["[Anthem] Amazing Grace.pro", "[Hymn] Amazing Grace.pro"]);

        let resolved = resolve_song_library_match(
            Some(&index),
            "Amazing Grace",
            "Amazing Grace",
            "Amazing Grace",
            "Amazing Grace",
        );

        assert!(matches!(resolved, SongLibraryMatch::Candidate(_)));
    }

    #[test]
    fn song_resolution_requires_review_for_a_unique_heuristic_match() {
        let (_dir, index) = build_index(&["This Is Amazing Grace.pro"]);

        let resolved = resolve_song_library_match(
            Some(&index),
            "Amazing Grace",
            "Amazing Grace",
            "Amazing Grace",
            "Amazing Grace",
        );

        assert!(matches!(resolved, SongLibraryMatch::Candidate(_)));
    }

    #[test]
    fn song_resolution_requires_review_for_a_role_conflict() {
        let (_dir, index) = build_index(&["[Anthem] Amazing Grace.pro"]);

        let resolved = resolve_song_library_match(
            Some(&index),
            "Amazing Grace",
            "#649 Amazing Grace",
            "Amazing Grace",
            "Amazing Grace",
        );

        assert!(matches!(resolved, SongLibraryMatch::Candidate(_)));
    }

    #[test]
    fn song_resolution_preserves_an_exact_parenthesized_title() {
        let (_dir, index) = build_index(&["O Praise The Name (Anastasis).pro"]);

        let resolved = resolve_song_library_match(
            Some(&index),
            "O Praise The Name (Anastasis)",
            "O Praise The Name (Anastasis)",
            "O Praise The Name (Anastasis)",
            "O Praise The Name (Anastasis)",
        );

        assert!(matches!(
            resolved,
            SongLibraryMatch::Resolved(path)
                if path.ends_with("O Praise The Name (Anastasis).pro")
        ));
    }

    #[test]
    fn song_resolution_requires_review_between_title_variants() {
        let (_dir, index) = build_index(&[
            "O Praise The Name (Anastasis).pro",
            "O Praise The Name (Live).pro",
        ]);

        let resolved = resolve_song_library_match(
            Some(&index),
            "O Praise The Name",
            "O Praise The Name",
            "O Praise The Name",
            "O Praise The Name",
        );

        assert!(matches!(resolved, SongLibraryMatch::Candidate(_)));
    }

    #[test]
    fn explicit_hymn_title_beats_an_untagged_title() {
        let (_dir, index) = build_index(&["Amazing Grace.pro", "[Hymn] Amazing Grace.pro"]);

        let resolved = resolve_song_library_match(
            Some(&index),
            "Amazing Grace",
            "[Hymn] Amazing Grace",
            "[Hymn] Amazing Grace",
            "Amazing Grace",
        );

        assert!(matches!(
            resolved,
            SongLibraryMatch::Resolved(path) if path.ends_with("[Hymn] Amazing Grace.pro")
        ));
    }

    #[test]
    fn dropping_a_parenthesized_qualifier_is_only_a_candidate() {
        let (_dir, index) = build_index(&["O Praise The Name.pro"]);

        let resolved = resolve_song_library_match(
            Some(&index),
            "O Praise The Name (Anastasis)",
            "O Praise The Name (Anastasis)",
            "O Praise The Name (Anastasis)",
            "O Praise The Name (Anastasis)",
        );

        assert!(matches!(resolved, SongLibraryMatch::Candidate(_)));
    }

    #[test]
    fn unbracketed_anthem_conflicts_with_a_numbered_hymn() {
        let (_dir, index) = build_index(&["Anthem: Amazing Grace.pro"]);

        let resolved = resolve_song_library_match(
            Some(&index),
            "Amazing Grace",
            "#649 Amazing Grace",
            "Amazing Grace",
            "Amazing Grace",
        );

        assert!(matches!(resolved, SongLibraryMatch::Candidate(_)));
    }

    #[test]
    fn original_special_item_prefix_is_role_evidence() {
        let (_dir, index) = build_index(&["[Hymn] Amazing Grace.pro"]);

        let resolved = resolve_song_library_match(
            Some(&index),
            "Amazing Grace",
            "Offertory: Amazing Grace",
            "Amazing Grace",
            "Amazing Grace",
        );

        assert!(matches!(resolved, SongLibraryMatch::Candidate(_)));
    }
}
