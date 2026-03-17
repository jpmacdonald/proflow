//! Core type definitions for compile-time safety.
//!
//! This module provides newtype wrappers around string identifiers to prevent
//! accidental mixing of different ID types at compile time.

use serde::{Deserialize, Serialize};
use std::fmt;

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

/// `Planning Center` item identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ItemId(pub String);

impl ItemId {
    /// Create a new `ItemId` from a string.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl fmt::Display for ItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for ItemId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for ItemId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl AsRef<str> for ItemId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}
