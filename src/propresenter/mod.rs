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
/// Checked macro-to-Audience-Look destination resolution.
pub mod audience;
/// Background image support.
pub mod background;
/// File deserialization (reading .pro files).
pub mod deserialize;
/// Generated protobuf types.
pub mod generated;
/// Checked final presentation documents produced by the semantic renderer.
pub mod generated_document;
/// Installed cue-group metadata.
pub mod groups;
/// Standalone semantic presentation inspection.
pub mod inspection;
/// Native presentation library catalog and fuzzy matching.
pub mod library;
/// Live ProPresenter playlist library helpers for fidelity tooling.
#[cfg(any(test, feature = "dev-tools"))]
pub mod live;
/// Macro injection for presentations.
pub mod macros;
/// Media dependency discovery.
pub mod media;
mod native_url;
mod native_zip;
/// Playlist package inspection utilities.
pub mod package;
/// Playlist file support (.proplaylist).
pub mod playlist;
/// Checked renderer-independent presentation specifications.
pub mod presentation_spec;
/// Pure presentation-specification renderer.
pub mod render;
/// Checked presentation-canvas dimensions and inspection.
pub mod resolution;
/// RTF conversion utilities.
pub mod rtf;
/// Verse-aware scripture slide layout.
pub mod scripture_layout;
/// File serialization (writing .pro files).
pub mod serialize;
/// Native macOS text layout evidence used to prove final glyph fit.
pub mod text_fit;
/// Pure checked text flow for presentation slides.
pub mod text_flow;
/// Installed theme loading and exact text-box geometry.
pub mod theme;
