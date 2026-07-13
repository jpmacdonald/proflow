//! RTF conversion utilities for `ProPresenter`.
//!
//! Provides RTF parsing and generation for `ProPresenter` slide content.
//! All text generation flows through `StyledSegment` — a block of text with
//! an optional color override. Plain text is just a segment with `color: None`.

// Allow unwrap for compile-time constant regex patterns in LazyLock blocks
#![allow(dead_code, clippy::unwrap_used)]

use std::fmt::Write;

/// Superscript digit characters for detection
const SUPERSCRIPT_CHARS: &[char] = &['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];

/// Check if a char is a superscript digit
fn is_superscript(c: char) -> bool {
    SUPERSCRIPT_CHARS.contains(&c)
}

/// Convert superscript character to regular digit
const fn superscript_to_digit(c: char) -> char {
    match c {
        '⁰' => '0',
        '¹' => '1',
        '²' => '2',
        '³' => '3',
        '⁴' => '4',
        '⁵' => '5',
        '⁶' => '6',
        '⁷' => '7',
        '⁸' => '8',
        '⁹' => '9',
        _ => c,
    }
}

/// RTF generation options for `ProPresenter` compatibility.
///
/// Represents the template's baseline style. Per-segment overrides in
/// `StyledSegment` layer on top of these defaults.
#[derive(Debug, Clone)]
pub struct RtfOptions {
    /// Cocoa RTF producer version copied from the template.
    pub cocoa_rtf_version: u32,
    /// Font name (default: Helvetica)
    pub font_name: String,
    /// Font size in points (default: 80)
    pub font_size: u32,
    /// Text color RGB (default: white)
    pub color: (u8, u8, u8),
    /// Kerning value (default: 5)
    pub kerning: i32,
    /// Whether the template baseline is bold
    pub bold: bool,
    /// Whether the template baseline is italic
    pub italic: bool,
    /// Native paragraph controls copied from the template's first paragraph.
    pub paragraph_controls: String,
}

impl Default for RtfOptions {
    fn default() -> Self {
        Self {
            cocoa_rtf_version: 2821,
            font_name: "Helvetica".to_string(),
            font_size: 80,
            color: (255, 255, 255), // White
            kerning: 5,
            bold: false,
            italic: false,
            paragraph_controls:
                r"\pard\pardeftab1680\sl20\slleading480\pardirnatural\partightenfactor0".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// StyledSegment — the universal text primitive
// ---------------------------------------------------------------------------

/// A section of text with an optional color override.
///
/// This is the universal primitive for slide text. Every slide is built
/// from segments. Segments with `color: None` use the template's default.
/// Multiple segments on the same slide can have different colors (e.g.,
/// white for LEADER and yellow for ALL in a responsive reading).
#[derive(Debug, Clone, Default)]
pub struct StyledSegment {
    /// The text content of this segment.
    pub text: String,
    /// RGB color override. `None` = template default.
    pub color: Option<(u8, u8, u8)>,
    /// Bold override. `None` = template default.
    pub bold: Option<bool>,
    /// Italic override. `None` = template default.
    pub italic: Option<bool>,
}

impl StyledSegment {
    /// Create a segment with no style overrides (all template defaults).
    pub fn unstyled(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }

    /// Convert plain text lines into unstyled segments.
    pub fn from_plain(lines: &[String]) -> Vec<Self> {
        lines.iter().map(|l| Self::unstyled(l.as_str())).collect()
    }
}

// ---------------------------------------------------------------------------
// RTF generation — single path for all text
// ---------------------------------------------------------------------------

/// Generate `ProPresenter`-compatible RTF from styled segments.
///
/// Builds a color table from all unique colors, then emits RTF that
/// switches `\cfN` at each segment boundary. Segments without a color
/// override use the base color from `options`.
pub fn segments_to_rtf(segments: &[StyledSegment], options: &RtfOptions) -> String {
    let base = options.color;

    // Collect unique override colors for the color table
    let mut extra_colors: Vec<(u8, u8, u8)> = Vec::new();
    for seg in segments {
        if let Some(c) = seg.color {
            if c != base && !extra_colors.contains(&c) {
                extra_colors.push(c);
            }
        }
    }

    let (r, g, b) = base;
    let font_size_halfpoints = options.font_size * 2;

    let mut rtf = String::new();

    // RTF header
    let _ = write!(
        rtf,
        r"{{\rtf1\ansi\ansicpg1252\cocoartf{}",
        options.cocoa_rtf_version
    );
    rtf.push('\n');
    rtf.push_str(r"\cocoatextscaling0\cocoaplatform0");

    // Font table
    let font_name = &options.font_name;
    let _ = write!(rtf, r"{{\fonttbl\f0\fswiss\fcharset0 {font_name};}}");
    rtf.push('\n');

    // Color table: ;auto; base(1); base(2); extra(3)...
    rtf.push_str(r"{\colortbl;");
    let _ = write!(rtf, r"\red{r}\green{g}\blue{b};");
    let _ = write!(rtf, r"\red{r}\green{g}\blue{b};");
    for (er, eg, eb) in &extra_colors {
        let _ = write!(rtf, r"\red{er}\green{eg}\blue{eb};");
    }
    rtf.push('}');
    rtf.push('\n');

    // Expanded color table (Cocoa compatibility)
    rtf.push_str(r"{\*\expandedcolortbl;;");
    rtf.push_str(r"\cssrgb\c100000\c100000\c100000;");
    for _ in &extra_colors {
        rtf.push(';');
    }
    rtf.push('}');
    rtf.push('\n');

    rtf.push_str(r"\deftab1680");
    rtf.push('\n');

    // Paragraph formatting
    rtf.push_str(&options.paragraph_controls);
    rtf.push('\n');
    rtf.push('\n');

    let kerning = options.kerning;
    let kerning_tw = options.kerning * 5;

    // Resolve a segment's color to a color table index
    let color_index = |seg: &StyledSegment| -> usize {
        match seg.color {
            None => 2,
            Some(c) if c == base => 2,
            Some(c) => extra_colors
                .iter()
                .position(|ec| *ec == c)
                .map_or(2, |i| i + 3),
        }
    };

    for (i, seg) in segments.iter().enumerate() {
        if i > 0 {
            rtf.push_str(r"\par ");
        }

        let cf = color_index(seg);
        let _ = write!(
            rtf,
            r"\f0\fs{font_size_halfpoints} \cf{cf} \kerning1\expnd{kerning}\expndtw{kerning_tw}"
        );

        // Bold: segment override > template baseline
        let is_bold = seg.bold.unwrap_or(options.bold);
        rtf.push_str(if is_bold { r"\b" } else { r"\b0" });

        // Italic: segment override > template baseline
        let is_italic = seg.italic.unwrap_or(options.italic);
        rtf.push_str(if is_italic { r"\i" } else { r"\i0" });

        rtf.push('\n');

        write_rtf_text(&mut rtf, &seg.text);
    }

    rtf.push('}');
    rtf
}

/// Generate RTF bytes from styled segments.
pub fn segments_to_rtf_bytes(segments: &[StyledSegment], options: &RtfOptions) -> Vec<u8> {
    segments_to_rtf(segments, options).into_bytes()
}

// ---------------------------------------------------------------------------
// Convenience wrappers for plain text
// ---------------------------------------------------------------------------

/// Convert plain text to `ProPresenter`-compatible RTF with styling options.
///
/// Thin wrapper around `segments_to_rtf` — splits text on newlines into
/// default-colored segments.
pub fn text_to_rtf_styled(text: &str, options: &RtfOptions) -> String {
    let segments: Vec<StyledSegment> = text.split('\n').map(StyledSegment::unstyled).collect();
    segments_to_rtf(&segments, options)
}

/// Convert plain text to RTF format with default styling.
pub fn text_to_rtf(text: &str) -> String {
    text_to_rtf_styled(text, &RtfOptions::default())
}

/// Convert plain text to RTF bytes with default styling.
pub fn text_to_rtf_bytes(text: &str) -> Vec<u8> {
    text_to_rtf(text).into_bytes()
}

/// Convert plain text to RTF bytes with styling options.
pub fn text_to_rtf_bytes_styled(text: &str, options: &RtfOptions) -> Vec<u8> {
    text_to_rtf_styled(text, options).into_bytes()
}

// ---------------------------------------------------------------------------
// RTF text encoding helpers
// ---------------------------------------------------------------------------

/// Write a block of text with RTF encoding (superscripts, escaping).
fn write_rtf_text(rtf: &mut String, text: &str) {
    let mut in_super = false;
    for c in text.chars() {
        if is_superscript(c) {
            if !in_super {
                rtf.push_str(r"{\super ");
                in_super = true;
            }
            rtf.push(superscript_to_digit(c));
        } else {
            if in_super {
                rtf.push('}');
                in_super = false;
            }
            write_rtf_char(rtf, c);
        }
    }
    if in_super {
        rtf.push('}');
    }
}

/// Write a single character with RTF escaping.
fn write_rtf_char(rtf: &mut String, c: char) {
    match c {
        '\n' => rtf.push_str(r"\par "),
        '\\' => rtf.push_str(r"\\"),
        '{' => rtf.push_str(r"\{"),
        '}' => rtf.push_str(r"\}"),
        '\u{2019}' => rtf.push_str(r"\'92"), // Right single quote
        '\u{2018}' => rtf.push_str(r"\'91"), // Left single quote
        '\u{201C}' => rtf.push_str(r"\'93"), // Left double quote
        '\u{201D}' => rtf.push_str(r"\'94"), // Right double quote
        '\u{2013}' => rtf.push_str(r"\'96"), // En dash
        '\u{2014}' => rtf.push_str(r"\'97"), // Em dash
        '\u{2026}' => rtf.push_str(r"\'85"), // Ellipsis
        _ if c as u32 > 127 => {
            let code = c as i32;
            let _ = write!(rtf, r"\u{code}?");
        }
        _ => rtf.push(c),
    }
}

// ---------------------------------------------------------------------------
// RTF extraction / parsing
// ---------------------------------------------------------------------------

/// Extract RTF options from existing RTF data.
///
/// Parses RTF to extract font name, size, and color settings.
/// Used to match the style of an existing template.
#[allow(clippy::unnecessary_wraps)] // Returns None for future invalid-input cases
pub fn extract_rtf_options(rtf_data: &[u8]) -> Option<RtfOptions> {
    let rtf = String::from_utf8_lossy(rtf_data);

    let mut options = RtfOptions::default();

    if let Some(version) = regex::Regex::new(r"\\cocoartf(\d+)")
        .ok()
        .and_then(|re| re.captures(&rtf))
        .and_then(|captures| captures.get(1))
        .and_then(|value| value.as_str().parse::<u32>().ok())
    {
        options.cocoa_rtf_version = version;
    }

    if let Some(paragraph_controls) = regex::Regex::new(r"(?m)(\\pard[^\r\n{}]*)")
        .ok()
        .and_then(|re| re.captures(&rtf))
        .and_then(|captures| captures.get(1))
        .map(|value| value.as_str().trim_end().to_string())
        .filter(|value| !value.is_empty())
    {
        options.paragraph_controls = paragraph_controls;
    }

    // Extract font name from fonttbl
    if let Some(font_match) = regex::Regex::new(r"\\f0\\fswiss\\fcharset0 ([^;]+);")
        .ok()
        .and_then(|re| re.captures(&rtf))
    {
        options.font_name = font_match
            .get(1)
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();
    }

    // Extract font size (half-points)
    if let Some(size_match) = regex::Regex::new(r"\\fs(\d+)")
        .ok()
        .and_then(|re| re.captures(&rtf))
    {
        if let Some(size_str) = size_match.get(1) {
            if let Ok(half_points) = size_str.as_str().parse::<u32>() {
                options.font_size = half_points / 2;
            }
        }
    }

    // Extract color from colortbl (first non-auto color)
    if let Some(color_match) = regex::Regex::new(r"\\red(\d+)\\green(\d+)\\blue(\d+)")
        .ok()
        .and_then(|re| re.captures(&rtf))
    {
        let r = color_match
            .get(1)
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(255);
        let g = color_match
            .get(2)
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(255);
        let b = color_match
            .get(3)
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(255);
        options.color = (r, g, b);
    }

    // Extract kerning
    if let Some(kern_match) = regex::Regex::new(r"\\expnd(-?\d+)")
        .ok()
        .and_then(|re| re.captures(&rtf))
    {
        if let Some(kern_str) = kern_match.get(1) {
            options.kerning = kern_str.as_str().parse().unwrap_or(5);
        }
    }

    // Detect bold/italic — present means on, absent or \b0/\i0 means off
    options.bold = rtf.contains(r"\b ") || rtf.contains(r"\b\");
    options.italic = rtf.contains(r"\i ") || rtf.contains(r"\i\");

    Some(options)
}

/// Convert RTF data to plain text.
///
/// Simplified parser for common RTF patterns.
pub fn rtf_to_text(rtf_data: &str) -> Option<String> {
    if !rtf_data.starts_with("{\\rtf") {
        return None;
    }

    let chars: Vec<char> = rtf_data.chars().collect();
    let mut text = String::new();
    let mut index = 0usize;
    let mut state = RtfParserState::default();
    let mut group_stack = Vec::new();
    let mut group_start = false;

    while index < chars.len() {
        match chars[index] {
            '{' => {
                group_stack.push(state);
                group_start = true;
                index += 1;
            }
            '}' => {
                state = group_stack.pop().unwrap_or_default();
                group_start = false;
                index += 1;
            }
            '\\' => {
                index += 1;
                parse_rtf_control(&chars, &mut index, &mut text, &mut state, &mut group_start);
            }
            value => {
                // Raw CR/LF bytes format the RTF source; visible line breaks
                // are represented by control words such as `\par` or `\line`.
                if !state.skip_group && !matches!(value, '\r' | '\n') {
                    text.push(value);
                }
                group_start = false;
                index += 1;
            }
        }
    }

    let text = normalize_extracted_rtf_text(&text);

    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

#[derive(Clone, Copy)]
struct RtfParserState {
    skip_group: bool,
    unicode_fallback_len: usize,
}

impl Default for RtfParserState {
    fn default() -> Self {
        Self {
            skip_group: false,
            unicode_fallback_len: 1,
        }
    }
}

fn parse_rtf_control(
    chars: &[char],
    index: &mut usize,
    text: &mut String,
    state: &mut RtfParserState,
    group_start: &mut bool,
) {
    if *index >= chars.len() {
        return;
    }

    if chars[*index] == '*' {
        if *group_start {
            state.skip_group = true;
        }
        *group_start = false;
        *index += 1;
        return;
    }

    if chars[*index] == '\'' {
        *index += 1;
        let value = parse_hex_byte(chars, index);
        if !state.skip_group {
            text.push(cp1252_byte_to_char(value));
        }
        *group_start = false;
        return;
    }

    if !chars[*index].is_ascii_alphabetic() {
        let escaped = chars[*index];
        if !state.skip_group {
            match escaped {
                '\\' | '{' | '}' => text.push(escaped),
                '\n' => text.push('\n'),
                '\r' => {
                    text.push('\n');
                    if chars.get(*index + 1) == Some(&'\n') {
                        *index += 1;
                    }
                }
                '~' => text.push(' '),
                '-' => text.push('\u{00ad}'),
                '_' => text.push('\u{2011}'),
                _ => {}
            }
        }
        *group_start = false;
        *index += 1;
        return;
    }

    let word_start = *index;
    while *index < chars.len() && chars[*index].is_ascii_alphabetic() {
        *index += 1;
    }
    let word = chars[word_start..*index].iter().collect::<String>();
    let signed = *index < chars.len() && chars[*index] == '-';
    if signed {
        *index += 1;
    }
    let number_start = *index;
    while *index < chars.len() && chars[*index].is_ascii_digit() {
        *index += 1;
    }
    let number = (number_start != *index).then(|| {
        chars[number_start..*index]
            .iter()
            .collect::<String>()
            .parse::<i32>()
            .unwrap_or(0)
            * if signed { -1 } else { 1 }
    });
    if *index < chars.len() && chars[*index] == ' ' {
        *index += 1;
    }

    if *group_start && is_ignored_destination(&word) {
        state.skip_group = true;
    }
    if !state.skip_group {
        match word.as_str() {
            "par" | "line" => text.push('\n'),
            "tab" => text.push('\t'),
            "emdash" => text.push('\u{2014}'),
            "endash" => text.push('\u{2013}'),
            "bullet" => text.push('\u{2022}'),
            "uc" => {
                if let Some(value) = number.and_then(|value| usize::try_from(value).ok()) {
                    state.unicode_fallback_len = value;
                }
            }
            "u" => {
                if let Some(value) = number {
                    let unsigned = if value < 0 {
                        (value + 65_536).cast_unsigned()
                    } else {
                        value.cast_unsigned()
                    };
                    if let Some(ch) = char::from_u32(unsigned) {
                        text.push(ch);
                    }
                }
                skip_rtf_fallback(chars, index, state.unicode_fallback_len);
            }
            _ => {}
        }
    }
    *group_start = false;
}

fn skip_rtf_fallback(chars: &[char], index: &mut usize, count: usize) {
    for _ in 0..count {
        let Some(&next) = chars.get(*index) else {
            return;
        };
        if matches!(next, '{' | '}') {
            return;
        }
        if next != '\\' {
            *index += 1;
            continue;
        }

        *index += 1;
        let Some(&escaped) = chars.get(*index) else {
            return;
        };
        if escaped == '\'' {
            *index = (*index + 3).min(chars.len());
        } else {
            *index += 1;
        }
    }
}

fn is_ignored_destination(word: &str) -> bool {
    matches!(
        word,
        "fonttbl"
            | "colortbl"
            | "expandedcolortbl"
            | "stylesheet"
            | "info"
            | "generator"
            | "listtable"
            | "listoverridetable"
            | "datastore"
            | "themedata"
            | "colorschememapping"
            | "header"
            | "footer"
            | "pict"
            | "object"
    )
}

fn parse_hex_byte(chars: &[char], index: &mut usize) -> u8 {
    let hi = chars
        .get(*index)
        .and_then(|ch| ch.to_digit(16))
        .and_then(|value| u8::try_from(value).ok())
        .unwrap_or(0);
    *index += usize::from(*index < chars.len());
    let lo = chars
        .get(*index)
        .and_then(|ch| ch.to_digit(16))
        .and_then(|value| u8::try_from(value).ok())
        .unwrap_or(0);
    *index += usize::from(*index < chars.len());
    (hi << 4) | lo
}

fn cp1252_byte_to_char(value: u8) -> char {
    match value {
        0x80 => '\u{20ac}',
        0x82 => '\u{201a}',
        0x83 => '\u{0192}',
        0x84 => '\u{201e}',
        0x85 => '\u{2026}',
        0x86 => '\u{2020}',
        0x87 => '\u{2021}',
        0x88 => '\u{02c6}',
        0x89 => '\u{2030}',
        0x8a => '\u{0160}',
        0x8b => '\u{2039}',
        0x8c => '\u{0152}',
        0x8e => '\u{017d}',
        0x91 => '\u{2018}',
        0x92 => '\u{2019}',
        0x93 => '\u{201c}',
        0x94 => '\u{201d}',
        0x95 => '\u{2022}',
        0x96 => '\u{2013}',
        0x97 => '\u{2014}',
        0x98 => '\u{02dc}',
        0x99 => '\u{2122}',
        0x9a => '\u{0161}',
        0x9b => '\u{203a}',
        0x9c => '\u{0153}',
        0x9e => '\u{017e}',
        0x9f => '\u{0178}',
        _ => char::from(value),
    }
}

fn normalize_extracted_rtf_text(text: &str) -> String {
    let normalized = text.replace('\u{00a0}', " ");
    let lines = normalized.lines().map(str::trim_end).collect::<Vec<_>>();
    let first = lines.iter().position(|line| !line.trim().is_empty());
    let last = lines.iter().rposition(|line| !line.trim().is_empty());
    match (first, last) {
        (Some(first), Some(last)) => lines[first..=last].join("\n"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn test_simple_rtf() {
        let rtf = r"{\rtf1\ansi{\fonttbl\f0\fswiss Helvetica;}\f0\pard Test text\par}";
        let result = rtf_to_text(rtf);
        assert!(result.is_some());
        assert!(result.unwrap().contains("Test text"));
    }

    #[test]
    fn test_not_rtf() {
        assert_eq!(rtf_to_text("plain text"), None);
    }

    #[test]
    fn test_multiline_rtf() {
        let rtf = r"{\rtf1\ansi Line 1\par Line 2\par}";
        let result = rtf_to_text(rtf).unwrap();
        assert!(result.contains("Line 1"));
        assert!(result.contains("Line 2"));
    }

    #[test]
    fn test_legacy_backslash_newline_breaks() {
        let rtf = "{\\rtf1\\ansi Line 1\\\nLine 2\\\nLine 3}";
        let result = rtf_to_text(rtf).unwrap();
        assert_eq!(result, "Line 1\nLine 2\nLine 3");
    }

    #[test]
    fn raw_rtf_formatting_newlines_are_not_visible_text() {
        let rtf = "{\\rtf1\\ansi\nLine 1\n\\par Line 2\n}";

        assert_eq!(rtf_to_text(rtf).as_deref(), Some("Line 1\nLine 2"));
    }

    #[test]
    fn test_rtf_to_text_skips_cocoa_header_groups() {
        let rtf = r"{\rtf1\ansi{\fonttbl\f0\fswiss\fcharset0 Helvetica;}{\colortbl;\red255\green255\blue255;}\pard\pardeftab1680\f0\fs96 \cf1 Actual text\par Next line}";
        let result = rtf_to_text(rtf).unwrap();
        assert_eq!(result, "Actual text\nNext line");
    }

    #[test]
    fn test_rtf_to_text_preserves_internal_blank_lines() {
        let rtf = r"{\rtf1\ansi First\par \par Second\par}";
        let result = rtf_to_text(rtf).unwrap();
        assert_eq!(result, "First\n\nSecond");
    }

    #[test]
    fn test_rtf_to_text_blank_slide_is_none() {
        let rtf = r"{\rtf1\ansi{\fonttbl\f0\fswiss Helvetica;}\pard\par}";
        assert_eq!(rtf_to_text(rtf), None);
    }

    #[test]
    fn generated_unicode_round_trips_without_the_ansi_fallback() {
        let rtf = text_to_rtf_styled("O\u{00a0}Lord", &RtfOptions::default());

        assert_eq!(rtf_to_text(&rtf).as_deref(), Some("O Lord"));
    }

    #[test]
    fn unicode_fallback_length_is_scoped_to_its_rtf_group() {
        let rtf = r"{\rtf1\ansi\uc0 {\uc1\u8217 ?}\u8239 X}";

        assert_eq!(rtf_to_text(rtf).as_deref(), Some("’\u{202f}X"));
    }

    #[test]
    fn regenerated_rtf_preserves_template_paragraph_controls_and_version() {
        let template = br"{\rtf1\ansi\ansicpg1252\cocoartf2709
{\fonttbl\f0\fswiss\fcharset0 Helvetica;}
{\colortbl;\red255\green255\blue255;}
\pard\qc\sl240\slmult1\pardirnatural
\f0\fs96 Old text}";
        let options = extract_rtf_options(template).expect("extract native RTF options");

        let regenerated = segments_to_rtf(&[StyledSegment::unstyled("New text")], &options);

        assert!(regenerated.contains("\\cocoartf2709"));
        assert!(regenerated.contains("\\pard\\qc\\sl240\\slmult1\\pardirnatural"));
        assert!(regenerated.contains("New text"));
    }

    #[test]
    fn test_styled_segments_unstyled() {
        let segments = StyledSegment::from_plain(&["Hello".to_string(), "World".to_string()]);
        let rtf = segments_to_rtf(&segments, &RtfOptions::default());
        assert!(rtf.contains("Hello"));
        assert!(rtf.contains("World"));
        assert!(!rtf.contains("\\red255\\green255\\blue0"));
    }

    #[test]
    fn test_styled_segments_mixed_styles() {
        let segments = vec![
            StyledSegment::unstyled("Leader line"),
            StyledSegment {
                text: "Response line".to_string(),
                color: Some((255, 255, 0)),
                bold: Some(true),
                ..StyledSegment::default()
            },
        ];
        let rtf = segments_to_rtf(&segments, &RtfOptions::default());
        // Yellow in color table
        assert!(rtf.contains("\\red255\\green255\\blue0"));
        // Bold on the response segment (followed by italic state)
        assert!(rtf.contains("\\b\\i0"));
        // Both texts present
        assert!(rtf.contains("Leader line"));
        assert!(rtf.contains("Response line"));
    }
}
