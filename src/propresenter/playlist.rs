//! ProPresenter playlist file support.
//!
//! Writes protobuf-encoded playlist files (.proplaylist) to disk.

use std::fs::File;
use std::io::Write;
use std::path::Path;
use prost::Message;
use uuid::Uuid;

use crate::propresenter::generated::rv_data::{self, playlist, playlist_document, playlist_item};

/// Errors that can occur when writing playlist files
#[derive(Debug, thiserror::Error)]
pub enum PlaylistError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Encoding error: {0}")]
    Encode(String),
}

/// A playlist entry representing a matched file for a service item
#[derive(Debug, Clone)]
pub struct PlaylistEntry {
    /// Display name for the playlist item
    pub name: String,
    /// Path to the .pro file
    pub presentation_path: String,
    /// Optional arrangement UUID to use
    pub arrangement_uuid: Option<Uuid>,
}

/// Build a PlaylistDocument from a list of entries
pub fn build_playlist(name: &str, entries: &[PlaylistEntry]) -> rv_data::PlaylistDocument {
    let items: Vec<rv_data::PlaylistItem> = entries
        .iter()
        .map(|entry| {
            rv_data::PlaylistItem {
                uuid: Some(rv_data::Uuid { string: Uuid::new_v4().to_string() }),
                name: entry.name.clone(),
                tags: Vec::new(),
                is_hidden: false,
                item_type: Some(playlist_item::ItemType::Presentation(
                    playlist_item::Presentation {
                        document_path: Some(rv_data::Url {
                            platform: rv_data::url::Platform::Macos as i32,
                            storage: Some(rv_data::url::Storage::AbsoluteString(
                                entry.presentation_path.clone(),
                            )),
                            relative_file_path: None,
                        }),
                        arrangement: entry.arrangement_uuid.map(|u| rv_data::Uuid { 
                            string: u.to_string() 
                        }),
                        content_destination: rv_data::action::ContentDestination::Global as i32,
                        user_music_key: None,
                        arrangement_name: String::new(),
                    },
                )),
            }
        })
        .collect();

    let root_node = rv_data::Playlist {
        uuid: Some(rv_data::Uuid { string: Uuid::new_v4().to_string() }),
        name: name.to_string(),
        r#type: playlist::Type::Playlist as i32,
        expanded: true,
        targeted_layer_uuid: None,
        smart_directory_path: None,
        hot_key: None,
        cues: Vec::new(),
        children: Vec::new(),
        timecode_enabled: false,
        timing: playlist::TimingType::None as i32,
        startup_info: None,
        children_type: Some(playlist::ChildrenType::Items(playlist::PlaylistItems { items })),
        link_data: None,
    };

    rv_data::PlaylistDocument {
        application_info: Some(rv_data::ApplicationInfo {
            platform: rv_data::application_info::Platform::Macos as i32,
            platform_version: Some(rv_data::Version {
                major_version: 14,
                minor_version: 0,
                patch_version: 0,
                build: "14A309".to_string(),
            }),
            application: rv_data::application_info::Application::Propresenter as i32,
            application_version: Some(rv_data::Version {
                major_version: 7,
                minor_version: 14,
                patch_version: 0,
                build: "7.14.0".to_string(),
            }),
        }),
        r#type: playlist_document::Type::Presentation as i32,
        root_node: Some(root_node),
        tags: Vec::new(),
        live_video_playlist: None,
        downloads_playlist: None,
    }
}

/// Write a playlist document to a .proplaylist file
pub fn write_playlist_file(
    playlist: &rv_data::PlaylistDocument,
    path: impl AsRef<Path>,
) -> Result<(), PlaylistError> {
    let mut buf = Vec::new();
    playlist.encode(&mut buf)
        .map_err(|e| PlaylistError::Encode(e.to_string()))?;
    
    let mut file = File::create(path)?;
    file.write_all(&buf)?;
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn get_test_output_path(filename: &str) -> PathBuf {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("out");
        path.push("test");
        // Ensure directory exists
        std::fs::create_dir_all(&path).ok();
        path.push(filename);
        path
    }

    #[test]
    fn test_build_empty_playlist() {
        let playlist = build_playlist("Test Playlist", &[]);
        
        assert!(playlist.root_node.is_some());
        let root = playlist.root_node.unwrap();
        assert_eq!(root.name, "Test Playlist");
        assert_eq!(root.r#type, playlist::Type::Playlist as i32);
    }

    #[test]
    fn test_build_playlist_with_entries() {
        let entries = vec![
            PlaylistEntry {
                name: "Amazing Grace".to_string(),
                presentation_path: "/path/to/amazing_grace.pro".to_string(),
                arrangement_uuid: None,
            },
            PlaylistEntry {
                name: "How Great Thou Art".to_string(),
                presentation_path: "/path/to/how_great.pro".to_string(),
                arrangement_uuid: Some(Uuid::new_v4()),
            },
        ];

        let playlist = build_playlist("Sunday Service", &entries);
        
        let root = playlist.root_node.unwrap();
        match root.children_type {
            Some(playlist::ChildrenType::Items(items)) => {
                assert_eq!(items.items.len(), 2);
                assert_eq!(items.items[0].name, "Amazing Grace");
                assert_eq!(items.items[1].name, "How Great Thou Art");
            }
            _ => panic!("Expected Items in children_type"),
        }
    }

    #[test]
    fn test_write_playlist_file() {
        let entries = vec![
            PlaylistEntry {
                name: "Test Song".to_string(),
                presentation_path: "/Users/Shared/ProPresenter/Libraries/Default/Test.pro".to_string(),
                arrangement_uuid: None,
            },
        ];

        let playlist = build_playlist("Test Playlist", &entries);
        let output_path = get_test_output_path("test_playlist.proplaylist");
        
        write_playlist_file(&playlist, &output_path).expect("Failed to write playlist");
        
        assert!(output_path.exists());
        
        // Verify file can be read back
        let contents = std::fs::read(&output_path).expect("Failed to read playlist");
        assert!(!contents.is_empty());
    }
}

