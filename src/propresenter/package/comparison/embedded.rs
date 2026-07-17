use std::collections::{BTreeMap, BTreeSet};

use super::super::model::{
    EmbeddedPresentationStructure, EmbeddedPresentationSummary, PlaylistPackage,
    PlaylistPackageIssue,
};
use super::super::presentation::{
    embedded_presentation_structures, embedded_presentation_summaries,
};
use crate::propresenter::inspection::{
    ActionLabelSignature, PresentationStructureSummary, TextStyleSignature,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct FileFingerprint {
    size: u64,
    crc32: u32,
}

pub(super) fn compare_embedded_presentations(
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

    for archive_path in names {
        match (
            expected_files.get(&archive_path),
            actual_files.get(&archive_path),
        ) {
            (Some(expected), Some(actual)) if expected == actual => {}
            (Some(expected), Some(actual)) if expected.len() != actual.len() => {
                issues.push(PlaylistPackageIssue {
                    kind: "embedded_presentation_count_mismatch".to_string(),
                    index: None,
                    message: format!(
                        "presentation archive member '{archive_path}' appears {} time(s), found {}",
                        expected.len(),
                        actual.len()
                    ),
                });
            }
            (Some(expected), Some(actual)) => issues.push(PlaylistPackageIssue {
                kind: "embedded_presentation_crc_mismatch".to_string(),
                index: None,
                message: format!(
                    "presentation archive member '{archive_path}' fingerprints differ: expected {expected:?}, found {actual:?}"
                ),
            }),
            (Some(_), None) => issues.push(PlaylistPackageIssue {
                kind: "missing_embedded_presentation".to_string(),
                index: None,
                message: format!("missing embedded presentation '{archive_path}'"),
            }),
            (None, Some(_)) => issues.push(PlaylistPackageIssue {
                kind: "extra_embedded_presentation".to_string(),
                index: None,
                message: format!("extra embedded presentation '{archive_path}'"),
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

    for archive_path in names {
        let (Some(expected), Some(actual)) = (
            expected_summaries.get(&archive_path),
            actual_summaries.get(&archive_path),
        ) else {
            continue;
        };
        for index in 0..expected.len().min(actual.len()) {
            compare_embedded_presentation_summary(
                &archive_path,
                &expected[index],
                &actual[index],
                issues,
            );
        }
    }
}

pub(super) fn compare_embedded_presentation_structures(
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

    for archive_path in names {
        let (Some(expected), Some(actual)) = (
            expected_structures.get(&archive_path),
            actual_structures.get(&archive_path),
        ) else {
            continue;
        };
        for index in 0..expected.len().min(actual.len()) {
            compare_presentation_structure_summary(
                &archive_path,
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
            .entry(summary.archive_path.clone())
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
            .entry(structure.archive_path.clone())
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
pub fn compare_presentation_structure_summary(
    archive_path: &str,
    expected: &PresentationStructureSummary,
    actual: &PresentationStructureSummary,
    issues: &mut Vec<PlaylistPackageIssue>,
) {
    if expected.name != actual.name {
        issues.push(PlaylistPackageIssue {
            kind: "embedded_presentation_name_mismatch".to_string(),
            index: None,
            message: format!(
                "presentation '{archive_path}' expected name {:?}, found {:?}",
                expected.name, actual.name
            ),
        });
    }

    if expected.bible_reference != actual.bible_reference {
        issues.push(PlaylistPackageIssue {
            kind: "embedded_presentation_bible_reference_mismatch".to_string(),
            index: None,
            message: format!(
                "presentation '{archive_path}' expected Bible reference {:?}, found {:?}",
                expected.bible_reference, actual.bible_reference
            ),
        });
    }

    if expected.reference_diagnostics != actual.reference_diagnostics {
        issues.push(PlaylistPackageIssue {
            kind: "embedded_presentation_reference_diagnostics_mismatch".to_string(),
            index: None,
            message: format!(
                "presentation '{archive_path}' expected reference diagnostics {:?}, found {:?}",
                expected.reference_diagnostics, actual.reference_diagnostics
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
                "presentation '{archive_path}' expected cue groups {expected_groups:?}, found {actual_groups:?}"
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
                "presentation '{archive_path}' expected group bindings {expected_group_bindings:?}, found {actual_group_bindings:?}"
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
                "presentation '{archive_path}' expected arrangements {expected_arrangements:?}, found {actual_arrangements:?}"
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
                "presentation '{archive_path}' expected {} operator cues, found {}",
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
                    "presentation '{archive_path}' operator cue {index} expected {:?}, found {:?}",
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
    archive_path: &str,
    expected: &EmbeddedPresentationSummary,
    actual: &EmbeddedPresentationSummary,
    issues: &mut Vec<PlaylistPackageIssue>,
) {
    if expected.presentation_uuid != actual.presentation_uuid {
        issues.push(PlaylistPackageIssue {
            kind: "embedded_presentation_uuid_mismatch".to_string(),
            index: None,
            message: format!(
                "presentation '{archive_path}' expected UUID {:?}, found {:?}",
                expected.presentation_uuid, actual.presentation_uuid
            ),
        });
    }

    if expected.cue_count != actual.cue_count {
        issues.push(PlaylistPackageIssue {
            kind: "embedded_presentation_cue_count_mismatch".to_string(),
            index: None,
            message: format!(
                "presentation '{archive_path}' expected {} cues, found {}",
                expected.cue_count, actual.cue_count
            ),
        });
    }

    if expected.cue_group_count != actual.cue_group_count {
        issues.push(PlaylistPackageIssue {
            kind: "embedded_presentation_group_count_mismatch".to_string(),
            index: None,
            message: format!(
                "presentation '{archive_path}' expected {} cue groups, found {}",
                expected.cue_group_count, actual.cue_group_count
            ),
        });
    }

    if expected.arrangement_names != actual.arrangement_names {
        issues.push(PlaylistPackageIssue {
            kind: "embedded_presentation_arrangement_names_mismatch".to_string(),
            index: None,
            message: format!(
                "presentation '{archive_path}' expected arrangements {:?}, found {:?}",
                expected.arrangement_names, actual.arrangement_names
            ),
        });
    }
}

pub(super) fn compare_media_assets(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::propresenter::generated::rv_data;
    use crate::propresenter::package::PackageFileSummary;
    use prost::Message;

    fn package(files: &[(&str, &str, u32)]) -> PlaylistPackage {
        let mut embedded_file_details = Vec::new();
        let mut embedded_file_data = BTreeMap::new();
        for (archive_path, notes, crc32) in files {
            let data = rv_data::Presentation {
                name: "Same semantic summary".to_string(),
                notes: (*notes).to_string(),
                ..rv_data::Presentation::default()
            }
            .encode_to_vec();
            embedded_file_details.push(PackageFileSummary {
                name: (*archive_path).to_string(),
                basename: "Shared.pro".to_string(),
                size: data.len() as u64,
                crc32: *crc32,
                is_presentation: true,
                compression_method: "Stored".to_string(),
                is_directory: false,
                version_made_by: (0, 0),
                unix_mode: None,
                extra_field_ids: Vec::new(),
                comment: String::new(),
            });
            embedded_file_data.insert((*archive_path).to_string(), data);
        }
        PlaylistPackage {
            document: rv_data::PlaylistDocument::default(),
            document_data: Vec::new(),
            document_round_trip_exact: true,
            embedded_files: files
                .iter()
                .map(|(archive_path, _, _)| (*archive_path).to_string())
                .collect(),
            embedded_file_details,
            embedded_file_data,
            archive_entries: Vec::new(),
            archive_comment: Vec::new(),
        }
    }

    #[test]
    fn same_basename_presentations_are_fingerprinted_by_full_archive_path() {
        let expected = package(&[
            ("first/Shared.pro", "A", 0x1111_1111),
            ("second/Shared.pro", "B", 0x2222_2222),
        ]);
        let actual = package(&[
            ("first/Shared.pro", "B", 0x2222_2222),
            ("second/Shared.pro", "A", 0x1111_1111),
        ]);
        let mut issues = Vec::new();

        compare_embedded_presentations(&expected, &actual, &mut issues);

        assert_eq!(
            issues
                .iter()
                .filter(|issue| issue.kind == "embedded_presentation_crc_mismatch")
                .count(),
            2
        );
        assert!(issues.iter().any(|issue| {
            issue.message.contains("first/Shared.pro")
                && !issue.message.contains("second/Shared.pro")
        }));
    }
}
