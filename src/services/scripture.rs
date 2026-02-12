//! Scripture lookup service.
//!
//! This module provides abstractions for scripture lookup and formatting.

/// A Bible version identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BibleVersion {
    /// Short code (e.g., "NRSV", "ESV", "NIV").
    pub code: String,
    /// Full name (e.g., "New Revised Standard Version").
    pub name: String,
}

impl BibleVersion {
    /// Create a new Bible version.
    pub fn new(code: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            name: name.into(),
        }
    }
}

/// A parsed scripture reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptureRef {
    /// Book name (e.g., "Genesis", "Matthew").
    pub book: String,
    /// Chapter number.
    pub chapter: u32,
    /// Starting verse.
    pub start_verse: u32,
    /// Ending verse (same as start for single verse).
    pub end_verse: u32,
}

impl ScriptureRef {
    /// Create a reference for a single verse.
    pub fn single(book: impl Into<String>, chapter: u32, verse: u32) -> Self {
        Self {
            book: book.into(),
            chapter,
            start_verse: verse,
            end_verse: verse,
        }
    }

    /// Create a reference for a verse range.
    pub fn range(book: impl Into<String>, chapter: u32, start: u32, end: u32) -> Self {
        Self {
            book: book.into(),
            chapter,
            start_verse: start,
            end_verse: end,
        }
    }

    /// Format as a display string (e.g., "Genesis 1:1-5").
    pub fn display(&self) -> String {
        if self.start_verse == self.end_verse {
            format!("{} {}:{}", self.book, self.chapter, self.start_verse)
        } else {
            format!(
                "{} {}:{}-{}",
                self.book, self.chapter, self.start_verse, self.end_verse
            )
        }
    }
}

/// A single verse with its text.
#[derive(Debug, Clone)]
pub struct Verse {
    /// Verse number.
    pub number: u32,
    /// Verse text.
    pub text: String,
}

/// Trait for scripture lookup providers.
///
/// Different implementations can provide scripture from various sources
/// (local files, API, embedded data).
pub trait ScriptureProvider {
    /// Look up verses for a scripture reference.
    ///
    /// # Arguments
    /// * `reference` - The scripture reference to look up
    /// * `version` - The Bible version to use
    ///
    /// # Returns
    /// A vector of verses, or an error if lookup failed.
    fn lookup(
        &self,
        reference: &ScriptureRef,
        version: &BibleVersion,
    ) -> Result<Vec<Verse>, crate::error::Error>;

    /// Get the list of available Bible versions.
    fn available_versions(&self) -> Vec<BibleVersion>;

    /// Check if a specific version is available.
    fn has_version(&self, code: &str) -> bool {
        self.available_versions().iter().any(|v| v.code == code)
    }
}

/// Parse a scripture reference string.
///
/// Supports formats like:
/// - "Genesis 1:1"
/// - "Gen 1:1-5"
/// - "1 John 3:16"
/// - "Psalm 23"
pub fn parse_reference(input: &str) -> Option<ScriptureRef> {
    // This is a simplified parser - the actual implementation in bible/mod.rs is more complete
    let input = input.trim();

    // Find the chapter:verse part
    let parts: Vec<&str> = input.rsplitn(2, ' ').collect();
    if parts.len() != 2 {
        return None;
    }

    let book = parts[1].to_string();
    let chapter_verse = parts[0];

    // Parse chapter:verse or chapter:start-end
    let cv_parts: Vec<&str> = chapter_verse.split(':').collect();
    if cv_parts.is_empty() || cv_parts.len() > 2 {
        return None;
    }

    let chapter: u32 = cv_parts[0].parse().ok()?;

    if cv_parts.len() == 1 {
        // Just chapter, assume verse 1
        return Some(ScriptureRef::single(book, chapter, 1));
    }

    let verse_part = cv_parts[1];
    if let Some(dash_pos) = verse_part.find('-') {
        let start: u32 = verse_part[..dash_pos].parse().ok()?;
        let end: u32 = verse_part[dash_pos + 1..].parse().ok()?;
        Some(ScriptureRef::range(book, chapter, start, end))
    } else {
        let verse: u32 = verse_part.parse().ok()?;
        Some(ScriptureRef::single(book, chapter, verse))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn test_scripture_ref_display() {
        let single = ScriptureRef::single("Genesis", 1, 1);
        assert_eq!(single.display(), "Genesis 1:1");

        let range = ScriptureRef::range("Psalm", 23, 1, 6);
        assert_eq!(range.display(), "Psalm 23:1-6");
    }

    #[test]
    fn test_parse_reference() {
        let single = parse_reference("Genesis 1:1");
        assert!(single.is_some());
        let single = single.unwrap();
        assert_eq!(single.book, "Genesis");
        assert_eq!(single.chapter, 1);
        assert_eq!(single.start_verse, 1);

        let range = parse_reference("Psalm 23:1-6");
        assert!(range.is_some());
        let range = range.unwrap();
        assert_eq!(range.book, "Psalm");
        assert_eq!(range.start_verse, 1);
        assert_eq!(range.end_verse, 6);
    }

    #[test]
    fn test_bible_version() {
        let nrsv = BibleVersion::new("NRSV", "New Revised Standard Version");
        assert_eq!(nrsv.code, "NRSV");
        assert_eq!(nrsv.name, "New Revised Standard Version");
    }
}
