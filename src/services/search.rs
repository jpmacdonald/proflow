//! Search strategies for file matching.
//!
//! This module provides abstractions for different search strategies
//! used to match `Planning Center` items to `ProPresenter` library files.

use crate::utils::file_matcher::FileEntry;

/// Trait for file search strategies.
///
/// Different strategies can be combined to provide comprehensive
/// matching with fallbacks.
pub trait SearchStrategy: Send + Sync {
    /// Find matching files for a query string.
    ///
    /// # Arguments
    /// * `query` - The search query (typically item title)
    /// * `files` - The available files to search
    /// * `limit` - Maximum number of results to return
    ///
    /// # Returns
    /// A vector of matching file entries, sorted by relevance.
    fn find_matches<'a>(
        &self,
        query: &str,
        files: &'a [FileEntry],
        limit: usize,
    ) -> Vec<&'a FileEntry>;

    /// Get the name of this search strategy (for debugging/logging).
    fn name(&self) -> &'static str;
}

/// Fuzzy string matching search strategy.
pub struct FuzzySearch {
    /// Minimum score threshold (0-1000).
    pub min_score: i64,
}

impl Default for FuzzySearch {
    fn default() -> Self {
        Self { min_score: 50 }
    }
}

impl SearchStrategy for FuzzySearch {
    fn find_matches<'a>(
        &self,
        query: &str,
        files: &'a [FileEntry],
        limit: usize,
    ) -> Vec<&'a FileEntry> {
        use fuzzy_matcher::skim::SkimMatcherV2;
        use fuzzy_matcher::FuzzyMatcher;

        let matcher = SkimMatcherV2::default();
        let query_lower = query.to_lowercase();

        let mut scored: Vec<_> = files
            .iter()
            .filter_map(|entry| {
                let score = matcher
                    .fuzzy_match(&entry.normalized_name.to_lowercase(), &query_lower)
                    .unwrap_or(0);
                if score >= self.min_score {
                    Some((entry, score))
                } else {
                    None
                }
            })
            .collect();

        scored.sort_by(|a, b| b.1.cmp(&a.1));
        scored.into_iter().take(limit).map(|(e, _)| e).collect()
    }

    fn name(&self) -> &'static str {
        "FuzzySearch"
    }
}

/// Liturgical term mapping for common worship items.
pub struct LiturgicalSearch {
    mappings: Vec<(&'static str, &'static [&'static str])>,
}

impl Default for LiturgicalSearch {
    fn default() -> Self {
        Self {
            mappings: vec![
                ("call to worship", &["call to worship", "gathering", "opening"]),
                ("gloria patri", &["gloria patri", "glory be", "doxology"]),
                ("doxology", &["doxology", "praise god from whom", "old 100th"]),
                ("lords prayer", &["lord's prayer", "our father"]),
                ("apostles creed", &["apostles creed", "apostle's creed", "i believe"]),
                ("nicene creed", &["nicene creed", "we believe"]),
                ("kyrie", &["kyrie", "lord have mercy"]),
                ("sanctus", &["sanctus", "holy holy holy"]),
                ("agnus dei", &["agnus dei", "lamb of god"]),
                ("benediction", &["benediction", "blessing", "go in peace"]),
                ("assurance", &["assurance of pardon", "words of assurance"]),
                ("confession", &["confession", "prayer of confession"]),
            ],
        }
    }
}

impl SearchStrategy for LiturgicalSearch {
    fn find_matches<'a>(
        &self,
        query: &str,
        files: &'a [FileEntry],
        limit: usize,
    ) -> Vec<&'a FileEntry> {
        let query_lower = query.to_lowercase();

        // Find applicable mappings
        let search_terms: Vec<&str> = self
            .mappings
            .iter()
            .filter(|(key, _)| query_lower.contains(key))
            .flat_map(|(_, terms)| terms.iter().copied())
            .collect();

        if search_terms.is_empty() {
            return Vec::new();
        }

        // Search files for any matching term
        files
            .iter()
            .filter(|entry| {
                let name_lower = entry.normalized_name.to_lowercase();
                search_terms.iter().any(|term| name_lower.contains(term))
            })
            .take(limit)
            .collect()
    }

    fn name(&self) -> &'static str {
        "LiturgicalSearch"
    }
}

/// Composite search that tries multiple strategies.
pub struct CompositeSearch {
    strategies: Vec<Box<dyn SearchStrategy>>,
}

impl CompositeSearch {
    /// Create a new composite search with the given strategies.
    pub fn new(strategies: Vec<Box<dyn SearchStrategy>>) -> Self {
        Self { strategies }
    }

    /// Create with default strategies (liturgical + fuzzy).
    pub fn with_defaults() -> Self {
        Self::new(vec![
            Box::new(LiturgicalSearch::default()),
            Box::new(FuzzySearch::default()),
        ])
    }
}

impl SearchStrategy for CompositeSearch {
    fn find_matches<'a>(
        &self,
        query: &str,
        files: &'a [FileEntry],
        limit: usize,
    ) -> Vec<&'a FileEntry> {
        let mut results = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for strategy in &self.strategies {
            for entry in strategy.find_matches(query, files, limit) {
                let key = &entry.full_path;
                if !seen.contains(key) {
                    seen.insert(key.clone());
                    results.push(entry);
                    if results.len() >= limit {
                        return results;
                    }
                }
            }
        }

        results
    }

    fn name(&self) -> &'static str {
        "CompositeSearch"
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;
    use std::path::PathBuf;

    fn make_entry(name: &str) -> FileEntry {
        FileEntry {
            file_name: format!("{name}.pro"),
            normalized_name: name.to_string(),
            file_name_lower: format!("{}.pro", name.to_lowercase()),
            normalized_lower: name.to_lowercase(),
            display_name: name.to_string(),
            _relative_path: String::new(),
            full_path: PathBuf::from(format!("/lib/{name}.pro")),
        }
    }

    #[test]
    fn test_fuzzy_search() {
        let files = vec![
            make_entry("Amazing Grace"),
            make_entry("How Great Thou Art"),
            make_entry("Be Thou My Vision"),
        ];

        let search = FuzzySearch::default();
        let results = search.find_matches("amazing", &files, 10);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].normalized_name, "Amazing Grace");
    }

    #[test]
    fn test_liturgical_search() {
        let files = vec![
            make_entry("Gloria Patri"),
            make_entry("Random Song"),
            make_entry("Doxology"),
        ];

        let search = LiturgicalSearch::default();
        let results = search.find_matches("gloria patri", &files, 10);

        assert!(!results.is_empty());
        assert!(results.iter().any(|e| e.normalized_name == "Gloria Patri"));
    }
}
