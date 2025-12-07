//! Bible verse lookup and scripture reference parsing.

use std::collections::HashMap;
use std::path::PathBuf;
use lazy_static::lazy_static;

/// Supported Bible versions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BibleVersion {
    #[default]
    NRSVue,
    NRSV,
    NIV,
    KJV,
}

impl BibleVersion {
    pub fn all() -> &'static [BibleVersion] {
        &[Self::NRSVue, Self::NRSV, Self::NIV, Self::KJV]
    }
    
    pub fn name(&self) -> &'static str {
        match self {
            Self::NRSVue => "NRSVue",
            Self::NRSV => "NRSV",
            Self::NIV => "NIV",
            Self::KJV => "KJV",
        }
    }
    
    pub fn file_name(&self) -> &'static str {
        match self {
            Self::NRSVue => "NRSVUE.json",
            Self::NRSV => "NRSV.json",
            Self::NIV => "NIV.json",
            Self::KJV => "KJV.json",
        }
    }
    
    /// Try to detect version from text like "(NRSV)" or "NRSVue"
    pub fn from_text(text: &str) -> Option<Self> {
        let upper = text.to_uppercase();
        if upper.contains("NRSVUE") { return Some(Self::NRSVue); }
        if upper.contains("NRSV") { return Some(Self::NRSV); }
        if upper.contains("NIV") { return Some(Self::NIV); }
        if upper.contains("KJV") { return Some(Self::KJV); }
        None
    }
}

/// A parsed scripture reference
#[derive(Debug, Clone)]
pub struct ScriptureRef {
    pub book: String,
    pub chapter: u32,
    pub start_verse: u32,
    pub end_verse: Option<u32>,
}

/// Bible data structure: Book -> Chapter -> Verse -> Text
type BibleData = HashMap<String, HashMap<String, HashMap<String, String>>>;

lazy_static! {
    /// Book name normalization map
    static ref BOOK_ALIASES: HashMap<&'static str, &'static str> = {
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
    };
}

/// Superscript digit mapping
const SUPERSCRIPT_DIGITS: &[char] = &['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];

/// Convert a number to superscript Unicode characters
/// These will be converted to RTF \super tags during .pro export
fn to_superscript(n: u32) -> String {
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

/// Parse a scripture reference string like "Isaiah 32:15-17" or "1 John 3:1-3"
/// Also handles complex titles like "Scripture: Isaiah 32:15-17; Luke 1:76-79 NRSVue (Hope)"
pub fn parse_scripture_ref(text: &str) -> Option<ScriptureRef> {
    // Strip "Scripture:" or "Reading:" prefix
    let text = text.trim_start_matches("Scripture:")
        .trim_start_matches("Scripture Reading:")
        .trim_start_matches("Reading:")
        .trim();
    
    // Take only the first reference if multiple (separated by ; or ,)
    let first_ref = text.split(';').next()
        .or_else(|| text.split(',').next())?
        .trim();
    
    // Remove version and location indicators like "(NRSV)" or "(Hope)" or "NRSVue"
    // Also handle version without parens at end
    let cleaned = first_ref
        .split('(').next()?
        .trim()
        .trim_end_matches("NRSVue")
        .trim_end_matches("NRSVUE")
        .trim_end_matches("NRSV")
        .trim_end_matches("NIV")
        .trim_end_matches("KJV")
        .trim_end_matches("ESV")
        .trim();
    
    parse_single_reference(cleaned)
}

/// Parse multiple scripture references from a title
pub fn parse_scripture_refs(text: &str) -> Vec<ScriptureRef> {
    // Strip prefix
    let text = text.trim_start_matches("Scripture:")
        .trim_start_matches("Scripture Reading:")
        .trim_start_matches("Reading:")
        .trim();
    
    // Split by ; or , and parse each
    text.split(|c| c == ';' || c == ',')
        .filter_map(|part| {
            let cleaned = part.trim()
                .split('(').next()?
                .trim()
                .trim_end_matches("NRSVue")
                .trim_end_matches("NRSVUE")
                .trim_end_matches("NRSV")
                .trim_end_matches("NIV")
                .trim_end_matches("KJV")
                .trim_end_matches("ESV")
                .trim();
            parse_single_reference(cleaned)
        })
        .collect()
}

/// Parse a single scripture reference like "Isaiah 32:15-17"
fn parse_single_reference(text: &str) -> Option<ScriptureRef> {
    // Handle "v" notation (e.g., "Luke 2v1-20")
    let text = text.replace('v', ":");
    
    // Find where the chapter:verse starts (look for digits followed by colon)
    let mut parts = text.rsplitn(2, |c: char| c.is_whitespace());
    let verse_part = parts.next()?;
    let book_part = parts.next()?.trim();
    
    // Parse chapter:verse-verse pattern
    let (chapter_str, verse_range) = verse_part.split_once(':')?;
    let chapter: u32 = chapter_str.parse().ok()?;
    
    let (start_verse, end_verse) = if verse_range.contains('-') {
        let mut range_parts = verse_range.split('-');
        let start: u32 = range_parts.next()?.parse().ok()?;
        let end: u32 = range_parts.next()?.parse().ok()?;
        (start, Some(end))
    } else {
        (verse_range.parse().ok()?, None)
    };
    
    let book = normalize_book_name(book_part)?;
    
    Some(ScriptureRef {
        book: book.to_string(),
        chapter,
        start_verse,
        end_verse,
    })
}

/// Bible lookup service
pub struct BibleService {
    data_path: PathBuf,
    cache: HashMap<BibleVersion, BibleData>,
}

impl BibleService {
    pub fn new(data_path: PathBuf) -> Self {
        Self {
            data_path,
            cache: HashMap::new(),
        }
    }
    
    /// Load a Bible version into cache
    fn load_version(&mut self, version: BibleVersion) -> Result<(), String> {
        if self.cache.contains_key(&version) {
            return Ok(());
        }
        
        let path = self.data_path.join(version.file_name());
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
        
        let data: BibleData = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse {}: {}", path.display(), e))?;
        
        self.cache.insert(version, data);
        Ok(())
    }
    
    /// Look up verses and format with superscript verse numbers
    /// Each verse is its own paragraph (for slide splitting)
    pub fn lookup(&mut self, reference: &ScriptureRef, version: BibleVersion) -> Result<(ScriptureHeader, Vec<String>), String> {
        self.load_version(version)?;
        
        let bible = self.cache.get(&version)
            .ok_or_else(|| "Bible data not loaded".to_string())?;
        
        let book_data = bible.get(&reference.book)
            .ok_or_else(|| format!("Book not found: {}", reference.book))?;
        
        let chapter_data = book_data.get(&reference.chapter.to_string())
            .ok_or_else(|| format!("Chapter {} not found in {}", reference.chapter, reference.book))?;
        
        let end = reference.end_verse.unwrap_or(reference.start_verse);
        let mut lines = Vec::new();
        
        // Build header info (for pane title, not content)
        let header = ScriptureHeader {
            book: reference.book.clone(),
            chapter: reference.chapter,
            start_verse: reference.start_verse,
            end_verse: reference.end_verse,
            version,
        };
        
        // Build all verses as one continuous block of text
        // User will add line breaks to create slides
        let mut verse_text = String::new();
        for verse_num in reference.start_verse..=end {
            if let Some(text) = chapter_data.get(&verse_num.to_string()) {
                // Normalize whitespace in source text
                let clean_text: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
                if !verse_text.is_empty() {
                    verse_text.push(' ');
                }
                verse_text.push_str(&format!("{}{}", to_superscript(verse_num), clean_text));
            }
        }
        
        // Single line of text - user will wrap/split as needed
        lines.push(verse_text);
        lines.push(String::new()); // Trailing empty line for editor
        
        Ok((header, lines))
    }
}

/// Scripture header info for display in pane title
#[derive(Debug, Clone)]
pub struct ScriptureHeader {
    pub book: String,
    pub chapter: u32,
    pub start_verse: u32,
    pub end_verse: Option<u32>,
    pub version: BibleVersion,
}

impl ScriptureHeader {
    /// Format for display (e.g., "Isaiah 32:15-17 NRSVue")
    pub fn display(&self) -> String {
        if let Some(end) = self.end_verse {
            format!("{} {}:{}-{} {}", self.book, self.chapter, self.start_verse, end, self.version.name())
        } else {
            format!("{} {}:{} {}", self.book, self.chapter, self.start_verse, self.version.name())
        }
    }
    
    /// Format for filename (colon → v)
    pub fn filename(&self) -> String {
        if let Some(end) = self.end_verse {
            format!("{} {}v{}-{} ({})", self.book, self.chapter, self.start_verse, end, self.version.name())
        } else {
            format!("{} {}v{} ({})", self.book, self.chapter, self.start_verse, self.version.name())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_simple_ref() {
        let r = parse_scripture_ref("Isaiah 32:15-17").unwrap();
        assert_eq!(r.book, "Isaiah");
        assert_eq!(r.chapter, 32);
        assert_eq!(r.start_verse, 15);
        assert_eq!(r.end_verse, Some(17));
    }
    
    #[test]
    fn test_parse_numbered_book() {
        let r = parse_scripture_ref("1 John 3:1-3").unwrap();
        assert_eq!(r.book, "1 John");
        assert_eq!(r.chapter, 3);
        assert_eq!(r.start_verse, 1);
        assert_eq!(r.end_verse, Some(3));
    }
    
    #[test]
    fn test_parse_with_version() {
        let r = parse_scripture_ref("Luke 1:76-79 (NRSV)").unwrap();
        assert_eq!(r.book, "Luke");
        assert_eq!(r.chapter, 1);
        assert_eq!(r.start_verse, 76);
        assert_eq!(r.end_verse, Some(79));
    }
    
    #[test]
    fn test_parse_single_verse() {
        let r = parse_scripture_ref("John 3:16").unwrap();
        assert_eq!(r.book, "John");
        assert_eq!(r.chapter, 3);
        assert_eq!(r.start_verse, 16);
        assert_eq!(r.end_verse, None);
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
        assert_eq!(BibleVersion::from_text("NRSVue"), Some(BibleVersion::NRSVue));
        assert_eq!(BibleVersion::from_text("KJV"), Some(BibleVersion::KJV));
        assert_eq!(BibleVersion::from_text("NIV"), Some(BibleVersion::NIV));
    }
}

