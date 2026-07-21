use std::collections::BTreeMap;

use prost::Message;

use crate::propresenter::deserialize::ProPresenterError;
use crate::propresenter::generated::rv_data;
use crate::propresenter::inspection::PresentationStructureSummary;

/// Errors that can occur while reading a `ProPresenter` playlist package.
#[derive(Debug, thiserror::Error)]
pub enum PackageError {
    /// An I/O error occurred while opening or reading the archive.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// The file is not a readable zip package.
    #[error("Zip error: {0}")]
    Zip(#[from] zip::result::ZipError),

    /// The protobuf `data` entry could not be decoded.
    #[error("Decode error: {0}")]
    Decode(#[from] prost::DecodeError),

    /// The package did not contain the required protobuf `data` entry.
    #[error("Missing data entry")]
    MissingData,

    /// The archive contained the same member name more than once.
    #[error("Duplicate archive entry: {0}")]
    DuplicateArchiveEntry(String),

    /// A `.pro` archive member was not an identified native presentation.
    #[error("Embedded presentation {name:?} is invalid: {reason}")]
    InvalidEmbeddedPresentation {
        /// Archive member name.
        name: String,
        /// Native presentation format or decoding failure.
        #[source]
        reason: ProPresenterError,
    },
}

/// Decoded contents of a `.proplaylist` package.
#[derive(Debug)]
pub struct PlaylistPackage {
    document: rv_data::PlaylistDocument,
    document_data: Vec<u8>,
    embedded_file_data: BTreeMap<String, Vec<u8>>,
    archive_entries: Vec<PackageFileSummary>,
    archive_comment: Vec<u8>,
}

impl PlaylistPackage {
    pub(super) const fn new(
        document: rv_data::PlaylistDocument,
        document_data: Vec<u8>,
        embedded_file_data: BTreeMap<String, Vec<u8>>,
        archive_entries: Vec<PackageFileSummary>,
        archive_comment: Vec<u8>,
    ) -> Self {
        Self {
            document,
            document_data,
            embedded_file_data,
            archive_entries,
            archive_comment,
        }
    }

    /// Decoded protobuf playlist document from the `data` archive entry.
    #[must_use]
    pub const fn document(&self) -> &rv_data::PlaylistDocument {
        &self.document
    }

    /// Consume the inspection result and return its decoded document.
    #[must_use]
    pub fn into_document(self) -> rv_data::PlaylistDocument {
        self.document
    }

    /// Raw protobuf bytes from the `data` archive entry.
    #[must_use]
    pub fn document_data(&self) -> &[u8] {
        &self.document_data
    }

    /// Whether decoding and re-encoding reproduces the exact `data` bytes.
    #[must_use]
    pub fn document_round_trip_is_exact(&self) -> bool {
        self.document.encode_to_vec() == self.document_data
    }

    /// Names of non-`data` archive entries in physical archive order.
    pub fn embedded_files(&self) -> impl Iterator<Item = &str> {
        self.embedded_file_details().map(|file| file.name.as_str())
    }

    /// Metadata for non-`data` archive entries in physical archive order.
    pub fn embedded_file_details(&self) -> impl Iterator<Item = &PackageFileSummary> {
        self.archive_entries
            .iter()
            .filter(|entry| entry.name != "data")
    }

    /// Number of non-`data` archive entries.
    #[must_use]
    pub fn embedded_file_count(&self) -> usize {
        self.embedded_file_data.len()
    }

    /// Raw bytes for one exact non-`data` archive path.
    #[must_use]
    pub fn embedded_file(&self, name: &str) -> Option<&[u8]> {
        self.embedded_file_data.get(name).map(Vec::as_slice)
    }

    /// Whether the package contains one exact non-`data` archive path.
    #[must_use]
    pub fn has_embedded_file(&self, name: &str) -> bool {
        self.embedded_file_data.contains_key(name)
    }

    /// Every archive entry in physical order, including the `data` member.
    #[must_use]
    pub fn archive_entries(&self) -> &[PackageFileSummary] {
        &self.archive_entries
    }

    /// Raw ZIP archive comment.
    #[must_use]
    pub fn archive_comment(&self) -> &[u8] {
        &self.archive_comment
    }
}

/// Metadata for a non-`data` file inside a `.proplaylist` package.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PackageFileSummary {
    /// Archive entry name.
    pub name: String,
    /// Basename of the archive entry.
    pub basename: String,
    /// Uncompressed byte size.
    pub size: u64,
    /// Zip CRC32 of the uncompressed content.
    pub crc32: u32,
    /// Whether the archive entry looks like an embedded presentation.
    pub is_presentation: bool,
    /// ZIP compression method.
    pub compression_method: String,
    /// Whether the entry is a directory marker.
    pub is_directory: bool,
    /// ZIP creator version tuple.
    pub version_made_by: (u8, u8),
    /// Unix mode recorded in the central directory, when present.
    pub unix_mode: Option<u32>,
    /// Nonvolatile extra-field identifiers. Extended timestamps are omitted.
    pub extra_field_ids: Vec<u16>,
    /// Per-entry ZIP comment.
    pub comment: String,
}

/// The inferred shape of an already-written package.
///
/// This is inspection data, not a write policy. In particular, a portable
/// import containing only basename `.pro` members may still have the same
/// observable shape as a library-local archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaylistArchiveShape {
    /// Archive has no media or path-qualified members beyond presentations.
    ///
    /// This does not prove that presentation items are links: native exports
    /// with embedded basename `.pro` members have the same inspected shape.
    PresentationsOnly,
    /// Archive contains media or path-qualified members associated with export.
    ContainsMedia,
}

/// A compact view of a presentation item inside a playlist document.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PlaylistItemSummary {
    /// Playlist item UUID. Volatile across writes, but useful for inspection.
    pub item_uuid: Option<String>,
    /// Playlist item display name.
    pub name: String,
    /// Tag UUIDs attached to this item, in serialized order.
    pub item_tags: Vec<String>,
    /// Whether the item is hidden in the playlist.
    pub is_hidden: bool,
    /// Document URL platform enum value.
    pub document_platform: Option<i32>,
    /// URL absolute string, typically a `file:///` URL.
    pub absolute_string: Option<String>,
    /// URL relative path string when the storage oneof uses `relative_path`.
    pub storage_relative_path: Option<String>,
    /// Local relative file path used by `ProPresenter` to resolve the item.
    pub local_relative_path: Option<String>,
    /// The local relative root enum value.
    pub local_root: Option<i32>,
    /// External relative file path, when the package points at an external volume.
    pub external_relative_path: Option<String>,
    /// Arrangement UUID referenced by the playlist item.
    pub arrangement_uuid: Option<String>,
    /// Content destination enum value.
    pub content_destination: i32,
    /// User-selected music key and scale enum values.
    pub user_music_key: Option<(i32, i32)>,
    /// Serialized arrangement display name.
    pub arrangement_name: String,
}

/// Stable aligned item difference that avoids cascaded positional noise.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PlaylistItemAlignedDiff {
    /// Stable issue kind.
    pub kind: String,
    /// Expected/reference playlist index, when present.
    pub expected_index: Option<usize>,
    /// Actual/candidate playlist index, when present.
    pub actual_index: Option<usize>,
    /// Stable item key used for alignment, usually the library-relative path.
    pub key: String,
    /// Expected/reference item name, when present.
    pub expected_name: Option<String>,
    /// Actual/candidate item name, when present.
    pub actual_name: Option<String>,
    /// Human-readable details.
    pub message: String,
}

/// Normalized comparison between two playlist packages.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PlaylistPackageComparison {
    /// Expected/reference package path.
    pub expected_path: String,
    /// Actual/candidate package path.
    pub actual_path: String,
    /// Inferred archive shape for the expected package.
    pub expected_shape: PlaylistArchiveShape,
    /// Inferred archive shape for the actual package.
    pub actual_shape: PlaylistArchiveShape,
    /// Whether no comparison issues were found.
    pub compatible: bool,
    /// Human-readable comparison issues.
    pub issues: Vec<PlaylistPackageIssue>,
    /// Expected package presentation item count.
    pub expected_item_count: usize,
    /// Actual package presentation item count.
    pub actual_item_count: usize,
    /// Expected non-`data` archive entry count.
    pub expected_embedded_file_count: usize,
    /// Actual non-`data` archive entry count.
    pub actual_embedded_file_count: usize,
}

/// A single normalized playlist package comparison issue.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PlaylistPackageIssue {
    /// Stable issue kind.
    pub kind: String,
    /// Optional item index for item-level differences.
    pub index: Option<usize>,
    /// Human-readable details.
    pub message: String,
}

/// Semantic summary of an embedded `.pro` presentation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct EmbeddedPresentationSummary {
    /// Archive entry path.
    pub archive_path: String,
    /// Archive entry basename.
    pub basename: String,
    /// Presentation UUID inside the embedded `.pro` file.
    pub presentation_uuid: Option<String>,
    /// Presentation name inside the embedded `.pro` file.
    pub presentation_name: String,
    /// Number of cues/slides in the presentation.
    pub cue_count: usize,
    /// Number of cue groups in the presentation.
    pub cue_group_count: usize,
    /// Arrangement names defined in the presentation.
    pub arrangement_names: Vec<String>,
}

/// Semantic structure for an embedded `.pro` presentation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct EmbeddedPresentationStructure {
    /// Archive entry path.
    pub archive_path: String,
    /// Archive entry basename.
    pub basename: String,
    /// Presentation structure summary.
    pub structure: PresentationStructureSummary,
}
