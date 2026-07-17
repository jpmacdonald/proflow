use super::super::items::normalize_absolute_path_value;
use super::super::model::{PlaylistItemSummary, PlaylistPackageIssue};

pub(super) fn compare_items(
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
