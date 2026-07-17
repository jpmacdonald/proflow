//! `ProPresenter` file serialization.
//!
//! Writes protobuf-encoded presentation files to disk.

#![allow(dead_code)]

use std::ffi::{OsStr, OsString};
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::propresenter::deserialize::has_native_document_identity;
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

    /// A native presentation must have a stable document name and UUID.
    #[error("Presentation requires a non-empty name and UUID")]
    MissingDocumentIdentity,
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
/// let presentation = rv_data::Presentation {
///     uuid: Some(rv_data::Uuid { string: "example-id".to_string() }),
///     name: "Example".to_string(),
///     ..Default::default()
/// };
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
    let buf = encode_presentation(presentation)?;

    write_file_atomically(path, |mut file| {
        file.write_all(&buf)?;
        Ok(file)
    })
}

/// Write a complete sibling temporary file and atomically replace `path` only
/// after the temporary file has been flushed successfully.
pub(super) fn write_file_atomically<E, F>(path: &Path, write: F) -> Result<(), E>
where
    E: From<io::Error>,
    F: FnOnce(File) -> Result<File, E>,
{
    let temporary_path = temporary_output_path(path);
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary_path)?;

    let result = write(file).and_then(|file| {
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary_path, path)?;
        Ok(())
    });

    if result.is_err() {
        let _cleanup_result = std::fs::remove_file(&temporary_path);
    }

    result
}

fn temporary_output_path(path: &Path) -> PathBuf {
    let mut temporary_name = OsString::from(".");
    temporary_name.push(
        path.file_name()
            .unwrap_or_else(|| OsStr::new("propresenter")),
    );
    temporary_name.push(format!(".{}.tmp", uuid::Uuid::new_v4()));
    path.with_file_name(temporary_name)
}

/// Encode a presentation to protobuf bytes (for embedding in playlists).
pub fn encode_presentation(
    presentation: &rv_data::Presentation,
) -> Result<Vec<u8>, SerializeError> {
    if !has_native_document_identity(presentation) {
        return Err(SerializeError::MissingDocumentIdentity);
    }
    Ok(presentation.encode_to_vec())
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        clippy::float_cmp
    )]

    use super::*;
    use crate::propresenter::deserialize::read_presentation_file;
    use std::fs;
    use std::path::PathBuf;

    fn get_example_path(filename: &str) -> PathBuf {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("tests/fixtures/propresenter/native/examples");
        path.push(filename);
        path
    }

    fn assert_fixture_round_trip(filename: &str) {
        let original = read_presentation_file(get_example_path(filename))
            .expect("native fixture should decode");
        let directory = tempfile::tempdir().expect("create temporary output directory");
        let output_path = directory.path().join(filename);

        write_presentation_file(&original, &output_path).expect("native fixture should serialize");

        assert_eq!(
            fs::read(&output_path).expect("read serialized fixture"),
            original.encode_to_vec(),
            "serialized bytes changed for {filename}"
        );
        assert_eq!(
            read_presentation_file(&output_path).expect("serialized fixture should decode"),
            original,
            "decoded presentation changed for {filename}"
        );
    }

    #[test]
    fn native_fixtures_round_trip_without_repository_outputs() {
        for filename in [
            "title-nametag.pro",
            "hymn-amazing-grace.pro",
            "scripture-titus-2v11-13-nrsvue.pro",
        ] {
            assert_fixture_round_trip(filename);
        }
    }

    #[test]
    fn test_write_empty_presentation() {
        // An otherwise-empty native document still requires an identity.
        let empty = rv_data::Presentation {
            uuid: Some(rv_data::Uuid {
                string: "empty-presentation-id".to_string(),
            }),
            name: "Empty Presentation".to_string(),
            ..Default::default()
        };

        let directory = tempfile::tempdir().expect("create temporary output directory");
        let output_path = directory.path().join("empty_presentation.pro");
        write_presentation_file(&empty, &output_path).expect("Failed to write empty presentation");

        let round_trip =
            read_presentation_file(&output_path).expect("Failed to read empty presentation");

        // Verify properties match
        assert_eq!(round_trip.name, "Empty Presentation");
        assert!(round_trip.cues.is_empty());
        assert!(round_trip.cue_groups.is_empty());
        assert!(round_trip.arrangements.is_empty());
    }

    #[test]
    fn unidentified_presentation_is_rejected_before_writing() {
        let directory = tempfile::tempdir().expect("tempdir");
        let output_path = directory.path().join("invalid.pro");

        let error = write_presentation_file(&rv_data::Presentation::default(), &output_path)
            .expect_err("an unidentified document must not be written");

        assert!(matches!(error, SerializeError::MissingDocumentIdentity));
        assert!(!output_path.exists());
    }

    #[test]
    fn test_verify_group_structure() {
        // Read the native nametag fixture as our reference.
        let example_path = get_example_path("title-nametag.pro");
        let example =
            read_presentation_file(&example_path).expect("Failed to read example presentation");

        // Verify group structure
        assert!(
            !example.cue_groups.is_empty(),
            "Presentation should have at least one cue group"
        );

        if let Some(first_group) = example.cue_groups.first() {
            // Verify group has required fields
            assert!(first_group.group.is_some(), "Cue group should have a group");
            if let Some(group) = &first_group.group {
                assert!(group.uuid.is_some(), "Group should have a UUID");
                assert!(group.hot_key.is_some(), "Group should have a hot key");
                // Even if empty, these fields should exist
                assert_eq!(group.name, "", "Group name should be empty string");
                assert_eq!(
                    group.application_group_name, "",
                    "Application group name should be empty string"
                );
            }

            // Verify cue identifiers match actual cues
            let cue_uuids: Vec<String> = example
                .cues
                .iter()
                .filter_map(|cue| cue.uuid.as_ref())
                .map(|uuid| uuid.string.clone())
                .collect();

            let group_cue_uuids: Vec<String> = first_group
                .cue_identifiers
                .iter()
                .map(|uuid| uuid.string.clone())
                .collect();

            assert!(
                !group_cue_uuids.is_empty(),
                "Group should have cue identifiers"
            );

            // Verify all cues in the group exist in the presentation
            for uuid in &group_cue_uuids {
                assert!(
                    cue_uuids.contains(uuid),
                    "Group references cue that doesn't exist in presentation"
                );
            }
        }
    }

    #[test]
    fn atomic_write_preserves_existing_file_when_generation_fails() {
        let directory = tempfile::tempdir().expect("tempdir");
        let output_path = directory.path().join("existing.pro");
        fs::write(&output_path, b"known-good").expect("write existing output");

        let result = write_file_atomically::<SerializeError, _>(&output_path, |_file| {
            Err(io::Error::other("injected write failure").into())
        });

        assert!(result.is_err());
        assert_eq!(
            fs::read(&output_path).expect("read preserved output"),
            b"known-good"
        );
    }
}
