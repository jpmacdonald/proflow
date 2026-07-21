use std::collections::BTreeSet;

use super::super::model::{PackageFileSummary, PlaylistPackage, PlaylistPackageIssue};
use super::infer_archive_shape;

pub(super) fn compare_inferred_archive_shapes(
    expected: &PlaylistPackage,
    actual: &PlaylistPackage,
    issues: &mut Vec<PlaylistPackageIssue>,
) {
    let expected_shape = infer_archive_shape(expected);
    let actual_shape = infer_archive_shape(actual);
    if expected_shape != actual_shape {
        issues.push(PlaylistPackageIssue {
            kind: "archive_shape_mismatch".to_string(),
            index: None,
            message: format!("expected {expected_shape:?}, found {actual_shape:?}"),
        });
    }
}

pub(super) fn compare_archive_shape(
    expected: &PlaylistPackage,
    actual: &PlaylistPackage,
    issues: &mut Vec<PlaylistPackageIssue>,
) {
    let expected_entries = expected.archive_entries();
    let actual_entries = actual.archive_entries();
    if expected_entries.len() != actual_entries.len() {
        issues.push(PlaylistPackageIssue {
            kind: "archive_entry_count_mismatch".to_string(),
            index: None,
            message: format!(
                "expected {} archive entries, found {}",
                expected_entries.len(),
                actual_entries.len()
            ),
        });
    }

    let expected_paths = expected_entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect::<Vec<_>>();
    let actual_paths = actual_entries
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

    for index in 0..expected_entries.len().min(actual_entries.len()) {
        let expected_entry = &expected_entries[index];
        let actual_entry = &actual_entries[index];
        if expected_entry.name == actual_entry.name {
            compare_archive_entry_metadata(index, expected_entry, actual_entry, issues);
        }
    }

    if expected.archive_comment() != actual.archive_comment() {
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

pub(super) fn compare_playlist_schema_coverage(
    expected: &PlaylistPackage,
    actual: &PlaylistPackage,
    issues: &mut Vec<PlaylistPackageIssue>,
) {
    if !expected.document_round_trip_is_exact() {
        issues.push(PlaylistPackageIssue {
            kind: "expected_playlist_schema_round_trip_loss".to_string(),
            index: None,
            message: "reference playlist data is not byte-exact after decode and encode; the protobuf schema may be incomplete".to_string(),
        });
    }
    if !actual.document_round_trip_is_exact() {
        issues.push(PlaylistPackageIssue {
            kind: "actual_playlist_schema_round_trip_loss".to_string(),
            index: None,
            message: "candidate playlist data is not byte-exact after decode and encode; the protobuf schema may be incomplete".to_string(),
        });
    }
}
