//! Hymnal lookup service for curated .txt hymn files.
//!
//! Scans a directory of files named `#NUMBER - Title.txt` and provides
//! lookup by hymn number (extracted from item titles) or fuzzy title match.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock;

use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use regex::Regex;

/// Regex matching `#510` style hymn numbers.
#[allow(clippy::expect_used)]
static RE_HASH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"#(\d+)").expect("valid regex: RE_HASH")
});

/// Regex matching `Hymn 510` or `Hymn #510` patterns.
#[allow(clippy::expect_used)]
static RE_HYMN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)[Hh]ymn\s*#?(\d+)").expect("valid regex: RE_HYMN")
});

/// Regex matching hymnal filenames like `#510 - Jesus Shall Reign`.
#[allow(clippy::expect_used)]
static RE_FILENAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^#(\d+)\s*-\s*(.+)$").expect("valid regex: RE_FILENAME")
});

/// A single hymn loaded from disk.
#[derive(Debug, Clone)]
pub struct HymnEntry {
    /// Hymn number in the hymnal.
    pub number: u32,
    /// Display title of the hymn.
    pub title: String,
    /// Lowercased title for case-insensitive matching.
    title_lower: String,
    /// Lines of hymn content.
    pub content: Vec<String>,
}

/// Lazily loaded hymnal directory index.
pub struct HymnalService {
    hymnal_path: PathBuf,
    by_number: HashMap<u32, usize>,
    entries: Vec<HymnEntry>,
    loaded: bool,
}

impl HymnalService {
    /// Create a new hymnal service backed by the given directory path.
    pub fn new(path: PathBuf) -> Self {
        Self {
            hymnal_path: path,
            by_number: HashMap::new(),
            entries: Vec::new(),
            loaded: false,
        }
    }

    /// Try to match an item title to a hymn, returning (hymn title, content lines).
    ///
    /// Checks by extracted number first (O(1)), then falls back to fuzzy title match.
    pub fn lookup_from_title(&mut self, item_title: &str) -> Option<(String, Vec<String>)> {
        self.ensure_loaded();

        if let Some(num) = extract_hymn_number(item_title) {
            if let Some(entry) = self.lookup_by_number(num) {
                return Some((entry.title.clone(), entry.content.clone()));
            }
        }

        self.lookup_by_title(item_title)
    }

    fn ensure_loaded(&mut self) {
        if !self.loaded {
            self.load();
        }
    }

    fn load(&mut self) {
        self.loaded = true;

        let dir = match std::fs::read_dir(&self.hymnal_path) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("Failed to read hymnal directory {}: {e}", self.hymnal_path.display());
                return;
            }
        };

        for entry in dir.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "txt") {
                continue;
            }

            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };

            let Some((number, title)) = parse_hymnal_filename(stem) else {
                continue;
            };

            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c.lines().map(String::from).collect::<Vec<_>>(),
                Err(_) => continue,
            };

            let idx = self.entries.len();
            self.entries.push(HymnEntry {
                number,
                title_lower: title.to_lowercase(),
                title,
                content,
            });
            self.by_number.insert(number, idx);
        }

        tracing::info!("Loaded {} hymns from {}", self.entries.len(), self.hymnal_path.display());
    }

    fn lookup_by_number(&self, number: u32) -> Option<&HymnEntry> {
        self.by_number.get(&number).and_then(|&idx| self.entries.get(idx))
    }

    fn lookup_by_title(&self, query: &str) -> Option<(String, Vec<String>)> {
        // Fuzzy match with a minimum quality threshold
        const MIN_SCORE: i64 = 80;

        if self.entries.is_empty() {
            return None;
        }

        let matcher = SkimMatcherV2::default();
        let query_lower = query.to_lowercase();

        // Exact substring match wins outright
        for entry in &self.entries {
            if entry.title_lower == query_lower
                || query_lower.contains(&entry.title_lower)
                || entry.title_lower.contains(&query_lower)
            {
                return Some((entry.title.clone(), entry.content.clone()));
            }
        }

        let best = self.entries.iter()
            .filter_map(|entry| {
                let score = matcher.fuzzy_match(&entry.title, query)?;
                (score >= MIN_SCORE).then_some((score, entry))
            })
            .max_by_key(|(score, _)| *score);

        best.map(|(_, entry)| (entry.title.clone(), entry.content.clone()))
    }
}

/// Extract a hymn number from an item title.
///
/// Recognizes patterns like `#510`, `Hymn #510`, `Hymn 510`.
fn extract_hymn_number(text: &str) -> Option<u32> {
    RE_HASH.captures(text)
        .or_else(|| RE_HYMN.captures(text))
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse::<u32>().ok())
}

/// Parse a hymnal filename like `#510 - Jesus Shall Reign` into (510, "Jesus Shall Reign").
fn parse_hymnal_filename(stem: &str) -> Option<(u32, String)> {
    let caps = RE_FILENAME.captures(stem)?;
    let number = caps.get(1)?.as_str().parse::<u32>().ok()?;
    let title = caps.get(2)?.as_str().trim().to_string();
    Some((number, title))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn test_extract_hymn_number_hash() {
        assert_eq!(extract_hymn_number("#510 Jesus Shall Reign"), Some(510));
        assert_eq!(extract_hymn_number("Hymn #42 Amazing Grace"), Some(42));
    }

    #[test]
    fn test_extract_hymn_number_word() {
        assert_eq!(extract_hymn_number("Hymn 123"), Some(123));
        assert_eq!(extract_hymn_number("hymn 7"), Some(7));
    }

    #[test]
    fn test_extract_hymn_number_none() {
        assert_eq!(extract_hymn_number("Just a title"), None);
        assert_eq!(extract_hymn_number("Call to Worship"), None);
    }

    #[test]
    fn test_parse_hymnal_filename() {
        let (num, title) = parse_hymnal_filename("#510 - Jesus Shall Reign").unwrap();
        assert_eq!(num, 510);
        assert_eq!(title, "Jesus Shall Reign");
    }

    #[test]
    fn test_parse_hymnal_filename_tight_spacing() {
        let (num, title) = parse_hymnal_filename("#42-Amazing Grace").unwrap();
        assert_eq!(num, 42);
        assert_eq!(title, "Amazing Grace");
    }

    #[test]
    fn test_parse_hymnal_filename_invalid() {
        assert!(parse_hymnal_filename("Some Random File").is_none());
        assert!(parse_hymnal_filename("510 - No Hash").is_none());
    }

    #[test]
    fn test_lookup_from_filesystem() {
        // Construct a HymnalService pointing at a nonexistent directory — should load gracefully
        let mut svc = HymnalService::new(PathBuf::from("/tmp/nonexistent_hymnal_dir_proflow_test"));
        assert!(svc.lookup_from_title("#999 Test").is_none());
    }
}
