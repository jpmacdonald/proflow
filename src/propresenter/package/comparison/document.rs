use std::collections::BTreeMap;

use super::super::items::normalize_absolute_path_value;
use super::super::model::PlaylistPackageIssue;
use crate::propresenter::generated::rv_data::{self, playlist, playlist_item, url};

/// Compare the complete decoded playlist document after applying the only
/// permitted semantic normalizations:
///
/// - valid, globally unique playlist-container and playlist-item identity UUID
///   values are volatile;
/// - UUID letter case is not semantic;
/// - machine-specific absolute prefixes before `Libraries/` are ignored.
///
/// Application versions, root and child names/types/expanded state, tags,
/// links, item fields, and item order are not volatile.
pub(super) fn compare_playlist_documents(
    expected: &rv_data::PlaylistDocument,
    actual: &rv_data::PlaylistDocument,
    issues: &mut Vec<PlaylistPackageIssue>,
) {
    let expected = normalized_playlist_document(expected, "expected", issues);
    let actual = normalized_playlist_document(actual, "actual", issues);

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

fn normalized_playlist_document(
    document: &rv_data::PlaylistDocument,
    side: &'static str,
    issues: &mut Vec<PlaylistPackageIssue>,
) -> rv_data::PlaylistDocument {
    let mut normalized = document.clone();
    let mut identities = IdentityNormalizer::new(side, issues);
    for tag in &mut normalized.tags {
        normalize_uuid_case(tag.uuid.as_mut());
    }
    if let Some(root) = &mut normalized.root_node {
        identities.normalize_playlist_node(root, "root_node");
    }
    if let Some(live_video) = &mut normalized.live_video_playlist {
        identities.normalize_playlist_node(live_video, "live_video_playlist");
    }
    if let Some(downloads) = &mut normalized.downloads_playlist {
        identities.normalize_playlist_node(downloads, "downloads_playlist");
    }
    normalized
}

struct IdentityNormalizer<'a> {
    side: &'static str,
    identities: BTreeMap<String, FirstIdentity>,
    issues: &'a mut Vec<PlaylistPackageIssue>,
}

struct FirstIdentity {
    normalized_index: usize,
    path: String,
}

impl<'a> IdentityNormalizer<'a> {
    const fn new(side: &'static str, issues: &'a mut Vec<PlaylistPackageIssue>) -> Self {
        Self {
            side,
            identities: BTreeMap::new(),
            issues,
        }
    }

    fn normalize_playlist_node(&mut self, node: &mut rv_data::Playlist, path: &str) {
        self.normalize_identity(node.uuid.as_mut(), &format!("{path}.uuid"));
        normalize_uuid_case(node.targeted_layer_uuid.as_mut());
        if let Some(document_path) = &mut node.smart_directory_path {
            normalize_document_url(document_path);
        }
        for (index, child) in node.children.iter_mut().enumerate() {
            self.normalize_playlist_node(child, &format!("{path}.children[{index}]"));
        }
        match &mut node.children_type {
            Some(playlist::ChildrenType::Playlists(playlists)) => {
                for (index, child) in playlists.playlists.iter_mut().enumerate() {
                    self.normalize_playlist_node(
                        child,
                        &format!("{path}.children_type.playlists[{index}]"),
                    );
                }
            }
            Some(playlist::ChildrenType::Items(items)) => {
                for (index, item) in items.items.iter_mut().enumerate() {
                    self.normalize_playlist_item(
                        item,
                        &format!("{path}.children_type.items[{index}]"),
                    );
                }
            }
            None => {}
        }
    }

    fn normalize_playlist_item(&mut self, item: &mut rv_data::PlaylistItem, path: &str) {
        self.normalize_identity(item.uuid.as_mut(), &format!("{path}.uuid"));
        for tag in &mut item.tags {
            normalize_uuid_case(Some(tag));
        }
        match &mut item.item_type {
            Some(playlist_item::ItemType::Presentation(presentation)) => {
                if let Some(document_path) = &mut presentation.document_path {
                    normalize_document_url(document_path);
                }
                normalize_uuid_case(presentation.arrangement.as_mut());
            }
            Some(playlist_item::ItemType::PlanningCenter(planning_center)) => {
                if let Some(linked_data) = &mut planning_center.linked_data {
                    self.normalize_playlist_item(
                        linked_data,
                        &format!("{path}.planning_center.linked_data"),
                    );
                }
            }
            Some(playlist_item::ItemType::Placeholder(placeholder)) => {
                if let Some(linked_data) = &mut placeholder.linked_data {
                    self.normalize_playlist_item(
                        linked_data,
                        &format!("{path}.placeholder.linked_data"),
                    );
                }
            }
            _ => {}
        }
    }

    fn normalize_identity(&mut self, uuid: Option<&mut rv_data::Uuid>, path: &str) {
        let Some(uuid) = uuid else {
            self.push_issue(
                "missing",
                format!("{} playlist identity at {path} is missing", self.side),
            );
            return;
        };

        let key = match uuid::Uuid::parse_str(&uuid.string) {
            Ok(parsed) => parsed.to_string(),
            Err(error) => {
                self.push_issue(
                    "invalid",
                    format!(
                        "{} playlist identity at {path} is not a UUID ({:?}): {error}",
                        self.side, uuid.string
                    ),
                );
                format!("<invalid:{}>", uuid.string.to_ascii_lowercase())
            }
        };

        let first = self
            .identities
            .get(&key)
            .map(|identity| (identity.normalized_index, identity.path.clone()));
        let normalized_index = if let Some((index, first_path)) = first {
            self.push_issue(
                "duplicate",
                format!(
                    "{} playlist identity {:?} at {path} duplicates {first_path}",
                    self.side, uuid.string
                ),
            );
            index
        } else {
            let index = self.identities.len();
            self.identities.insert(
                key,
                FirstIdentity {
                    normalized_index: index,
                    path: path.to_string(),
                },
            );
            index
        };
        uuid.string = format!("<volatile-identity-{normalized_index}>");
    }

    fn push_issue(&mut self, suffix: &str, message: String) {
        self.issues.push(PlaylistPackageIssue {
            kind: format!("{}_playlist_identity_uuid_{suffix}", self.side),
            index: None,
            message,
        });
    }
}

fn normalize_document_url(document_path: &mut rv_data::Url) {
    if let Some(url::Storage::AbsoluteString(value)) = &mut document_path.storage {
        *value = normalize_absolute_path_value(value);
    }
}

fn normalize_uuid_case(uuid: Option<&mut rv_data::Uuid>) {
    if let Some(uuid) = uuid {
        uuid.string.make_ascii_lowercase();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIRST_UUID: &str = "6f14f866-67aa-4c54-b46f-693e55420f96";
    const SECOND_UUID: &str = "36fbe956-99f3-4fa0-8476-8401a8d11b08";
    const THIRD_UUID: &str = "ad111534-6bf4-40fe-bf83-3b7743ceced6";

    fn document(root_uuid: &str, child_uuid: Option<&str>) -> rv_data::PlaylistDocument {
        rv_data::PlaylistDocument {
            root_node: Some(rv_data::Playlist {
                uuid: Some(rv_data::Uuid {
                    string: root_uuid.to_string(),
                }),
                children: child_uuid
                    .map(|uuid| rv_data::Playlist {
                        uuid: Some(rv_data::Uuid {
                            string: uuid.to_string(),
                        }),
                        ..rv_data::Playlist::default()
                    })
                    .into_iter()
                    .collect(),
                ..rv_data::Playlist::default()
            }),
            ..rv_data::PlaylistDocument::default()
        }
    }

    #[test]
    fn regenerated_unique_identity_uuids_compare_equal() {
        let expected = document(FIRST_UUID, Some(SECOND_UUID));
        let actual = document(SECOND_UUID, Some(THIRD_UUID));
        let mut issues = Vec::new();

        compare_playlist_documents(&expected, &actual, &mut issues);

        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn malformed_identity_does_not_normalize_equal_to_valid_identity() {
        let expected = document("not-a-uuid", None);
        let actual = document(FIRST_UUID, None);
        let mut issues = Vec::new();

        compare_playlist_documents(&expected, &actual, &mut issues);

        assert!(issues
            .iter()
            .any(|issue| issue.kind == "expected_playlist_identity_uuid_invalid"));
    }

    #[test]
    fn duplicate_identity_does_not_normalize_equal_to_unique_identities() {
        let expected = document(FIRST_UUID, Some(FIRST_UUID));
        let actual = document(SECOND_UUID, Some(THIRD_UUID));
        let mut issues = Vec::new();

        compare_playlist_documents(&expected, &actual, &mut issues);

        assert!(issues
            .iter()
            .any(|issue| issue.kind == "expected_playlist_identity_uuid_duplicate"));
        assert!(issues
            .iter()
            .any(|issue| issue.kind == "playlist_root_mismatch"));
    }
}
