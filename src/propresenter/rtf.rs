//! Simple RTF to plain text conversion.
//!
//! This is a basic implementation that handles common RTF patterns
//! without requiring external dependencies.

#![allow(dead_code)]

use regex::Regex;

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
