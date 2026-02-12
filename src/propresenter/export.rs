//! Export editor content to `ProPresenter` presentation files.
//!
//! Converts edited text content (with verse markers) into `ProPresenter` .pro files.

use std::path::Path;
use uuid::Uuid;

use super::builder::PresentationBuilder;
use super::convert::convert_presentation_to_rv_data;
use super::data_model::{
    self as dm, Action, BaseSlide, Color, CompletionActionType, CompletionTargetType, Cue,
    CueGroup, Element, Font, Group, ParagraphStyle, Point, Rect, Size, Slide, TextAlignment,
    TextElement,
};
use super::serialize::write_presentation_file;

/// Errors that can occur during export
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    /// An I/O error occurred during file operations
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// The presentation could not be built from the provided content
    #[error("Build error: {0}")]
    Build(String),

    /// Failed to serialize the presentation to disk
    #[error("Serialize error: {0}")]
    Serialize(#[from] super::serialize::SerializeError),
}

/// A parsed stanza from editor content
#[derive(Debug, Clone)]
pub struct Stanza {
    /// Group name (e.g., "Verse 1", "Chorus")
    pub label: Option<String>,
    /// Text content of the stanza
    pub lines: Vec<String>,
}

/// Parse editor content into stanzas
/// 
/// Content format:
/// - Lines starting with `[Label]` define group labels
/// - Blank lines separate stanzas
/// - Non-blank lines are slide content
pub fn parse_stanzas(content: &[String]) -> Vec<Stanza> {
    let mut stanzas = Vec::new();
    let mut current_label: Option<String> = None;
    let mut current_lines: Vec<String> = Vec::new();

    for line in content {
        let trimmed = line.trim();
        
        // Check for label markers like [Verse 1], [Chorus], etc.
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            // Save previous stanza if exists
            if !current_lines.is_empty() {
                stanzas.push(Stanza {
                    label: current_label.take(),
                    lines: std::mem::take(&mut current_lines),
                });
            }
            current_label = Some(trimmed[1..trimmed.len()-1].to_string());
        } else if trimmed.is_empty() {
            // Blank line: end current stanza
            if !current_lines.is_empty() {
                stanzas.push(Stanza {
                    label: current_label.take(),
                    lines: std::mem::take(&mut current_lines),
                });
            }
        } else {
            // Regular content line
            current_lines.push(line.clone());
        }
    }

    // Don't forget the last stanza
    if !current_lines.is_empty() {
        stanzas.push(Stanza {
            label: current_label,
            lines: current_lines,
        });
    }

    stanzas
}

/// Create a slide from stanza content
fn create_slide(content: &str) -> Slide {
    let text_element = TextElement {
        content: content.to_string(),
        font: Font {
            name: "Helvetica".to_string(),
            size: 72.0,
            bold: false,
            italic: false,
            family: "Helvetica".to_string(),
            face: "Regular".to_string(),
        },
        color: Color {
            red: 1.0,
            green: 1.0,
            blue: 1.0,
            alpha: 1.0,
        },
        paragraph_style: ParagraphStyle {
            alignment: TextAlignment::Center,
            ..Default::default()
        },
        shadow: Some(dm::Shadow {
            color: Color { red: 0.0, green: 0.0, blue: 0.0, alpha: 0.5 },
            radius: 4.0,
            offset: Point { x: 2.0, y: 2.0 },
            opacity: 0.5,
            angle: 315.0,
            style: dm::ShadowStyle::Drop,
            enable: true,
        }),
        mask: None,
        bounds: Some(Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size { width: 1920.0, height: 1080.0 },
        }),
        custom_attributes: Vec::new(),
        text_scroller: None,
    };

    Slide {
        base: BaseSlide {
            uuid: Uuid::new_v4(),
            elements: vec![Element::Text(text_element)],
            element_build_order: Vec::new(),
            guidelines: Vec::new(),
            draws_background_color: false,
            background_color: None,
            size: Size { width: 1920.0, height: 1080.0 },
        },
        notes: None,
        template_guidelines: Vec::new(),
        chord_chart: None,
        transition: None,
    }
}

/// Group colors for different stanza types
fn get_group_color(label: &str) -> Color {
    let label_lower = label.to_lowercase();
    if label_lower.contains("verse") {
        Color { red: 0.2, green: 0.4, blue: 1.0, alpha: 1.0 }  // Blue
    } else if label_lower.contains("chorus") {
        Color { red: 0.2, green: 0.8, blue: 0.4, alpha: 1.0 }  // Green
    } else if label_lower.contains("bridge") {
        Color { red: 0.8, green: 0.2, blue: 0.8, alpha: 1.0 }  // Magenta
    } else if label_lower.contains("tag") || label_lower.contains("ending") {
        Color { red: 0.2, green: 0.8, blue: 0.8, alpha: 1.0 }  // Cyan
    } else {
        Color { red: 0.6, green: 0.6, blue: 0.6, alpha: 1.0 }  // Gray
    }
}

/// Build a presentation from editor content
pub fn build_presentation_from_content(
    name: &str,
    content: &[String],
) -> Result<dm::Presentation, ExportError> {
    let stanzas = parse_stanzas(content);
    
    if stanzas.is_empty() {
        return Err(ExportError::Build("No content to export".to_string()));
    }

    let mut cues = Vec::new();
    let mut cue_groups = Vec::new();

    for stanza in &stanzas {
        // Combine stanza lines into slide content
        let slide_content = stanza.lines.join("\n");
        let slide = create_slide(&slide_content);
        
        let cue_uuid = Uuid::new_v4();
        let cue = Cue {
            uuid: cue_uuid,
            name: stanza.label.clone().unwrap_or_else(|| "Slide".to_string()),
            actions: vec![Action::Slide {
                uuid: Uuid::new_v4(),
                name: stanza.label.clone().unwrap_or_else(|| "Slide".to_string()),
                slide,
                delay_time: 0.0,
                duration: 0.0,
                enabled: true,
                layer_identification: None,
            }],
            enabled: true,
            hot_key: None,
            completion_target_type: CompletionTargetType::None,
            completion_target_uuid: None,
            completion_action_type: CompletionActionType::First,
            completion_action_uuid: None,
            completion_time: 0.0,
        };
        cues.push(cue);

        // Create a cue group for this stanza
        let group_name = stanza.label.clone().unwrap_or_else(|| "Slide".to_string());
        let group = CueGroup {
            group: Group {
                uuid: Uuid::new_v4(),
                name: group_name.clone(),
                color: get_group_color(&group_name),
                hot_key: None,
                application_group_identifier: Uuid::new_v4().to_string(),
            },
            cue_identifiers: vec![cue_uuid],
        };
        cue_groups.push(group);
    }

    // Build the arrangement with all groups
    let arrangement = dm::Arrangement {
        uuid: Uuid::new_v4(),
        name: "Default".to_string(),
        group_identifiers: cue_groups.iter().map(|g| g.group.uuid).collect(),
    };

    // Use the builder to create a valid presentation
    let presentation = PresentationBuilder::new(name)
        .with_category("Songs")
        .with_cues(cues)
        .with_cue_groups(cue_groups)
        .with_arrangements(vec![arrangement])
        .build()
        .map_err(ExportError::Build)?;

    Ok(presentation)
}

/// Export editor content to a .pro file
pub fn export_to_pro_file(
    name: &str,
    content: &[String],
    output_path: impl AsRef<Path>,
) -> Result<(), ExportError> {
    let presentation = build_presentation_from_content(name, content)?;
    let rv_presentation = convert_presentation_to_rv_data(presentation);
    write_presentation_file(&rv_presentation, output_path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn test_parse_stanzas_simple() {
        let content = vec![
            "[Verse 1]".to_string(),
            "Amazing grace how sweet the sound".to_string(),
            "That saved a wretch like me".to_string(),
            String::new(),
            "[Chorus]".to_string(),
            "I once was lost but now am found".to_string(),
        ];

        let stanzas = parse_stanzas(&content);
        assert_eq!(stanzas.len(), 2);
        assert_eq!(stanzas[0].label, Some("Verse 1".to_string()));
        assert_eq!(stanzas[0].lines.len(), 2);
        assert_eq!(stanzas[1].label, Some("Chorus".to_string()));
    }

    #[test]
    fn test_parse_stanzas_no_labels() {
        let content = vec![
            "First slide content".to_string(),
            String::new(),
            "Second slide content".to_string(),
        ];

        let stanzas = parse_stanzas(&content);
        assert_eq!(stanzas.len(), 2);
        assert!(stanzas[0].label.is_none());
        assert!(stanzas[1].label.is_none());
    }

    #[test]
    fn test_build_presentation() {
        let content = vec![
            "[Verse 1]".to_string(),
            "Test content".to_string(),
        ];

        let result = build_presentation_from_content("Test Song", &content);
        assert!(result.is_ok());
        
        let presentation = result.unwrap();
        assert_eq!(presentation.name, "Test Song");
        assert!(!presentation.cues.is_empty());
        assert!(!presentation.arrangements.is_empty());
    }
}

