//! `ProPresenter` file format support.
//!
//! This module provides types and utilities for reading, writing, and
//! manipulating `ProPresenter` presentation files (.pro) and playlist files (.proplaylist).

use serde::{Deserialize, Serialize};

pub use resolution::{PresentationSize, PresentationSizeError, PresentationSizeStatus};

/// The detected or user-assigned slide type for a service item.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, rmcp::schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum SlideType {
    /// Generic text slides.
    #[default]
    Text,
    /// Bible verse slides.
    Scripture,
    /// Song lyrics with verse/chorus markers.
    #[serde(alias = "song")]
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
/// Live ProPresenter playlist library helpers.
pub mod live;
/// Macro injection for presentations.
pub mod macros;
/// Media dependency discovery.
pub mod media;
mod native_zip;
/// Playlist package inspection utilities.
pub mod package;
/// Playlist file support (.proplaylist).
pub mod playlist;
/// Checked presentation-canvas dimensions and inspection.
pub mod resolution;
/// RTF conversion utilities.
pub mod rtf;
/// File serialization (writing .pro files).
pub mod serialize;
/// Song presentation structure repair.
pub mod song;
/// Template-based slide generation.
pub mod template;
/// UUID generation utilities.
pub mod uuid;
