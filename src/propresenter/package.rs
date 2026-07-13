//! Inspection helpers for `ProPresenter` playlist packages.
//!
//! A `.proplaylist` file is a zip archive containing a protobuf `data` entry
//! and, for exported playlists, embedded `.pro` presentation files. These
//! helpers decode that package shape so generated files can be compared against
//! files written back by `ProPresenter`.

use prost::Message;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

use crate::propresenter::generated::rv_data::{self, action, playlist, playlist_item, url};
use crate::propresenter::macros::macro_action_name;
use crate::propresenter::rtf::{extract_rtf_options, rtf_to_text};

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

    /// A `.pro` archive member was not a decodable presentation.
    #[error("Embedded presentation {name:?} is invalid: {reason}")]
    InvalidEmbeddedPresentation {
        /// Archive member name.
        name: String,
        /// Protobuf decoding failure.
        #[source]
        reason: prost::DecodeError,
    },
}

/// Decoded contents of a `.proplaylist` package.
#[derive(Debug, Clone)]
pub struct PlaylistPackage {
    /// The decoded protobuf playlist document from the `data` archive entry.
    pub document: rv_data::PlaylistDocument,
    /// Raw protobuf bytes from the `data` archive entry.
    pub document_data: Vec<u8>,
    /// Whether decode then encode reproduced `document_data` byte-for-byte.
    pub document_round_trip_exact: bool,
    /// Non-`data` archive entries, usually embedded `.pro` files and media.
    pub embedded_files: Vec<String>,
    /// Detailed metadata for non-`data` archive entries.
    pub embedded_file_details: Vec<PackageFileSummary>,
    /// Raw bytes for non-`data` archive entries keyed by archive path.
    pub embedded_file_data: BTreeMap<String, Vec<u8>>,
    /// Every archive entry in physical order, including the `data` member.
    pub archive_entries: Vec<PackageFileSummary>,
    /// Raw ZIP archive comment.
    pub archive_comment: Vec<u8>,
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

/// The inferred package shape.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    rmcp::schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum PlaylistPackageMode {
    /// Local playlist package with embedded presentations only.
    #[default]
    LibraryLocal,
    /// Exported package with media assets and/or full absolute archive paths.
    ExportPortable,
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

struct AlignedPlaylistItem<'a> {
    key: String,
    expected_index: usize,
    actual_index: usize,
    expected: &'a PlaylistItemSummary,
    actual: &'a PlaylistItemSummary,
}

/// Normalized comparison between two playlist packages.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PlaylistPackageComparison {
    /// Expected/reference package path.
    pub expected_path: String,
    /// Actual/candidate package path.
    pub actual_path: String,
    /// Inferred package mode for the expected package.
    pub expected_mode: PlaylistPackageMode,
    /// Inferred package mode for the actual package.
    pub actual_mode: PlaylistPackageMode,
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

/// Operator-facing presentation structure summary.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PresentationStructureSummary {
    /// Presentation UUID. Useful for duplicate-file detection; normally volatile
    /// for freshly generated documents.
    pub uuid: Option<String>,
    /// Presentation name.
    pub name: String,
    /// Scripture metadata attached to the presentation, when present.
    pub bible_reference: Option<BibleReferenceSummary>,
    /// Cue summaries in raw protobuf order.
    pub cues: Vec<CueStructureSummary>,
    /// Cue group summaries in protobuf order.
    pub cue_groups: Vec<CueGroupStructureSummary>,
    /// Arrangement summaries in protobuf order.
    pub arrangements: Vec<ArrangementStructureSummary>,
    /// Cue indexes in operator traversal order.
    pub operator_cue_indexes: Vec<usize>,
}

/// Cue-level semantic summary.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CueStructureSummary {
    /// Raw cue index.
    pub index: usize,
    /// Cue UUID.
    pub uuid: Option<String>,
    /// Cue name.
    pub name: String,
    /// Cue group names containing this cue.
    pub group_names: Vec<String>,
    /// Extracted slide text, preserving internal blank lines where possible.
    pub text: String,
    /// Extracted slide text split into lines.
    pub text_lines: Vec<String>,
    /// Whether the cue has no alphanumeric text.
    pub is_blank: bool,
    /// Macro action names on this cue, in action order.
    pub macros: Vec<String>,
    /// Labels attached to slide actions on this cue, in action order.
    pub slide_labels: Vec<ActionLabelSignature>,
    /// Background media basenames on this cue, in action order.
    pub background_media: Vec<String>,
    /// Action kind signature, in action order.
    pub action_kinds: Vec<String>,
    /// Slide text/layout style signatures, used as a proxy for theme/template parity.
    pub text_styles: Vec<TextStyleSignature>,
}

/// Label attached to a slide action.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ActionLabelSignature {
    /// Operator-visible label text.
    pub text: String,
    /// Label color normalized to an RGBA hex string.
    pub color: Option<String>,
}

/// Presentation-level scripture metadata.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct BibleReferenceSummary {
    /// Native book index.
    pub book_index: u32,
    /// Operator-visible book name.
    pub book_name: String,
    /// Inclusive chapter range.
    pub chapter_range: Option<IntRangeSummary>,
    /// Inclusive verse range.
    pub verse_range: Option<IntRangeSummary>,
    /// Full translation name.
    pub translation_name: String,
    /// Translation abbreviation shown to the operator.
    pub translation_display_abbreviation: String,
    /// Translation abbreviation used internally by `ProPresenter`.
    pub translation_internal_abbreviation: String,
    /// Native book lookup key.
    pub book_key: String,
}

/// Inclusive integer range from native presentation metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct IntRangeSummary {
    /// First value in the range.
    pub start: i32,
    /// Last value in the range.
    pub end: i32,
}

/// Text element style/layout summary.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TextStyleSignature {
    /// Text element name from the slide, when present.
    pub element_name: String,
    /// Element bounds in slide coordinates.
    pub bounds: Option<String>,
    /// Slide canvas size.
    pub slide_size: Option<String>,
    /// Font family/name resolved from attributes or RTF.
    pub font_name: Option<String>,
    /// Font size in points.
    pub font_size: Option<u32>,
    /// Text color in hex RGB/RGBA form.
    pub color: Option<String>,
    /// Bold style flag.
    pub bold: Option<bool>,
    /// Italic style flag.
    pub italic: Option<bool>,
    /// Vertical alignment enum name.
    pub vertical_alignment: String,
    /// Text scale behavior enum name.
    pub scale_behavior: String,
    /// Text transform enum name.
    pub transform: String,
    /// Text margins.
    pub margins: Option<String>,
}

/// Cue-group semantic summary.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CueGroupStructureSummary {
    /// Raw cue group index.
    pub index: usize,
    /// Cue group UUID.
    pub uuid: Option<String>,
    /// Cue group name.
    pub name: String,
    /// Group color normalized to an RGBA hex string.
    pub color: Option<String>,
    /// Keyboard shortcut bound to the group.
    pub hot_key: Option<HotKeySignature>,
    /// Identifier of the corresponding application-defined group.
    pub application_group_identifier: Option<String>,
    /// Name of the corresponding application-defined group.
    pub application_group_name: String,
    /// Cue indexes in this group.
    pub cue_indexes: Vec<usize>,
}

/// Keyboard shortcut attached to a cue group.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct HotKeySignature {
    /// Native key-code value.
    pub code: i32,
    /// Native control identifier.
    pub control_identifier: String,
}

/// Arrangement semantic summary.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ArrangementStructureSummary {
    /// Raw arrangement index.
    pub index: usize,
    /// Arrangement UUID.
    pub uuid: Option<String>,
    /// Arrangement name.
    pub name: String,
    /// Group names in arrangement order.
    pub group_names: Vec<String>,
    /// Cue indexes in arrangement traversal order.
    pub cue_indexes: Vec<usize>,
}

/// Read and decode a `.proplaylist` package.
pub fn read_playlist_package(path: impl AsRef<Path>) -> Result<PlaylistPackage, PackageError> {
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(file)?;
    let archive_comment = archive.comment().to_vec();
    let mut archive_entries = Vec::with_capacity(archive.len());
    let mut embedded_file_details = Vec::new();
    let mut embedded_files = Vec::new();
    let mut embedded_file_data = BTreeMap::new();
    let mut seen_names = BTreeSet::new();
    let mut data = None;

    for index in 0..archive.len() {
        let mut file = archive.by_index(index)?;
        let name = native_archive_member_name(&file);
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        if !seen_names.insert(name.clone()) {
            return Err(PackageError::DuplicateArchiveEntry(name));
        }
        let summary = package_file_summary(&file, &name);
        archive_entries.push(summary.clone());

        if name == "data" {
            data = Some(bytes);
        } else {
            if summary.is_presentation {
                rv_data::Presentation::decode(bytes.as_slice()).map_err(|reason| {
                    PackageError::InvalidEmbeddedPresentation {
                        name: name.clone(),
                        reason,
                    }
                })?;
            }
            embedded_file_details.push(summary);
            embedded_files.push(name.clone());
            embedded_file_data.insert(name, bytes);
        }
    }

    let document_data = data.ok_or(PackageError::MissingData)?;
    let document = rv_data::PlaylistDocument::decode(document_data.as_slice())?;
    let document_round_trip_exact = document.encode_to_vec() == document_data;
    Ok(PlaylistPackage {
        document,
        document_data,
        document_round_trip_exact,
        embedded_files,
        embedded_file_details,
        embedded_file_data,
        archive_entries,
        archive_comment,
    })
}

fn native_archive_member_name(file: &zip::read::ZipFile<'_>) -> String {
    std::str::from_utf8(file.name_raw()).map_or_else(
        |_| file.name().to_string(),
        std::string::ToString::to_string,
    )
}

/// Return a compact summary of all presentation items in a playlist document.
pub fn presentation_items(document: &rv_data::PlaylistDocument) -> Vec<PlaylistItemSummary> {
    let mut items = Vec::new();
    if let Some(root) = &document.root_node {
        collect_playlist_items(root, &mut items);
    }
    items
}

/// Compare playlist items by stable presentation identity rather than raw
/// position, reducing cascaded noise when one manual item is absent.
#[must_use]
pub fn compare_playlist_items_aligned(
    expected: &[PlaylistItemSummary],
    actual: &[PlaylistItemSummary],
) -> Vec<PlaylistItemAlignedDiff> {
    let mut actual_by_key: BTreeMap<String, VecDeque<(usize, &PlaylistItemSummary)>> =
        BTreeMap::new();
    for (index, item) in actual.iter().enumerate() {
        actual_by_key
            .entry(playlist_item_alignment_key(item))
            .or_default()
            .push_back((index, item));
    }

    let mut diffs = Vec::new();
    let mut matched = Vec::new();
    for (expected_index, expected_item) in expected.iter().enumerate() {
        let key = playlist_item_alignment_key(expected_item);
        let Some((actual_index, actual_item)) =
            actual_by_key.get_mut(&key).and_then(VecDeque::pop_front)
        else {
            diffs.push(PlaylistItemAlignedDiff {
                kind: "missing_item_aligned".to_string(),
                expected_index: Some(expected_index),
                actual_index: None,
                key,
                expected_name: Some(expected_item.name.clone()),
                actual_name: None,
                message: format!("missing item '{}'", expected_item.name),
            });
            continue;
        };

        matched.push(AlignedPlaylistItem {
            key,
            expected_index,
            actual_index,
            expected: expected_item,
            actual: actual_item,
        });
    }

    for (key, mut items) in actual_by_key {
        while let Some((actual_index, actual_item)) = items.pop_front() {
            diffs.push(PlaylistItemAlignedDiff {
                kind: "extra_item_aligned".to_string(),
                expected_index: None,
                actual_index: Some(actual_index),
                key: key.clone(),
                expected_name: None,
                actual_name: Some(actual_item.name.clone()),
                message: format!("extra item '{}'", actual_item.name),
            });
        }
    }

    let order_changed = matched
        .windows(2)
        .any(|pair| pair[0].actual_index > pair[1].actual_index);
    for aligned in matched {
        compare_aligned_item(aligned, order_changed, &mut diffs);
    }

    diffs.sort_by(|left, right| {
        left.expected_index
            .cmp(&right.expected_index)
            .then(left.actual_index.cmp(&right.actual_index))
            .then(left.kind.cmp(&right.kind))
            .then(left.key.cmp(&right.key))
    });
    diffs
}

fn compare_aligned_item(
    aligned: AlignedPlaylistItem<'_>,
    order_changed: bool,
    diffs: &mut Vec<PlaylistItemAlignedDiff>,
) {
    let AlignedPlaylistItem {
        key,
        expected_index,
        actual_index,
        expected,
        actual,
    } = aligned;
    if order_changed && expected_index != actual_index {
        diffs.push(PlaylistItemAlignedDiff {
            kind: "moved_item_aligned".to_string(),
            expected_index: Some(expected_index),
            actual_index: Some(actual_index),
            key: key.clone(),
            expected_name: Some(expected.name.clone()),
            actual_name: Some(actual.name.clone()),
            message: format!(
                "item '{}' moved from index {expected_index} to {actual_index}",
                expected.name
            ),
        });
    }

    if expected.name != actual.name {
        diffs.push(PlaylistItemAlignedDiff {
            kind: "item_name_mismatch_aligned".to_string(),
            expected_index: Some(expected_index),
            actual_index: Some(actual_index),
            key: key.clone(),
            expected_name: Some(expected.name.clone()),
            actual_name: Some(actual.name.clone()),
            message: format!(
                "expected item name '{}', found '{}'",
                expected.name, actual.name
            ),
        });
    }

    let expected_arrangement = expected.arrangement_uuid.as_deref().map(str::to_lowercase);
    let actual_arrangement = actual.arrangement_uuid.as_deref().map(str::to_lowercase);
    if expected_arrangement != actual_arrangement {
        diffs.push(PlaylistItemAlignedDiff {
            kind: "item_arrangement_mismatch_aligned".to_string(),
            expected_index: Some(expected_index),
            actual_index: Some(actual_index),
            key,
            expected_name: Some(expected.name.clone()),
            actual_name: Some(actual.name.clone()),
            message: format!(
                "expected arrangement {:?}, found {:?}",
                expected.arrangement_uuid, actual.arrangement_uuid
            ),
        });
    }
}

/// Return semantic summaries for embedded presentation files that decode cleanly.
#[must_use]
pub fn embedded_presentation_summaries(
    package: &PlaylistPackage,
) -> Vec<EmbeddedPresentationSummary> {
    let mut summaries = Vec::new();
    for file in package
        .embedded_file_details
        .iter()
        .filter(|file| file.is_presentation)
    {
        let Some(data) = package.embedded_file_data.get(&file.name) else {
            continue;
        };
        let Ok(presentation) = rv_data::Presentation::decode(data.as_slice()) else {
            continue;
        };
        summaries.push(EmbeddedPresentationSummary {
            archive_path: file.name.clone(),
            basename: file.basename.clone(),
            presentation_uuid: presentation.uuid.as_ref().map(|uuid| uuid.string.clone()),
            presentation_name: presentation.name,
            cue_count: presentation.cues.len(),
            cue_group_count: presentation.cue_groups.len(),
            arrangement_names: presentation
                .arrangements
                .iter()
                .map(|arrangement| arrangement.name.clone())
                .collect(),
        });
    }
    summaries
}

/// Return semantic structures for embedded presentation files that decode cleanly.
#[must_use]
pub fn embedded_presentation_structures(
    package: &PlaylistPackage,
) -> Vec<EmbeddedPresentationStructure> {
    let mut structures = Vec::new();
    for file in package
        .embedded_file_details
        .iter()
        .filter(|file| file.is_presentation)
    {
        let Some(data) = package.embedded_file_data.get(&file.name) else {
            continue;
        };
        let Ok(presentation) = rv_data::Presentation::decode(data.as_slice()) else {
            continue;
        };
        structures.push(EmbeddedPresentationStructure {
            archive_path: file.name.clone(),
            basename: file.basename.clone(),
            structure: summarize_presentation_structure(&presentation),
        });
    }
    structures
}

/// Return a semantic summary for a presentation.
#[must_use]
#[allow(
    clippy::too_many_lines,
    reason = "keeping every presentation field translation together makes the semantic boundary auditable"
)]
pub fn summarize_presentation_structure(
    presentation: &rv_data::Presentation,
) -> PresentationStructureSummary {
    let cue_indexes_by_uuid = cue_indexes_by_uuid(presentation);
    let cue_group_indexes_by_uuid = cue_group_indexes_by_uuid(presentation);
    let cue_group_names_by_cue_uuid = cue_group_names_by_cue_uuid(presentation);

    let cues = presentation
        .cues
        .iter()
        .enumerate()
        .map(|(index, cue)| {
            summarize_cue(
                index,
                cue,
                cue.uuid
                    .as_ref()
                    .and_then(|uuid| cue_group_names_by_cue_uuid.get(uuid.string.as_str()))
                    .cloned()
                    .unwrap_or_default(),
            )
        })
        .collect::<Vec<_>>();

    let cue_groups = presentation
        .cue_groups
        .iter()
        .enumerate()
        .map(|(index, cue_group)| {
            let (uuid, name) = cue_group
                .group
                .as_ref()
                .map(|group| {
                    (
                        group.uuid.as_ref().map(|uuid| uuid.string.clone()),
                        group.name.clone(),
                    )
                })
                .unwrap_or_default();
            let cue_indexes = cue_group
                .cue_identifiers
                .iter()
                .filter_map(|cue_id| cue_indexes_by_uuid.get(cue_id.string.as_str()).copied())
                .collect();
            CueGroupStructureSummary {
                index,
                uuid,
                name,
                color: cue_group
                    .group
                    .as_ref()
                    .and_then(|group| group.color.as_ref())
                    .map(color_signature),
                hot_key: cue_group
                    .group
                    .as_ref()
                    .and_then(|group| group.hot_key.as_ref())
                    .map(|hot_key| HotKeySignature {
                        code: hot_key.code,
                        control_identifier: hot_key.control_identifier.clone(),
                    }),
                application_group_identifier: cue_group
                    .group
                    .as_ref()
                    .and_then(|group| group.application_group_identifier.as_ref())
                    .map(|uuid| uuid.string.clone()),
                application_group_name: cue_group
                    .group
                    .as_ref()
                    .map(|group| group.application_group_name.clone())
                    .unwrap_or_default(),
                cue_indexes,
            }
        })
        .collect::<Vec<_>>();

    let arrangements = presentation
        .arrangements
        .iter()
        .enumerate()
        .map(|(index, arrangement)| {
            let mut group_names = Vec::new();
            let mut cue_indexes = Vec::new();
            for group_id in &arrangement.group_identifiers {
                let Some(group_index) = cue_group_indexes_by_uuid.get(group_id.string.as_str())
                else {
                    continue;
                };
                let Some(group) = presentation.cue_groups.get(*group_index) else {
                    continue;
                };
                group_names.push(
                    group
                        .group
                        .as_ref()
                        .map(|group| group.name.clone())
                        .unwrap_or_default(),
                );
                cue_indexes.extend(
                    group
                        .cue_identifiers
                        .iter()
                        .filter_map(|cue_id| cue_indexes_by_uuid.get(cue_id.string.as_str()))
                        .copied(),
                );
            }
            ArrangementStructureSummary {
                index,
                uuid: arrangement.uuid.as_ref().map(|uuid| uuid.string.clone()),
                name: arrangement.name.clone(),
                group_names,
                cue_indexes,
            }
        })
        .collect::<Vec<_>>();

    let operator_cue_indexes = operator_cue_indexes(presentation, &cue_indexes_by_uuid);

    PresentationStructureSummary {
        uuid: presentation.uuid.as_ref().map(|uuid| uuid.string.clone()),
        name: presentation.name.clone(),
        bible_reference: presentation
            .bible_reference
            .as_ref()
            .map(summarize_bible_reference),
        cues,
        cue_groups,
        arrangements,
        operator_cue_indexes,
    }
}

/// Infer the package mode from archive entries.
pub fn infer_package_mode(package: &PlaylistPackage) -> PlaylistPackageMode {
    if package.embedded_file_details.iter().any(|file| {
        !file.is_presentation
            || Path::new(&file.name)
                .parent()
                .is_some_and(|p| p != Path::new(""))
    }) {
        PlaylistPackageMode::ExportPortable
    } else {
        PlaylistPackageMode::LibraryLocal
    }
}

/// Compare two `.proplaylist` packages after normalizing volatile path roots.
pub fn compare_playlist_packages(
    expected_path: impl AsRef<Path>,
    actual_path: impl AsRef<Path>,
) -> Result<PlaylistPackageComparison, PackageError> {
    let expected_path = expected_path.as_ref();
    let actual_path = actual_path.as_ref();
    let expected = read_playlist_package(expected_path)?;
    let actual = read_playlist_package(actual_path)?;
    let expected_items = presentation_items(&expected.document);
    let actual_items = presentation_items(&actual.document);

    let mut issues = Vec::new();
    compare_package_modes(&expected, &actual, &mut issues);
    compare_archive_shape(&expected, &actual, &mut issues);
    compare_playlist_schema_coverage(&expected, &actual, &mut issues);
    compare_playlist_documents(&expected.document, &actual.document, &mut issues);
    compare_items(&expected_items, &actual_items, &mut issues);
    compare_embedded_presentations(&expected, &actual, &mut issues);
    compare_embedded_presentation_structures(&expected, &actual, &mut issues);
    compare_media_assets(&expected, &actual, &mut issues);

    Ok(PlaylistPackageComparison {
        expected_path: expected_path.display().to_string(),
        actual_path: actual_path.display().to_string(),
        expected_mode: infer_package_mode(&expected),
        actual_mode: infer_package_mode(&actual),
        compatible: issues.is_empty(),
        issues,
        expected_item_count: expected_items.len(),
        actual_item_count: actual_items.len(),
        expected_embedded_file_count: expected.embedded_file_details.len(),
        actual_embedded_file_count: actual.embedded_file_details.len(),
    })
}

fn package_file_summary(file: &zip::read::ZipFile<'_>, name: &str) -> PackageFileSummary {
    let basename = Path::new(name)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(name)
        .to_string();
    PackageFileSummary {
        is_presentation: Path::new(&basename)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("pro")),
        basename,
        size: file.size(),
        crc32: file.crc32(),
        name: name.to_string(),
        compression_method: format!("{:?}", file.compression()),
        is_directory: file.is_dir(),
        version_made_by: file.version_made_by(),
        unix_mode: file.unix_mode(),
        extra_field_ids: nonvolatile_extra_field_ids(file.extra_data()),
        comment: file.comment().to_string(),
    }
}

fn nonvolatile_extra_field_ids(mut data: &[u8]) -> Vec<u16> {
    let mut ids = Vec::new();
    while data.len() >= 4 {
        let id = u16::from_le_bytes([data[0], data[1]]);
        let length = usize::from(u16::from_le_bytes([data[2], data[3]]));
        data = &data[4..];
        if data.len() < length {
            break;
        }
        // 0x5455 is the extended timestamp field. Modification time is
        // intentionally volatile and cannot establish package fidelity.
        if id != 0x5455 {
            ids.push(id);
        }
        data = &data[length..];
    }
    ids
}

fn compare_package_modes(
    expected: &PlaylistPackage,
    actual: &PlaylistPackage,
    issues: &mut Vec<PlaylistPackageIssue>,
) {
    let expected_mode = infer_package_mode(expected);
    let actual_mode = infer_package_mode(actual);
    if expected_mode != actual_mode {
        issues.push(PlaylistPackageIssue {
            kind: "package_mode_mismatch".to_string(),
            index: None,
            message: format!("expected {expected_mode:?}, found {actual_mode:?}"),
        });
    }
}

fn compare_archive_shape(
    expected: &PlaylistPackage,
    actual: &PlaylistPackage,
    issues: &mut Vec<PlaylistPackageIssue>,
) {
    if expected.archive_entries.len() != actual.archive_entries.len() {
        issues.push(PlaylistPackageIssue {
            kind: "archive_entry_count_mismatch".to_string(),
            index: None,
            message: format!(
                "expected {} archive entries, found {}",
                expected.archive_entries.len(),
                actual.archive_entries.len()
            ),
        });
    }

    let expected_paths = expected
        .archive_entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect::<Vec<_>>();
    let actual_paths = actual
        .archive_entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect::<Vec<_>>();
    let expected_path_set = expected_paths.iter().copied().collect::<BTreeSet<_>>();
    let actual_path_set = actual_paths.iter().copied().collect::<BTreeSet<_>>();
    for path in expected_path_set.difference(&actual_path_set) {
        issues.push(PlaylistPackageIssue {
            kind: "missing_archive_entry".to_string(),
            index: None,
            message: format!("missing archive entry '{path}'"),
        });
    }
    for path in actual_path_set.difference(&expected_path_set) {
        issues.push(PlaylistPackageIssue {
            kind: "extra_archive_entry".to_string(),
            index: None,
            message: format!("extra archive entry '{path}'"),
        });
    }
    if expected_paths != actual_paths {
        issues.push(PlaylistPackageIssue {
            kind: "archive_entry_order_mismatch".to_string(),
            index: None,
            message: format!("expected archive order {expected_paths:?}, found {actual_paths:?}"),
        });
    }

    for index in 0..expected
        .archive_entries
        .len()
        .min(actual.archive_entries.len())
    {
        let expected_entry = &expected.archive_entries[index];
        let actual_entry = &actual.archive_entries[index];
        if expected_entry.name == actual_entry.name {
            compare_archive_entry_metadata(index, expected_entry, actual_entry, issues);
        }
    }

    if expected.archive_comment != actual.archive_comment {
        issues.push(PlaylistPackageIssue {
            kind: "archive_comment_mismatch".to_string(),
            index: None,
            message: "ZIP archive comments differ".to_string(),
        });
    }
}

/// ZIP timestamps, byte offsets, compressed sizes, and CRCs for the `data`
/// member are volatile or derived from normalized document UUIDs. Everything
/// below is part of the package contract and is compared in physical order.
fn compare_archive_entry_metadata(
    index: usize,
    expected: &PackageFileSummary,
    actual: &PackageFileSummary,
    issues: &mut Vec<PlaylistPackageIssue>,
) {
    let expected_metadata = (
        expected.compression_method.as_str(),
        expected.is_directory,
        expected.version_made_by,
        expected.unix_mode,
        expected.extra_field_ids.as_slice(),
        expected.comment.as_str(),
    );
    let actual_metadata = (
        actual.compression_method.as_str(),
        actual.is_directory,
        actual.version_made_by,
        actual.unix_mode,
        actual.extra_field_ids.as_slice(),
        actual.comment.as_str(),
    );
    if expected_metadata != actual_metadata {
        issues.push(PlaylistPackageIssue {
            kind: "archive_entry_metadata_mismatch".to_string(),
            index: Some(index),
            message: format!(
                "archive entry {:?} metadata differs: expected {expected_metadata:?}, found {actual_metadata:?}",
                expected.name
            ),
        });
    }
}

fn compare_playlist_schema_coverage(
    expected: &PlaylistPackage,
    actual: &PlaylistPackage,
    issues: &mut Vec<PlaylistPackageIssue>,
) {
    if !expected.document_round_trip_exact {
        issues.push(PlaylistPackageIssue {
            kind: "expected_playlist_schema_round_trip_loss".to_string(),
            index: None,
            message: "reference playlist data is not byte-exact after decode and encode; the protobuf schema may be incomplete".to_string(),
        });
    }
    if !actual.document_round_trip_exact {
        issues.push(PlaylistPackageIssue {
            kind: "actual_playlist_schema_round_trip_loss".to_string(),
            index: None,
            message: "candidate playlist data is not byte-exact after decode and encode; the protobuf schema may be incomplete".to_string(),
        });
    }
}

/// Compare the complete decoded playlist document after applying the only
/// permitted semantic normalizations:
///
/// - playlist-container and playlist-item identity UUID values are volatile,
///   but their presence is required;
/// - UUID letter case is not semantic;
/// - machine-specific absolute prefixes before `Libraries/` are ignored.
///
/// Application versions, root and child names/types/expanded state, tags,
/// links, item fields, and item order are not volatile.
fn compare_playlist_documents(
    expected: &rv_data::PlaylistDocument,
    actual: &rv_data::PlaylistDocument,
    issues: &mut Vec<PlaylistPackageIssue>,
) {
    let expected = normalized_playlist_document(expected);
    let actual = normalized_playlist_document(actual);

    compare_document_field(
        "playlist_application_info_mismatch",
        "playlist application/platform metadata differs",
        &expected.application_info,
        &actual.application_info,
        issues,
    );
    compare_document_field(
        "playlist_document_type_mismatch",
        "playlist document types differ",
        &expected.r#type,
        &actual.r#type,
        issues,
    );
    compare_document_field(
        "playlist_root_mismatch",
        "playlist root hierarchy or item metadata differs",
        &expected.root_node,
        &actual.root_node,
        issues,
    );
    compare_document_field(
        "playlist_tags_mismatch",
        "playlist document tags differ",
        &expected.tags,
        &actual.tags,
        issues,
    );
    compare_document_field(
        "playlist_live_video_mismatch",
        "live-video playlist metadata differs",
        &expected.live_video_playlist,
        &actual.live_video_playlist,
        issues,
    );
    compare_document_field(
        "playlist_downloads_mismatch",
        "downloads playlist metadata differs",
        &expected.downloads_playlist,
        &actual.downloads_playlist,
        issues,
    );
}

fn compare_document_field<T: PartialEq>(
    kind: &str,
    message: &str,
    expected: &T,
    actual: &T,
    issues: &mut Vec<PlaylistPackageIssue>,
) {
    if expected != actual {
        issues.push(PlaylistPackageIssue {
            kind: kind.to_string(),
            index: None,
            message: message.to_string(),
        });
    }
}

fn normalized_playlist_document(document: &rv_data::PlaylistDocument) -> rv_data::PlaylistDocument {
    let mut normalized = document.clone();
    for tag in &mut normalized.tags {
        normalize_uuid_case(tag.uuid.as_mut());
    }
    if let Some(root) = &mut normalized.root_node {
        normalize_playlist_node(root);
    }
    if let Some(live_video) = &mut normalized.live_video_playlist {
        normalize_playlist_node(live_video);
    }
    if let Some(downloads) = &mut normalized.downloads_playlist {
        normalize_playlist_node(downloads);
    }
    normalized
}

fn normalize_playlist_node(node: &mut rv_data::Playlist) {
    normalize_volatile_uuid(node.uuid.as_mut());
    normalize_uuid_case(node.targeted_layer_uuid.as_mut());
    if let Some(path) = &mut node.smart_directory_path {
        normalize_document_url(path);
    }
    for child in &mut node.children {
        normalize_playlist_node(child);
    }
    match &mut node.children_type {
        Some(playlist::ChildrenType::Playlists(playlists)) => {
            for child in &mut playlists.playlists {
                normalize_playlist_node(child);
            }
        }
        Some(playlist::ChildrenType::Items(items)) => {
            for item in &mut items.items {
                normalize_playlist_item(item);
            }
        }
        None => {}
    }
}

fn normalize_playlist_item(item: &mut rv_data::PlaylistItem) {
    normalize_volatile_uuid(item.uuid.as_mut());
    for tag in &mut item.tags {
        normalize_uuid_case(Some(tag));
    }
    match &mut item.item_type {
        Some(playlist_item::ItemType::Presentation(presentation)) => {
            if let Some(path) = &mut presentation.document_path {
                normalize_document_url(path);
            }
            normalize_uuid_case(presentation.arrangement.as_mut());
        }
        Some(playlist_item::ItemType::PlanningCenter(planning_center)) => {
            if let Some(linked_data) = &mut planning_center.linked_data {
                normalize_playlist_item(linked_data);
            }
        }
        Some(playlist_item::ItemType::Placeholder(placeholder)) => {
            if let Some(linked_data) = &mut placeholder.linked_data {
                normalize_playlist_item(linked_data);
            }
        }
        _ => {}
    }
}

fn normalize_document_url(document_path: &mut rv_data::Url) {
    if let Some(url::Storage::AbsoluteString(value)) = &mut document_path.storage {
        *value = normalize_absolute_path_value(value);
    }
}

fn normalize_volatile_uuid(uuid: Option<&mut rv_data::Uuid>) {
    if let Some(uuid) = uuid {
        uuid.string = "<volatile-identity>".to_string();
    }
}

fn normalize_uuid_case(uuid: Option<&mut rv_data::Uuid>) {
    if let Some(uuid) = uuid {
        uuid.string.make_ascii_lowercase();
    }
}

fn compare_items(
    expected: &[PlaylistItemSummary],
    actual: &[PlaylistItemSummary],
    issues: &mut Vec<PlaylistPackageIssue>,
) {
    if expected.len() != actual.len() {
        issues.push(PlaylistPackageIssue {
            kind: "item_count_mismatch".to_string(),
            index: None,
            message: format!("expected {} items, found {}", expected.len(), actual.len()),
        });
    }

    for index in 0..expected.len().max(actual.len()) {
        match (expected.get(index), actual.get(index)) {
            (Some(expected), Some(actual)) => compare_item(index, expected, actual, issues),
            (Some(expected), None) => issues.push(PlaylistPackageIssue {
                kind: "missing_item".to_string(),
                index: Some(index),
                message: format!("missing item '{}'", expected.name),
            }),
            (None, Some(actual)) => issues.push(PlaylistPackageIssue {
                kind: "extra_item".to_string(),
                index: Some(index),
                message: format!("extra item '{}'", actual.name),
            }),
            (None, None) => {}
        }
    }
}

fn compare_item(
    index: usize,
    expected: &PlaylistItemSummary,
    actual: &PlaylistItemSummary,
    issues: &mut Vec<PlaylistPackageIssue>,
) {
    compare_item_identity(index, expected, actual, issues);
    compare_item_paths(index, expected, actual, issues);
    compare_item_presentation_options(index, expected, actual, issues);
}

fn compare_item_identity(
    index: usize,
    expected: &PlaylistItemSummary,
    actual: &PlaylistItemSummary,
    issues: &mut Vec<PlaylistPackageIssue>,
) {
    if expected.item_uuid.is_some() != actual.item_uuid.is_some() {
        issues.push(PlaylistPackageIssue {
            kind: "item_uuid_presence_mismatch".to_string(),
            index: Some(index),
            message: format!(
                "expected UUID presence {}, found {}",
                expected.item_uuid.is_some(),
                actual.item_uuid.is_some()
            ),
        });
    }

    if expected.name != actual.name {
        issues.push(PlaylistPackageIssue {
            kind: "item_name_mismatch".to_string(),
            index: Some(index),
            message: format!("expected '{}', found '{}'", expected.name, actual.name),
        });
    }

    let expected_tags = expected
        .item_tags
        .iter()
        .map(|uuid| uuid.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let actual_tags = actual
        .item_tags
        .iter()
        .map(|uuid| uuid.to_ascii_lowercase())
        .collect::<Vec<_>>();
    if expected_tags != actual_tags {
        issues.push(PlaylistPackageIssue {
            kind: "item_tags_mismatch".to_string(),
            index: Some(index),
            message: format!(
                "expected item tags {:?}, found {:?}",
                expected.item_tags, actual.item_tags
            ),
        });
    }

    if expected.is_hidden != actual.is_hidden {
        issues.push(PlaylistPackageIssue {
            kind: "item_hidden_mismatch".to_string(),
            index: Some(index),
            message: format!(
                "expected hidden={}, found hidden={}",
                expected.is_hidden, actual.is_hidden
            ),
        });
    }
}

fn compare_item_paths(
    index: usize,
    expected: &PlaylistItemSummary,
    actual: &PlaylistItemSummary,
    issues: &mut Vec<PlaylistPackageIssue>,
) {
    if expected.document_platform != actual.document_platform {
        issues.push(PlaylistPackageIssue {
            kind: "item_document_platform_mismatch".to_string(),
            index: Some(index),
            message: format!(
                "expected document platform {:?}, found {:?}",
                expected.document_platform, actual.document_platform
            ),
        });
    }

    if expected.local_relative_path != actual.local_relative_path {
        issues.push(PlaylistPackageIssue {
            kind: "item_relative_path_mismatch".to_string(),
            index: Some(index),
            message: format!(
                "expected {:?}, found {:?}",
                expected.local_relative_path, actual.local_relative_path
            ),
        });
    }

    if expected.storage_relative_path != actual.storage_relative_path {
        issues.push(PlaylistPackageIssue {
            kind: "item_storage_relative_path_mismatch".to_string(),
            index: Some(index),
            message: format!(
                "expected {:?}, found {:?}",
                expected.storage_relative_path, actual.storage_relative_path
            ),
        });
    }

    if expected.local_root != actual.local_root {
        issues.push(PlaylistPackageIssue {
            kind: "item_local_root_mismatch".to_string(),
            index: Some(index),
            message: format!(
                "expected local root {:?}, found {:?}",
                expected.local_root, actual.local_root
            ),
        });
    }

    if expected.external_relative_path != actual.external_relative_path {
        issues.push(PlaylistPackageIssue {
            kind: "item_external_relative_path_mismatch".to_string(),
            index: Some(index),
            message: format!(
                "expected external path {:?}, found {:?}",
                expected.external_relative_path, actual.external_relative_path
            ),
        });
    }

    let expected_absolute_path = expected
        .absolute_string
        .as_deref()
        .map(normalize_absolute_path_value);
    let actual_absolute_path = actual
        .absolute_string
        .as_deref()
        .map(normalize_absolute_path_value);
    if expected_absolute_path != actual_absolute_path {
        issues.push(PlaylistPackageIssue {
            kind: "item_absolute_path_mismatch".to_string(),
            index: Some(index),
            message: format!(
                "expected {:?}, found {:?}",
                expected.absolute_string, actual.absolute_string
            ),
        });
    }
}

fn compare_item_presentation_options(
    index: usize,
    expected: &PlaylistItemSummary,
    actual: &PlaylistItemSummary,
    issues: &mut Vec<PlaylistPackageIssue>,
) {
    let expected_arrangement = expected.arrangement_uuid.as_deref().map(str::to_lowercase);
    let actual_arrangement = actual.arrangement_uuid.as_deref().map(str::to_lowercase);
    if expected_arrangement != actual_arrangement {
        issues.push(PlaylistPackageIssue {
            kind: "item_arrangement_mismatch".to_string(),
            index: Some(index),
            message: format!(
                "expected arrangement {:?}, found {:?}",
                expected.arrangement_uuid, actual.arrangement_uuid
            ),
        });
    }

    if expected.content_destination != actual.content_destination {
        issues.push(PlaylistPackageIssue {
            kind: "item_content_destination_mismatch".to_string(),
            index: Some(index),
            message: format!(
                "expected content destination {}, found {}",
                expected.content_destination, actual.content_destination
            ),
        });
    }

    if expected.user_music_key != actual.user_music_key {
        issues.push(PlaylistPackageIssue {
            kind: "item_music_key_mismatch".to_string(),
            index: Some(index),
            message: format!(
                "expected music key {:?}, found {:?}",
                expected.user_music_key, actual.user_music_key
            ),
        });
    }

    if expected.arrangement_name != actual.arrangement_name {
        issues.push(PlaylistPackageIssue {
            kind: "item_arrangement_name_mismatch".to_string(),
            index: Some(index),
            message: format!(
                "expected arrangement name {:?}, found {:?}",
                expected.arrangement_name, actual.arrangement_name
            ),
        });
    }
}

fn normalize_absolute_path_value(value: &str) -> String {
    let decoded = percent_decode_lossy(value).replace('\\', "/");
    decoded.find("Libraries/").map_or_else(
        || decoded.rsplit('/').next().unwrap_or(&decoded).to_string(),
        |index| decoded[index..].to_string(),
    )
}

fn playlist_item_alignment_key(item: &PlaylistItemSummary) -> String {
    if let Some(path) = item.local_relative_path.as_deref() {
        return format!("path:{}", normalize_playlist_item_key(path));
    }
    if let Some(path) = item.storage_relative_path.as_deref() {
        return format!("storage:{}", normalize_playlist_item_key(path));
    }
    if let Some(path) = item.external_relative_path.as_deref() {
        return format!("external:{}", normalize_playlist_item_key(path));
    }
    if let Some(path) = item.absolute_string.as_deref() {
        return format!(
            "absolute:{}",
            normalize_playlist_item_key(&normalize_absolute_path_value(path))
        );
    }
    format!("name:{}", normalize_playlist_item_key(&item.name))
}

fn normalize_playlist_item_key(value: &str) -> String {
    percent_decode_lossy(value)
        .replace('\\', "/")
        .trim_start_matches("./")
        .to_ascii_lowercase()
}

fn percent_decode_lossy(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
            {
                decoded.push((hi << 4) | lo);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(decoded).unwrap_or_else(|_| value.to_string())
}

const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct FileFingerprint {
    size: u64,
    crc32: u32,
}

fn compare_embedded_presentations(
    expected: &PlaylistPackage,
    actual: &PlaylistPackage,
    issues: &mut Vec<PlaylistPackageIssue>,
) {
    let expected_files = presentation_fingerprints(expected);
    let actual_files = presentation_fingerprints(actual);
    let names: BTreeSet<_> = expected_files
        .keys()
        .chain(actual_files.keys())
        .cloned()
        .collect();

    for name in names {
        match (expected_files.get(&name), actual_files.get(&name)) {
            (Some(expected), Some(actual)) if expected == actual => {}
            (Some(expected), Some(actual)) if expected.len() != actual.len() => {
                issues.push(PlaylistPackageIssue {
                    kind: "embedded_presentation_count_mismatch".to_string(),
                    index: None,
                    message: format!(
                        "presentation '{name}' appears {} time(s), found {}",
                        expected.len(),
                        actual.len()
                    ),
                });
            }
            (Some(expected), Some(actual)) => issues.push(PlaylistPackageIssue {
                kind: "embedded_presentation_crc_mismatch".to_string(),
                index: None,
                message: format!(
                    "presentation '{name}' fingerprints differ: expected {expected:?}, found {actual:?}"
                ),
            }),
            (Some(_), None) => issues.push(PlaylistPackageIssue {
                kind: "missing_embedded_presentation".to_string(),
                index: None,
                message: format!("missing embedded presentation '{name}'"),
            }),
            (None, Some(_)) => issues.push(PlaylistPackageIssue {
                kind: "extra_embedded_presentation".to_string(),
                index: None,
                message: format!("extra embedded presentation '{name}'"),
            }),
            (None, None) => {}
        }
    }

    compare_embedded_presentation_semantics(expected, actual, issues);
}

fn presentation_fingerprints(package: &PlaylistPackage) -> BTreeMap<String, Vec<FileFingerprint>> {
    let mut fingerprints: BTreeMap<String, Vec<FileFingerprint>> = BTreeMap::new();
    for file in package
        .embedded_file_details
        .iter()
        .filter(|file| file.is_presentation)
    {
        fingerprints
            .entry(file.basename.clone())
            .or_default()
            .push(FileFingerprint {
                size: file.size,
                crc32: file.crc32,
            });
    }

    for values in fingerprints.values_mut() {
        values.sort_unstable();
    }

    fingerprints
}

fn compare_embedded_presentation_semantics(
    expected: &PlaylistPackage,
    actual: &PlaylistPackage,
    issues: &mut Vec<PlaylistPackageIssue>,
) {
    let expected_summaries = semantic_presentation_summaries(expected);
    let actual_summaries = semantic_presentation_summaries(actual);
    let names: BTreeSet<_> = expected_summaries
        .keys()
        .chain(actual_summaries.keys())
        .cloned()
        .collect();

    for name in names {
        let (Some(expected), Some(actual)) =
            (expected_summaries.get(&name), actual_summaries.get(&name))
        else {
            continue;
        };
        for index in 0..expected.len().min(actual.len()) {
            compare_embedded_presentation_summary(&name, &expected[index], &actual[index], issues);
        }
    }
}

fn compare_embedded_presentation_structures(
    expected: &PlaylistPackage,
    actual: &PlaylistPackage,
    issues: &mut Vec<PlaylistPackageIssue>,
) {
    let expected_structures = semantic_presentation_structures(expected);
    let actual_structures = semantic_presentation_structures(actual);
    let names: BTreeSet<_> = expected_structures
        .keys()
        .chain(actual_structures.keys())
        .cloned()
        .collect();

    for name in names {
        let (Some(expected), Some(actual)) =
            (expected_structures.get(&name), actual_structures.get(&name))
        else {
            continue;
        };
        for index in 0..expected.len().min(actual.len()) {
            compare_presentation_structure_summary(
                &name,
                &expected[index].structure,
                &actual[index].structure,
                issues,
            );
        }
    }
}

fn semantic_presentation_summaries(
    package: &PlaylistPackage,
) -> BTreeMap<String, Vec<EmbeddedPresentationSummary>> {
    let mut summaries: BTreeMap<String, Vec<EmbeddedPresentationSummary>> = BTreeMap::new();
    for summary in embedded_presentation_summaries(package) {
        summaries
            .entry(summary.basename.clone())
            .or_default()
            .push(summary);
    }
    for values in summaries.values_mut() {
        values.sort_by(|left, right| {
            left.archive_path
                .cmp(&right.archive_path)
                .then(left.presentation_uuid.cmp(&right.presentation_uuid))
        });
    }
    summaries
}

fn semantic_presentation_structures(
    package: &PlaylistPackage,
) -> BTreeMap<String, Vec<EmbeddedPresentationStructure>> {
    let mut structures: BTreeMap<String, Vec<EmbeddedPresentationStructure>> = BTreeMap::new();
    for structure in embedded_presentation_structures(package) {
        structures
            .entry(structure.basename.clone())
            .or_default()
            .push(structure);
    }
    for values in structures.values_mut() {
        values.sort_by(|left, right| {
            left.archive_path
                .cmp(&right.archive_path)
                .then(left.structure.uuid.cmp(&right.structure.uuid))
        });
    }
    structures
}

#[allow(
    clippy::too_many_lines,
    reason = "the field-by-field parity report is clearer as one locally auditable comparison"
)]
fn compare_presentation_structure_summary(
    basename: &str,
    expected: &PresentationStructureSummary,
    actual: &PresentationStructureSummary,
    issues: &mut Vec<PlaylistPackageIssue>,
) {
    if expected.name != actual.name {
        issues.push(PlaylistPackageIssue {
            kind: "embedded_presentation_name_mismatch".to_string(),
            index: None,
            message: format!(
                "presentation '{basename}' expected name {:?}, found {:?}",
                expected.name, actual.name
            ),
        });
    }

    if expected.bible_reference != actual.bible_reference {
        issues.push(PlaylistPackageIssue {
            kind: "embedded_presentation_bible_reference_mismatch".to_string(),
            index: None,
            message: format!(
                "presentation '{basename}' expected Bible reference {:?}, found {:?}",
                expected.bible_reference, actual.bible_reference
            ),
        });
    }

    let expected_groups = expected
        .cue_groups
        .iter()
        .map(|group| (group.name.as_str(), group.cue_indexes.as_slice()))
        .collect::<Vec<_>>();
    let actual_groups = actual
        .cue_groups
        .iter()
        .map(|group| (group.name.as_str(), group.cue_indexes.as_slice()))
        .collect::<Vec<_>>();
    if expected_groups != actual_groups {
        issues.push(PlaylistPackageIssue {
            kind: "embedded_presentation_group_order_mismatch".to_string(),
            index: None,
            message: format!(
                "presentation '{basename}' expected cue groups {expected_groups:?}, found {actual_groups:?}"
            ),
        });
    }

    let group_bindings = |summary: &PresentationStructureSummary| {
        summary
            .cue_groups
            .iter()
            .map(|group| {
                (
                    group.name.clone(),
                    group.color.clone(),
                    group.hot_key.clone(),
                    group.application_group_identifier.clone(),
                    group.application_group_name.clone(),
                )
            })
            .collect::<Vec<_>>()
    };
    let expected_group_bindings = group_bindings(expected);
    let actual_group_bindings = group_bindings(actual);
    if expected_group_bindings != actual_group_bindings {
        issues.push(PlaylistPackageIssue {
            kind: "embedded_presentation_group_binding_mismatch".to_string(),
            index: None,
            message: format!(
                "presentation '{basename}' expected group bindings {expected_group_bindings:?}, found {actual_group_bindings:?}"
            ),
        });
    }

    let expected_arrangements = expected
        .arrangements
        .iter()
        .map(|arrangement| {
            (
                arrangement.name.as_str(),
                arrangement.group_names.as_slice(),
                arrangement.cue_indexes.as_slice(),
            )
        })
        .collect::<Vec<_>>();
    let actual_arrangements = actual
        .arrangements
        .iter()
        .map(|arrangement| {
            (
                arrangement.name.as_str(),
                arrangement.group_names.as_slice(),
                arrangement.cue_indexes.as_slice(),
            )
        })
        .collect::<Vec<_>>();
    if expected_arrangements != actual_arrangements {
        issues.push(PlaylistPackageIssue {
            kind: "embedded_presentation_arrangement_order_mismatch".to_string(),
            index: None,
            message: format!(
                "presentation '{basename}' expected arrangements {expected_arrangements:?}, found {actual_arrangements:?}"
            ),
        });
    }

    let expected_operator_cues = operator_cue_signatures(expected);
    let actual_operator_cues = operator_cue_signatures(actual);
    if expected_operator_cues.len() != actual_operator_cues.len() {
        issues.push(PlaylistPackageIssue {
            kind: "embedded_presentation_operator_cue_count_mismatch".to_string(),
            index: None,
            message: format!(
                "presentation '{basename}' expected {} operator cues, found {}",
                expected_operator_cues.len(),
                actual_operator_cues.len()
            ),
        });
    }
    for index in 0..expected_operator_cues.len().min(actual_operator_cues.len()) {
        if expected_operator_cues[index] != actual_operator_cues[index] {
            issues.push(PlaylistPackageIssue {
                kind: "embedded_presentation_operator_cue_mismatch".to_string(),
                index: Some(index),
                message: format!(
                    "presentation '{basename}' operator cue {index} expected {:?}, found {:?}",
                    expected_operator_cues[index], actual_operator_cues[index]
                ),
            });
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OperatorCueSignature {
    group_names: Vec<String>,
    text_lines: Vec<String>,
    is_blank: bool,
    macros: Vec<String>,
    slide_labels: Vec<ActionLabelSignature>,
    background_media: Vec<String>,
    action_kinds: Vec<String>,
    text_styles: Vec<TextStyleSignature>,
}

fn operator_cue_signatures(summary: &PresentationStructureSummary) -> Vec<OperatorCueSignature> {
    let cue_by_index = summary
        .cues
        .iter()
        .map(|cue| (cue.index, cue))
        .collect::<BTreeMap<_, _>>();
    summary
        .operator_cue_indexes
        .iter()
        .filter_map(|index| cue_by_index.get(index))
        .map(|cue| OperatorCueSignature {
            group_names: cue.group_names.clone(),
            text_lines: cue.text_lines.clone(),
            is_blank: cue.is_blank,
            macros: cue.macros.clone(),
            slide_labels: cue.slide_labels.clone(),
            background_media: cue.background_media.clone(),
            action_kinds: cue.action_kinds.clone(),
            text_styles: cue.text_styles.clone(),
        })
        .collect()
}

fn compare_embedded_presentation_summary(
    basename: &str,
    expected: &EmbeddedPresentationSummary,
    actual: &EmbeddedPresentationSummary,
    issues: &mut Vec<PlaylistPackageIssue>,
) {
    if expected.presentation_uuid != actual.presentation_uuid {
        issues.push(PlaylistPackageIssue {
            kind: "embedded_presentation_uuid_mismatch".to_string(),
            index: None,
            message: format!(
                "presentation '{basename}' expected UUID {:?}, found {:?}",
                expected.presentation_uuid, actual.presentation_uuid
            ),
        });
    }

    if expected.cue_count != actual.cue_count {
        issues.push(PlaylistPackageIssue {
            kind: "embedded_presentation_cue_count_mismatch".to_string(),
            index: None,
            message: format!(
                "presentation '{basename}' expected {} cues, found {}",
                expected.cue_count, actual.cue_count
            ),
        });
    }

    if expected.cue_group_count != actual.cue_group_count {
        issues.push(PlaylistPackageIssue {
            kind: "embedded_presentation_group_count_mismatch".to_string(),
            index: None,
            message: format!(
                "presentation '{basename}' expected {} cue groups, found {}",
                expected.cue_group_count, actual.cue_group_count
            ),
        });
    }

    if expected.arrangement_names != actual.arrangement_names {
        issues.push(PlaylistPackageIssue {
            kind: "embedded_presentation_arrangement_names_mismatch".to_string(),
            index: None,
            message: format!(
                "presentation '{basename}' expected arrangements {:?}, found {:?}",
                expected.arrangement_names, actual.arrangement_names
            ),
        });
    }
}

fn compare_media_assets(
    expected: &PlaylistPackage,
    actual: &PlaylistPackage,
    issues: &mut Vec<PlaylistPackageIssue>,
) {
    let expected_media = media_fingerprints(expected);
    let actual_media = media_fingerprints(actual);
    let paths = expected_media
        .keys()
        .chain(actual_media.keys())
        .cloned()
        .collect::<BTreeSet<_>>();

    for path in paths {
        match (expected_media.get(&path), actual_media.get(&path)) {
            (Some(expected), Some(actual)) if expected == actual => {}
            (Some(expected), Some(actual)) => issues.push(PlaylistPackageIssue {
                kind: "media_asset_fingerprint_mismatch".to_string(),
                index: None,
                message: format!(
                    "media asset '{path}' fingerprints differ: expected {expected:?}, found {actual:?}"
                ),
            }),
            (Some(_), None) => issues.push(PlaylistPackageIssue {
                kind: "missing_media_asset".to_string(),
                index: None,
                message: format!("missing media asset '{path}'"),
            }),
            (None, Some(_)) => issues.push(PlaylistPackageIssue {
                kind: "extra_media_asset".to_string(),
                index: None,
                message: format!("extra media asset '{path}'"),
            }),
            (None, None) => {}
        }
    }
}

fn media_fingerprints(package: &PlaylistPackage) -> BTreeMap<String, Vec<FileFingerprint>> {
    let mut fingerprints: BTreeMap<String, Vec<FileFingerprint>> = BTreeMap::new();
    for file in package
        .embedded_file_details
        .iter()
        .filter(|file| !file.is_presentation)
    {
        fingerprints
            .entry(file.name.clone())
            .or_default()
            .push(FileFingerprint {
                size: file.size,
                crc32: file.crc32,
            });
    }
    for values in fingerprints.values_mut() {
        values.sort_unstable();
    }
    fingerprints
}

fn summarize_cue(
    index: usize,
    cue: &rv_data::Cue,
    mut group_names: Vec<String>,
) -> CueStructureSummary {
    group_names.sort();

    let text = cue_text(cue);
    CueStructureSummary {
        index,
        uuid: cue.uuid.as_ref().map(|uuid| uuid.string.clone()),
        name: cue.name.clone(),
        group_names,
        text_lines: text.lines().map(str::to_string).collect(),
        is_blank: !text.chars().any(char::is_alphanumeric),
        text,
        macros: cue
            .actions
            .iter()
            .filter_map(macro_action_name)
            .map(str::to_string)
            .collect(),
        slide_labels: cue.actions.iter().filter_map(slide_action_label).collect(),
        background_media: cue
            .actions
            .iter()
            .filter_map(background_media_basename)
            .collect(),
        action_kinds: cue.actions.iter().map(action_kind).collect(),
        text_styles: cue_text_styles(cue),
    }
}

fn summarize_bible_reference(
    reference: &rv_data::presentation::BibleReference,
) -> BibleReferenceSummary {
    BibleReferenceSummary {
        book_index: reference.book_index,
        book_name: reference.book_name.clone(),
        chapter_range: reference.chapter_range.as_ref().map(summarize_int_range),
        verse_range: reference.verse_range.as_ref().map(summarize_int_range),
        translation_name: reference.translation_name.clone(),
        translation_display_abbreviation: reference.translation_display_abbreviation.clone(),
        translation_internal_abbreviation: reference.translation_internal_abbreviation.clone(),
        book_key: reference.book_key.clone(),
    }
}

const fn summarize_int_range(range: &rv_data::IntRange) -> IntRangeSummary {
    IntRangeSummary {
        start: range.start,
        end: range.end,
    }
}

fn slide_action_label(action: &rv_data::Action) -> Option<ActionLabelSignature> {
    if !matches!(
        &action.action_type_data,
        Some(action::ActionTypeData::Slide(_))
    ) {
        return None;
    }
    action.label.as_ref().map(|label| ActionLabelSignature {
        text: label.text.clone(),
        color: label.color.as_ref().map(color_signature),
    })
}

fn cue_text_styles(cue: &rv_data::Cue) -> Vec<TextStyleSignature> {
    let mut styles = Vec::new();
    for action in &cue.actions {
        let Some(action::ActionTypeData::Slide(slide_type)) = &action.action_type_data else {
            continue;
        };
        let Some(action::slide_type::Slide::Presentation(slide)) = &slide_type.slide else {
            continue;
        };
        let Some(base_slide) = &slide.base_slide else {
            continue;
        };
        let slide_size = base_slide.size.as_ref().map(size_signature);
        for element in &base_slide.elements {
            let Some(graphics) = &element.element else {
                continue;
            };
            let Some(text) = &graphics.text else {
                continue;
            };
            styles.push(text_style_signature(graphics, text, slide_size.as_deref()));
        }
    }
    styles
}

fn text_style_signature(
    graphics: &rv_data::graphics::Element,
    text: &rv_data::graphics::Text,
    slide_size: Option<&str>,
) -> TextStyleSignature {
    let rtf_options = extract_rtf_options(&text.rtf_data);
    let font = text
        .attributes
        .as_ref()
        .and_then(|attributes| attributes.font.as_ref());
    let fill_color = text
        .attributes
        .as_ref()
        .and_then(|attributes| attributes.fill.as_ref())
        .and_then(text_fill_color);

    TextStyleSignature {
        element_name: graphics.name.clone(),
        bounds: graphics.bounds.as_ref().map(rect_signature),
        slide_size: slide_size.map(str::to_string),
        font_name: font
            .map(|font| {
                if font.name.is_empty() {
                    font.family.clone()
                } else {
                    font.name.clone()
                }
            })
            .filter(|name| !name.is_empty())
            .or_else(|| {
                rtf_options
                    .as_ref()
                    .map(|options| options.font_name.clone())
            }),
        font_size: font
            .and_then(|font| rounded_font_size(font.size))
            .or_else(|| rtf_options.as_ref().map(|options| options.font_size)),
        color: fill_color.or_else(|| {
            rtf_options
                .as_ref()
                .map(|options| rgb_signature(options.color))
        }),
        bold: font
            .map(|font| font.bold)
            .or_else(|| rtf_options.as_ref().map(|options| options.bold)),
        italic: font
            .map(|font| font.italic)
            .or_else(|| rtf_options.as_ref().map(|options| options.italic)),
        vertical_alignment: enum_suffix(
            rv_data::graphics::text::VerticalAlignment::try_from(text.vertical_alignment)
                .ok()
                .map_or("UNKNOWN", |value| value.as_str_name()),
        ),
        scale_behavior: enum_suffix(
            rv_data::graphics::text::ScaleBehavior::try_from(text.scale_behavior)
                .ok()
                .map_or("UNKNOWN", |value| value.as_str_name()),
        ),
        transform: enum_suffix(
            rv_data::graphics::text::Transform::try_from(text.transform)
                .ok()
                .map_or("UNKNOWN", |value| value.as_str_name()),
        ),
        margins: text.margins.as_ref().map(edge_insets_signature),
    }
}

fn rounded_font_size(value: f64) -> Option<u32> {
    let rounded = value.round();
    if !rounded.is_finite() || !(1.0..=f64::from(u32::MAX)).contains(&rounded) {
        return None;
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some(rounded as u32)
}

fn text_fill_color(fill: &rv_data::graphics::text::attributes::Fill) -> Option<String> {
    match fill {
        rv_data::graphics::text::attributes::Fill::TextSolidFill(color) => {
            Some(color_signature(color))
        }
        _ => None,
    }
}

fn color_signature(color: &rv_data::Color) -> String {
    format!(
        "#{:02X}{:02X}{:02X}{:02X}",
        color_component(color.red),
        color_component(color.green),
        color_component(color.blue),
        color_component(color.alpha)
    )
}

fn rgb_signature(color: (u8, u8, u8)) -> String {
    format!("#{:02X}{:02X}{:02X}", color.0, color.1, color.2)
}

fn color_component(value: f32) -> u8 {
    if !value.is_finite() {
        return 0;
    }
    let value = if value <= 1.0 { value * 255.0 } else { value };
    let rounded = value.clamp(0.0, 255.0).round();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        rounded as u8
    }
}

fn rect_signature(rect: &rv_data::graphics::Rect) -> String {
    let origin = rect.origin.as_ref();
    let size = rect.size.as_ref();
    format!(
        "{},{},{},{}",
        format_coord(origin.and_then(|origin| origin.x).unwrap_or_default()),
        format_coord(origin.map_or(0.0, |origin| origin.y)),
        format_coord(size.map_or(0.0, |size| size.width)),
        format_coord(size.map_or(0.0, |size| size.height))
    )
}

fn size_signature(size: &rv_data::graphics::Size) -> String {
    format!("{}x{}", format_coord(size.width), format_coord(size.height))
}

fn edge_insets_signature(insets: &rv_data::graphics::EdgeInsets) -> String {
    format!(
        "{},{},{},{}",
        format_coord(insets.left),
        format_coord(insets.right),
        format_coord(insets.top),
        format_coord(insets.bottom)
    )
}

fn format_coord(value: f64) -> String {
    format!("{value:.1}")
}

fn enum_suffix(value: &str) -> String {
    value.rsplit('_').next().unwrap_or(value).to_lowercase()
}

fn cue_text(cue: &rv_data::Cue) -> String {
    let mut texts = Vec::new();
    for action in &cue.actions {
        let Some(action::ActionTypeData::Slide(slide_type)) = &action.action_type_data else {
            continue;
        };
        let Some(action::slide_type::Slide::Presentation(slide)) = &slide_type.slide else {
            continue;
        };
        let Some(base_slide) = &slide.base_slide else {
            continue;
        };
        for element in &base_slide.elements {
            let Some(graphics) = &element.element else {
                continue;
            };
            let Some(text) = &graphics.text else {
                continue;
            };
            let rtf = String::from_utf8_lossy(&text.rtf_data);
            if let Some(text) = rtf_to_text(&rtf) {
                texts.push(text.replace("\r\n", "\n").replace('\r', "\n"));
            }
        }
    }
    texts.join("\n\n")
}

fn action_kind(action: &rv_data::Action) -> String {
    match &action.action_type_data {
        Some(action::ActionTypeData::Slide(_)) => "slide".to_string(),
        Some(action::ActionTypeData::Macro(_)) => macro_action_name(action)
            .map_or_else(|| "macro".to_string(), |name| format!("macro:{name}")),
        Some(action::ActionTypeData::Media(media)) => {
            let layer = action::LayerType::try_from(media.layer_type)
                .ok()
                .map_or_else(
                    || media.layer_type.to_string(),
                    |layer| {
                        layer
                            .as_str_name()
                            .trim_start_matches("LAYER_TYPE_")
                            .to_lowercase()
                    },
                );
            format!("media:{layer}")
        }
        Some(_) => format!("other:{}", action.r#type),
        None => format!("none:{}", action.r#type),
    }
}

fn background_media_basename(action: &rv_data::Action) -> Option<String> {
    let Some(action::ActionTypeData::Media(media_type)) = &action.action_type_data else {
        return None;
    };
    if action.r#type != action::ActionType::BackgroundMedia as i32
        && media_type.layer_type != action::LayerType::Background as i32
    {
        return None;
    }
    media_type
        .element
        .as_ref()
        .and_then(|media| media.url.as_ref())
        .and_then(url_storage_string)
        .map(|source| path_basename(&source))
}

fn url_storage_string(url: &rv_data::Url) -> Option<String> {
    match url.storage.as_ref()? {
        url::Storage::AbsoluteString(value) | url::Storage::RelativePath(value) => {
            Some(value.clone())
        }
    }
}

fn path_basename(value: &str) -> String {
    let decoded = percent_decode_lossy(value.trim_start_matches("file://"));
    Path::new(&decoded)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&decoded)
        .to_string()
}

fn cue_indexes_by_uuid(presentation: &rv_data::Presentation) -> BTreeMap<&str, usize> {
    presentation
        .cues
        .iter()
        .enumerate()
        .filter_map(|(index, cue)| cue.uuid.as_ref().map(|uuid| (uuid.string.as_str(), index)))
        .collect()
}

fn cue_group_indexes_by_uuid(presentation: &rv_data::Presentation) -> BTreeMap<&str, usize> {
    presentation
        .cue_groups
        .iter()
        .enumerate()
        .filter_map(|(index, cue_group)| {
            cue_group
                .group
                .as_ref()
                .and_then(|group| group.uuid.as_ref())
                .map(|uuid| (uuid.string.as_str(), index))
        })
        .collect()
}

fn cue_group_names_by_cue_uuid(
    presentation: &rv_data::Presentation,
) -> BTreeMap<&str, Vec<String>> {
    let mut names: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for cue_group in &presentation.cue_groups {
        let group_name = cue_group
            .group
            .as_ref()
            .map(|group| group.name.clone())
            .unwrap_or_default();
        for cue_id in &cue_group.cue_identifiers {
            names
                .entry(cue_id.string.as_str())
                .or_default()
                .push(group_name.clone());
        }
    }
    names
}

fn operator_cue_indexes(
    presentation: &rv_data::Presentation,
    cue_indexes_by_uuid: &BTreeMap<&str, usize>,
) -> Vec<usize> {
    let cue_group_indexes_by_uuid = cue_group_indexes_by_uuid(presentation);
    if let Some(arrangement) = selected_or_default_arrangement(presentation) {
        let mut indexes = Vec::new();
        for group_id in &arrangement.group_identifiers {
            let Some(group_index) = cue_group_indexes_by_uuid.get(group_id.string.as_str()) else {
                continue;
            };
            let Some(group) = presentation.cue_groups.get(*group_index) else {
                continue;
            };
            indexes.extend(
                group
                    .cue_identifiers
                    .iter()
                    .filter_map(|cue_id| cue_indexes_by_uuid.get(cue_id.string.as_str()))
                    .copied(),
            );
        }
        if !indexes.is_empty() {
            return indexes;
        }
    }

    let mut indexes = Vec::new();
    for group in &presentation.cue_groups {
        indexes.extend(
            group
                .cue_identifiers
                .iter()
                .filter_map(|cue_id| cue_indexes_by_uuid.get(cue_id.string.as_str()))
                .copied(),
        );
    }
    if indexes.is_empty() {
        (0..presentation.cues.len()).collect()
    } else {
        indexes
    }
}

fn selected_or_default_arrangement(
    presentation: &rv_data::Presentation,
) -> Option<&rv_data::presentation::Arrangement> {
    if let Some(selected) = &presentation.selected_arrangement {
        if let Some(arrangement) = presentation.arrangements.iter().find(|arrangement| {
            arrangement
                .uuid
                .as_ref()
                .is_some_and(|uuid| uuid.string == selected.string)
        }) {
            return Some(arrangement);
        }
    }

    presentation
        .arrangements
        .iter()
        .find(|arrangement| arrangement.name.eq_ignore_ascii_case("Default"))
        .or_else(|| presentation.arrangements.first())
}

fn collect_playlist_items(playlist: &rv_data::Playlist, items: &mut Vec<PlaylistItemSummary>) {
    for child in &playlist.children {
        collect_playlist_items(child, items);
    }

    match &playlist.children_type {
        Some(playlist::ChildrenType::Playlists(playlists)) => {
            for child in &playlists.playlists {
                collect_playlist_items(child, items);
            }
        }
        Some(playlist::ChildrenType::Items(playlist_items)) => {
            for item in &playlist_items.items {
                if let Some(summary) = summarize_presentation_item(item) {
                    items.push(summary);
                }
            }
        }
        None => {}
    }
}

fn summarize_presentation_item(item: &rv_data::PlaylistItem) -> Option<PlaylistItemSummary> {
    let Some(playlist_item::ItemType::Presentation(presentation)) = &item.item_type else {
        return None;
    };

    let document_path = presentation.document_path.as_ref();
    let absolute_string = document_path.and_then(|url| match &url.storage {
        Some(url::Storage::AbsoluteString(value)) => Some(value.clone()),
        _ => None,
    });
    let storage_relative_path = document_path.and_then(|url| match &url.storage {
        Some(url::Storage::RelativePath(value)) => Some(value.clone()),
        _ => None,
    });
    let (local_relative_path, local_root, external_relative_path) =
        document_path.map_or((None, None, None), summarize_relative_file_path);

    Some(PlaylistItemSummary {
        item_uuid: item.uuid.as_ref().map(|uuid| uuid.string.clone()),
        name: item.name.clone(),
        item_tags: item.tags.iter().map(|uuid| uuid.string.clone()).collect(),
        is_hidden: item.is_hidden,
        document_platform: document_path.map(|url| url.platform),
        absolute_string,
        storage_relative_path,
        local_relative_path,
        local_root,
        external_relative_path,
        arrangement_uuid: presentation
            .arrangement
            .as_ref()
            .map(|uuid| uuid.string.clone()),
        content_destination: presentation.content_destination,
        user_music_key: presentation
            .user_music_key
            .as_ref()
            .map(|key| (key.music_key, key.music_scale)),
        arrangement_name: presentation.arrangement_name.clone(),
    })
}

fn summarize_relative_file_path(
    document_path: &rv_data::Url,
) -> (Option<String>, Option<i32>, Option<String>) {
    match &document_path.relative_file_path {
        Some(url::RelativeFilePath::Local(local)) => {
            (Some(local.path.clone()), Some(local.root), None)
        }
        Some(url::RelativeFilePath::External(external)) => {
            (None, None, Some(external.path.clone()))
        }
        None => (None, None, None),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;
    use crate::propresenter::media::presentation_media_dependencies;
    use crate::propresenter::playlist::{
        build_playlist, write_playlist_file, write_playlist_file_with_options, PlaylistEntry,
        PlaylistMediaAsset, PlaylistMetadata, PlaylistWriteOptions, SelectedArrangement,
    };
    use crate::propresenter::SlideType;
    use serde::Deserialize;
    use std::io::Write;
    use tempfile::tempdir;
    use uuid::Uuid;

    #[derive(Debug, Deserialize)]
    struct RealFixtureManifest {
        playlists: Vec<RealPlaylistFixture>,
        presentations: Vec<RealPresentationFixture>,
    }

    #[derive(Debug, Deserialize)]
    struct RealPlaylistFixture {
        path: String,
        provenance: String,
        independent_native_export: bool,
        mode: PlaylistPackageMode,
        item_count: usize,
        embedded_file_count: usize,
        required_embedded_files: Vec<String>,
    }

    #[derive(Debug, Deserialize)]
    struct RealPresentationFixture {
        path: String,
        provenance: String,
        independent_native_export: bool,
        name: String,
        cue_count: usize,
        cue_group_count: usize,
        arrangement_count: usize,
        media_dependency_count: usize,
    }

    fn real_fixture_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/fixtures/propresenter/real")
    }

    fn real_manifest() -> RealFixtureManifest {
        serde_json::from_str(include_str!(
            "../../data/fixtures/propresenter/real/manifest.json"
        ))
        .expect("real fixture manifest should parse")
    }

    fn presentation_bytes(name: &str) -> Vec<u8> {
        rv_data::Presentation {
            name: name.to_string(),
            ..rv_data::Presentation::default()
        }
        .encode_to_vec()
    }

    fn test_metadata() -> PlaylistMetadata {
        PlaylistMetadata::offline_test()
    }

    fn playlist_item(name: &str, local_relative_path: &str) -> PlaylistItemSummary {
        PlaylistItemSummary {
            item_uuid: None,
            name: name.to_string(),
            item_tags: Vec::new(),
            is_hidden: false,
            document_platform: None,
            absolute_string: None,
            storage_relative_path: None,
            local_relative_path: Some(local_relative_path.to_string()),
            local_root: Some(0),
            external_relative_path: None,
            arrangement_uuid: None,
            content_destination: 0,
            user_music_key: None,
            arrangement_name: String::new(),
        }
    }

    #[test]
    fn absolute_path_normalization_ignores_windows_and_macos_machine_roots() {
        let windows = r"C:\Users\Operator\ProPresenter\Libraries\Default\Song.pro";
        let macos = "file:///Users/operator/ProPresenter/Libraries/Default/Song.pro";

        assert_eq!(
            normalize_absolute_path_value(windows),
            "Libraries/Default/Song.pro"
        );
        assert_eq!(
            normalize_absolute_path_value(macos),
            "Libraries/Default/Song.pro"
        );
    }

    #[test]
    fn aligned_item_compare_does_not_cascade_after_missing_item() {
        let expected = vec![
            playlist_item("Prelude", "Libraries/Default/Prelude.pro"),
            playlist_item("Sermon", "Libraries/Default/5-10-26-SERMON.pro"),
            playlist_item("Prayer", "Libraries/Default/Prayer.pro"),
        ];
        let actual = vec![
            playlist_item("Prelude", "Libraries/Default/Prelude.pro"),
            playlist_item("Prayer", "Libraries/Default/Prayer.pro"),
        ];

        let diffs = compare_playlist_items_aligned(&expected, &actual);

        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].kind, "missing_item_aligned");
        assert_eq!(diffs[0].expected_name.as_deref(), Some("Sermon"));
    }

    #[test]
    fn aligned_item_compare_reports_real_reorders() {
        let expected = vec![
            playlist_item("Prelude", "Libraries/Default/Prelude.pro"),
            playlist_item("Prayer", "Libraries/Default/Prayer.pro"),
        ];
        let actual = vec![
            playlist_item("Prayer", "Libraries/Default/Prayer.pro"),
            playlist_item("Prelude", "Libraries/Default/Prelude.pro"),
        ];

        let diffs = compare_playlist_items_aligned(&expected, &actual);

        assert!(diffs.iter().any(|diff| diff.kind == "moved_item_aligned"
            && diff.expected_name.as_deref() == Some("Prelude")));
        assert!(diffs.iter().any(|diff| diff.kind == "moved_item_aligned"
            && diff.expected_name.as_deref() == Some("Prayer")));
    }

    #[test]
    fn presentation_structure_summarizes_style_signatures() {
        let fixture = real_fixture_dir().join("heidleberg-chatechism-question-1.pro");
        let data = std::fs::read(fixture).expect("read fixture");
        let mut presentation =
            rv_data::Presentation::decode(data.as_slice()).expect("decode presentation");

        let summary = summarize_presentation_structure(&presentation);
        let title_cue = &summary.cues[0];
        assert_eq!(title_cue.macros, vec!["Name Tag/Title"]);
        let style = title_cue
            .text_styles
            .first()
            .expect("title cue should expose text style");
        assert_eq!(style.slide_size.as_deref(), Some("1920.0x1080.0"));
        assert!(style.bounds.is_some());
        assert!(style.font_size.is_some());
        assert!(style.color.is_some());

        mutate_first_text_font_size(&mut presentation, 12.0);
        let changed = summarize_presentation_structure(&presentation);
        assert_ne!(
            operator_cue_signatures(&summary),
            operator_cue_signatures(&changed),
            "style changes should be visible to parity comparison"
        );
    }

    #[test]
    fn presentation_structure_summarizes_scripture_labels_and_group_bindings() {
        let presentation = presentation_with_semantic_metadata();

        let summary = summarize_presentation_structure(&presentation);

        assert_eq!(
            summary.bible_reference,
            Some(BibleReferenceSummary {
                book_index: 42,
                book_name: "John".to_string(),
                chapter_range: Some(IntRangeSummary { start: 3, end: 3 }),
                verse_range: Some(IntRangeSummary { start: 16, end: 17 }),
                translation_name: "New Revised Standard Version Updated Edition".to_string(),
                translation_display_abbreviation: "NRSVue".to_string(),
                translation_internal_abbreviation: "NRSVUE".to_string(),
                book_key: "JHN".to_string(),
            })
        );
        assert_eq!(
            summary.cues[0].slide_labels,
            vec![ActionLabelSignature {
                text: "John 3:16-17".to_string(),
                color: Some("#FF0000FF".to_string()),
            }]
        );
        assert_eq!(summary.cue_groups[0].color.as_deref(), Some("#4080BFFF"));
        assert_eq!(
            summary.cue_groups[0].hot_key,
            Some(HotKeySignature {
                code: rv_data::KeyCode::AnsiV as i32,
                control_identifier: "verse".to_string(),
            })
        );
        assert_eq!(
            summary.cue_groups[0]
                .application_group_identifier
                .as_deref(),
            Some("APPLICATION-GROUP")
        );
        assert_eq!(summary.cue_groups[0].application_group_name, "Verse");
    }

    #[test]
    fn semantic_comparison_reports_scripture_label_and_group_binding_changes() {
        let expected = summarize_presentation_structure(&presentation_with_semantic_metadata());
        let mut changed_presentation = presentation_with_semantic_metadata();
        changed_presentation
            .bible_reference
            .as_mut()
            .expect("Bible reference")
            .verse_range
            .as_mut()
            .expect("verse range")
            .end = 18;
        changed_presentation.cues[0].actions[0]
            .label
            .as_mut()
            .expect("slide label")
            .text = "John 3:16-18".to_string();
        changed_presentation.cue_groups[0]
            .group
            .as_mut()
            .expect("cue group")
            .application_group_name = "Scripture".to_string();
        let actual = summarize_presentation_structure(&changed_presentation);
        let mut issues = Vec::new();

        compare_presentation_structure_summary("Scripture.pro", &expected, &actual, &mut issues);

        assert!(issues
            .iter()
            .any(|issue| { issue.kind == "embedded_presentation_bible_reference_mismatch" }));
        assert!(issues
            .iter()
            .any(|issue| issue.kind == "embedded_presentation_group_binding_mismatch"));
        assert!(issues
            .iter()
            .any(|issue| issue.kind == "embedded_presentation_operator_cue_mismatch"));
    }

    fn presentation_with_semantic_metadata() -> rv_data::Presentation {
        let cue_uuid = rv_data::Uuid {
            string: "CUE".to_string(),
        };
        rv_data::Presentation {
            name: "John 3:16-17".to_string(),
            bible_reference: Some(rv_data::presentation::BibleReference {
                book_index: 42,
                book_name: "John".to_string(),
                chapter_range: Some(rv_data::IntRange { start: 3, end: 3 }),
                verse_range: Some(rv_data::IntRange { start: 16, end: 17 }),
                translation_name: "New Revised Standard Version Updated Edition".to_string(),
                translation_display_abbreviation: "NRSVue".to_string(),
                translation_internal_abbreviation: "NRSVUE".to_string(),
                book_key: "JHN".to_string(),
            }),
            cues: vec![rv_data::Cue {
                uuid: Some(cue_uuid.clone()),
                actions: vec![rv_data::Action {
                    label: Some(action::Label {
                        text: "John 3:16-17".to_string(),
                        color: Some(rv_data::Color {
                            red: 1.0,
                            green: 0.0,
                            blue: 0.0,
                            alpha: 1.0,
                        }),
                    }),
                    action_type_data: Some(action::ActionTypeData::Slide(
                        action::SlideType::default(),
                    )),
                    ..rv_data::Action::default()
                }],
                ..rv_data::Cue::default()
            }],
            cue_groups: vec![rv_data::presentation::CueGroup {
                group: Some(rv_data::Group {
                    uuid: Some(rv_data::Uuid {
                        string: "GROUP".to_string(),
                    }),
                    name: "Verse".to_string(),
                    color: Some(rv_data::Color {
                        red: 0.25,
                        green: 0.5,
                        blue: 0.75,
                        alpha: 1.0,
                    }),
                    hot_key: Some(rv_data::HotKey {
                        code: rv_data::KeyCode::AnsiV as i32,
                        control_identifier: "verse".to_string(),
                    }),
                    application_group_identifier: Some(rv_data::Uuid {
                        string: "APPLICATION-GROUP".to_string(),
                    }),
                    application_group_name: "Verse".to_string(),
                }),
                cue_identifiers: vec![cue_uuid],
            }],
            ..rv_data::Presentation::default()
        }
    }

    fn mutate_first_text_font_size(presentation: &mut rv_data::Presentation, size: f64) {
        for cue in &mut presentation.cues {
            for action in &mut cue.actions {
                let Some(action::ActionTypeData::Slide(slide_type)) = &mut action.action_type_data
                else {
                    continue;
                };
                let Some(action::slide_type::Slide::Presentation(slide)) = &mut slide_type.slide
                else {
                    continue;
                };
                let Some(base_slide) = &mut slide.base_slide else {
                    continue;
                };
                for element in &mut base_slide.elements {
                    let Some(graphics) = &mut element.element else {
                        continue;
                    };
                    let Some(text) = &mut graphics.text else {
                        continue;
                    };
                    let attributes = text.attributes.get_or_insert_with(Default::default);
                    let font = attributes.font.get_or_insert_with(Default::default);
                    font.size = size;
                    return;
                }
            }
        }
        panic!("fixture should contain at least one text element");
    }

    #[test]
    fn reads_generated_playlist_package() {
        let dir = tempdir().expect("tempdir");
        let output_path = dir.path().join("service.proplaylist");
        let entries = vec![PlaylistEntry {
            name: "Call to Worship".to_string(),
            slide_type: SlideType::Text,
            from_matched_file: true,
            presentation_path:
                "/Users/jimmy/Documents/ProPresenter/Libraries/Default/Call to Worship.pro"
                    .to_string(),
            selected_arrangement: None,
            user_music_key: None,
            embedded_data: Some(presentation_bytes("Call to Worship")),
        }];

        let document = build_playlist("Service", &entries, &test_metadata());
        write_playlist_file(&document, &entries, &output_path).expect("write playlist");

        let package = read_playlist_package(&output_path).expect("read package");
        assert_eq!(
            infer_package_mode(&package),
            PlaylistPackageMode::LibraryLocal
        );
        assert_eq!(package.embedded_files, vec!["Call to Worship.pro"]);
        assert_eq!(package.embedded_file_details.len(), 1);
        assert_eq!(
            package.embedded_file_details[0].basename,
            "Call to Worship.pro"
        );
        assert!(package.embedded_file_details[0].is_presentation);

        let items = presentation_items(&package.document);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "Call to Worship");
        assert_eq!(
            items[0].local_relative_path.as_deref(),
            Some("Libraries/Default/Call to Worship.pro")
        );
    }

    #[test]
    fn reads_native_unflagged_utf8_member_names_without_mojibake() {
        let directory = tempdir().expect("tempdir");
        let output_path = directory.path().join("unicode.proplaylist");
        let entries = vec![PlaylistEntry {
            name: "O Praise The Name (Anástasis)".to_string(),
            slide_type: SlideType::Lyrics,
            from_matched_file: true,
            presentation_path: "/Libraries/Default/O Praise The Name (Anástasis).pro".to_string(),
            selected_arrangement: None,
            user_music_key: None,
            embedded_data: Some(presentation_bytes("O Praise The Name (Anástasis)")),
        }];
        let document = build_playlist("Service", &entries, &test_metadata());
        write_playlist_file(&document, &entries, &output_path).expect("write playlist");

        let package = read_playlist_package(output_path).expect("read playlist");

        assert_eq!(
            package.embedded_files,
            ["O Praise The Name (Anástasis).pro"]
        );
        assert!(package
            .embedded_file_data
            .contains_key("O Praise The Name (Anástasis).pro"));
    }

    #[test]
    fn rejects_package_with_malformed_embedded_presentation() {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("malformed.proplaylist");
        let file = File::create(&path).expect("create package");
        let mut archive = zip::ZipWriter::new(file);
        let options = zip::write::FileOptions::default();
        archive
            .start_file("Broken.pro", options)
            .expect("start presentation");
        archive.write_all(&[1, 2, 3]).expect("write malformed");
        archive.start_file("data", options).expect("start data");
        archive
            .write_all(&build_playlist("Empty", &[], &test_metadata()).encode_to_vec())
            .expect("write data");
        archive.finish().expect("finish package");

        let result = read_playlist_package(path);

        assert!(matches!(
            result,
            Err(PackageError::InvalidEmbeddedPresentation { name, .. }) if name == "Broken.pro"
        ));
    }

    #[test]
    fn reads_checked_in_propresenter_fixture() {
        let fixture =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/test.proplaylist");

        let package = read_playlist_package(fixture).expect("read fixture package");
        assert!(package.document_round_trip_exact);
        assert_eq!(
            infer_package_mode(&package),
            PlaylistPackageMode::LibraryLocal
        );
        assert_eq!(
            package.embedded_files,
            vec![
                "__template_info__.pro",
                "__template_scripture__.pro",
                "__template_song__.pro"
            ]
        );
        assert_eq!(
            package
                .embedded_file_details
                .iter()
                .map(|file| (file.basename.as_str(), file.size, file.crc32))
                .collect::<Vec<_>>(),
            vec![
                ("__template_info__.pro", 1731, 0x0232_052d),
                ("__template_scripture__.pro", 1354, 0xc8a6_509b),
                ("__template_song__.pro", 1705, 0x8040_52a0),
            ]
        );

        let items = presentation_items(&package.document);
        assert_eq!(items.len(), 3);
        assert_eq!(
            items[0].local_relative_path.as_deref(),
            Some("Libraries/Default/__template_scripture__.pro")
        );
        assert!(items[0]
            .absolute_string
            .as_deref()
            .is_some_and(|path| path.starts_with("file:///Users/jimmy/")));
    }

    #[test]
    fn real_fixture_manifest_matches_corpus() {
        let fixture_dir = real_fixture_dir();
        let manifest = real_manifest();

        for fixture in manifest.playlists {
            if fixture.independent_native_export {
                assert_eq!(fixture.provenance, "independent_native_export");
            } else {
                assert_eq!(
                    fixture.provenance,
                    "proflow_reconstruction_from_live_library"
                );
            }
            let path = fixture_dir.join(&fixture.path);
            let package = read_playlist_package(&path).expect("read real playlist fixture");
            let items = presentation_items(&package.document);

            assert!(
                package.document_round_trip_exact,
                "{} playlist data should round-trip byte-for-byte",
                fixture.path
            );

            assert_eq!(
                infer_package_mode(&package),
                fixture.mode,
                "{}",
                fixture.path
            );
            assert_eq!(items.len(), fixture.item_count, "{}", fixture.path);
            assert_eq!(
                package.embedded_file_details.len(),
                fixture.embedded_file_count,
                "{}",
                fixture.path
            );
            for required in &fixture.required_embedded_files {
                assert!(
                    package.embedded_files.contains(required),
                    "{} should contain {required}",
                    fixture.path
                );
            }
            assert_eq!(
                embedded_presentation_summaries(&package).len(),
                fixture.embedded_file_count,
                "{}",
                fixture.path
            );
        }

        for fixture in manifest.presentations {
            assert_eq!(fixture.provenance, "native_library_file");
            assert!(fixture.independent_native_export);
            let data = std::fs::read(fixture_dir.join(&fixture.path)).expect("read presentation");
            let presentation = rv_data::Presentation::decode(data.as_slice())
                .expect("decode real presentation fixture");
            let media_dependencies = presentation_media_dependencies(&presentation);

            assert_eq!(presentation.name, fixture.name, "{}", fixture.path);
            assert_eq!(
                presentation.cues.len(),
                fixture.cue_count,
                "{}",
                fixture.path
            );
            assert_eq!(
                presentation.cue_groups.len(),
                fixture.cue_group_count,
                "{}",
                fixture.path
            );
            assert_eq!(
                presentation.arrangements.len(),
                fixture.arrangement_count,
                "{}",
                fixture.path
            );
            assert_eq!(
                media_dependencies.len(),
                fixture.media_dependency_count,
                "{}",
                fixture.path
            );
        }
    }

    #[test]
    fn compare_identical_package_is_compatible() {
        let fixture =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/test.proplaylist");

        let comparison =
            compare_playlist_packages(&fixture, &fixture).expect("compare identical fixture");

        assert!(comparison.compatible);
        assert!(comparison.issues.is_empty());
        assert_eq!(comparison.expected_item_count, 3);
        assert_eq!(comparison.actual_item_count, 3);
    }

    #[test]
    fn native_package_reconstruction_matches_evidenced_shape() {
        let expected_path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/test.proplaylist");
        let expected = read_playlist_package(&expected_path).expect("read native package");
        let entries = presentation_items(&expected.document)
            .into_iter()
            .map(|item| {
                let relative_path = item
                    .local_relative_path
                    .as_deref()
                    .expect("native item local path");
                let filename = Path::new(relative_path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .expect("native item filename");
                PlaylistEntry {
                    name: item.name,
                    slide_type: SlideType::Text,
                    from_matched_file: true,
                    presentation_path: format!("/Users/test/ProPresenter/{relative_path}"),
                    selected_arrangement: item.arrangement_uuid.as_deref().map(|uuid| {
                        SelectedArrangement::new(
                            Uuid::parse_str(uuid).expect("valid arrangement UUID"),
                            item.arrangement_name.clone(),
                        )
                        .expect("complete arrangement metadata")
                    }),
                    user_music_key: item.user_music_key.map(|(music_key, music_scale)| {
                        rv_data::MusicKeyScale {
                            music_key,
                            music_scale,
                        }
                    }),
                    embedded_data: expected.embedded_file_data.get(filename).cloned(),
                }
            })
            .collect::<Vec<_>>();
        let metadata =
            PlaylistMetadata::from_document(&expected.document).expect("native playlist metadata");
        let reconstructed = build_playlist("test", &entries, &metadata);
        let directory = tempdir().expect("tempdir");
        let actual_path = directory.path().join("reconstructed.proplaylist");
        write_playlist_file(&reconstructed, &entries, &actual_path).expect("write reconstruction");

        let comparison =
            compare_playlist_packages(&expected_path, &actual_path).expect("compare packages");

        assert!(comparison.compatible, "{:#?}", comparison.issues);
        assert!(comparison.issues.is_empty());
    }

    #[test]
    fn compare_detects_complete_presentation_item_metadata() {
        let directory = tempdir().expect("tempdir");
        let expected_path = directory.path().join("expected.proplaylist");
        let actual_path = directory.path().join("actual.proplaylist");
        let entries = vec![PlaylistEntry {
            name: "Song".to_string(),
            slide_type: SlideType::Lyrics,
            from_matched_file: true,
            presentation_path: "/Libraries/Default/Song.pro".to_string(),
            selected_arrangement: None,
            user_music_key: None,
            embedded_data: Some(presentation_bytes("Song")),
        }];
        let expected = build_playlist("Service", &entries, &test_metadata());
        let mut actual = expected.clone();
        let root = actual.root_node.as_mut().expect("root");
        let Some(playlist::ChildrenType::Playlists(playlists)) = &mut root.children_type else {
            panic!("playlist children");
        };
        let Some(playlist::ChildrenType::Items(items)) = &mut playlists.playlists[0].children_type
        else {
            panic!("playlist items");
        };
        let Some(playlist_item::ItemType::Presentation(presentation)) =
            &mut items.items[0].item_type
        else {
            panic!("presentation item");
        };
        let user_music_key = rv_data::MusicKeyScale {
            music_key: rv_data::music_key_scale::MusicKey::C as i32,
            music_scale: rv_data::music_key_scale::MusicScale::Major as i32,
        };
        presentation.user_music_key = Some(user_music_key.clone());
        let mut actual_entries = entries.clone();
        actual_entries[0].user_music_key = Some(user_music_key);

        write_playlist_file(&expected, &entries, &expected_path).expect("write expected");
        write_playlist_file(&actual, &actual_entries, &actual_path).expect("write actual");
        let comparison =
            compare_playlist_packages(&expected_path, &actual_path).expect("compare packages");

        assert!(!comparison.compatible);
        assert!(comparison
            .issues
            .iter()
            .any(|issue| issue.kind == "playlist_root_mismatch"));
        assert!(comparison
            .issues
            .iter()
            .any(|issue| issue.kind == "item_music_key_mismatch"));
    }

    #[test]
    fn native_playlist_reconstruction_is_not_mislabeled_compatible() {
        let fixture_dir = real_fixture_dir();
        for fixture in real_manifest()
            .playlists
            .into_iter()
            .filter(|fixture| !fixture.independent_native_export)
        {
            let expected_path = fixture_dir.join(&fixture.path);
            let expected = read_playlist_package(&expected_path).expect("read real fixture");
            let items = presentation_items(&expected.document);
            let presentation_files: Vec<_> = expected
                .embedded_file_details
                .iter()
                .filter(|file| file.is_presentation)
                .collect();
            assert_eq!(items.len(), presentation_files.len(), "{}", fixture.path);

            let entries: Vec<_> = items
                .iter()
                .zip(presentation_files.iter())
                .map(|(item, file)| PlaylistEntry {
                    name: item.name.clone(),
                    slide_type: SlideType::Text,
                    from_matched_file: true,
                    presentation_path: item
                        .local_relative_path
                        .as_ref()
                        .map(|path| format!("/Users/jimmy/Documents/ProPresenter/{path}"))
                        .or_else(|| item.absolute_string.clone())
                        .unwrap_or_default(),
                    // These legacy reconstructed fixtures can contain the old
                    // UUID-without-name bug. The typed entry deliberately
                    // cannot restate that partial metadata, and this test
                    // already requires the reconstruction to compare unequal.
                    selected_arrangement: item
                        .arrangement_uuid
                        .as_deref()
                        .filter(|_| !item.arrangement_name.trim().is_empty())
                        .map(|uuid| {
                            SelectedArrangement::new(
                                Uuid::parse_str(uuid).expect("valid arrangement UUID"),
                                item.arrangement_name.clone(),
                            )
                            .expect("complete arrangement metadata")
                        }),
                    user_music_key: item.user_music_key.map(|(music_key, music_scale)| {
                        rv_data::MusicKeyScale {
                            music_key,
                            music_scale,
                        }
                    }),
                    embedded_data: expected.embedded_file_data.get(&file.name).cloned(),
                })
                .collect();

            let metadata = PlaylistMetadata::from_document(&expected.document)
                .expect("fixture playlist metadata");
            let playlist = build_playlist("Round Trip", &entries, &metadata);
            let dir = tempdir().expect("tempdir");
            let actual_path = dir.path().join(&fixture.path);
            write_playlist_file(&playlist, &entries, &actual_path).expect("write round trip");

            let comparison =
                compare_playlist_packages(&expected_path, &actual_path).expect("compare packages");
            assert!(!comparison.compatible, "{}", fixture.path);
            assert!(comparison
                .issues
                .iter()
                .any(|issue| issue.kind == "playlist_root_mismatch"));
        }
    }

    #[test]
    fn compare_reports_embedded_presentation_crc_mismatch() {
        let dir = tempdir().expect("tempdir");
        let expected_path = dir.path().join("expected.proplaylist");
        let actual_path = dir.path().join("actual.proplaylist");
        let mut entries = vec![PlaylistEntry {
            name: "Call to Worship".to_string(),
            slide_type: SlideType::Text,
            from_matched_file: true,
            presentation_path:
                "/Users/jimmy/Documents/ProPresenter/Libraries/Default/Call to Worship.pro"
                    .to_string(),
            selected_arrangement: None,
            user_music_key: None,
            embedded_data: Some(presentation_bytes("Expected")),
        }];

        let document = build_playlist("Service", &entries, &test_metadata());
        write_playlist_file(&document, &entries, &expected_path).expect("write expected");
        entries[0].embedded_data = Some(presentation_bytes("Actual"));
        let document = build_playlist("Service", &entries, &test_metadata());
        write_playlist_file(&document, &entries, &actual_path).expect("write actual");

        let comparison =
            compare_playlist_packages(&expected_path, &actual_path).expect("compare packages");

        assert!(!comparison.compatible);
        assert!(comparison
            .issues
            .iter()
            .any(|issue| issue.kind == "embedded_presentation_crc_mismatch"));
    }

    #[test]
    fn compare_reports_media_content_mismatch_at_same_archive_path() {
        let dir = tempdir().expect("tempdir");
        let expected_media_dir = dir.path().join("expected-media");
        let actual_media_dir = dir.path().join("actual-media");
        std::fs::create_dir_all(&expected_media_dir).expect("create expected media directory");
        std::fs::create_dir_all(&actual_media_dir).expect("create actual media directory");
        let expected_media = expected_media_dir.join("default.jpg");
        let actual_media = actual_media_dir.join("default.jpg");
        std::fs::write(&expected_media, [1, 2, 3]).expect("write expected media");
        std::fs::write(&actual_media, [1, 2, 4]).expect("write actual media");

        let document = build_playlist("Service", &[], &test_metadata());
        let expected_path = dir.path().join("expected.proplaylist");
        let actual_path = dir.path().join("actual.proplaylist");
        let options_for = |source_path| PlaylistWriteOptions {
            package_mode: PlaylistPackageMode::ExportPortable,
            media_assets: vec![PlaylistMediaAsset {
                source_path,
                archive_path: Some("media/default.jpg".to_string()),
            }],
            include_discovered_media_assets: false,
        };
        write_playlist_file_with_options(
            &document,
            &[],
            &expected_path,
            &options_for(expected_media),
        )
        .expect("write expected package");
        write_playlist_file_with_options(&document, &[], &actual_path, &options_for(actual_media))
            .expect("write actual package");

        let comparison =
            compare_playlist_packages(&expected_path, &actual_path).expect("compare packages");

        assert!(!comparison.compatible);
        assert!(comparison
            .issues
            .iter()
            .any(|issue| issue.kind == "media_asset_fingerprint_mismatch"));
    }
}
