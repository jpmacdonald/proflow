//! `ProPresenter` file format support.
//!
//! This module provides types and utilities for reading, writing, and
//! manipulating `ProPresenter` presentation files (.pro) and playlist files (.proplaylist).

use serde::{Deserialize, Serialize};

/// The detected or user-assigned slide type for a service item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SlideType {
    /// Generic text slides.
    #[default]
    Text,
    /// Bible verse slides.
    Scripture,
    /// Song lyrics with verse/chorus markers.
    Lyrics,
    /// Nametags and sermon titles.
    Title,
    /// Image-based slides (offertory, announcements).
    Graphic,
}

impl SlideType {
    /// Returns the human-readable name of this slide type.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Scripture => "Scripture",
            Self::Lyrics => "Lyrics",
            Self::Title => "Title",
            Self::Graphic => "Graphic",
            Self::Text => "Text",
        }
    }

    /// Cycle to next type (for 't' key override).
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Scripture => Self::Lyrics,
            Self::Lyrics => Self::Title,
            Self::Title => Self::Graphic,
            Self::Graphic => Self::Text,
            Self::Text => Self::Scripture,
        }
    }
}

/// Arrangement selection.
pub mod arrangement;
/// Background image support.
pub mod background;
/// File deserialization (reading .pro files).
pub mod deserialize;
/// Extract plain text from presentations.
pub mod extract;
/// Generated protobuf types.
pub mod generated;
/// Macro injection for presentations.
pub mod macros;
/// Playlist file support (.proplaylist).
pub mod playlist;
/// RTF conversion utilities.
pub mod rtf;
/// File serialization (writing .pro files).
pub mod serialize;
/// Template-based slide generation.
pub mod template;
/// UUID generation utilities.
pub mod uuid;
