//! Typed corpus and passage lookup failures.

use super::BibleVersion;
use super::PathBuf;

/// Failure to validate the translation corpora in one project data bundle.
#[derive(Debug, thiserror::Error)]
pub enum BibleCorpusError {
    /// A corpus file could not be read.
    #[error("failed to read {version} corpus at {}: {source}", path.display())]
    Read {
        /// Translation assigned to the file.
        version: BibleVersion,
        /// Exact corpus path.
        path: PathBuf,
        /// Filesystem failure.
        source: std::io::Error,
    },
    /// A corpus file is not valid Bible JSON.
    #[error("invalid {version} corpus at {}: {source}", path.display())]
    Parse {
        /// Translation assigned to the file.
        version: BibleVersion,
        /// Exact corpus path.
        path: PathBuf,
        /// JSON failure.
        source: serde_json::Error,
    },
    /// The corpus directory could not be listed.
    #[error("failed to list Bible corpora in {}: {source}", path.display())]
    List {
        /// Corpus directory.
        path: PathBuf,
        /// Filesystem failure.
        source: std::io::Error,
    },
    /// Two files name the same translation in different letter case.
    #[error("more than one corpus file names {version}; {} is the second", path.display())]
    DuplicateFile {
        /// Translation named twice.
        version: BibleVersion,
        /// The later of the two files.
        path: PathBuf,
    },
    /// Two translation labels point at identical source text.
    #[error("Bible corpora for {first} and {second} are byte-identical; translation identity is ambiguous")]
    DuplicateTranslation {
        /// First translation using the bytes.
        first: BibleVersion,
        /// Conflicting translation using the same bytes.
        second: BibleVersion,
    },
    /// Corpus bytes no longer match the immutable runtime snapshot.
    #[error(
        "{version} corpus at {} changed after startup (expected SHA-256 {expected}, found {actual})",
        path.display()
    )]
    Changed {
        /// Translation assigned to the file.
        version: BibleVersion,
        /// Exact corpus path.
        path: PathBuf,
        /// Hash captured by the immutable snapshot.
        expected: String,
        /// Hash observed later.
        actual: String,
    },
}

/// Failure to read verses from the captured Bible corpora.
#[derive(Debug, thiserror::Error)]
pub enum ScriptureLookupError {
    /// The requested translation was not present in the data root at capture.
    #[error("Bible corpus for {} is not installed: {}", version.name(), path.display())]
    CorpusNotInstalled {
        /// Requested translation.
        version: BibleVersion,
        /// Path the translation file would occupy.
        path: PathBuf,
    },
    /// Caller-supplied source bytes do not match the captured corpus.
    #[error(transparent)]
    CorpusChanged(#[from] BibleCorpusError),
    /// The corpus has no book with this canonical name.
    #[error("book not found: {book}")]
    BookNotFound {
        /// Canonical book name requested.
        book: String,
    },
    /// The book has no such chapter.
    #[error("chapter {chapter} not found in {book}")]
    ChapterNotFound {
        /// Canonical book name.
        book: String,
        /// Requested chapter.
        chapter: u32,
    },
    /// A chapter object carries a verse key that is not a number.
    #[error("invalid verse key '{key}' in {book} {chapter}")]
    InvalidVerseKey {
        /// Canonical book name.
        book: String,
        /// Chapter containing the key.
        chapter: u32,
        /// Offending key.
        key: String,
    },
}
