//! Description parser for PCO item descriptions.
//!
//! Extracts slide-worthy content from Planning Center descriptions, handling
//! `[SLIDE]`/`[SLIDE/ALL]` markers, responsive reading patterns (`Leader:`/`People:`),
//! and content nametag formatting.

use serde::Serialize;

use super::classify::strip_speaker;
use crate::propresenter::rtf::StyledSegment;

/// Banana yellow used for congregational responses (ALL/PEOPLE lines).
const YELLOW: &str = "#FEFC8B";

/// A parsed segment of description text with formatting hints.
#[derive(Debug, Clone, Serialize)]
pub struct ParsedSegment {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
}

/// Result of parsing a description into slide content.
#[derive(Debug, Clone, Serialize)]
pub struct ParsedContent {
    pub segments: Vec<ParsedSegment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_text: Option<String>,
}

/// Parse a PCO item description into slide-ready segments.
///
/// Strategy varies by type:
/// - `liturgical_edited`: marker-based (`[SLIDE]`, `[SLIDE/ALL]`) or responsive reading
/// - `content_nametag`: extract piece title, composer, performer
///
/// Returns `None` if the description has no slide-worthy content.
pub fn parse_description(
    description: &str,
    item_title: &str,
    type_key: &str,
) -> Option<ParsedContent> {
    match type_key {
        "liturgical_edited" => parse_liturgical(description, item_title),
        "content_nametag" => Some(parse_content_nametag(description, item_title)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Liturgical parsing
// ---------------------------------------------------------------------------

fn parse_liturgical(description: &str, item_title: &str) -> Option<ParsedContent> {
    let title_text = strip_speaker(item_title);

    // Strategy 1: responsive reading (Leader:/People: prefixes) — takes priority
    // because descriptions sometimes have [SLIDE] metadata on instruction lines
    // alongside LEADER:/ALL: content lines.
    if has_responsive_pattern(description) {
        return parse_responsive(description, &title_text);
    }

    // Strategy 2: marker-based parsing ([SLIDE], [SLIDE/ALL], [no slide])
    if has_slide_markers(description) {
        return parse_markers(description, &title_text);
    }

    // Strategy 3: plain text — all lines become default-colored segments
    let lines: Vec<&str> = description
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() {
        return None;
    }

    let segments = lines
        .iter()
        .map(|line| ParsedSegment {
            text: (*line).to_string(),
            color: None,
            bold: None,
            italic: None,
        })
        .collect();

    Some(ParsedContent {
        segments,
        title_text: Some(title_text),
    })
}

/// Check for [SLIDE], [SLIDE/ALL], or [no slide] markers.
fn has_slide_markers(description: &str) -> bool {
    let upper = description.to_uppercase();
    upper.contains("[SLIDE]")
        || upper.contains("[SLIDE/ALL]")
        || upper.contains("[NO SLIDE]")
        || upper.contains("NO SLIDE]")
}

/// Responsive reading leader prefixes (case-insensitive, with or without colon).
const LEADER_PREFIXES: &[&str] = &["leader:", "leader "];
/// Responsive reading congregation prefixes.
const CONGREGATION_PREFIXES: &[&str] =
    &["people:", "people ", "all:", "all ", "unison:", "unison "];

/// Check for responsive reading patterns.
///
/// Requires at least one leader-type AND one congregation-type prefix
/// appearing at the start of a line. Avoids false positives from short
/// prefixes like "l:" matching "url:".
fn has_responsive_pattern(description: &str) -> bool {
    let mut has_leader = false;
    let mut has_congregation = false;

    for line in description.lines() {
        let lower = line.trim().to_lowercase();
        if !has_leader {
            has_leader = LEADER_PREFIXES.iter().any(|p| lower.starts_with(p));
        }
        if !has_congregation {
            has_congregation = CONGREGATION_PREFIXES.iter().any(|p| lower.starts_with(p));
        }
        if has_leader && has_congregation {
            return true;
        }
    }
    false
}

/// Parse marker-based descriptions.
///
/// Scans for `[SLIDE/ALL]` and `[SLIDE]` markers. Content inside square brackets
/// after `[SLIDE/ALL]` is extracted as yellow. Content after `[SLIDE]` uses
/// default color. Lines with `[no slide]` or similar are skipped.
fn parse_markers(description: &str, title_text: &str) -> Option<ParsedContent> {
    let mut segments: Vec<ParsedSegment> = Vec::new();

    for line in description.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let upper = trimmed.to_uppercase();

        // Skip non-slide lines
        if upper.contains("NO SLIDE]") || upper.contains("[SILENT") {
            continue;
        }

        if upper.contains("[SLIDE/ALL]") {
            let content = extract_bracketed_content(trimmed)
                .or_else(|| extract_after_marker(trimmed, "[SLIDE/ALL]"));
            if let Some(text) = content {
                segments.push(ParsedSegment {
                    text,
                    color: Some(YELLOW.to_string()),
                    bold: None,
                    italic: None,
                });
            }
        } else if upper.contains("[SLIDE]") {
            let content = extract_bracketed_content(trimmed)
                .or_else(|| extract_after_marker(trimmed, "[SLIDE]"));
            if let Some(text) = content {
                segments.push(ParsedSegment {
                    text,
                    color: None,
                    bold: None,
                    italic: None,
                });
            }
        }
    }

    if segments.is_empty() {
        return None;
    }

    Some(ParsedContent {
        segments,
        title_text: Some(title_text.to_string()),
    })
}

/// Parse responsive reading descriptions (`Leader:`/`People:` format).
///
/// Keeps LEADER:/ALL:/PEOPLE: cue prefixes in the output text.
/// Groups content by blank-line-separated sections — each section becomes
/// one slide (one `ParsedSegment` per cued line within the section).
/// Metadata lines (containing `[SLIDE]`, `Liturgist:`, etc.) are filtered out.
fn parse_responsive(description: &str, title_text: &str) -> Option<ParsedContent> {
    let mut segments: Vec<ParsedSegment> = Vec::new();
    let mut current_color: Option<String> = None;

    for line in description.lines() {
        let trimmed = line.trim();

        // Blank lines become slide breaks (empty segment as separator)
        if trimmed.is_empty() {
            if !segments.is_empty() {
                segments.push(ParsedSegment {
                    text: String::new(),
                    color: None,
                    bold: None,
                    italic: None,
                });
            }
            continue;
        }

        // Skip metadata/instruction lines
        if is_metadata_line(trimmed) {
            continue;
        }

        let lower = trimmed.to_lowercase();

        if starts_with_any(&lower, LEADER_PREFIXES) {
            current_color = None;
            segments.push(ParsedSegment {
                text: trimmed.to_string(),
                color: None,
                bold: None,
                italic: None,
            });
        } else if starts_with_any(&lower, CONGREGATION_PREFIXES) {
            current_color = Some(YELLOW.to_string());
            segments.push(ParsedSegment {
                text: trimmed.to_string(),
                color: Some(YELLOW.to_string()),
                bold: None,
                italic: None,
            });
        } else {
            // Continuation line — inherit previous color
            segments.push(ParsedSegment {
                text: trimmed.to_string(),
                color: current_color.clone(),
                bold: None,
                italic: None,
            });
        }
    }

    // Trim trailing empty separators
    while segments.last().is_some_and(|s| s.text.is_empty()) {
        segments.pop();
    }

    if segments.is_empty() {
        return None;
    }

    Some(ParsedContent {
        segments,
        title_text: Some(title_text.to_string()),
    })
}

/// Check if a line starts with any of the given prefixes.
fn starts_with_any(lower: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|p| lower.starts_with(p))
}

/// Detect metadata/instruction lines that should not become slide content.
fn is_metadata_line(line: &str) -> bool {
    let upper = line.to_uppercase();
    // Lines with [SLIDE], [NO SLIDE], etc. are PCO cues
    if upper.contains("[SLIDE]") || upper.contains("[NO SLIDE]") || upper.contains("[SILENT") {
        return true;
    }
    // "Liturgist:" instruction lines (not the same as "Leader:")
    let lower = line.to_lowercase();
    if lower.starts_with("liturgist:") {
        return true;
    }
    false
}

// ---------------------------------------------------------------------------
// Content nametag parsing
// ---------------------------------------------------------------------------

fn parse_content_nametag(description: &str, item_title: &str) -> ParsedContent {
    // Extract piece title from item title after colon
    // e.g. "Organ Prelude: Meditation with Aria" → "Meditation with Aria"
    // When no colon, the title isn't useful nametag content (e.g. "Giving of
    // Tithes and Offerings") — use description lines directly if available.
    let has_colon = item_title.contains(':');
    let piece_title = if has_colon {
        item_title
            .split_once(':')
            .map(|(_, rest)| strip_speaker(rest.trim()))
            .unwrap_or_default()
    } else {
        String::new()
    };

    let mut segments: Vec<ParsedSegment> = Vec::new();

    // Parse description for composer/performer info.
    // Format: "Performer, Instrument / Composer / Arranger"
    let raw_parts: Vec<String> = description
        .split('/')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if has_colon && !piece_title.is_empty() {
        // Standard nametag: piece title from the item title, details from description
        segments.push(ParsedSegment {
            text: piece_title,
            color: None,
            bold: None,
            italic: None,
        });

        if !raw_parts.is_empty() {
            let (performer, others) = split_performer_and_others(&raw_parts);
            if !others.is_empty() {
                segments.push(ParsedSegment {
                    text: others.join(" / "),
                    color: None,
                    bold: None,
                    italic: None,
                });
            }
            if let Some(performer) = performer {
                segments.push(ParsedSegment {
                    text: performer,
                    color: None,
                    bold: None,
                    italic: None,
                });
            }
        }
    } else {
        // No colon in title — use description lines as content directly
        let desc_lines: Vec<&str> = description
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();

        if desc_lines.is_empty() {
            // Last resort: use stripped title
            segments.push(ParsedSegment {
                text: strip_speaker(item_title),
                color: None,
                bold: None,
                italic: None,
            });
        } else {
            for line in desc_lines {
                segments.push(ParsedSegment {
                    text: line.to_string(),
                    color: None,
                    bold: None,
                    italic: None,
                });
            }
        }
    }

    ParsedContent {
        segments,
        title_text: None,
    }
}

/// Split parts into performer (first entry with comma) and everything else.
fn split_performer_and_others(parts: &[String]) -> (Option<String>, Vec<String>) {
    let mut performer: Option<String> = None;
    let mut others: Vec<String> = Vec::new();
    for part in parts {
        if part.contains(',') && performer.is_none() {
            performer = Some(part.clone());
        } else {
            others.push(part.clone());
        }
    }
    (performer, others)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract content from inside square brackets in a line.
///
/// For a line like `[SLIDE/ALL] - [Precious Lord, the cross...]`, extracts
/// "Precious Lord, the cross...".
fn extract_bracketed_content(line: &str) -> Option<String> {
    // Find the last set of square brackets (skip the marker itself)
    let upper = line.to_uppercase();
    let marker_end = upper
        .find("[SLIDE/ALL]")
        .map(|i| i + "[SLIDE/ALL]".len())
        .or_else(|| upper.find("[SLIDE]").map(|i| i + "[SLIDE]".len()))?;

    let rest = &line[marker_end..];

    // Look for [content] in the rest
    if let Some(start) = rest.find('[') {
        if let Some(end) = rest[start..].find(']') {
            let content = rest[start + 1..start + end].trim();
            if !content.is_empty() {
                return Some(content.to_string());
            }
        }
    }

    None
}

/// Extract text after a marker like "[SLIDE]", stripping the marker and
/// any leading separator.
fn extract_after_marker(line: &str, marker: &str) -> Option<String> {
    let upper = line.to_uppercase();
    let pos = upper.find(&marker.to_uppercase())?;
    let rest = line[pos + marker.len()..].trim();
    let rest = rest.trim_start_matches('-').trim_start_matches(':').trim();
    if rest.is_empty() {
        return None;
    }
    Some(rest.to_string())
}

/// Convert `ParsedContent` segments into `StyledSegment` for RTF generation.
pub fn to_styled_segments(parsed: &ParsedContent) -> Vec<StyledSegment> {
    parsed
        .segments
        .iter()
        .map(|seg| StyledSegment {
            text: seg.text.clone(),
            color: seg.color.as_deref().and_then(parse_hex_color),
            bold: seg.bold,
            italic: seg.italic,
        })
        .collect()
}

/// Parse a hex color string like "#FEFC8B" into RGB tuple.
fn parse_hex_color(s: &str) -> Option<(u8, u8, u8)> {
    let hex = s.strip_prefix('#').unwrap_or(s);
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some((r, g, b))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn test_responsive_reading() {
        let desc = "Leader: The Lord is my shepherd;\nPeople: I shall not want.\nLeader: He makes me lie down in green pastures.\nAll: He restores my soul.";
        let result = parse_description(desc, "Call to Worship (Robert)", "liturgical_edited");
        assert!(result.is_some());
        let content = result.unwrap();
        assert_eq!(content.title_text.as_deref(), Some("Call to Worship"));
        assert_eq!(content.segments.len(), 4);
        // Leader lines keep prefix and have no color (white/default)
        assert!(content.segments[0].text.starts_with("Leader:"));
        assert!(content.segments[0].color.is_none());
        // People/All lines keep prefix and are yellow
        assert!(content.segments[1].text.starts_with("People:"));
        assert_eq!(content.segments[1].color.as_deref(), Some("#FEFC8B"));
        assert!(content.segments[3].text.starts_with("All:"));
        assert_eq!(content.segments[3].color.as_deref(), Some("#FEFC8B"));
    }

    #[test]
    fn test_marker_parsing() {
        let desc = "[CONFESSION no slide] - If we say that we have no sin...\n[SLIDE/ALL] - [Precious Lord, the cross is ever before us...]\n[SILENT CONFESSION]\n[ASSURANCE no slide] - Rejoice!";
        let result = parse_description(desc, "Prayer of Confession (Hope)", "liturgical_edited");
        assert!(result.is_some());
        let content = result.unwrap();
        assert_eq!(content.segments.len(), 1);
        assert!(content.segments[0].text.contains("Precious Lord"));
        assert_eq!(content.segments[0].color.as_deref(), Some("#FEFC8B"));
    }

    #[test]
    fn test_content_nametag() {
        let desc = "Marilyn Shenenberger, Organ / Darwin Wolford / Eugene Butler";
        let result = parse_description(
            desc,
            "Organ Prelude: Meditation with Aria",
            "content_nametag",
        );
        assert!(result.is_some());
        let content = result.unwrap();
        assert_eq!(content.segments[0].text, "Meditation with Aria");
        assert!(content.title_text.is_none());
    }

    #[test]
    fn test_to_styled_segments() {
        let parsed = ParsedContent {
            segments: vec![
                ParsedSegment {
                    text: "Hello".to_string(),
                    color: None,
                    bold: None,
                    italic: None,
                },
                ParsedSegment {
                    text: "World".to_string(),
                    color: Some("#FEFC8B".to_string()),
                    bold: Some(true),
                    italic: None,
                },
            ],
            title_text: None,
        };
        let styled = to_styled_segments(&parsed);
        assert_eq!(styled.len(), 2);
        assert!(styled[0].color.is_none());
        assert_eq!(styled[1].color, Some((254, 252, 139)));
        assert_eq!(styled[1].bold, Some(true));
    }

    #[test]
    fn test_no_content_returns_none() {
        let result = parse_description("", "Empty Item", "liturgical_edited");
        assert!(result.is_none());
    }

    #[test]
    fn test_plain_text_fallback() {
        let desc = "Grace and peace to you.\nIn Christ we are made whole.";
        let result = parse_description(desc, "Affirmation of Faith", "liturgical_edited");
        assert!(result.is_some());
        let content = result.unwrap();
        assert_eq!(content.segments.len(), 2);
        // Plain text has no color override
        assert!(content.segments[0].color.is_none());
    }

    #[test]
    fn test_short_prefix_no_false_positive() {
        // "l:" and "p:" should NOT trigger responsive reading detection
        let desc = "See the full color: blue.\nVisit url: example.com\nAll: together now.";
        // Has "all:" but no "leader:" — should NOT be responsive
        assert!(!has_responsive_pattern(desc));
    }

    #[test]
    fn test_slide_all_without_brackets() {
        let desc = "[SLIDE/ALL] Hear our prayer, O Lord.";
        let result = parse_description(desc, "Prayer (Hope)", "liturgical_edited");
        assert!(result.is_some());
        let content = result.unwrap();
        assert_eq!(content.segments.len(), 1);
        assert!(content.segments[0].text.contains("Hear our prayer"));
        assert_eq!(content.segments[0].color.as_deref(), Some("#FEFC8B"));
    }

    #[test]
    fn test_content_nametag_no_colon() {
        let desc = "Special offering for missions";
        let result = parse_description(desc, "Giving of Tithes and Offerings", "content_nametag");
        assert!(result.is_some());
        let content = result.unwrap();
        // Should use description content, not the full title
        assert_eq!(content.segments[0].text, "Special offering for missions");
    }

    #[test]
    fn test_responsive_without_colon() {
        // "Leader " (space, no colon) should also work
        let desc = "Leader The Lord is good.\nPeople We give thanks.";
        let result = parse_description(desc, "Responsive Reading", "liturgical_edited");
        assert!(result.is_some());
        let content = result.unwrap();
        assert_eq!(content.segments.len(), 2);
        // Prefixes kept in text
        assert!(content.segments[0].text.starts_with("Leader"));
        assert!(content.segments[0].color.is_none());
        assert!(content.segments[1].text.starts_with("People"));
        assert_eq!(content.segments[1].color.as_deref(), Some("#FEFC8B"));
    }

    #[test]
    fn test_responsive_filters_metadata() {
        let desc = "Liturgist: Bill Ichord; Scripture/Liturgy [SLIDE]\nLEADER: The Lord reigns.\nALL: Let the earth rejoice.";
        let result = parse_description(desc, "Call to Worship", "liturgical_edited");
        assert!(result.is_some());
        let content = result.unwrap();
        // Metadata line filtered out, only LEADER and ALL remain
        assert_eq!(content.segments.len(), 2);
        assert!(content.segments[0].text.starts_with("LEADER:"));
        assert!(content.segments[1].text.starts_with("ALL:"));
    }

    #[test]
    fn test_responsive_blank_line_separators() {
        let desc = "LEADER: First section.\nALL: Response one.\n\nLEADER: Second section.\nALL: Response two.";
        let result = parse_description(desc, "Call to Worship", "liturgical_edited");
        assert!(result.is_some());
        let content = result.unwrap();
        // 4 content segments + 1 empty separator = 5
        assert_eq!(content.segments.len(), 5);
        assert!(content.segments[2].text.is_empty());
    }
}
