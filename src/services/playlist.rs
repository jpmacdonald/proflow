//! Playlist generation service.
//!
//! This module provides abstractions for generating playlists in various formats.

use std::path::Path;
use crate::types::SlideType;

/// Represents an item to be included in a playlist.
#[derive(Debug, Clone)]
pub struct PlaylistItem {
    /// Display name for the item.
    pub name: String,
    /// Slide type for type-aware filename sanitization.
    pub slide_type: SlideType,
    /// Path to the presentation file (if external).
    pub file_path: Option<String>,
    /// Embedded presentation data (if generated).
    pub embedded_data: Option<Vec<u8>>,
}

/// Trait for playlist generation.
///
/// Different implementations can generate playlists in various formats
/// (`ProPresenter`, `ProPresenter` 6, plain text, etc.).
pub trait PlaylistGenerator {
    /// The error type for this generator.
    type Error: std::error::Error;

    /// Generate a playlist from the given items.
    ///
    /// # Arguments
    /// * `name` - The playlist name
    /// * `items` - The items to include in the playlist
    /// * `output` - The output path for the playlist file
    ///
    /// # Returns
    /// Ok(()) on success, or an error if generation failed.
    fn generate(&self, name: &str, items: &[PlaylistItem], output: &Path)
        -> Result<(), Self::Error>;

    /// Get the file extension for this playlist format.
    fn extension(&self) -> &'static str;

    /// Get the format name (for display purposes).
    fn format_name(&self) -> &'static str;
}

/// `ProPresenter` 7 playlist generator using the existing propresenter module.
#[derive(Debug, Default)]
pub struct ProPresenter7Playlist;

impl PlaylistGenerator for ProPresenter7Playlist {
    type Error = crate::propresenter::playlist::PlaylistError;

    fn generate(
        &self,
        name: &str,
        items: &[PlaylistItem],
        output: &Path,
    ) -> Result<(), Self::Error> {
        use crate::propresenter::playlist::{build_playlist, write_playlist_file, PlaylistEntry};

        let entries: Vec<PlaylistEntry> = items
            .iter()
            .map(|item| PlaylistEntry {
                name: item.name.clone(),
                slide_type: item.slide_type,
                from_matched_file: false,
                presentation_path: item.file_path.clone().unwrap_or_default(),
                arrangement_uuid: None,
                embedded_data: item.embedded_data.clone(),
            })
            .collect();

        let playlist = build_playlist(name, &entries);
        write_playlist_file(&playlist, &entries, output)?;

        Ok(())
    }

    fn extension(&self) -> &'static str {
        "proplaylist"
    }

    fn format_name(&self) -> &'static str {
        "ProPresenter 7"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_playlist_item_creation() {
        let item = PlaylistItem {
            name: "Test Song".to_string(),
            slide_type: SlideType::Lyrics,
            file_path: Some("/path/to/song.pro".to_string()),
            embedded_data: None,
        };

        assert_eq!(item.name, "Test Song");
        assert!(item.file_path.is_some());
        assert!(item.embedded_data.is_none());
    }
}
