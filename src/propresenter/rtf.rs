//! RTF conversion utilities for `ProPresenter`.
//!
//! Provides RTF parsing and generation for `ProPresenter` slide content.

// Allow unwrap for compile-time constant regex patterns in LazyLock blocks
#![allow(dead_code, clippy::unwrap_used)]

use std::fmt::Write;
use std::sync::LazyLock;
use regex::Regex;

/// Superscript digit characters for detection
const SUPERSCRIPT_CHARS: &[char] = &['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];

/// Check if a char is a superscript digit
fn is_superscript(c: char) -> bool {
    SUPERSCRIPT_CHARS.contains(&c)
}

/// Convert superscript character to regular digit
const fn superscript_to_digit(c: char) -> char {
    match c {
        '⁰' => '0', '¹' => '1', '²' => '2', '³' => '3', '⁴' => '4',
        '⁵' => '5', '⁶' => '6', '⁷' => '7', '⁸' => '8', '⁹' => '9',
        _ => c,
    }
}

/// RTF generation options for `ProPresenter` compatibility
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
}

impl Default for RtfOptions {
    fn default() -> Self {
        Self {
            font_name: "Helvetica".to_string(),
            font_size: 80,
            color: (255, 255, 255), // White
            kerning: 5,
        }
    }
}

/// Convert plain text to RTF format (simple version for backwards compatibility)
/// 
/// Handles:
/// - Unicode superscript digits → RTF \super tags
/// - Newlines → \par
/// - Basic escaping
pub fn text_to_rtf(text: &str) -> String {
    text_to_rtf_styled(text, &RtfOptions::default())
}

/// Convert plain text to ProPresenter-compatible RTF format with styling
/// 
/// Generates RTF that matches `ProPresenter`'s expected format including:
/// - Proper color table with the specified color
/// - Font table with the specified font
/// - Paragraph formatting
/// - Superscript support
pub fn text_to_rtf_styled(text: &str, options: &RtfOptions) -> String {
    let (r, g, b) = options.color;
    let font_size_halfpoints = options.font_size * 2;
    
    // Build RTF header matching ProPresenter's format
    let mut rtf = String::new();
    
    // RTF header
    rtf.push_str(r"{\rtf1\ansi\ansicpg1252\cocoartf2821");
    rtf.push('\n');
    
    // Cocoa platform settings
    rtf.push_str(r"\cocoatextscaling0\cocoaplatform0");
    
    // Font table
    let font_name = &options.font_name;
    let _ = write!(rtf, r"{{\fonttbl\f0\fswiss\fcharset0 {font_name};}}");

    rtf.push('\n');
    
    // Color table - index 0 is auto, index 1 and 2 are our color
    let _ = write!(rtf, r"{{\colortbl;\red{r}\green{g}\blue{b};\red{r}\green{g}\blue{b};}}");

    rtf.push('\n');
    
    // Expanded color table for Cocoa
    rtf.push_str(r"{\*\expandedcolortbl;;\cssrgb\c100000\c100000\c100000;}");
    rtf.push('\n');
    
    // Default tab width
    rtf.push_str(r"\deftab1680");
    rtf.push('\n');
    
    // Paragraph formatting
    rtf.push_str(r"\pard\pardeftab1680\sl20\slleading480\pardirnatural\partightenfactor0");
    rtf.push('\n');
    rtf.push('\n');
    
    // Font size, color reference (cf2 = color table index 2), and kerning
    // expnd is in quarter-points, expndtw is in twentieths of a point
    // 1 quarter-point = 5 twentieths of a point (20/4 = 5)
    let kerning = options.kerning;
    let kerning_tw = options.kerning * 5; // expndtw = expnd * 5 (quarter-points to twentieths)
    let _ = write!(rtf, r"\f0\fs{font_size_halfpoints} \cf2 \kerning1\expnd{kerning}\expndtw{kerning_tw}");

    rtf.push('\n');
    
    // Write the actual text content with proper RTF encoding
    let mut in_super = false;

    for c in text.chars() {
        if is_superscript(c) {
            // Start superscript if not already
            if !in_super {
                rtf.push_str(r"{\super ");
                in_super = true;
            }
            rtf.push(superscript_to_digit(c));
        } else {
            // End superscript if we were in one
            if in_super {
                rtf.push('}');
                in_super = false;
            }
            
            match c {
                '\n' => rtf.push_str(r"\par "),
                '\\' => rtf.push_str(r"\\"),
                '{' => rtf.push_str(r"\{"),
                '}' => rtf.push_str(r"\}"),
                // Handle common Unicode characters that need escaping for Windows-1252
                '\u{2019}' => rtf.push_str(r"\'92"),  // Right single quote (')
                '\u{2018}' => rtf.push_str(r"\'91"),  // Left single quote (')
                '\u{201C}' => rtf.push_str(r"\'93"),  // Left double quote (")
                '\u{201D}' => rtf.push_str(r"\'94"),  // Right double quote (")
                '\u{2013}' => rtf.push_str(r"\'96"),  // En dash (–)
                '\u{2014}' => rtf.push_str(r"\'97"),  // Em dash (—)
                '\u{2026}' => rtf.push_str(r"\'85"),  // Ellipsis (…)
                // Any other non-ASCII: use Unicode RTF escape
                _ if c as u32 > 127 => {
                    // \uN? where N is the Unicode code point, ? is a fallback char
                    let code = c as i32;
                    let _ = write!(rtf, r"\u{code}?");
                }
                _ => rtf.push(c),
            }
        }
    }
    
    // Close any open superscript
    if in_super {
        rtf.push('}');
    }
    
    // Close RTF document
    rtf.push('}');
    rtf
}

/// Convert plain text to RTF bytes (for `ProPresenter`)
pub fn text_to_rtf_bytes(text: &str) -> Vec<u8> {
    text_to_rtf(text).into_bytes()
}

/// Convert plain text to RTF bytes with styling options
pub fn text_to_rtf_bytes_styled(text: &str, options: &RtfOptions) -> Vec<u8> {
    text_to_rtf_styled(text, options).into_bytes()
}

/// Extract RTF options from existing RTF data
///
/// Parses RTF to extract font name, size, and color settings.
/// This can be used to match the style of an existing template.
#[allow(clippy::unnecessary_wraps)] // Returns None for future invalid-input cases
pub fn extract_rtf_options(rtf_data: &[u8]) -> Option<RtfOptions> {
    let rtf = String::from_utf8_lossy(rtf_data);
    
    let mut options = RtfOptions::default();
    
    // Extract font name from fonttbl
    if let Some(font_match) = regex::Regex::new(r"\\f0\\fswiss\\fcharset0 ([^;]+);")
        .ok()
        .and_then(|re| re.captures(&rtf))
    {
        options.font_name = font_match.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
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
        let r = color_match.get(1).and_then(|m| m.as_str().parse().ok()).unwrap_or(255);
        let g = color_match.get(2).and_then(|m| m.as_str().parse().ok()).unwrap_or(255);
        let b = color_match.get(3).and_then(|m| m.as_str().parse().ok()).unwrap_or(255);
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
    
    Some(options)
}

/// Convert RTF data to plain text
///
/// This is a simplified parser that handles common RTF patterns.
/// For complex RTF documents, consider using a full RTF library.
pub fn rtf_to_text(rtf_data: &str) -> Option<String> {
    // Header groups: fonttbl, colortbl, expandedcolortbl, stylesheet, etc.
    static RE_HEADER_GROUPS: LazyLock<Regex> = LazyLock::new(|| Regex::new(
        r"\{\\\*?\\(?:fonttbl|colortbl|expandedcolortbl|stylesheet|info|generator)[^{}]*(?:\{[^{}]*\}[^{}]*)*\}"
    ).unwrap());
    // Convert \par and \line to newlines
    static RE_NEWLINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\(?:par|line)\s?").unwrap());
    // Control words with optional numeric parameter and trailing space
    static RE_CONTROL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\[a-zA-Z]+[-]?\d*\s?").unwrap());
    // Remaining braces
    static RE_BRACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[{}]").unwrap());

    // Check if it looks like RTF
    if !rtf_data.starts_with("{\\rtf") {
        return None;
    }

    let mut text = rtf_data.to_string();

    // Strip header groups (fonttbl, colortbl, etc.) that contain no user text
    text = RE_HEADER_GROUPS.replace_all(&text, "").to_string();

    // Convert paragraph breaks to newlines
    text = RE_NEWLINE.replace_all(&text, "\n").to_string();

    // Remove control words
    text = RE_CONTROL.replace_all(&text, "").to_string();

    // Remove braces
    text = RE_BRACES.replace_all(&text, "").to_string();

    // Clean up whitespace
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
}
