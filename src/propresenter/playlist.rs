//! `ProPresenter` playlist file support.
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
use crate::types::SlideType;

/// Errors that can occur when writing playlist files
#[derive(Debug, thiserror::Error)]
pub enum PlaylistError {
    /// An I/O error occurred during file operations
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Failed to encode the protobuf playlist data
    #[error("Encoding error: {0}")]
    Encode(String),

    /// A zip archive error occurred
    #[error("Zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
}

/// A playlist entry representing a matched file for a service item
#[derive(Debug, Clone)]
pub struct PlaylistEntry {
    /// Display name for the playlist item
    pub name: String,
    /// Slide type for type-aware filename sanitization
    pub slide_type: SlideType,
    /// When true, `name` is already a valid filename stem from an existing file
    /// on disk and should not be re-sanitized.
    pub from_matched_file: bool,
    /// Path to the .pro file (external reference)
    pub presentation_path: String,
    /// Optional arrangement UUID to use
    pub arrangement_uuid: Option<Uuid>,
    /// Optional embedded presentation data (if Some, embeds in zip instead of referencing external)
    pub embedded_data: Option<Vec<u8>>,
}

impl PlaylistEntry {
    /// Get the filesystem-safe embedded filename for this entry.
    ///
    /// Matched files use their name verbatim (already valid from disk).
    /// Generated files run through type-specific sanitization.
    pub fn embedded_filename(&self) -> String {
        if self.from_matched_file {
            format!("{}.pro", self.name)
        } else {
            get_embedded_filename(&self.name, self.slide_type)
        }
    }
}

/// Convert a file path to a `ProPresenter` `file:///` URL
fn path_to_file_url(path: &str) -> String {
    // URL-encode characters that have special meaning in URIs.
    // Note: file:/// has three slashes for absolute paths on macOS.
    let encoded = path
        .replace(' ', "%20")
        .replace('#', "%23")
        .replace('&', "%26");
    format!("file://{encoded}")
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

/// Sanitize a name for use as a filename, applying type-specific rules.
///
/// **Song**: name passed in should already be the song DB title; kept as-is
/// (parenthetical content is part of the song name).
///
/// **Scripture**: strips common prefixes ("Scripture", "Reading") and speaker
/// names in parentheses, converts verse colons to `v`.
///
/// **Title / Text / Graphic**: strips parenthetical speaker names, converts
/// colons to ` - `.
///
/// All types strip unsafe filesystem characters and normalize whitespace.
pub fn sanitize_filename(name: &str, slide_type: SlideType) -> String {
    match slide_type {
        SlideType::Lyrics => sanitize_song(name),
        SlideType::Scripture => sanitize_scripture(name),
        _ => sanitize_general(name),
    }
}

/// Songs: keep the name mostly verbatim (parenthetical content is part of the
/// title). Only strip unsafe filesystem chars.
fn sanitize_song(name: &str) -> String {
    strip_unsafe_chars(name)
}

/// Scripture: strip prefix labels and speaker names, convert verse colons to `v`.
fn sanitize_scripture(name: &str) -> String {
    let mut s = name.to_string();

    // Strip parenthetical speaker names
    s = strip_parens(&s);

    // Strip common prefixes: "Scripture Reading", "Scripture", "Reading"
    for prefix in &["Scripture Reading", "Scripture", "Reading"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            // Strip the separator after prefix (" - ", ": ", " ")
            s = rest
                .strip_prefix(" - ")
                .or_else(|| rest.strip_prefix(": "))
                .or_else(|| rest.strip_prefix(" -"))
                .or_else(|| rest.strip_prefix(':'))
                .unwrap_or(rest)
                .trim()
                .to_string();
            break;
        }
    }

    // Convert verse colons: digit:digit → digit v digit
    let chars: Vec<char> = s.chars().collect();
    let mut result = String::with_capacity(s.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == ':' && i > 0 && chars[i - 1].is_ascii_digit()
            && i + 1 < chars.len() && chars[i + 1].is_ascii_digit()
        {
            result.push('v');
        } else {
            result.push(c);
        }
    }

    strip_unsafe_chars(result.trim())
}

/// General items (Title, Text, Graphic): strip parenthetical speaker names,
/// convert colons to ` - `.
fn sanitize_general(name: &str) -> String {
    let s = strip_parens(name);

    let chars: Vec<char> = s.chars().collect();
    let mut result = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == ':' {
            // Trim trailing space before inserting to avoid double space
            if result.ends_with(' ') {
                result.pop();
            }
            result.push_str(" - ");
            // Skip a following space to avoid " -  foo"
            if i + 1 < chars.len() && chars[i + 1] == ' ' {
                i += 1;
            }
        } else {
            result.push(chars[i]);
        }
        i += 1;
    }

    strip_unsafe_chars(result.trim())
}

/// Strip parenthetical content (including nested parens), then trim.
///
/// Unmatched `)` at depth 0 is also discarded — stray closing parens
/// appear in real Planning Center data (e.g. double-paren typos).
fn strip_parens(name: &str) -> String {
    let mut result = String::with_capacity(name.len());
    let mut depth = 0u32;
    for c in name.chars() {
        match c {
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            ')' => {}
            _ if depth == 0 => result.push(c),
            _ => {}
        }
    }
    result.trim().to_string()
}

/// Strip characters that are unsafe in filenames and collapse whitespace.
///
/// Includes `:` which is forbidden on macOS (legacy HFS+ path separator).
fn strip_unsafe_chars(name: &str) -> String {
    name.chars()
        .filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Get the embedded filename for a presentation (sanitized for zip entry).
///
/// Falls back to "Untitled" if sanitization produces an empty name
/// (e.g. unfilled placeholder like "Scripture (Robert)").
pub fn get_embedded_filename(name: &str, slide_type: SlideType) -> String {
    let sanitized = sanitize_filename(name, slide_type);
    if sanitized.is_empty() {
        "Untitled.pro".to_string()
    } else {
        format!("{sanitized}.pro")
    }
}

/// Build a `PlaylistDocument` from a list of entries
///
/// `ProPresenter` expects a two-level structure:
/// - Root Playlist (container) with `playlists` field containing child playlists
/// - Child Playlist with `items` field containing the actual `PlaylistItems`
pub fn build_playlist(name: &str, entries: &[PlaylistEntry]) -> rv_data::PlaylistDocument {
    let items: Vec<rv_data::PlaylistItem> = entries
        .iter()
        .map(|entry| {
            let embedded_filename = entry.embedded_filename();
            
            // For embedded files, create a path that ProPresenter can find:
            // - absolute_string: file:/// URL (can be placeholder path)
            // - relative_file_path: Libraries/Default/<filename>.pro with root=Show
            let (file_url, relative_path) = if entry.embedded_data.is_some() {
                // Create a plausible absolute path (ProPresenter will use relative)
                let encoded_name = embedded_filename.replace(' ', "%20");
                let abs_path = format!("file:///Libraries/Default/{encoded_name}");
                // Relative path within ProPresenter's library structure
                let rel = url::RelativeFilePath::Local(url::LocalRelativePath {
                    root: url::local_relative_path::Root::Show as i32,
                    path: format!("Libraries/Default/{embedded_filename}"),
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
/// If entries have `embedded_data`, those .pro files are bundled into the zip.
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

    // Write embedded .pro files first (at root level like the sample).
    // Deduplicate filenames to avoid zip entry collisions.
    let mut used_names = std::collections::HashSet::new();
    for entry in entries {
        if let Some(data) = &entry.embedded_data {
            let base = entry.embedded_filename();
            let filename = if used_names.contains(&base) {
                let stem = base.trim_end_matches(".pro");
                let mut n = 2u32;
                loop {
                    let candidate = format!("{stem} ({n}).pro");
                    if !used_names.contains(&candidate) {
                        break candidate;
                    }
                    n += 1;
                }
            } else {
                base
            };
            used_names.insert(filename.clone());
            zip.start_file(&filename, options)?;
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
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

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
        // Root node is the container named "PLAYLIST"
        assert_eq!(root.name, "PLAYLIST");
        // The inner playlist holds the actual name
        match root.children_type {
            Some(playlist::ChildrenType::Playlists(arr)) => {
                assert_eq!(arr.playlists.len(), 1);
                assert_eq!(arr.playlists[0].name, "Test Playlist");
            }
            _ => panic!("Expected Playlists in root"),
        }
    }

    #[test]
    fn test_build_playlist_with_entries() {
        let entries = vec![
            PlaylistEntry {
                name: "Amazing Grace".to_string(),
                slide_type: SlideType::Lyrics,
                from_matched_file: true,
                presentation_path: "/path/to/amazing_grace.pro".to_string(),
                arrangement_uuid: None,
                embedded_data: None,
            },
            PlaylistEntry {
                name: "How Great Thou Art".to_string(),
                slide_type: SlideType::Lyrics,
                from_matched_file: true,
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

    // -- Scripture sanitization --

    #[test]
    fn test_scripture_strips_prefix_and_converts_colons() {
        assert_eq!(
            sanitize_filename("Scripture - 1 Kings 18:18-21 (Connie)", SlideType::Scripture),
            "1 Kings 18v18-21"
        );
        assert_eq!(
            sanitize_filename("Scripture: 1 Kings 18:18-21", SlideType::Scripture),
            "1 Kings 18v18-21"
        );
        assert_eq!(
            sanitize_filename("Reading - John 3:16", SlideType::Scripture),
            "John 3v16"
        );
    }

    #[test]
    fn test_scripture_bare_reference() {
        assert_eq!(
            sanitize_filename("Matthew 6:1-2", SlideType::Scripture),
            "Matthew 6v1-2"
        );
        assert_eq!(
            sanitize_filename("Psalm 119:105-106", SlideType::Scripture),
            "Psalm 119v105-106"
        );
    }

    #[test]
    fn test_scripture_strips_speaker_parens() {
        // "Scripture (Robert)" is an unfilled placeholder — stripping the speaker
        // and the "Scripture" prefix leaves nothing, which is expected.
        assert_eq!(
            sanitize_filename("Scripture (Robert)", SlideType::Scripture),
            ""
        );
        // A filled-in scripture title with speaker should produce just the reference.
        assert_eq!(
            sanitize_filename("Scripture - John 3:16 (Robert)", SlideType::Scripture),
            "John 3v16"
        );
    }

    // -- Song sanitization --

    #[test]
    fn test_song_keeps_parens() {
        assert_eq!(
            sanitize_filename("Firm Foundation (He Won't)", SlideType::Lyrics),
            "Firm Foundation (He Won't)"
        );
        assert_eq!(
            sanitize_filename("Morning By Morning (I Will Trust)", SlideType::Lyrics),
            "Morning By Morning (I Will Trust)"
        );
        assert_eq!(
            sanitize_filename("Oceans (Where Feet May Fail)", SlideType::Lyrics),
            "Oceans (Where Feet May Fail)"
        );
    }

    #[test]
    fn test_song_strips_unsafe_chars() {
        assert_eq!(
            sanitize_filename("What?", SlideType::Lyrics),
            "What"
        );
    }

    // -- General (Text/Title/Graphic) sanitization --

    #[test]
    fn test_general_strips_speaker_parens() {
        assert_eq!(
            sanitize_filename("Welcome (Robert)", SlideType::Graphic),
            "Welcome"
        );
        assert_eq!(
            sanitize_filename("Children's Message (Connie)", SlideType::Title),
            "Children's Message"
        );
        assert_eq!(
            sanitize_filename("Benediction (Robert)", SlideType::Text),
            "Benediction"
        );
    }

    #[test]
    fn test_general_colon_to_dash() {
        assert_eq!(
            sanitize_filename("Prelude: Truro Procession", SlideType::Text),
            "Prelude - Truro Procession"
        );
        assert_eq!(
            sanitize_filename("Sermon: Showdown (Robert)", SlideType::Title),
            "Sermon - Showdown"
        );
    }

    #[test]
    fn test_general_unsafe_chars_stripped() {
        assert_eq!(
            sanitize_filename("He said \"hello\"", SlideType::Text),
            "He said hello"
        );
    }

    #[test]
    fn test_general_passthrough() {
        assert_eq!(
            sanitize_filename("Amazing Grace", SlideType::Text),
            "Amazing Grace"
        );
    }

    // -- get_embedded_filename --

    #[test]
    fn test_get_embedded_filename() {
        assert_eq!(
            get_embedded_filename("Scripture - Matthew 6:1-2 (Connie)", SlideType::Scripture),
            "Matthew 6v1-2.pro"
        );
        assert_eq!(
            get_embedded_filename("Prelude: lalala", SlideType::Text),
            "Prelude - lalala.pro"
        );
        assert_eq!(
            get_embedded_filename("Firm Foundation (He Won't)", SlideType::Lyrics),
            "Firm Foundation (He Won't).pro"
        );
    }

    #[test]
    fn test_matched_file_skips_sanitization() {
        let entry = PlaylistEntry {
            name: "Morning By Morning (I Will Trust)".to_string(),
            slide_type: SlideType::Text, // Wrong type, but from_matched_file should bypass
            from_matched_file: true,
            presentation_path: String::new(),
            arrangement_uuid: None,
            embedded_data: None,
        };
        // Parens preserved because matched files skip sanitization
        assert_eq!(entry.embedded_filename(), "Morning By Morning (I Will Trust).pro");
    }

    #[test]
    fn test_deduplication_in_embedded_filenames() {
        let entries = vec![
            PlaylistEntry {
                name: "Scripture (Robert)".to_string(),
                slide_type: SlideType::Scripture,
                from_matched_file: false,
                presentation_path: String::new(),
                arrangement_uuid: None,
                embedded_data: Some(vec![1]),
            },
            PlaylistEntry {
                name: "Scripture (Hope)".to_string(),
                slide_type: SlideType::Scripture,
                from_matched_file: false,
                presentation_path: String::new(),
                arrangement_uuid: None,
                embedded_data: Some(vec![2]),
            },
        ];

        let playlist = build_playlist("Test", &entries);
        let output_path = get_test_output_path("test_dedup.proplaylist");
        // Should not panic from duplicate zip entries
        write_playlist_file(&playlist, &entries, &output_path).expect("Failed to write playlist");

        // Verify both entries are in the zip
        let file = std::fs::File::open(&output_path).expect("open");
        let mut archive = zip::ZipArchive::new(file).expect("zip");
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(names.contains(&"Untitled.pro".to_string()));
        assert!(names.contains(&"Untitled (2).pro".to_string()));
    }

    #[test]
    fn test_write_playlist_file() {
        let entries = vec![
            PlaylistEntry {
                name: "Test Song".to_string(),
                slide_type: SlideType::Lyrics,
                from_matched_file: true,
                presentation_path: "/Users/Shared/ProPresenter/Libraries/Default/Test.pro".to_string(),
                arrangement_uuid: None,
                embedded_data: None,
            },
        ];

        let playlist = build_playlist("Test Playlist", &entries);
        let output_path = get_test_output_path("test_playlist.proplaylist");

        write_playlist_file(&playlist, &entries, &output_path).expect("Failed to write playlist");

        assert!(output_path.exists());

        let contents = std::fs::read(&output_path).expect("Failed to read playlist");
        assert!(!contents.is_empty());
    }
}

