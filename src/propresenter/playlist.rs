//! ProPresenter playlist file support.
//!
//! Writes protobuf-encoded playlist files (.proplaylist) to disk.

use std::fs::File;
use std::io::Write;
use std::path::Path;
use prost::Message;
use uuid::Uuid;
use zip::write::FileOptions;
use zip::ZipWriter;

use crate::propresenter::generated::rv_data::{self, playlist, playlist_document, playlist_item, url};

/// Errors that can occur when writing playlist files
#[derive(Debug, thiserror::Error)]
pub enum PlaylistError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Encoding error: {0}")]
    Encode(String),

    #[error("Zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
}

/// A playlist entry representing a matched file for a service item
#[derive(Debug, Clone)]
pub struct PlaylistEntry {
    /// Display name for the playlist item
    pub name: String,
    /// Path to the .pro file (external reference)
    pub presentation_path: String,
    /// Optional arrangement UUID to use
    pub arrangement_uuid: Option<Uuid>,
    /// Optional embedded presentation data (if Some, embeds in zip instead of referencing external)
    pub embedded_data: Option<Vec<u8>>,
}

/// Convert a file path to a ProPresenter file:/// URL
fn path_to_file_url(path: &str) -> String {
    // URL-encode spaces and special chars, prefix with file:///
    // Note: file:/// has three slashes for absolute paths on macOS
    let encoded = path
        .replace(' ', "%20")
        .replace('&', "%26");
    format!("file://{}", encoded)
}

/// Extract relative path from absolute (e.g., ".../Libraries/Default/foo.pro" -> "Libraries/Default/foo.pro")
fn extract_relative_path(path: &str) -> Option<url::RelativeFilePath> {
    // Look for "Libraries/" in the path
    let rel_path = if let Some(idx) = path.find("Libraries/") {
        path[idx..].to_string()
    } else {
        // Fallback: just the filename
        std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .map(String::from)?
    };
    
    // Use LocalRelativePath with root = Show (10) for library paths
    Some(url::RelativeFilePath::Local(url::LocalRelativePath {
        root: url::local_relative_path::Root::Show as i32,
        path: rel_path,
    }))
}

/// Get the embedded filename for a presentation (sanitized for zip entry)
pub fn get_embedded_filename(name: &str) -> String {
    // Sanitize for zip entry name
    let safe: String = name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect();
    format!("{}.pro", safe)
}

/// Build a PlaylistDocument from a list of entries
/// 
/// ProPresenter expects a two-level structure:
/// - Root Playlist (container) with `playlists` field containing child playlists
/// - Child Playlist with `items` field containing the actual PlaylistItems
pub fn build_playlist(name: &str, entries: &[PlaylistEntry]) -> rv_data::PlaylistDocument {
    let items: Vec<rv_data::PlaylistItem> = entries
        .iter()
        .map(|entry| {
            let embedded_filename = get_embedded_filename(&entry.name);
            
            // For embedded files, create a path that ProPresenter can find:
            // - absolute_string: file:/// URL (can be placeholder path)
            // - relative_file_path: Libraries/Default/<filename>.pro with root=Show
            let (file_url, relative_path) = if entry.embedded_data.is_some() {
                // Create a plausible absolute path (ProPresenter will use relative)
                let abs_path = format!("file:///Libraries/Default/{}", embedded_filename.replace(' ', "%20"));
                // Relative path within ProPresenter's library structure
                let rel = url::RelativeFilePath::Local(url::LocalRelativePath {
                    root: url::local_relative_path::Root::Show as i32,
                    path: format!("Libraries/Default/{}", embedded_filename),
                });
                (abs_path, Some(rel))
            } else {
                let file_url = path_to_file_url(&entry.presentation_path);
                let relative_path = extract_relative_path(&entry.presentation_path);
                (file_url, relative_path)
            };
            
            rv_data::PlaylistItem {
                uuid: Some(rv_data::Uuid { string: Uuid::new_v4().to_string() }),
                name: entry.name.clone(),
                tags: Vec::new(),
                is_hidden: false,
                item_type: Some(playlist_item::ItemType::Presentation(
                    playlist_item::Presentation {
                        document_path: Some(rv_data::Url {
                            platform: rv_data::url::Platform::Macos as i32,
                            storage: Some(rv_data::url::Storage::AbsoluteString(file_url)),
                            relative_file_path: relative_path,
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

    // Inner playlist containing the actual items
    let inner_playlist = rv_data::Playlist {
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

    // Root playlist container (holds child playlists via PlaylistArray)
    let root_node = rv_data::Playlist {
        uuid: Some(rv_data::Uuid { string: Uuid::new_v4().to_string() }),
        name: "PLAYLIST".to_string(),
        r#type: playlist::Type::Unknown as i32, // Root uses default/unknown
        expanded: true,
        targeted_layer_uuid: None,
        smart_directory_path: None,
        hot_key: None,
        cues: Vec::new(),
        children: Vec::new(),
        timecode_enabled: false,
        timing: playlist::TimingType::None as i32,
        startup_info: None,
        children_type: Some(playlist::ChildrenType::Playlists(playlist::PlaylistArray {
            playlists: vec![inner_playlist],
        })),
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
/// 
/// If entries have embedded_data, those .pro files are bundled into the zip.
pub fn write_playlist_file(
    playlist: &rv_data::PlaylistDocument,
    entries: &[PlaylistEntry],
    path: impl AsRef<Path>,
) -> Result<(), PlaylistError> {
    let mut buf = Vec::new();
    playlist
        .encode(&mut buf)
        .map_err(|e| PlaylistError::Encode(e.to_string()))?;

    // ProPresenter .proplaylist is a zip with:
    // - "data" entry containing the protobuf playlist document
    // - Embedded .pro files at root level (ProPresenter searches by basename)
    let file = File::create(path)?;
    let mut zip = ZipWriter::new(file);
    let options = FileOptions::default().compression_method(zip::CompressionMethod::Stored);

    // Write embedded .pro files first (at root level like the sample)
    for entry in entries {
        if let Some(data) = &entry.embedded_data {
            let filename = get_embedded_filename(&entry.name);
            zip.start_file(&filename, options.clone())?;
            zip.write_all(data)?;
        }
    }

    // Write the playlist data last
    zip.start_file("data", options)?;
    zip.write_all(&buf)?;
    zip.finish()?;

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
                embedded_data: None,
            },
            PlaylistEntry {
                name: "How Great Thou Art".to_string(),
                presentation_path: "/path/to/how_great.pro".to_string(),
                arrangement_uuid: Some(Uuid::new_v4()),
                embedded_data: None,
            },
        ];

        let playlist = build_playlist("Sunday Service", &entries);
        
        let root = playlist.root_node.unwrap();
        match root.children_type {
            Some(playlist::ChildrenType::Playlists(arr)) => {
                assert_eq!(arr.playlists.len(), 1);
                let inner = &arr.playlists[0];
                match &inner.children_type {
                    Some(playlist::ChildrenType::Items(items)) => {
                        assert_eq!(items.items.len(), 2);
                        assert_eq!(items.items[0].name, "Amazing Grace");
                        assert_eq!(items.items[1].name, "How Great Thou Art");
                    }
                    _ => panic!("Expected Items in inner playlist"),
                }
            }
            _ => panic!("Expected Playlists in root"),
        }
    }

    #[test]
    fn test_write_playlist_file() {
        let entries = vec![
            PlaylistEntry {
                name: "Test Song".to_string(),
                presentation_path: "/Users/Shared/ProPresenter/Libraries/Default/Test.pro".to_string(),
                arrangement_uuid: None,
                embedded_data: None,
            },
        ];

        let playlist = build_playlist("Test Playlist", &entries);
        let output_path = get_test_output_path("test_playlist.proplaylist");
        
        write_playlist_file(&playlist, &entries, &output_path).expect("Failed to write playlist");
        
        assert!(output_path.exists());
        
        // Verify file can be read back
        let contents = std::fs::read(&output_path).expect("Failed to read playlist");
        assert!(!contents.is_empty());
    }
}

