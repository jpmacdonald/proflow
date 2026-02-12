//! `ProPresenter` file serialization.
//!
//! Writes protobuf-encoded presentation files to disk.

#![allow(dead_code)]

use std::fs::File;
use std::io::{self, Write};
use std::path::Path;
use thiserror::Error;

use crate::propresenter::generated::rv_data;
use prost::Message;

/// Errors that can occur when writing `ProPresenter` files
#[derive(Error, Debug)]
pub enum SerializeError {
    /// An I/O error occurred during file operations
    #[error("IO error: {0}")]
    IoError(#[from] io::Error),

    /// Failed to encode the protobuf data
    #[error("Failed to encode ProPresenter file: {0}")]
    EncodeError(String),
}

/// Write a presentation to a `ProPresenter` file
///
/// # Arguments
///
/// * `presentation` - The presentation to serialize
/// * `path` - Path where the .pro file should be written
///
/// # Returns
///
/// Returns a Result indicating success or containing a `SerializeError`
///
/// # Example
///
/// ```no_run
/// use std::path::Path;
/// use proflow::propresenter::serialize::write_presentation_file;
/// use proflow::propresenter::generated::rv_data;
///
/// let presentation = rv_data::Presentation::default();
/// let path = Path::new("example.pro");
/// match write_presentation_file(&presentation, &path) {
///     Ok(_) => println!("Successfully wrote presentation"),
///     Err(e) => eprintln!("Error writing presentation: {}", e),
/// }
/// ```
pub fn write_presentation_file(
    presentation: &rv_data::Presentation,
    path: impl AsRef<Path>,
) -> Result<(), SerializeError> {
    let path = path.as_ref();
    let buf = encode_presentation(presentation);
    
    // Create the file and write the buffer to it
    let mut file = File::create(path)?;
    file.write_all(&buf)?;
    
    Ok(())
}

/// Encode a presentation to protobuf bytes (for embedding in playlists)
#[allow(clippy::expect_used)] // encode() only fails if buffer can't grow, impossible with Vec
pub fn encode_presentation(presentation: &rv_data::Presentation) -> Vec<u8> {
    let mut buf = Vec::new();
    presentation.encode(&mut buf).expect("Vec<u8> cannot fail to grow");
    buf
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic, clippy::float_cmp)]

    use super::*;
    use crate::propresenter::deserialize::read_presentation_file;
    use std::path::PathBuf;
    use std::fs;
    
    fn get_example_path(filename: &str) -> PathBuf {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("data");
        path.push("examples");
        path.push("propresenter");
        path.push(filename);
        path
    }

    fn get_test_output_path(filename: &str) -> PathBuf {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("out");
        path.push("test");
        path.push(filename);
        path
    }

    fn get_pro_output_path(filename: &str) -> PathBuf {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("out");
        path.push(filename);
        path
    }
    
    #[test]
    fn test_round_trip_simple_presentation() {
        // Read the original presentation
        let path = get_example_path("Tom Nametag.pro");
        let original = read_presentation_file(&path).expect("Failed to read original presentation");
        
        // Save the raw presentation struct to a test output file
        let test_output_path = get_test_output_path("test_output_tom_nametag.txt");
        let mut test_file = File::create(&test_output_path).expect("Failed to create test output file");
        writeln!(test_file, "{original:#?}").expect("Failed to write test output");

        // Write the presentation to a .pro file that can be opened in ProPresenter
        let pro_output_path = get_pro_output_path("tom_nametag_round_trip.pro");
        write_presentation_file(&original, &pro_output_path).expect("Failed to write presentation");
        
        // Read it back
        let round_trip = read_presentation_file(&pro_output_path).expect("Failed to read round-tripped presentation");
        
        // Verify key properties match
        assert_eq!(original.name, round_trip.name);
        assert_eq!(original.uuid, round_trip.uuid);
        assert_eq!(original.cues.len(), round_trip.cues.len());
        
        // Verify first cue's properties
        if let (Some(original_cue), Some(round_trip_cue)) = (original.cues.first(), round_trip.cues.first()) {
            assert_eq!(original_cue.uuid, round_trip_cue.uuid);
            assert_eq!(original_cue.actions.len(), round_trip_cue.actions.len());
        }
        
        println!("Successfully verified round-trip serialization");
        println!("Test output saved to: {}", test_output_path.display());
        println!("ProPresenter file saved to: {}", pro_output_path.display());
    }

    #[test]
    fn test_round_trip_amazing_grace() {
        // Read the original presentation
        let path = get_example_path("[Hymn] Amazing Grace.pro");
        let original = read_presentation_file(&path).expect("Failed to read original presentation");
        
        // Save the raw presentation struct to a test output file
        let test_output_path = get_test_output_path("test_output_amazing_grace.txt");
        let mut test_file = File::create(&test_output_path).expect("Failed to create test output file");
        writeln!(test_file, "{original:#?}").expect("Failed to write test output");

        // Write the presentation to a .pro file that can be opened in ProPresenter
        let pro_output_path = get_pro_output_path("amazing_grace_round_trip.pro");
        write_presentation_file(&original, &pro_output_path).expect("Failed to write presentation");
        
        // Read it back
        let round_trip = read_presentation_file(&pro_output_path).expect("Failed to read round-tripped presentation");
        
        // Verify key properties match
        assert_eq!(original.name, round_trip.name);
        assert_eq!(original.uuid, round_trip.uuid);
        assert_eq!(original.cues.len(), round_trip.cues.len());
        assert_eq!(original.cue_groups.len(), round_trip.cue_groups.len());
        
        // Verify first cue's properties
        if let (Some(original_cue), Some(round_trip_cue)) = (original.cues.first(), round_trip.cues.first()) {
            assert_eq!(original_cue.uuid, round_trip_cue.uuid);
            assert_eq!(original_cue.actions.len(), round_trip_cue.actions.len());
            
            // Verify text content in first action
            if let (Some(original_action), Some(round_trip_action)) = (original_cue.actions.first(), round_trip_cue.actions.first()) {
                assert_eq!(original_action.is_enabled, round_trip_action.is_enabled);
                assert_eq!(original_action.r#type, round_trip_action.r#type);
            }
        }
        
        println!("Successfully verified Amazing Grace round-trip serialization");
        println!("Test output saved to: {}", test_output_path.display());
        println!("ProPresenter file saved to: {}", pro_output_path.display());
    }

    #[test]
    fn test_round_trip_bible_verse() {
        // Read the original presentation
        let path = get_example_path("Titus 2v11-13 (NRSVue).pro");
        let original = read_presentation_file(&path).expect("Failed to read original presentation");
        
        // Save the raw presentation struct to a test output file
        let test_output_path = get_test_output_path("test_output_bible_verse.txt");
        let mut test_file = File::create(&test_output_path).expect("Failed to create test output file");
        writeln!(test_file, "{original:#?}").expect("Failed to write test output");

        // Write the presentation to a .pro file that can be opened in ProPresenter
        let pro_output_path = get_pro_output_path("titus_2v11-13_round_trip.pro");
        write_presentation_file(&original, &pro_output_path).expect("Failed to write presentation");
        
        // Read it back
        let round_trip = read_presentation_file(&pro_output_path).expect("Failed to read round-tripped presentation");
        
        // Verify key properties match
        assert_eq!(original.name, round_trip.name);
        assert_eq!(original.uuid, round_trip.uuid);
        assert_eq!(original.cues.len(), round_trip.cues.len());
        assert_eq!(original.cue_groups.len(), round_trip.cue_groups.len());
        
        // Verify first cue's properties
        if let (Some(original_cue), Some(round_trip_cue)) = (original.cues.first(), round_trip.cues.first()) {
            assert_eq!(original_cue.uuid, round_trip_cue.uuid);
            assert_eq!(original_cue.actions.len(), round_trip_cue.actions.len());
            
            // Verify text content in first action
            if let (Some(original_action), Some(round_trip_action)) = (original_cue.actions.first(), round_trip_cue.actions.first()) {
                assert_eq!(original_action.is_enabled, round_trip_action.is_enabled);
                assert_eq!(original_action.r#type, round_trip_action.r#type);
            }
        }
        
        println!("Successfully verified Bible verse round-trip serialization");
        println!("Test output saved to: {}", test_output_path.display());
        println!("ProPresenter file saved to: {}", pro_output_path.display());
    }

    #[test]
    fn test_write_empty_presentation() {
        // Create an empty presentation
        let empty = rv_data::Presentation::default();
        
        // Save the raw presentation struct to a test output file
        let test_output_path = get_test_output_path("test_output_empty.txt");
        let mut test_file = File::create(&test_output_path).expect("Failed to create test output file");
        writeln!(test_file, "{empty:#?}").expect("Failed to write test output");
        
        // Write the presentation to a .pro file that can be opened in ProPresenter
        let pro_output_path = get_pro_output_path("empty_presentation.pro");
        write_presentation_file(&empty, &pro_output_path).expect("Failed to write empty presentation");
        
        // Read it back
        let round_trip = read_presentation_file(&pro_output_path).expect("Failed to read empty presentation");
        
        // Verify properties match
        assert_eq!(round_trip.name, "");
        assert!(round_trip.cues.is_empty());
        assert!(round_trip.cue_groups.is_empty());
        assert!(round_trip.arrangements.is_empty());
        
        println!("Successfully verified empty presentation serialization");
        println!("Test output saved to: {}", test_output_path.display());
        println!("ProPresenter file saved to: {}", pro_output_path.display());
    }

    #[test]
    #[ignore = "Missing welcome_slides.pro file, needs to be created first"]
    fn test_analyze_welcome_slides() {
        let path = PathBuf::from("out/presentations/welcome_slides.pro");
        let presentation = read_presentation_file(&path).expect("Failed to read welcome slides");
        
        // Save the raw presentation struct to a test output file for analysis
        let test_output_path = get_test_output_path("test_output_welcome_slides.txt");
        let mut test_file = File::create(&test_output_path).expect("Failed to create test output file");
        writeln!(test_file, "{presentation:#?}").expect("Failed to write test output");
        
        // Basic presentation properties
        assert_eq!(presentation.name, "Welcome Slides");
        assert!(presentation.uuid.is_some(), "Presentation should have UUID");
        assert_eq!(presentation.category, "Services");
        
        // Verify slides exist
        assert!(!presentation.cues.is_empty(), "Expected presentation to have slides");
        assert_eq!(presentation.cues.len(), 2, "Should have exactly 2 slides");
        
        // Check first slide (title slide)
        if let Some(first_cue) = presentation.cues.first() {
            assert!(!first_cue.actions.is_empty(), "First slide should have actions");
            
            if let Some(first_action) = first_cue.actions.first() {
                assert!(first_action.is_enabled, "First action should be enabled");
                if let Some(rv_data::action::ActionTypeData::Slide(slide_type)) = &first_action.action_type_data {
                    if let Some(rv_data::action::slide_type::Slide::Presentation(pres_slide)) = &slide_type.slide {
                        if let Some(base_slide) = &pres_slide.base_slide {
                            // Should have two elements (title and subtitle)
                            assert_eq!(base_slide.elements.len(), 2, "Title slide should have 2 elements");
                            
                            // Check first element (title)
                            if let Some(first_element) = base_slide.elements.first() {
                                if let Some(graphics_element) = &first_element.element {
                                    if let Some(text_element) = &graphics_element.text {
                                        // Text element assertions
                                        assert!(text_element.attributes.is_some(), "Text element should have attributes");
                                        if let Some(attrs) = &text_element.attributes {
                                            assert!(attrs.font.is_some(), "Text attributes should have font info");
                                            if let Some(font) = &attrs.font {
                                                assert_eq!(font.name, "Helvetica");
                                                assert_eq!(font.size, 72.0);
                                                assert!(font.bold);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        
        println!("Successfully analyzed welcome slides presentation");
        println!("Test output saved to: {}", test_output_path.display());
    }

    #[test]
    #[ignore = "Missing welcome_slides.pro file, needs to be created first"]
    fn test_compare_with_example() {
        // Read an example presentation for comparison
        let example_path = get_example_path("Tom Nametag.pro");
        let example = read_presentation_file(&example_path).expect("Failed to read example presentation");
        
        // Save the example presentation struct for analysis
        let example_output_path = get_test_output_path("test_output_example_comparison.txt");
        let mut example_file = File::create(&example_output_path).expect("Failed to create example output file");
        writeln!(example_file, "{example:#?}").expect("Failed to write example output");

        // Read our generated presentation
        let our_path = PathBuf::from("out/presentations/welcome_slides.pro");
        let our_presentation = read_presentation_file(&our_path).expect("Failed to read our presentation");
        
        // Save our presentation struct for analysis
        let our_output_path = get_test_output_path("test_output_our_presentation.txt");
        let mut our_file = File::create(&our_output_path).expect("Failed to create our output file");
        writeln!(our_file, "{our_presentation:#?}").expect("Failed to write our output");
        
        println!("Comparison files saved to:");
        println!("  Example: {}", example_output_path.display());
        println!("  Ours: {}", our_output_path.display());
    }

    #[test]
    #[ignore = "Missing upcoming_events.pro file, needs to be created first"]
    fn test_analyze_upcoming_events() {
        let input_path = PathBuf::from("upcoming_events.pro");
        let output_path = PathBuf::from("out/test/test_output_upcoming_events.txt");

        // Create the output directory if it doesn't exist
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }

        let presentation = read_presentation_file(&input_path).unwrap();
        let output = format!("{presentation:#?}");
        fs::write(&output_path, output).unwrap();
    }

    #[test]
    fn test_verify_group_structure() {
        // Read the Tom Nametag example as our reference
        let example_path = get_example_path("Tom Nametag.pro");
        let example = read_presentation_file(&example_path).expect("Failed to read example presentation");
        
        // Save the raw presentation struct for analysis
        let example_output_path = get_test_output_path("test_output_group_structure_example.txt");
        let mut example_file = File::create(&example_output_path).expect("Failed to create example output file");
        writeln!(example_file, "{example:#?}").expect("Failed to write example output");

        // Verify group structure
        assert!(!example.cue_groups.is_empty(), "Presentation should have at least one cue group");
        
        if let Some(first_group) = example.cue_groups.first() {
            // Verify group has required fields
            assert!(first_group.group.is_some(), "Cue group should have a group");
            if let Some(group) = &first_group.group {
                assert!(group.uuid.is_some(), "Group should have a UUID");
                assert!(group.hot_key.is_some(), "Group should have a hot key");
                // Even if empty, these fields should exist
                assert_eq!(group.name, "", "Group name should be empty string");
                assert_eq!(group.application_group_name, "", "Application group name should be empty string");
            }
            
            // Verify cue identifiers match actual cues
            let cue_uuids: Vec<String> = example.cues.iter()
                .filter_map(|cue| cue.uuid.as_ref())
                .map(|uuid| uuid.string.clone())
                .collect();
            
            let group_cue_uuids: Vec<String> = first_group.cue_identifiers.iter()
                .map(|uuid| uuid.string.clone())
                .collect();
            
            assert!(!group_cue_uuids.is_empty(), "Group should have cue identifiers");
            
            // Verify all cues in the group exist in the presentation
            for uuid in &group_cue_uuids {
                assert!(cue_uuids.contains(uuid), "Group references cue that doesn't exist in presentation");
            }
        }
        
        println!("Successfully verified group structure");
        println!("Example output saved to: {}", example_output_path.display());
    }
} 