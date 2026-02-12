//! Presentation comparison and parsing utilities.

#![allow(dead_code)]

use std::fmt::Write;
use crate::propresenter::generated::rv_data;

/// Convert a raw protobuf `Presentation` to JSON string for comparison.
///
/// Uses the generated protobuf types directly, not our data model,
/// to ensure we catch any discrepancies between the raw file format
/// and what our builders produce.
pub fn presentation_to_json_string(presentation: &rv_data::Presentation) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(presentation)
}

/// Compare two raw protobuf Presentations and return their differences.
/// Returns a simple summary of whether they match or differ.
pub fn compare_presentations(
    original: &rv_data::Presentation,
    recreated: &rv_data::Presentation,
) -> Result<String, Box<dyn std::error::Error>> {
    let original_json = presentation_to_json_string(original)?;
    let recreated_json = presentation_to_json_string(recreated)?;

    if original_json == recreated_json {
        Ok("Presentations are identical".to_string())
    } else {
        // Simple line-by-line comparison without external dependencies
        let orig_lines: Vec<&str> = original_json.lines().collect();
        let rec_lines: Vec<&str> = recreated_json.lines().collect();

        let mut diff = String::new();
        let max_len = orig_lines.len().max(rec_lines.len());

        for i in 0..max_len {
            let orig = orig_lines.get(i).copied().unwrap_or("");
            let rec = rec_lines.get(i).copied().unwrap_or("");

            if orig != rec {
                if !orig.is_empty() {
                    let _ = writeln!(diff, "-{orig}");
                }
                if !rec.is_empty() {
                    let _ = writeln!(diff, "+{rec}");
                }
            }
        }

        if diff.is_empty() {
            Ok("Presentations are identical".to_string())
        } else {
            Ok(format!("Presentations differ:\n{diff}"))
        }
    }
}
