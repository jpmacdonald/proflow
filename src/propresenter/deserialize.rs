use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use thiserror::Error;

use crate::propresenter::generated::rv_data;
use prost::Message;

/// On-disk content found behind a `.pro` filename.
///
/// Current `ProPresenter` library presentations are raw `Presentation`
/// protobuf messages. A filename alone is not a format contract: ZIP exports,
/// JSON fixtures, playlist nodes, and marker files can all be given the same
/// extension and must not enter the presentation index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationFileFormat {
    /// A raw presentation protobuf with the required document identity.
    NativePresentation,
    /// A ZIP container, including old test/export packages with `data.pro`.
    ZipArchive,
    /// JSON content rather than a native presentation.
    Json,
    /// A standalone playlist node protobuf rather than a presentation.
    Playlist,
    /// Plain UTF-8 text rather than a native presentation.
    Text,
    /// Binary content that is not a supported presentation.
    UnknownBinary,
}

impl std::fmt::Display for PresentationFileFormat {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::NativePresentation => "native presentation",
            Self::ZipArchive => "ZIP archive",
            Self::Json => "JSON",
            Self::Playlist => "playlist protobuf",
            Self::Text => "plain text",
            Self::UnknownBinary => "unknown binary",
        })
    }
}

/// Detect the actual content stored behind a `.pro` filename.
#[must_use]
pub fn detect_presentation_file_format(data: &[u8]) -> PresentationFileFormat {
    if data.starts_with(b"PK\x03\x04") || data.starts_with(b"PK\x05\x06") {
        return PresentationFileFormat::ZipArchive;
    }

    let utf8 = std::str::from_utf8(data).ok();
    if utf8.is_some_and(|text| {
        let trimmed = text.trim_start_matches('\u{feff}').trim_start();
        trimmed.starts_with('{') || trimmed.starts_with('[')
    }) {
        return PresentationFileFormat::Json;
    }

    if rv_data::Presentation::decode(data)
        .is_ok_and(|presentation| has_native_document_identity(&presentation))
    {
        return PresentationFileFormat::NativePresentation;
    }

    if rv_data::Playlist::decode(data).is_ok_and(|playlist| {
        playlist
            .uuid
            .as_ref()
            .is_some_and(|uuid| !uuid.string.trim().is_empty())
            && !playlist.name.trim().is_empty()
    }) {
        return PresentationFileFormat::Playlist;
    }

    if utf8.is_some_and(|text| {
        text.chars()
            .all(|character| !character.is_control() || character.is_whitespace())
    }) {
        return PresentationFileFormat::Text;
    }

    PresentationFileFormat::UnknownBinary
}

pub(crate) fn has_native_document_identity(presentation: &rv_data::Presentation) -> bool {
    !presentation.name.trim().is_empty()
        && presentation
            .uuid
            .as_ref()
            .is_some_and(|uuid| !uuid.string.trim().is_empty())
}

/// Errors that can occur when reading `ProPresenter` files
#[derive(Error, Debug)]
pub enum ProPresenterError {
    /// The specified file was not found
    #[error("File not found: {0}")]
    FileNotFound(String),

    /// An I/O error occurred during file operations
    #[error("IO error: {0}")]
    IoError(#[from] io::Error),

    /// Failed to decode the protobuf data
    #[error("Failed to decode ProPresenter file: {0}")]
    DecodeError(#[from] prost::DecodeError),

    /// The file has a `.pro` extension but is not a supported presentation.
    #[error("Unsupported ProPresenter file format at {path}: {format}")]
    UnsupportedFormat {
        /// Path whose content was inspected.
        path: String,
        /// Detected on-disk format.
        format: PresentationFileFormat,
    },
}

/// Read a `ProPresenter` file and return the deserialized presentation
///
/// # Arguments
///
/// * `path` - Path to the .pro file to read
///
/// # Returns
///
/// Returns a Result containing either the deserialized Presentation or a `ProPresenterError`
///
/// # Example
///
/// ```no_run
/// use std::path::Path;
/// use proflow::propresenter::deserialize::read_presentation_file;
///
/// let path = Path::new("example.pro");
/// match read_presentation_file(&path) {
///     Ok(presentation) => println!("Loaded presentation: {}", presentation.name),
///     Err(e) => eprintln!("Error loading presentation: {}", e),
/// }
/// ```
pub fn read_presentation_file(
    path: impl AsRef<Path>,
) -> Result<rv_data::Presentation, ProPresenterError> {
    let path = path.as_ref();

    // Open the file
    let mut file = File::open(path).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => ProPresenterError::FileNotFound(path.display().to_string()),
        _ => ProPresenterError::IoError(e),
    })?;

    // Read the file contents into a buffer
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;

    let format = detect_presentation_file_format(&buffer);
    if format != PresentationFileFormat::NativePresentation {
        return Err(ProPresenterError::UnsupportedFormat {
            path: path.display().to_string(),
            format,
        });
    }

    // The format gate above proves this is a decodable, identified document.
    let presentation = rv_data::Presentation::decode(&buffer[..])?;

    Ok(presentation)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;
    use crate::propresenter::generated::rv_data::action;
    use std::io::Write;
    use std::path::PathBuf;

    fn get_example_path(filename: &str) -> PathBuf {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("data");
        path.push("examples");
        path.push("propresenter");
        path.push(filename);
        path
    }

    fn uuid(value: &str) -> rv_data::Uuid {
        rv_data::Uuid {
            string: value.to_string(),
        }
    }

    #[test]
    fn detects_content_instead_of_trusting_pro_extension() {
        let native = rv_data::Presentation {
            uuid: Some(uuid("native-id")),
            name: "Native".to_string(),
            ..Default::default()
        }
        .encode_to_vec();
        let playlist = rv_data::Playlist {
            uuid: Some(uuid("playlist-id")),
            name: "Test Playlist".to_string(),
            r#type: rv_data::playlist::Type::Playlist as i32,
            ..Default::default()
        }
        .encode_to_vec();

        assert_eq!(
            detect_presentation_file_format(&native),
            PresentationFileFormat::NativePresentation
        );
        assert_eq!(
            detect_presentation_file_format(b"PK\x03\x04archive"),
            PresentationFileFormat::ZipArchive
        );
        assert_eq!(
            detect_presentation_file_format(b"  {\"slides\": []}"),
            PresentationFileFormat::Json
        );
        assert_eq!(
            detect_presentation_file_format(b"\xef\xbb\xbf [\n]"),
            PresentationFileFormat::Json
        );
        assert_eq!(
            detect_presentation_file_format(&playlist),
            PresentationFileFormat::Playlist
        );
        assert_eq!(
            detect_presentation_file_format(b"MOCKPRESENTATION"),
            PresentationFileFormat::Text
        );
        assert_eq!(
            detect_presentation_file_format("not a présentation".as_bytes()),
            PresentationFileFormat::Text
        );
        assert_eq!(
            detect_presentation_file_format(&[0xff, 0x00]),
            PresentationFileFormat::UnknownBinary
        );
        assert_eq!(
            detect_presentation_file_format(&[0x0a, 0x01, b'x', 0x12, 0x01, b'y']),
            PresentationFileFormat::UnknownBinary,
            "UTF-8 protobuf control bytes are not plain text"
        );
    }

    #[test]
    fn read_rejects_non_native_pro_file_with_typed_format() {
        let directory = tempfile::tempdir().expect("create temp directory");
        let path = directory.path().join("fixture.pro");
        std::fs::write(&path, b"{\"slides\":[]}").expect("write JSON fixture");

        let error = read_presentation_file(&path).expect_err("JSON is not a presentation");
        assert!(matches!(
            error,
            ProPresenterError::UnsupportedFormat {
                format: PresentationFileFormat::Json,
                ..
            }
        ));
    }

    #[test]
    fn test_read_non_existent_file() {
        let path = PathBuf::from("non_existent.pro");
        let result = read_presentation_file(&path);

        match result {
            Err(ProPresenterError::FileNotFound(_)) => (),
            _ => panic!("Expected FileNotFound error"),
        }
    }

    #[test]
    fn test_read_amazing_grace() {
        let path = get_example_path("[Hymn] Amazing Grace.pro");
        let result = read_presentation_file(&path);

        match result {
            Ok(presentation) => {
                // Basic presentation properties
                assert_eq!(presentation.name, "[Hymn] Amazing Grace");
                assert!(presentation.uuid.is_some(), "Presentation should have UUID");

                // Application info
                assert!(
                    presentation.application_info.is_some(),
                    "Expected application info to be present"
                );
                if let Some(app_info) = presentation.application_info {
                    assert_eq!(app_info.application, 1, "Expected application type to be 1");
                }

                // Verify slide structure
                assert!(
                    !presentation.cues.is_empty(),
                    "Expected presentation to have slides"
                );
                if let Some(first_cue) = presentation.cues.first() {
                    assert!(first_cue.uuid.is_some(), "First cue should have UUID");
                    assert!(
                        !first_cue.actions.is_empty(),
                        "First cue should have actions"
                    );

                    // Check first action
                    if let Some(first_action) = first_cue.actions.first() {
                        assert!(first_action.is_enabled, "First action should be enabled");
                        if let Some(action::ActionTypeData::Slide(slide_type)) =
                            &first_action.action_type_data
                        {
                            if let Some(action::slide_type::Slide::Presentation(pres_slide)) =
                                &slide_type.slide
                            {
                                if let Some(base_slide) = &pres_slide.base_slide {
                                    // Verify text content
                                    if let Some(first_element) = base_slide.elements.first() {
                                        if let Some(graphics_element) = &first_element.element {
                                            if let Some(text_element) = &graphics_element.text {
                                                assert!(
                                                    text_element.attributes.is_some(),
                                                    "Text should have attributes"
                                                );
                                                // Verify font settings for hymn text
                                                if let Some(attrs) = &text_element.attributes {
                                                    if let Some(font) = &attrs.font {
                                                        assert_eq!(
                                                            font.name, "Helvetica",
                                                            "Hymn should use Helvetica font"
                                                        );
                                                        assert!(
                                                            font.size > 0.0,
                                                            "Font size should be positive"
                                                        );
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

                // Verify cue groups (verses and chorus)
                assert!(
                    !presentation.cue_groups.is_empty(),
                    "Should have cue groups for verses"
                );
                if let Some(first_group) = presentation.cue_groups.first() {
                    assert!(first_group.group.is_some(), "Should have group info");
                    if let Some(group) = &first_group.group {
                        assert!(group.uuid.is_some(), "Group should have UUID");
                        assert!(
                            !first_group.cue_identifiers.is_empty(),
                            "Group should have cue identifiers"
                        );
                    }
                }

                println!("Successfully verified Amazing Grace presentation structure");
            }
            Err(e) => panic!("Failed to read presentation: {e}"),
        }
    }

    #[test]
    fn test_read_simple_presentation() {
        let path = get_example_path("Tom Nametag.pro");
        let result = read_presentation_file(&path);

        assert!(result.is_ok(), "Failed to read presentation file");
        let presentation = result.unwrap();

        // Save the raw presentation struct to a file for analysis
        if let Ok(mut file) = File::create("test_output_tom_nametag.txt") {
            writeln!(file, "{presentation:#?}").expect("Failed to write presentation debug output");
        }

        // Basic presentation properties
        assert_eq!(presentation.name, "Tom Nametag");
        assert!(
            presentation.uuid.is_some(),
            "Presentation should have a UUID"
        );
        assert!(
            presentation.category.is_empty(),
            "Category should be empty for this presentation"
        );
        assert!(
            presentation.notes.is_empty(),
            "Notes should be empty for this presentation"
        );

        // Application info assertions
        assert!(
            presentation.application_info.is_some(),
            "Application info should be present"
        );
        if let Some(app_info) = presentation.application_info {
            assert_eq!(app_info.application, 1, "Application type should be 1");
            assert!(
                app_info.application_version.is_some(),
                "Application version should be present"
            );
            if let Some(version) = app_info.application_version {
                assert_eq!(version.major_version, 7);
                assert_eq!(version.minor_version, 9);
                assert_eq!(version.patch_version, 2);
                assert_eq!(version.build, "118030852");
            }
        }

        // Cue assertions
        assert_eq!(presentation.cues.len(), 2, "Should have exactly 2 cues");

        // First cue assertions
        if let Some(first_cue) = presentation.cues.first() {
            assert!(first_cue.uuid.is_some(), "First cue should have a UUID");
            assert_eq!(
                first_cue.actions.len(),
                4,
                "First cue should have 4 actions"
            );

            // Check first action of first cue
            if let Some(first_action) = first_cue.actions.first() {
                assert_eq!(
                    first_action.r#type,
                    rv_data::action::ActionType::PresentationSlide as i32
                );
                assert!(first_action.is_enabled, "First action should be enabled");

                // Check slide content
                if let Some(action::ActionTypeData::Slide(slide_type)) =
                    &first_action.action_type_data
                {
                    if let Some(action::slide_type::Slide::Presentation(pres_slide)) =
                        &slide_type.slide
                    {
                        if let Some(base_slide) = &pres_slide.base_slide {
                            // Verify slide elements
                            assert_eq!(
                                base_slide.elements.len(),
                                2,
                                "Base slide should have 2 elements"
                            );

                            // Check first element (text)
                            if let Some(first_element) = base_slide.elements.first() {
                                if let Some(graphics_element) = &first_element.element {
                                    if let Some(text_element) = &graphics_element.text {
                                        // Text element assertions
                                        assert!(
                                            text_element.attributes.is_some(),
                                            "Text element should have attributes"
                                        );
                                        if let Some(attrs) = &text_element.attributes {
                                            assert!(
                                                attrs.font.is_some(),
                                                "Text attributes should have font info"
                                            );
                                            if let Some(font) = &attrs.font {
                                                assert_eq!(font.name, "HelveticaNeue");
                                                assert!(
                                                    (font.size - 59.2).abs() < f64::EPSILON,
                                                    "Font size should be 59.2"
                                                );
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

        // Verify cue groups
        assert!(
            !presentation.cue_groups.is_empty(),
            "Should have at least one cue group"
        );

        // Verify arrangements (this presentation doesn't have any)
        assert!(
            presentation.arrangements.is_empty(),
            "Should not have any arrangements"
        );

        println!("Successfully read and verified simple presentation!");
    }

    #[test]
    fn test_read_bible_verse() {
        let path = get_example_path("Titus 2v11-13 (NRSVue).pro");
        let result = read_presentation_file(&path);

        match result {
            Ok(presentation) => {
                // Save the raw presentation struct to a file for analysis
                if let Ok(mut file) = File::create("test_output_bible_verse.txt") {
                    writeln!(file, "{presentation:#?}")
                        .expect("Failed to write presentation debug output");
                }

                // Basic presentation properties
                assert!(
                    presentation.name.contains("Titus"),
                    "Expected presentation name to contain 'Titus'"
                );
                assert!(presentation.uuid.is_some(), "Presentation should have UUID");

                // Verify slides exist
                assert!(
                    !presentation.cues.is_empty(),
                    "Expected presentation to have slides"
                );

                // Check first cue structure
                if let Some(first_cue) = presentation.cues.first() {
                    assert!(first_cue.uuid.is_some(), "First cue should have UUID");
                    assert!(
                        !first_cue.actions.is_empty(),
                        "First cue should have actions"
                    );

                    // Verify Bible verse content
                    if let Some(first_action) = first_cue.actions.first() {
                        assert!(first_action.is_enabled, "First action should be enabled");
                        if let Some(action::ActionTypeData::Slide(slide_type)) =
                            &first_action.action_type_data
                        {
                            if let Some(action::slide_type::Slide::Presentation(pres_slide)) =
                                &slide_type.slide
                            {
                                if let Some(base_slide) = &pres_slide.base_slide {
                                    // Verify text formatting for Bible verse
                                    if let Some(first_element) = base_slide.elements.first() {
                                        if let Some(graphics_element) = &first_element.element {
                                            if let Some(text_element) = &graphics_element.text {
                                                assert!(
                                                    text_element.attributes.is_some(),
                                                    "Text should have attributes"
                                                );
                                                if let Some(attrs) = &text_element.attributes {
                                                    // Bible verses typically have specific formatting
                                                    assert!(
                                                        attrs.font.is_some(),
                                                        "Should have font settings"
                                                    );
                                                    assert!(
                                                        attrs.paragraph_style.is_some(),
                                                        "Should have paragraph style"
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Bible presentations should have cue groups
                assert!(
                    !presentation.cue_groups.is_empty(),
                    "Expected presentation to have cue groups"
                );
                if let Some(first_group) = presentation.cue_groups.first() {
                    assert!(first_group.group.is_some(), "Should have group info");
                    if let Some(group) = &first_group.group {
                        assert!(group.uuid.is_some(), "Group should have UUID");
                    }
                }

                println!("Successfully verified Bible verse presentation structure");
            }
            Err(e) => panic!("Failed to read presentation: {e}"),
        }
    }
}
