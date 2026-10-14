//! Bible verse lookup and scripture reference parsing.

mod reference;
use reference::VerseRange;
pub use reference::{DisjointVerseRanges, ScriptureRef, VerseSelection};
mod lookup_error;
pub use lookup_error::{BibleCorpusError, ScriptureLookupError};
mod book_names;
use book_names::BOOK_ALIASES;
mod parse;
pub use parse::parse_scripture_ref;
mod corpus;
pub use corpus::{BibleCorpusFingerprint, BibleCorpusSnapshot};

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use sha2::Sha256;

mod excerpt;
mod version;

pub use version::{BibleTranslations, BibleVersion};

pub use excerpt::{
    reconcile_prefix_excerpt, reconcile_selected_excerpt, ScriptureExcerptError,
    SelectedScriptureExcerptError,
};

/// Bible data structure: Book -> Chapter -> Verse -> Text
type BibleData = HashMap<String, HashMap<String, HashMap<String, String>>>;

/// Validate every installed Bible corpus and reject duplicate translation
/// identities.
///
/// Missing files are allowed because a project may install only the
/// translations it uses. A requested version is checked separately at the
/// reviewed-source boundary. Every file that is present must parse and must
/// not be mislabeled as a second byte-identical translation.
pub fn validate_bible_corpora(root: &std::path::Path) -> Result<(), BibleCorpusError> {
    BibleCorpusSnapshot::capture(root.to_path_buf()).map(|_| ())
}

#[derive(Debug)]
struct CachedBibleData {
    source_sha256: [u8; 32],
    data: BibleData,
}

/// Superscript digit mapping
const SUPERSCRIPT_DIGITS: &[char] = &['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];

/// Convert a number to superscript Unicode characters.
///
/// These will be converted to RTF `\super` tags during `.pro` export.
pub fn to_superscript(n: u32) -> String {
    n.to_string()
        .chars()
        .map(|c| SUPERSCRIPT_DIGITS[c.to_digit(10).unwrap_or(0) as usize])
        .collect()
}

/// A single verse with its number and plain text content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verse {
    /// 1-based verse number.
    pub number: u32,
    /// Verse text without superscript prefix.
    pub text: String,
}

/// Scripture header info for display in pane title
#[derive(Debug, Clone)]
pub struct ScriptureHeader {
    /// Canonical book name
    pub book: String,
    /// Chapter number
    pub chapter: u32,
    /// Exact verse selection.
    pub verses: VerseSelection,
    /// Bible version used for lookup
    pub version: BibleVersion,
    /// Verse numbers in the requested range that were not found in the data.
    pub missing_verses: Vec<u32>,
}

impl ScriptureHeader {
    /// Format for display (e.g., "Isaiah 32:15-17 `NRSVue`").
    pub fn display(&self) -> String {
        let reference = ScriptureRef {
            book: self.book.clone(),
            chapter: self.chapter,
            verses: self.verses.clone(),
        };
        format!("{reference} {}", self.version.name())
    }
}

#[cfg(test)]
mod tests;
