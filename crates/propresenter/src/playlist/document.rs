//! A playlist and its items are identified by what they are: a playlist by
//! its name, an item by its playlist, its position and the presentation it
//! plays. Building the same playlist again therefore produces the same
//! document, not a second playlist `ProPresenter` treats as unrelated.

use super::domain::{PlaylistEntry, PlaylistMetadata, PlaylistSet};
use super::naming::document_path_for_presentation_path;
use crate::generated::rv_data::{self, playlist, playlist_document, playlist_item};
use crate::identity::derived_uuid;

/// Build a native document containing one named child playlist.
#[cfg(any(test, feature = "dev-tools"))]
pub fn build_playlist(
    name: &str,
    entries: &[PlaylistEntry],
    metadata: &PlaylistMetadata,
) -> rv_data::PlaylistDocument {
    build_playlist_document(vec![build_child_playlist(name, entries)], metadata)
}

/// Build one native document containing every named child in a checked set.
///
/// Embedded names are allocated over the set's canonical flattened order, so
/// collisions and repeated sources behave identically across child boundaries.
#[must_use]
pub fn build_playlist_set(
    playlist_set: &PlaylistSet,
    metadata: &PlaylistMetadata,
) -> rv_data::PlaylistDocument {
    let children = playlist_set
        .children
        .iter()
        .map(|child| {
            build_child_playlist(&child.name, &playlist_set.entries[child.entries.clone()])
        })
        .collect();
    build_playlist_document(children, metadata)
}

fn build_child_playlist(name: &str, entries: &[PlaylistEntry]) -> rv_data::Playlist {
    let items = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| build_playlist_item(name, index, entry))
        .collect();

    rv_data::Playlist {
        uuid: Some(derived_uuid("proflow:playlist:v1", &[name.as_bytes()])),
        name: name.to_string(),
        r#type: playlist::Type::Unknown as i32,
        expanded: false,
        targeted_layer_uuid: None,
        smart_directory_path: None,
        hot_key: None,
        cues: Vec::new(),
        children: Vec::new(),
        timecode_enabled: false,
        timing: playlist::TimingType::None as i32,
        startup_info: None,
        children_type: Some(playlist::ChildrenType::Items(playlist::PlaylistItems {
            items,
        })),
        link_data: None,
    }
}

fn build_playlist_item(
    playlist: &str,
    index: usize,
    entry: &PlaylistEntry,
) -> rv_data::PlaylistItem {
    rv_data::PlaylistItem {
        uuid: Some(derived_uuid(
            "proflow:playlist-item:v1",
            &[
                playlist.as_bytes(),
                &(index as u64).to_be_bytes(),
                entry.presentation_path().as_bytes(),
            ],
        )),
        name: entry.name().to_string(),
        tags: Vec::new(),
        is_hidden: false,
        item_type: Some(playlist_item::ItemType::Presentation(
            playlist_item::Presentation {
                document_path: Some(document_path_for_presentation_path(
                    entry.presentation_path(),
                )),
                arrangement: entry
                    .selected_arrangement()
                    .map(|arrangement| rv_data::Uuid {
                        string: arrangement.uuid().to_string(),
                    }),
                content_destination: rv_data::action::ContentDestination::Global as i32,
                user_music_key: entry.user_music_key().cloned(),
                arrangement_name: entry
                    .selected_arrangement()
                    .map_or_else(String::new, |arrangement| arrangement.name().to_string()),
            },
        )),
    }
}

fn build_playlist_document(
    children: Vec<rv_data::Playlist>,
    metadata: &PlaylistMetadata,
) -> rv_data::PlaylistDocument {
    let child_names = children
        .iter()
        .map(|child| child.name.as_bytes())
        .collect::<Vec<_>>();
    let root_node = rv_data::Playlist {
        uuid: Some(derived_uuid("proflow:playlist-root:v1", &child_names)),
        name: "PLAYLIST".to_string(),
        r#type: playlist::Type::Unknown as i32,
        expanded: metadata.root_expanded,
        targeted_layer_uuid: None,
        smart_directory_path: None,
        hot_key: None,
        cues: Vec::new(),
        children: Vec::new(),
        timecode_enabled: false,
        timing: playlist::TimingType::None as i32,
        startup_info: None,
        children_type: Some(playlist::ChildrenType::Playlists(playlist::PlaylistArray {
            playlists: children,
        })),
        link_data: None,
    };

    rv_data::PlaylistDocument {
        application_info: Some(metadata.application_info.clone()),
        r#type: playlist_document::Type::Presentation as i32,
        root_node: Some(root_node),
        tags: Vec::new(),
        live_video_playlist: None,
        downloads_playlist: None,
    }
}
