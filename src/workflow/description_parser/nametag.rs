//! Parse performers and title text for content nametags.

use super::{split_performer_and_others, strip_speaker, ParsedContent, ParsedSegment, SpeakerRole};

pub(super) fn parse_content_nametag(description: &str, item_title: &str) -> ParsedContent {
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
    let display_lines = description
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('*'))
        .collect::<Vec<_>>();
    let display_description = display_lines.join(" ");

    // Parse description for composer/performer info.
    // Format: "Performer, Instrument / Composer / Arranger"
    let raw_parts: Vec<String> = display_description
        .split('/')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if has_colon && !piece_title.is_empty() {
        // Standard nametag: piece title from the item title, details from description
        segments.push(ParsedSegment {
            text: piece_title,
            speaker: SpeakerRole::Neutral,
            bold: None,
            italic: None,
        });

        if !raw_parts.is_empty() {
            let (performer, others) = split_performer_and_others(&raw_parts);
            if !others.is_empty() {
                segments.push(ParsedSegment {
                    text: others.join(" / "),
                    speaker: SpeakerRole::Neutral,
                    bold: None,
                    italic: None,
                });
            }
            if let Some(performer) = performer {
                segments.push(ParsedSegment {
                    text: performer,
                    speaker: SpeakerRole::Neutral,
                    bold: None,
                    italic: None,
                });
            }
        }
    } else {
        // No colon in title — use description lines as content directly
        if display_lines.is_empty() {
            // Last resort: use stripped title
            segments.push(ParsedSegment {
                text: strip_speaker(item_title),
                speaker: SpeakerRole::Neutral,
                bold: None,
                italic: None,
            });
        } else {
            for line in display_lines {
                segments.push(ParsedSegment {
                    text: line.to_string(),
                    speaker: SpeakerRole::Neutral,
                    bold: None,
                    italic: None,
                });
            }
        }
    }

    ParsedContent::new(segments, None)
}
