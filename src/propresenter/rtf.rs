//! RTF conversion utilities for ProPresenter.
//!
//! Handles both reading RTF (from .pro files) and writing RTF (for export).

#![allow(dead_code)]

use regex::Regex;

/// Superscript digit characters for detection
const SUPERSCRIPT_CHARS: &[char] = &['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];

/// Check if a char is a superscript digit
fn is_superscript(c: char) -> bool {
    SUPERSCRIPT_CHARS.contains(&c)
}

/// Convert superscript character to regular digit
fn superscript_to_digit(c: char) -> char {
    match c {
        '⁰' => '0', '¹' => '1', '²' => '2', '³' => '3', '⁴' => '4',
        '⁵' => '5', '⁶' => '6', '⁷' => '7', '⁸' => '8', '⁹' => '9',
        _ => c,
    }
}

/// Convert plain text to RTF format
/// 
/// Handles:
/// - Unicode superscript digits → RTF \super tags
/// - Newlines → \par
/// - Basic escaping
pub fn text_to_rtf(text: &str) -> String {
    let mut rtf = String::from(r"{\rtf1\ansi\deff0{\fonttbl{\f0 Helvetica;}}");
    rtf.push_str(r"\f0\fs144 "); // Font size 72pt = 144 half-points
    
    let mut in_super = false;
    let mut chars = text.chars().peekable();
    
    while let Some(c) = chars.next() {
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
                _ => rtf.push(c),
            }
        }
    }
    
    // Close any open superscript
    if in_super {
        rtf.push('}');
    }
    
    rtf.push('}');
    rtf
}

/// Convert plain text to RTF bytes (for ProPresenter)
pub fn text_to_rtf_bytes(text: &str) -> Vec<u8> {
    text_to_rtf(text).into_bytes()
}

/// Convert RTF data to plain text
///
/// This is a simplified parser that handles common RTF patterns.
/// For complex RTF documents, consider using a full RTF library.
pub fn rtf_to_text(rtf_data: &str) -> Option<String> {
    // Check if it looks like RTF
    if !rtf_data.starts_with("{\\rtf") {
        return None;
    }

    lazy_static::lazy_static! {
        // Remove RTF control words with optional numeric parameter
        static ref RE_CONTROL: Regex = Regex::new(r"\\[a-z]+\d*\s?").unwrap();
        // Remove RTF groups like {\fonttbl...} {\colortbl...}
        static ref RE_GROUPS: Regex = Regex::new(r"\{\\[^{}]*\}").unwrap();
        // Remove remaining braces
        static ref RE_BRACES: Regex = Regex::new(r"[{}]").unwrap();
        // Convert \par and \line to newlines
        static ref RE_NEWLINE: Regex = Regex::new(r"\\(?:par|line)\s?").unwrap();
    }

    let mut text = rtf_data.to_string();

    // Convert paragraph breaks to newlines first
    text = RE_NEWLINE.replace_all(&text, "\n").to_string();

    // Remove nested groups (do multiple passes for deeply nested)
    for _ in 0..5 {
        let new_text = RE_GROUPS.replace_all(&text, "").to_string();
        if new_text == text {
            break;
        }
        text = new_text;
    }

    // Remove remaining control words
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
    use super::*;

    #[test]
    fn test_simple_rtf() {
        let rtf = r#"{\rtf1\ansi{\fonttbl\f0\fswiss Helvetica;}\f0\pard Test text\par}"#;
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
        let rtf = r#"{\rtf1\ansi Line 1\par Line 2\par}"#;
        let result = rtf_to_text(rtf).unwrap();
        assert!(result.contains("Line 1"));
        assert!(result.contains("Line 2"));
    }
}
