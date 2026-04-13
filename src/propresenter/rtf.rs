//! RTF conversion utilities for `ProPresenter`.
//!
//! Provides RTF parsing and generation for `ProPresenter` slide content.
//! All text generation flows through `StyledSegment` — a block of text with
//! an optional color override. Plain text is just a segment with `color: None`.

// Allow unwrap for compile-time constant regex patterns in LazyLock blocks
#![allow(dead_code, clippy::unwrap_used)]

use regex::Regex;
use std::fmt::Write;
use std::sync::LazyLock;

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
}

impl Default for RtfOptions {
    fn default() -> Self {
        Self {
            font_name: "Helvetica".to_string(),
            font_size: 80,
            color: (255, 255, 255), // White
            kerning: 5,
            bold: false,
            italic: false,
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
    rtf.push_str(r"{\rtf1\ansi\ansicpg1252\cocoartf2821");
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
    rtf.push_str(r"\pard\pardeftab1680\sl20\slleading480\pardirnatural\partightenfactor0");
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
    static RE_HEADER_GROUPS: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
        r"\{\\\*?\\(?:fonttbl|colortbl|expandedcolortbl|stylesheet|info|generator)[^{}]*(?:\{[^{}]*\}[^{}]*)*\}"
    ).unwrap()
    });
    static RE_NEWLINE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\\(?:par|line)\s?").unwrap());
    static RE_CONTROL: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\\[a-zA-Z]+[-]?\d*\s?").unwrap());
    static RE_BRACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[{}]").unwrap());

    if !rtf_data.starts_with("{\\rtf") {
        return None;
    }

    let mut text = rtf_data.to_string();
    text = RE_HEADER_GROUPS.replace_all(&text, "").to_string();
    text = RE_NEWLINE.replace_all(&text, "\n").to_string();
    text = RE_CONTROL.replace_all(&text, "").to_string();
    text = RE_BRACES.replace_all(&text, "").to_string();

    let text = text
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    if text.is_empty() {
        None
    } else {
        Some(text)
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
