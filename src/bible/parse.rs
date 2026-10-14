//! Scripture reference parsing.

use super::{DisjointVerseRanges, ScriptureRef, VerseRange, VerseSelection, BOOK_ALIASES};

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
