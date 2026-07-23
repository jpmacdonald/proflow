//! Bible verse lookup and scripture reference parsing.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;
use std::path::PathBuf;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

mod excerpt;

pub use excerpt::{reconcile_prefix_excerpt, ScriptureExcerptError};

/// Supported Bible versions
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, schemars::JsonSchema,
)]
#[allow(clippy::upper_case_acronyms)] // NRSV, NIV, KJV are standard Bible version abbreviations
pub enum BibleVersion {
    /// New Revised Standard Version Updated Edition
    #[default]
    #[serde(rename = "NRSVue", alias = "NRSVUE")]
    NRSVue,
    /// New Revised Standard Version
    #[serde(rename = "NRSV")]
    NRSV,
    /// New International Version
    #[serde(rename = "NIV")]
    NIV,
    /// King James Version
    #[serde(rename = "KJV")]
    KJV,
    /// New King James Version
    #[serde(rename = "NKJV")]
    NKJV,
    /// New Living Translation
    #[serde(rename = "NLT")]
    NLT,
    /// New American Standard Bible
    #[serde(rename = "NASB")]
    NASB,
}

impl BibleVersion {
    /// Returns all available Bible versions.
    pub const fn all() -> &'static [Self] {
        &[
            Self::NRSVue,
            Self::NRSV,
            Self::NIV,
            Self::KJV,
            Self::NKJV,
            Self::NLT,
            Self::NASB,
        ]
    }

    /// Returns the human-readable name of this version.
    pub const fn name(self) -> &'static str {
        match self {
            Self::NRSVue => "NRSVue",
            Self::NRSV => "NRSV",
            Self::NIV => "NIV",
            Self::KJV => "KJV",
            Self::NKJV => "NKJV",
            Self::NLT => "NLT",
            Self::NASB => "NASB",
        }
    }

    /// Returns the JSON data filename for this version.
    pub const fn file_name(self) -> &'static str {
        match self {
            Self::NRSVue => "NRSVUE.json",
            Self::NRSV => "NRSV.json",
            Self::NIV => "NIV.json",
            Self::KJV => "KJV.json",
            Self::NKJV => "NKJV.json",
            Self::NLT => "NLT.json",
            Self::NASB => "NASB.json",
        }
    }

    /// Parse one exact supported translation identifier.
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_uppercase().as_str() {
            "NRSVUE" => Some(Self::NRSVue),
            "NRSV" => Some(Self::NRSV),
            "NIV" => Some(Self::NIV),
            "KJV" => Some(Self::KJV),
            "NKJV" => Some(Self::NKJV),
            "NLT" => Some(Self::NLT),
            "NASB" => Some(Self::NASB),
            _ => None,
        }
    }

    /// Try to detect version from text like "(NRSV)" or "`NRSVue`".
    pub fn from_text(text: &str) -> Option<Self> {
        let upper = text.to_uppercase();
        if upper.contains("NRSVUE") {
            return Some(Self::NRSVue);
        }
        if upper.contains("NRSV") {
            return Some(Self::NRSV);
        }
        if upper.contains("NKJV") {
            return Some(Self::NKJV);
        }
        if upper.contains("NIV") {
            return Some(Self::NIV);
        }
        if upper.contains("NASB") {
            return Some(Self::NASB);
        }
        if upper.contains("NLT") {
            return Some(Self::NLT);
        }
        if upper.contains("KJV") {
            return Some(Self::KJV);
        }
        None
    }
}

/// Verse selection within one scripture chapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerseSelection {
    /// Every verse present in the chapter.
    Chapter,
    /// One exact verse.
    Verse(u32),
    /// One inclusive verse range.
    Range {
        /// First requested verse.
        start: u32,
        /// Last requested verse.
        end: u32,
    },
    /// Multiple ordered, non-overlapping ranges in the same chapter.
    Disjoint(DisjointVerseRanges),
}

/// Checked ranges for a discontinuous selection such as `1-5, 9-17`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisjointVerseRanges(Box<[VerseRange]>);

impl DisjointVerseRanges {
    fn new(ranges: Vec<VerseRange>) -> Option<Self> {
        if ranges.len() < 2 || ranges.windows(2).any(|pair| pair[0].end >= pair[1].start) {
            return None;
        }
        Some(Self(ranges.into_boxed_slice()))
    }

    fn iter(&self) -> impl Iterator<Item = &VerseRange> {
        self.0.iter()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VerseRange {
    start: u32,
    end: u32,
}

impl std::fmt::Display for VerseRange {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.start == self.end {
            write!(formatter, "{}", self.start)
        } else {
            write!(formatter, "{}-{}", self.start, self.end)
        }
    }
}

/// A parsed scripture reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptureRef {
    /// Canonical book name (e.g., "Genesis")
    pub book: String,
    /// Chapter number
    pub chapter: u32,
    /// Exact verse selection.
    pub verses: VerseSelection,
}

impl std::fmt::Display for ScriptureRef {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.verses {
            VerseSelection::Chapter => write!(formatter, "{} {}", self.book, self.chapter),
            VerseSelection::Verse(verse) => {
                write!(formatter, "{} {}:{verse}", self.book, self.chapter)
            }
            VerseSelection::Range { start, end } => {
                write!(formatter, "{} {}:{start}-{end}", self.book, self.chapter)
            }
            VerseSelection::Disjoint(ranges) => {
                write!(formatter, "{} {}:", self.book, self.chapter)?;
                for (index, range) in ranges.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str(", ")?;
                    }
                    write!(formatter, "{range}")?;
                }
                Ok(())
            }
        }
    }
}

impl ScriptureRef {
    /// Zero-based canonical Protestant book index used by native Bible metadata.
    #[must_use]
    pub fn canonical_book_index(&self) -> Option<u32> {
        const BOOKS: &[&str] = &[
            "Genesis",
            "Exodus",
            "Leviticus",
            "Numbers",
            "Deuteronomy",
            "Joshua",
            "Judges",
            "Ruth",
            "1 Samuel",
            "2 Samuel",
            "1 Kings",
            "2 Kings",
            "1 Chronicles",
            "2 Chronicles",
            "Ezra",
            "Nehemiah",
            "Esther",
            "Job",
            "Psalms",
            "Proverbs",
            "Ecclesiastes",
            "Song of Solomon",
            "Isaiah",
            "Jeremiah",
            "Lamentations",
            "Ezekiel",
            "Daniel",
            "Hosea",
            "Joel",
            "Amos",
            "Obadiah",
            "Jonah",
            "Micah",
            "Nahum",
            "Habakkuk",
            "Zephaniah",
            "Haggai",
            "Zechariah",
            "Malachi",
            "Matthew",
            "Mark",
            "Luke",
            "John",
            "Acts",
            "Romans",
            "1 Corinthians",
            "2 Corinthians",
            "Galatians",
            "Ephesians",
            "Philippians",
            "Colossians",
            "1 Thessalonians",
            "2 Thessalonians",
            "1 Timothy",
            "2 Timothy",
            "Titus",
            "Philemon",
            "Hebrews",
            "James",
            "1 Peter",
            "2 Peter",
            "1 John",
            "2 John",
            "3 John",
            "Jude",
            "Revelation",
        ];
        BOOKS
            .iter()
            .position(|book| *book == self.book)
            .and_then(|index| u32::try_from(index).ok())
    }
}

/// Bible data structure: Book -> Chapter -> Verse -> Text
type BibleData = HashMap<String, HashMap<String, HashMap<String, String>>>;

/// Failure to validate the translation corpora in one project data bundle.
#[derive(Debug, thiserror::Error)]
pub enum BibleCorpusError {
    /// A corpus file could not be read.
    #[error("failed to read {version} corpus at {}: {source}", path.display())]
    Read {
        /// Translation assigned to the file.
        version: &'static str,
        /// Exact corpus path.
        path: PathBuf,
        /// Filesystem failure.
        source: std::io::Error,
    },
    /// A corpus file is not valid Bible JSON.
    #[error("invalid {version} corpus at {}: {source}", path.display())]
    Parse {
        /// Translation assigned to the file.
        version: &'static str,
        /// Exact corpus path.
        path: PathBuf,
        /// JSON failure.
        source: serde_json::Error,
    },
    /// Two translation labels point at identical source text.
    #[error("Bible corpora for {first} and {second} are byte-identical; translation identity is ambiguous")]
    DuplicateTranslation {
        /// First translation using the bytes.
        first: &'static str,
        /// Conflicting translation using the same bytes.
        second: &'static str,
    },
    /// Corpus bytes no longer match the immutable runtime snapshot.
    #[error(
        "{version} corpus at {} changed after startup (expected SHA-256 {expected}, found {actual})",
        path.display()
    )]
    Changed {
        /// Translation assigned to the file.
        version: &'static str,
        /// Exact corpus path.
        path: PathBuf,
        /// Hash captured by the immutable snapshot.
        expected: String,
        /// Hash observed later.
        actual: String,
    },
}

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

/// Book name normalization map
static BOOK_ALIASES: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    // Common abbreviations and variations
    m.insert("gen", "Genesis");
    m.insert("genesis", "Genesis");
    m.insert("ex", "Exodus");
    m.insert("exod", "Exodus");
    m.insert("exodus", "Exodus");
    m.insert("lev", "Leviticus");
    m.insert("leviticus", "Leviticus");
    m.insert("num", "Numbers");
    m.insert("numbers", "Numbers");
    m.insert("deut", "Deuteronomy");
    m.insert("deuteronomy", "Deuteronomy");
    m.insert("josh", "Joshua");
    m.insert("joshua", "Joshua");
    m.insert("judg", "Judges");
    m.insert("judges", "Judges");
    m.insert("ruth", "Ruth");
    m.insert("1 sam", "1 Samuel");
    m.insert("1 samuel", "1 Samuel");
    m.insert("1sam", "1 Samuel");
    m.insert("2 sam", "2 Samuel");
    m.insert("2 samuel", "2 Samuel");
    m.insert("2sam", "2 Samuel");
    m.insert("1 kings", "1 Kings");
    m.insert("1 kgs", "1 Kings");
    m.insert("1kings", "1 Kings");
    m.insert("2 kings", "2 Kings");
    m.insert("2 kgs", "2 Kings");
    m.insert("2kings", "2 Kings");
    m.insert("1 chr", "1 Chronicles");
    m.insert("1 chronicles", "1 Chronicles");
    m.insert("1chronicles", "1 Chronicles");
    m.insert("2 chr", "2 Chronicles");
    m.insert("2 chronicles", "2 Chronicles");
    m.insert("2chronicles", "2 Chronicles");
    m.insert("ezra", "Ezra");
    m.insert("neh", "Nehemiah");
    m.insert("nehemiah", "Nehemiah");
    m.insert("esth", "Esther");
    m.insert("esther", "Esther");
    m.insert("job", "Job");
    m.insert("ps", "Psalms");
    m.insert("psalm", "Psalms");
    m.insert("psalms", "Psalms");
    m.insert("prov", "Proverbs");
    m.insert("proverbs", "Proverbs");
    m.insert("eccl", "Ecclesiastes");
    m.insert("ecclesiastes", "Ecclesiastes");
    m.insert("song", "Song of Solomon");
    m.insert("song of solomon", "Song of Solomon");
    m.insert("song of songs", "Song of Solomon");
    m.insert("isa", "Isaiah");
    m.insert("isaiah", "Isaiah");
    m.insert("jer", "Jeremiah");
    m.insert("jeremiah", "Jeremiah");
    m.insert("lam", "Lamentations");
    m.insert("lamentations", "Lamentations");
    m.insert("ezek", "Ezekiel");
    m.insert("ezekiel", "Ezekiel");
    m.insert("dan", "Daniel");
    m.insert("daniel", "Daniel");
    m.insert("hos", "Hosea");
    m.insert("hosea", "Hosea");
    m.insert("joel", "Joel");
    m.insert("amos", "Amos");
    m.insert("obad", "Obadiah");
    m.insert("obadiah", "Obadiah");
    m.insert("jonah", "Jonah");
    m.insert("mic", "Micah");
    m.insert("micah", "Micah");
    m.insert("nah", "Nahum");
    m.insert("nahum", "Nahum");
    m.insert("hab", "Habakkuk");
    m.insert("habakkuk", "Habakkuk");
    m.insert("zeph", "Zephaniah");
    m.insert("zephaniah", "Zephaniah");
    m.insert("hag", "Haggai");
    m.insert("haggai", "Haggai");
    m.insert("zech", "Zechariah");
    m.insert("zechariah", "Zechariah");
    m.insert("mal", "Malachi");
    m.insert("malachi", "Malachi");
    // New Testament
    m.insert("matt", "Matthew");
    m.insert("matthew", "Matthew");
    m.insert("mark", "Mark");
    m.insert("luke", "Luke");
    m.insert("john", "John");
    m.insert("acts", "Acts");
    m.insert("rom", "Romans");
    m.insert("romans", "Romans");
    m.insert("1 cor", "1 Corinthians");
    m.insert("1 corinthians", "1 Corinthians");
    m.insert("1cor", "1 Corinthians");
    m.insert("2 cor", "2 Corinthians");
    m.insert("2 corinthians", "2 Corinthians");
    m.insert("2cor", "2 Corinthians");
    m.insert("gal", "Galatians");
    m.insert("galatians", "Galatians");
    m.insert("eph", "Ephesians");
    m.insert("ephesians", "Ephesians");
    m.insert("phil", "Philippians");
    m.insert("philippians", "Philippians");
    m.insert("col", "Colossians");
    m.insert("colossians", "Colossians");
    m.insert("1 thess", "1 Thessalonians");
    m.insert("1 thessalonians", "1 Thessalonians");
    m.insert("1thess", "1 Thessalonians");
    m.insert("2 thess", "2 Thessalonians");
    m.insert("2 thessalonians", "2 Thessalonians");
    m.insert("2thess", "2 Thessalonians");
    m.insert("1 tim", "1 Timothy");
    m.insert("1 timothy", "1 Timothy");
    m.insert("1tim", "1 Timothy");
    m.insert("2 tim", "2 Timothy");
    m.insert("2 timothy", "2 Timothy");
    m.insert("2tim", "2 Timothy");
    m.insert("titus", "Titus");
    m.insert("philem", "Philemon");
    m.insert("philemon", "Philemon");
    m.insert("heb", "Hebrews");
    m.insert("hebrews", "Hebrews");
    m.insert("james", "James");
    m.insert("jas", "James");
    m.insert("1 pet", "1 Peter");
    m.insert("1 peter", "1 Peter");
    m.insert("1pet", "1 Peter");
    m.insert("2 pet", "2 Peter");
    m.insert("2 peter", "2 Peter");
    m.insert("2pet", "2 Peter");
    m.insert("1 john", "1 John");
    m.insert("1john", "1 John");
    m.insert("2 john", "2 John");
    m.insert("2john", "2 John");
    m.insert("3 john", "3 John");
    m.insert("3john", "3 John");
    m.insert("jude", "Jude");
    m.insert("rev", "Revelation");
    m.insert("revelation", "Revelation");
    m.insert("revelations", "Revelation");
    m
});

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

/// Normalize book name to canonical form
fn normalize_book_name(name: &str) -> Option<&'static str> {
    let lower = name.to_lowercase();
    let trimmed = lower.trim();

    // Direct lookup
    if let Some(&canonical) = BOOK_ALIASES.get(trimmed) {
        return Some(canonical);
    }

    // Try without spaces for numbered books
    let no_space = trimmed.replace(' ', "");
    if let Some(&canonical) = BOOK_ALIASES.get(no_space.as_str()) {
        return Some(canonical);
    }

    None
}

/// Parse a scripture reference string like "Isaiah 32:15-17" or "1 John 3:1-3".
///
/// Also handles complex titles like "Scripture: Isaiah 32:15-17; Luke 1:76-79
/// `NRSVue` (Hope)" or "Scripture - Isaiah 35:1-10 (Adrian)".
pub fn parse_scripture_ref(text: &str) -> Option<ScriptureRef> {
    let text = strip_scripture_heading(text);

    // Semicolons separate passages. Commas remain part of a single reference
    // because they can select discontinuous ranges in one chapter.
    let first_ref = text.split(';').next()?.trim();

    // Remove version and location indicators like "(NRSV)" or "(Hope)" or "NRSVue"
    // Also handle version without parens at end
    let cleaned = first_ref
        .split('(')
        .next()?
        .trim()
        .trim_end_matches("NRSVue")
        .trim_end_matches("NRSVUE")
        .trim_end_matches("NRSV")
        .trim_end_matches("NKJV")
        .trim_end_matches("NIV")
        .trim_end_matches("NLT")
        .trim_end_matches("NASB")
        .trim_end_matches("KJV")
        .trim_end_matches("ESV")
        .trim();

    parse_single_reference(cleaned)
}

fn strip_scripture_heading(text: &str) -> &str {
    let trimmed = text.trim();
    for prefix in ["Scripture Reading", "Scripture", "Reading"] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return strip_heading_suffix(rest);
        }
    }
    trimmed
}

fn strip_heading_suffix(rest: &str) -> &str {
    let mut value = rest.trim_start();
    if let Some(stripped) = value.strip_prefix('(') {
        if let Some(end) = stripped.find(')') {
            value = stripped[end + 1..].trim_start();
        }
    }
    value
        .strip_prefix(':')
        .or_else(|| value.strip_prefix('-'))
        .map_or(value, str::trim_start)
        .trim()
}

/// Parse a single scripture reference like "Isaiah 32:15-17"
fn parse_single_reference(text: &str) -> Option<ScriptureRef> {
    // Normalize en-dash/em-dash to ASCII hyphen (PCO often uses typographic dashes)
    let text = text.replace(['\u{2013}', '\u{2014}'], "-");

    // Try every word boundary so whitespace after a comma remains part of the
    // verse selection while spaces inside canonical book names still work.
    text.char_indices()
        .rev()
        .filter(|(_, character)| character.is_whitespace())
        .find_map(|(boundary, _)| {
            let book = normalize_book_name(text[..boundary].trim())?;
            let (chapter, verses) = parse_chapter_selection(text[boundary..].trim())?;
            Some(ScriptureRef {
                book: book.to_string(),
                chapter,
                verses,
            })
        })
}

fn parse_chapter_selection(text: &str) -> Option<(u32, VerseSelection)> {
    let chapter_and_verses = text.split_once(':').or_else(|| text.split_once('v'));
    let (chapter_str, verse_text) = chapter_and_verses.unwrap_or((text, ""));
    let chapter: u32 = chapter_str.parse().ok()?;
    if chapter == 0 {
        return None;
    }

    let verses = if verse_text.is_empty() {
        VerseSelection::Chapter
    } else {
        let ranges = verse_text
            .split(',')
            .map(str::trim)
            .map(parse_verse_range)
            .collect::<Option<Vec<_>>>()?;
        match ranges.as_slice() {
            [range] if range.start == range.end => VerseSelection::Verse(range.start),
            [range] => VerseSelection::Range {
                start: range.start,
                end: range.end,
            },
            _ => VerseSelection::Disjoint(DisjointVerseRanges::new(ranges)?),
        }
    };
    Some((chapter, verses))
}

fn parse_verse_range(text: &str) -> Option<VerseRange> {
    let (start, end) = text.split_once('-').map_or_else(
        || text.parse().ok().map(|verse| (verse, verse)),
        |(start, end)| Some((start.parse().ok()?, end.parse().ok()?)),
    )?;
    (start > 0 && end >= start).then_some(VerseRange { start, end })
}

/// Immutable, parsed Bible corpora captured from one project data bundle.
///
/// Review reconciliation and rendering share this exact snapshot. A caller
/// supplying reviewed file bytes must prove they still match the parsed
/// corpus, so a changed file cannot be attributed to stale in-memory text.
#[derive(Debug)]
pub struct BibleCorpusSnapshot {
    root: PathBuf,
    corpora: HashMap<BibleVersion, CachedBibleData>,
}

impl BibleCorpusSnapshot {
    /// Parse every installed translation and reject duplicate identities.
    ///
    /// Missing translations are allowed until a plan requests one.
    pub fn capture(root: PathBuf) -> Result<Self, BibleCorpusError> {
        let mut hashes = BTreeMap::<[u8; 32], &'static str>::new();
        let mut corpora = HashMap::new();
        for version in BibleVersion::all() {
            let path = root.join(version.file_name());
            if !path.exists() {
                continue;
            }
            let bytes = read_corpus(*version, &path)?;
            let source_sha256: [u8; 32] = Sha256::digest(&bytes).into();
            if let Some(first) = hashes.insert(source_sha256, version.name()) {
                return Err(BibleCorpusError::DuplicateTranslation {
                    first,
                    second: version.name(),
                });
            }
            let data = parse_corpus(*version, &path, &bytes)?;
            corpora.insert(
                *version,
                CachedBibleData {
                    source_sha256,
                    data,
                },
            );
        }
        Ok(Self { root, corpora })
    }

    /// Verify every installed corpus still matches this snapshot.
    pub fn verify_current(&self) -> Result<(), BibleCorpusError> {
        for (version, corpus) in &self.corpora {
            let path = self.root.join(version.file_name());
            let bytes = read_corpus(*version, &path)?;
            Self::verify_bytes(*version, &path, &bytes, corpus)?;
        }
        Ok(())
    }

    /// Look up individual verses preserving verse boundaries.
    ///
    /// Returns a header (with any missing verse numbers) and a `Vec<Verse>`.
    pub fn lookup_verses(
        &self,
        reference: &ScriptureRef,
        version: BibleVersion,
    ) -> Result<(ScriptureHeader, Vec<Verse>), crate::error::Error> {
        self.lookup_captured(reference, version)
    }

    /// Look up verses from exact caller-supplied Bible source bytes.
    ///
    /// The cache is keyed by both version and content hash so a prior build
    /// cannot leak stale translation data into a reviewed build.
    pub fn lookup_reviewed_verses(
        &self,
        reference: &ScriptureRef,
        version: BibleVersion,
        source_bytes: &[u8],
    ) -> Result<(ScriptureHeader, Vec<Verse>), crate::error::Error> {
        let corpus = self.corpus(version)?;
        let path = self.root.join(version.file_name());
        Self::verify_bytes(version, &path, source_bytes, corpus)
            .map_err(|error| crate::error::Error::Scripture(error.to_string()))?;
        self.lookup_captured(reference, version)
    }

    fn lookup_captured(
        &self,
        reference: &ScriptureRef,
        version: BibleVersion,
    ) -> Result<(ScriptureHeader, Vec<Verse>), crate::error::Error> {
        let bible = &self.corpus(version)?.data;

        let book_data = bible.get(&reference.book).ok_or_else(|| {
            crate::error::Error::Scripture(format!("Book not found: {}", reference.book))
        })?;

        let chapter_data = book_data
            .get(&reference.chapter.to_string())
            .ok_or_else(|| {
                crate::error::Error::Scripture(format!(
                    "Chapter {} not found in {}",
                    reference.chapter, reference.book
                ))
            })?;

        let requested_verses = match &reference.verses {
            VerseSelection::Chapter => {
                let mut verses = chapter_data
                    .keys()
                    .map(|verse| {
                        verse.parse::<u32>().map_err(|error| {
                            crate::error::Error::Scripture(format!(
                                "Invalid verse key '{verse}' in {} {}: {error}",
                                reference.book, reference.chapter
                            ))
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                verses.sort_unstable();
                verses
            }
            VerseSelection::Verse(verse) => vec![*verse],
            VerseSelection::Range { start, end } => (*start..=*end).collect(),
            VerseSelection::Disjoint(ranges) => ranges
                .iter()
                .flat_map(|range| range.start..=range.end)
                .collect(),
        };
        let mut verses = Vec::new();
        let mut missing_verses = Vec::new();

        for verse_num in requested_verses {
            if let Some(text) = chapter_data.get(&verse_num.to_string()) {
                let clean_text: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
                verses.push(Verse {
                    number: verse_num,
                    text: clean_text,
                });
            } else {
                missing_verses.push(verse_num);
            }
        }

        let header = ScriptureHeader {
            book: reference.book.clone(),
            chapter: reference.chapter,
            verses: reference.verses.clone(),
            version,
            missing_verses,
        };

        Ok((header, verses))
    }

    fn corpus(&self, version: BibleVersion) -> Result<&CachedBibleData, crate::error::Error> {
        self.corpora.get(&version).ok_or_else(|| {
            crate::error::Error::Scripture(format!(
                "Bible corpus is not installed: {}",
                self.root.join(version.file_name()).display()
            ))
        })
    }

    fn verify_bytes(
        version: BibleVersion,
        path: &std::path::Path,
        bytes: &[u8],
        corpus: &CachedBibleData,
    ) -> Result<(), BibleCorpusError> {
        let actual: [u8; 32] = Sha256::digest(bytes).into();
        if actual == corpus.source_sha256 {
            return Ok(());
        }
        Err(BibleCorpusError::Changed {
            version: version.name(),
            path: path.to_path_buf(),
            expected: digest_hex(&corpus.source_sha256),
            actual: digest_hex(&actual),
        })
    }
}

fn read_corpus(version: BibleVersion, path: &std::path::Path) -> Result<Vec<u8>, BibleCorpusError> {
    std::fs::read(path).map_err(|source| BibleCorpusError::Read {
        version: version.name(),
        path: path.to_path_buf(),
        source,
    })
}

fn parse_corpus(
    version: BibleVersion,
    path: &std::path::Path,
    bytes: &[u8],
) -> Result<BibleData, BibleCorpusError> {
    serde_json::from_slice(bytes).map_err(|source| BibleCorpusError::Parse {
        version: version.name(),
        path: path.to_path_buf(),
        source,
    })
}

fn digest_hex(bytes: &[u8; 32]) -> String {
    let mut digest = String::with_capacity(64);
    for byte in bytes {
        let _ = write!(digest, "{byte:02x}");
    }
    digest
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
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    fn bible_source(text: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "John": { "3": { "16": text } }
        }))
        .expect("serialize Bible fixture")
    }

    fn john_3_16() -> ScriptureRef {
        ScriptureRef {
            book: "John".to_string(),
            chapter: 3,
            verses: VerseSelection::Verse(16),
        }
    }

    #[test]
    fn test_parse_simple_ref() {
        let r = parse_scripture_ref("Isaiah 32:15-17").unwrap();
        assert_eq!(r.book, "Isaiah");
        assert_eq!(r.chapter, 32);
        assert_eq!(r.verses, VerseSelection::Range { start: 15, end: 17 });
    }

    #[test]
    fn test_parse_numbered_book() {
        let r = parse_scripture_ref("1 John 3:1-3").unwrap();
        assert_eq!(r.book, "1 John");
        assert_eq!(r.chapter, 3);
        assert_eq!(r.verses, VerseSelection::Range { start: 1, end: 3 });
    }

    #[test]
    fn test_parse_with_version() {
        let r = parse_scripture_ref("Luke 1:76-79 (NRSV)").unwrap();
        assert_eq!(r.book, "Luke");
        assert_eq!(r.chapter, 1);
        assert_eq!(r.verses, VerseSelection::Range { start: 76, end: 79 });
    }

    #[test]
    fn test_parse_scripture_title_with_speaker_prefix() {
        let r = parse_scripture_ref("Scripture (Adrian) - Luke 8:26-39 NRSVue").unwrap();
        assert_eq!(r.book, "Luke");
        assert_eq!(r.chapter, 8);
        assert_eq!(r.verses, VerseSelection::Range { start: 26, end: 39 });
    }

    #[test]
    fn test_parse_single_verse() {
        let r = parse_scripture_ref("John 3:16").unwrap();
        assert_eq!(r.book, "John");
        assert_eq!(r.chapter, 3);
        assert_eq!(r.verses, VerseSelection::Verse(16));
    }

    #[test]
    fn discontinuous_ranges_are_one_reference() {
        let reference = parse_scripture_ref("Joshua 3:1-5, 9-17")
            .expect("valid discontinuous same-chapter reference");

        assert_eq!(reference.book, "Joshua");
        assert_eq!(reference.chapter, 3);
        assert_eq!(reference.to_string(), "Joshua 3:1-5, 9-17");
        let VerseSelection::Disjoint(ranges) = &reference.verses else {
            panic!("expected checked disjoint ranges");
        };
        assert_eq!(
            ranges
                .iter()
                .flat_map(|range| range.start..=range.end)
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4, 5, 9, 10, 11, 12, 13, 14, 15, 16, 17]
        );
    }

    #[test]
    fn discontinuous_ranges_reject_overlap_or_reverse_order() {
        assert!(parse_scripture_ref("Joshua 3:1-5, 5-9").is_none());
        assert!(parse_scripture_ref("Joshua 3:9-17, 1-5").is_none());
    }

    #[test]
    fn whole_chapter_is_a_distinct_reference() {
        let reference =
            parse_scripture_ref("Scripture: Jonah 3 NRSVue").expect("whole chapter reference");

        assert_eq!(reference.book, "Jonah");
        assert_eq!(reference.chapter, 3);
        assert_eq!(reference.verses, VerseSelection::Chapter);
        assert_eq!(reference.to_string(), "Jonah 3");
    }

    #[test]
    fn every_canonical_book_name_parses_without_rewriting_its_letters() {
        let books = BOOK_ALIASES
            .values()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();

        for book in books {
            let parsed = parse_scripture_ref(&format!("{book} 1:1"))
                .unwrap_or_else(|| panic!("canonical book {book:?} must parse"));
            assert_eq!(parsed.book, book);
        }
    }

    #[test]
    fn verse_letter_notation_is_limited_to_the_reference_token() {
        let parsed = parse_scripture_ref("Leviticus 2v11-13").expect("v notation");

        assert_eq!(parsed.book, "Leviticus");
        assert_eq!(parsed.chapter, 2);
        assert_eq!(parsed.verses, VerseSelection::Range { start: 11, end: 13 });
    }

    #[test]
    fn test_superscript() {
        assert_eq!(to_superscript(15), "¹⁵");
        assert_eq!(to_superscript(1), "¹");
        assert_eq!(to_superscript(100), "¹⁰⁰");
    }

    #[test]
    fn test_version_detection() {
        assert_eq!(BibleVersion::from_text("(NRSV)"), Some(BibleVersion::NRSV));
        assert_eq!(
            BibleVersion::from_text("NRSVue"),
            Some(BibleVersion::NRSVue)
        );
        assert_eq!(BibleVersion::from_text("KJV"), Some(BibleVersion::KJV));
        assert_eq!(BibleVersion::from_text("NIV"), Some(BibleVersion::NIV));
    }

    #[test]
    fn exact_version_names_reject_ambiguous_text() {
        assert_eq!(
            BibleVersion::from_name("nrsvue"),
            Some(BibleVersion::NRSVue)
        );
        assert_eq!(BibleVersion::from_name("NIV commentary"), None);
        assert_eq!(BibleVersion::from_name("ESV"), None);
    }

    #[test]
    fn reviewed_source_bytes_must_match_the_immutable_snapshot() {
        let root = tempfile::tempdir().expect("temporary Bible root");
        let path = root.path().join(BibleVersion::NRSVue.file_name());
        let original = bible_source("old text");
        std::fs::write(&path, &original).expect("write Bible source");
        let reference = john_3_16();
        let bible = BibleCorpusSnapshot::capture(root.path().to_path_buf())
            .expect("capture Bible snapshot");

        let (_, first) = bible
            .lookup_reviewed_verses(&reference, BibleVersion::NRSVue, &original)
            .expect("first reviewed lookup");
        let changed = bible.lookup_reviewed_verses(
            &reference,
            BibleVersion::NRSVue,
            &bible_source("new text"),
        );

        assert_eq!(first[0].text, "old text");
        assert!(changed.is_err());
    }

    #[test]
    fn whole_chapter_lookup_returns_every_verse_in_numeric_order() {
        let source = serde_json::to_vec(&serde_json::json!({
            "Jonah": { "3": { "10": "Tenth", "2": "Second", "1": "First" } }
        }))
        .expect("serialize whole chapter fixture");
        let reference = parse_scripture_ref("Jonah 3").expect("whole chapter reference");
        let root = tempfile::tempdir().expect("temporary Bible root");
        std::fs::write(root.path().join(BibleVersion::NRSVue.file_name()), &source)
            .expect("write whole chapter fixture");
        let bible = BibleCorpusSnapshot::capture(root.path().to_path_buf())
            .expect("capture Bible snapshot");

        let (header, verses) = bible
            .lookup_reviewed_verses(&reference, BibleVersion::NRSVue, &source)
            .expect("whole chapter lookup");

        assert_eq!(
            verses.iter().map(|verse| verse.number).collect::<Vec<_>>(),
            vec![1, 2, 10]
        );
        assert_eq!(header.display(), "Jonah 3 NRSVue");
        assert!(header.missing_verses.is_empty());
    }

    #[test]
    fn snapshot_verification_rejects_corpus_changes() {
        let root = tempfile::tempdir().expect("temporary Bible root");
        let path = root.path().join(BibleVersion::NRSVue.file_name());
        std::fs::write(&path, bible_source("old text")).expect("write first Bible source");
        let bible = BibleCorpusSnapshot::capture(root.path().to_path_buf())
            .expect("capture Bible snapshot");

        let (_, first) = bible
            .lookup_verses(&john_3_16(), BibleVersion::NRSVue)
            .expect("first file lookup");
        std::fs::write(&path, bible_source("new text")).expect("replace Bible source");

        assert_eq!(first[0].text, "old text");
        assert!(matches!(
            bible.verify_current(),
            Err(BibleCorpusError::Changed { .. })
        ));
    }

    #[test]
    fn corpus_validation_rejects_duplicate_translation_labels() {
        let root = tempfile::tempdir().expect("temporary Bible root");
        let bytes = bible_source("same mislabeled text");
        std::fs::write(root.path().join(BibleVersion::NRSVue.file_name()), &bytes)
            .expect("write first corpus");
        std::fs::write(root.path().join(BibleVersion::NRSV.file_name()), &bytes)
            .expect("write duplicate corpus");

        let error = validate_bible_corpora(root.path())
            .expect_err("duplicate translation bytes must be rejected");

        assert!(matches!(
            error,
            BibleCorpusError::DuplicateTranslation {
                first: "NRSVue",
                second: "NRSV"
            }
        ));
    }

    #[test]
    fn corpus_validation_rejects_malformed_installed_data() {
        let root = tempfile::tempdir().expect("temporary Bible root");
        std::fs::write(
            root.path().join(BibleVersion::NRSVue.file_name()),
            b"not json",
        )
        .expect("write malformed corpus");

        assert!(matches!(
            validate_bible_corpora(root.path()),
            Err(BibleCorpusError::Parse {
                version: "NRSVue",
                ..
            })
        ));
    }
}
