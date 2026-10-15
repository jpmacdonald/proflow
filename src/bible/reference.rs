//! Checked chapter and verse selections.

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
    pub(super) fn new(ranges: Vec<VerseRange>) -> Option<Self> {
        if ranges.len() < 2 || ranges.windows(2).any(|pair| pair[0].end >= pair[1].start) {
            return None;
        }
        Some(Self(ranges.into_boxed_slice()))
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = &VerseRange> {
        self.0.iter()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct VerseRange {
    pub(super) start: u32,
    pub(super) end: u32,
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
            "Psalm",
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
