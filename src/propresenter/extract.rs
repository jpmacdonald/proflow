//! Extract plain text content from ProPresenter files.

use std::path::Path;
use crate::propresenter::deserialize::read_presentation_file;
use crate::propresenter::rtf::rtf_to_text;
use crate::propresenter::generated::rv_data::{self, action::ActionTypeData};

/// Extract all slide text from a .pro file as editor-ready lines.
/// Returns lines with blank lines between slides (stanzas).
pub fn extract_text_from_pro(path: &Path) -> Result<Vec<String>, String> {
    let presentation = read_presentation_file(path)
        .map_err(|e| format!("Failed to read presentation: {}", e))?;
    
    let mut lines = Vec::new();
    let mut first_slide = true;
    
    // Iterate through cues to find slide actions
    for cue in &presentation.cues {
        for action in &cue.actions {
            if let Some(slide_text) = extract_slide_text(action) {
                // Add blank line between slides (except before first)
                if !first_slide && !lines.is_empty() {
                    lines.push(String::new());
                }
                first_slide = false;
                
                // Add cue name as label if meaningful
                if !cue.name.is_empty() && cue.name != "Slide" {
                    lines.push(format!("[{}]", cue.name));
                }
                
                // Add the slide text lines
                for line in slide_text.lines() {
                    lines.push(line.to_string());
                }
            }
        }
    }
    
    // Ensure trailing empty line for editor
    if !lines.is_empty() && !lines.last().map_or(true, |l| l.is_empty()) {
        lines.push(String::new());
    }
    
    Ok(lines)
}

/// Extract text from a slide action
fn extract_slide_text(action: &rv_data::Action) -> Option<String> {
    // Navigate through the oneof to find SlideType
    let action_data = action.action_type_data.as_ref()?;
    
    let slide_type = match action_data {
        ActionTypeData::Slide(s) => s,
        _ => return None,
    };
    
    // Get the slide from the oneof
    let slide_content = slide_type.slide.as_ref()?;
    
    use rv_data::action::slide_type::Slide;
    let presentation_slide = match slide_content {
        Slide::Presentation(ps) => ps,
        Slide::Prop(_) => return None,
    };
    
    // Get the base slide which contains elements
    let base_slide = presentation_slide.base_slide.as_ref()?;
    
    let mut text_parts = Vec::new();
    
    // Extract text from each element
    for element in &base_slide.elements {
        if let Some(graphics_element) = &element.element {
            if let Some(text) = extract_text_from_element(graphics_element) {
                if !text.trim().is_empty() {
                    text_parts.push(text);
                }
            }
        }
    }
    
    if text_parts.is_empty() {
        None
    } else {
        Some(text_parts.join("\n"))
    }
}

/// Extract text from a graphics element (handles RTF conversion)
fn extract_text_from_element(element: &rv_data::graphics::Element) -> Option<String> {
    // Get the text component of this element
    let text = element.text.as_ref()?;
    
    // Get RTF data
    if text.rtf_data.is_empty() {
        return None;
    }
    
    let rtf_string = String::from_utf8_lossy(&text.rtf_data);
    
    // Convert RTF to plain text
    rtf_to_text(&rtf_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    
    fn get_example_path(filename: &str) -> PathBuf {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("data");
        path.push("examples");
        path.push("propresenter");
        path.push(filename);
        path
    }
    
    #[test]
    fn test_extract_text_from_amazing_grace() {
        let path = get_example_path("[Hymn] Amazing Grace.pro");
        if path.exists() {
            let result = extract_text_from_pro(&path);
            assert!(result.is_ok(), "Should extract text successfully");
            let lines = result.unwrap();
            assert!(!lines.is_empty(), "Should have content");
        }
    }
}
