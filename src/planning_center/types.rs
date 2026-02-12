//! Planning Center data types.
//!
//! These types represent the data structures from the Planning Center API.

use chrono::{DateTime, Utc};

/// Represents a type of service (e.g., "Sunday Morning")
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    /// Unique identifier from Planning Center
    pub id: String,
    /// Human-readable service type name
    pub name: String,
}

/// Represents a specific instance of a Service on a particular date
#[derive(Debug, Clone)]
pub struct Plan {
    /// Unique identifier from Planning Center
    pub id: String,
    /// Parent service type identifier
    pub service_id: String,
    /// Name of the service type this plan belongs to
    pub service_name: String,
    /// Scheduled date and time
    pub date: DateTime<Utc>,
    /// Display title (e.g., "March 31st")
    pub title: String,
    /// Items in this plan (loaded separately)
    pub items: Vec<Item>,
}

/// Represents an element within a Plan (e.g., Song, Scripture, Header)
#[derive(Debug, Clone)]
pub struct Item {
    /// Unique identifier from Planning Center
    pub id: String,
    /// Position in the plan order
    pub position: usize,
    /// Display title
    pub title: String,
    /// Optional description text
    pub description: Option<String>,
    /// Classification for application routing
    pub category: Category,
    /// Optional note attached to this item
    pub note: Option<String>,
    /// Linked song data, if this item is a song
    pub song: Option<Song>,
    /// Linked scripture reference, if applicable
    pub scripture: Option<Scripture>,
}

/// Classifies the type of an Item for application purposes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    /// Plain text content
    Text,
    /// Visual/graphic content (e.g., announcements)
    Graphic,
    /// Title-style content (e.g., scripture readings, sermons)
    Title,
    /// Musical item with lyrics
    Song,
    /// Structural items like headers
    Other,
}

/// Song metadata from Planning Center
#[derive(Debug, Clone)]
pub struct Song {
    /// Song title
    pub title: String,
    /// Song author or composer
    pub author: Option<String>,
    /// Copyright information
    pub copyright: Option<String>,
    /// CCLI license number
    pub ccli: Option<String>,
    /// Associated theme tags
    pub themes: Option<Vec<String>>,
    /// Full lyrics text
    pub lyrics: Option<String>,
    /// Name of the selected arrangement
    pub arrangement: Option<String>,
}

/// Scripture reference
#[derive(Debug, Clone)]
pub struct Scripture {
    /// Book, chapter, and verse reference string
    pub reference: String,
    /// Full scripture passage text
    pub text: Option<String>,
    /// Bible translation identifier (e.g., "NIV", "ESV")
    pub translation: Option<String>,
}
