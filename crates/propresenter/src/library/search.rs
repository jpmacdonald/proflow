//! Fuzzy title scoring for presentation-library discovery.

#![allow(
    clippy::unwrap_used,
    reason = "fixed regex literals are validated once at module initialization"
)]

use std::sync::LazyLock;

use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use rayon::prelude::*;
use regex::Regex;

use super::{LibraryCatalog, LibraryEntry};

impl LibraryCatalog {
    /// Find matching presentations for a title query, ordered by relevance.
    pub fn find_matches(&self, query: impl AsRef<str>, max_results: usize) -> Vec<LibraryEntry> {
        let query = query.as_ref().trim();
        if query.is_empty() {
            return Vec::new();
        }

        let query_lower = query.to_lowercase();
        let normalized_query = normalize_name(query);
        let effective = if normalized_query.is_empty() {
            query
        } else {
            &normalized_query
        };
        let effective_lower = effective.to_lowercase();
        let matcher = SkimMatcherV2::default();
        let hymn_number = extract_hymn_number(query);
        let composite_parts = parse_composite_query(effective);
        let tokens = tokenize_query(&effective_lower);

        let mut scored: Vec<(i64, &LibraryEntry)> = self
            .entries
            .par_iter()
            .filter_map(|entry| {
                score_entry(
                    &matcher,
                    entry,
                    effective,
                    &effective_lower,
                    &query_lower,
                    hymn_number.as_deref(),
                    &composite_parts,
                    &tokens,
                )
                .map(|score| (score, entry))
            })
            .collect();
        scored.par_sort_unstable_by(|left, right| right.0.cmp(&left.0));

        apply_threshold_filter(scored, 5)
            .into_iter()
            .take(max_results)
            .map(|(_, entry)| entry.clone())
            .collect()
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn score_entry(
    matcher: &SkimMatcherV2,
    entry: &LibraryEntry,
    term: &str,
    term_lower: &str,
    query_lower: &str,
    hymn_number: Option<&str>,
    composite_parts: &[&str],
    tokens: &[&str],
) -> Option<i64> {
    let mut score = matcher
        .fuzzy_match(&entry.normalized_name, term)
        .unwrap_or(0)
        .max(matcher.fuzzy_match(&entry.file_name, term).unwrap_or(0));
    let mut quality = 0_u8;

    if query_lower.contains(&entry.file_name_lower) {
        #[allow(
            clippy::cast_possible_wrap,
            reason = "presentation filenames are bounded far below i64::MAX"
        )]
        let length_bonus = (entry.file_name_lower.len() as i64) * 100;
        score = score.max(25_000 + length_bonus);
        quality = 3;
    } else if query_lower.contains(&entry.normalized_lower) && entry.normalized_lower.len() > 5 {
        #[allow(
            clippy::cast_possible_wrap,
            reason = "presentation filenames are bounded far below i64::MAX"
        )]
        let length_bonus = (entry.normalized_lower.len() as i64) * 100;
        score = score.max(22_000 + length_bonus);
        quality = 3;
    }

    if entry.normalized_name.eq_ignore_ascii_case(term) {
        score = score.max(20_000);
        quality = 3;
    } else if entry.file_name.eq_ignore_ascii_case(term) {
        score = score.max(19_000);
        quality = 3;
    } else if entry.normalized_lower.starts_with(term_lower) {
        score = score.max(15_000);
        quality = quality.max(2);
    } else if entry.file_name_lower.starts_with(term_lower) {
        score = score.max(14_000);
        quality = quality.max(2);
    } else if entry.normalized_lower.contains(term_lower) {
        score = score.max(if term_lower.len() <= 2 { 800 } else { 8_000 });
        quality = quality.max(1);
    } else if entry.file_name_lower.contains(term_lower) {
        score = score.max(if term_lower.len() <= 2 { 600 } else { 6_000 });
        quality = quality.max(1);
    }

    if let Some(last_part) = composite_parts.last() {
        let last_lower = last_part.to_lowercase();
        if entry.normalized_name.eq_ignore_ascii_case(last_part)
            || entry.file_name.eq_ignore_ascii_case(last_part)
        {
            score = score.max(20_000);
            quality = 3;
        } else if entry.normalized_lower.starts_with(&last_lower)
            || entry.file_name_lower.starts_with(&last_lower)
        {
            score = score.max(15_000);
            quality = 3;
        } else if entry.normalized_lower.contains(&last_lower) {
            score = score.max(6_000);
            quality = quality.max(2);
        }
    }

    for &token in tokens {
        if let Some(token_score) = score_token(matcher, entry, token) {
            score = score.max(token_score);
            if token_score > 3_000 {
                quality = quality.max(2);
            } else if token_score > 1_000 {
                quality = quality.max(1);
            }
        }
    }

    if let Some(number) = hymn_number {
        if entry.file_name_lower.contains(&format!("#{number}"))
            || entry.file_name_lower.contains(&format!(" {number} "))
            || entry.file_name_lower.contains(&format!("-{number}"))
        {
            score = score.max(9_000);
            quality = 3;
        }
    }

    if quality < 3 && (query_lower.contains("lord's prayer") || query_lower.contains("our father"))
    {
        if entry.normalized_lower.contains("lord's prayer")
            || entry.file_name_lower.contains("lord's prayer")
        {
            score = score.max(10_000);
            quality = quality.max(2);
        } else if entry.normalized_lower.contains("our father")
            || entry.file_name_lower.contains("our father")
        {
            score = score.max(8_000);
            quality = quality.max(2);
        }
    }

    (quality > 0 || score > 300).then_some(score.max(10))
}

/// Normalize a presentation filename for title matching.
#[must_use]
pub fn normalize_name(name: &str) -> String {
    static BRACKETS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*\[[^\]]+\]\s*").unwrap());
    static HASH_NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*#\d+\s*").unwrap());
    static HYMN_NUMBER: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\s*(?i)hymn\s+(?:#?\d+\s*|)").unwrap());
    static ANTHEM: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\s*(?i)anthem\s*[:|-]?\s*").unwrap());
    static LEADING_NUMBER: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\s*\d+[\.:\-\s]+").unwrap());
    static PUNCTUATION: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"[,;:\(\)\[\]'!?]").unwrap());
    static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());

    let mut normalized = BRACKETS.replace(name, "").to_string();
    normalized = HASH_NUMBER.replace(&normalized, "").to_string();
    normalized = HYMN_NUMBER.replace(&normalized, "").to_string();
    normalized = ANTHEM.replace(&normalized, "").to_string();
    normalized = LEADING_NUMBER.replace(&normalized, "").to_string();
    normalized = PUNCTUATION.replace_all(&normalized, " ").to_string();
    normalized = SPACES.replace_all(&normalized, " ").to_string();
    normalized.trim().to_string()
}

fn parse_composite_query(query: &str) -> Vec<&str> {
    if query.contains('/') {
        query
            .split('/')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect()
    } else if query.to_lowercase().contains(" and ") {
        query
            .split(" and ")
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect()
    } else {
        Vec::new()
    }
}

fn tokenize_query(query_lower: &str) -> Vec<&str> {
    const STOP_WORDS: &[&str] = &["and", "the", "of", "to", "in"];
    let tokens: Vec<_> = query_lower
        .split_whitespace()
        .filter(|token| token.len() > 1 && !STOP_WORDS.contains(token))
        .collect();
    if tokens.is_empty() {
        vec![query_lower]
    } else {
        tokens
    }
}

fn score_token(matcher: &SkimMatcherV2, entry: &LibraryEntry, token: &str) -> Option<i64> {
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
        score += 3_000 + boost;
        if entry.normalized_lower.contains(&format!(" {token} "))
            || entry.normalized_lower.starts_with(&format!("{token} "))
            || entry.normalized_lower.ends_with(&format!(" {token}"))
            || entry.normalized_lower == token
        {
            score += 2_000;
        }
    } else if entry.file_name_lower.contains(token) {
        score += 2_000 + boost;
    }

    (score > 0).then_some(score)
}

fn apply_threshold_filter(
    results: Vec<(i64, &LibraryEntry)>,
    minimum_desired: usize,
) -> Vec<(i64, &LibraryEntry)> {
    if results.len() <= minimum_desired {
        return results;
    }

    let top_score = results.first().map_or(0, |(score, _)| *score);
    let threshold = match top_score {
        score if score > 10_000 => 500,
        score if score > 5_000 => 300,
        _ => 100,
    };
    if results.len() > minimum_desired * 2 {
        let filtered: Vec<_> = results
            .iter()
            .filter(|(score, _)| *score >= threshold)
            .copied()
            .collect();
        if filtered.len() >= minimum_desired {
            return filtered;
        }
    }
    results
}

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
    use super::extract_hymn_number;

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
}
