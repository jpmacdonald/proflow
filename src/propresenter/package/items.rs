use std::collections::{BTreeMap, VecDeque};

use super::model::{PlaylistItemAlignedDiff, PlaylistItemSummary};
use crate::propresenter::generated::rv_data::{self, playlist, playlist_item, url};
use crate::propresenter::inspection::percent_decode_lossy;

struct AlignedPlaylistItem<'a> {
    key: String,
    expected_index: usize,
    actual_index: usize,
    expected: &'a PlaylistItemSummary,
    actual: &'a PlaylistItemSummary,
}

/// Return a compact summary of every presentation item in document order.
#[must_use]
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

/// Normalize an absolute presentation path for semantic comparison.
#[must_use]
pub(super) fn normalize_absolute_path_value(value: &str) -> String {
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
