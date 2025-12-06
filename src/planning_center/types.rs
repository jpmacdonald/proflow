//! Planning Center data types.
//!
//! These types represent the data structures from the Planning Center API.

use chrono::{DateTime, Utc};

/// Represents a type of service (e.g., "Sunday Morning")
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    pub id: String,
    pub name: String,
}

/// Represents a specific instance of a Service on a particular date
#[derive(Debug, Clone)]
pub struct Plan {
    pub id: String,
    pub service_id: String,
    /// Name of the service type this plan belongs to
    #[allow(dead_code)]
    pub _service_name: String,
    pub date: DateTime<Utc>,
    /// Display title (e.g., "March 31st")
    pub title: String,
    /// Items in this plan (loaded separately)
    #[allow(dead_code)]
    pub _items: Vec<Item>,
}

/// Represents an element within a Plan (e.g., Song, Scripture, Header)
#[derive(Debug, Clone)]
pub struct Item {
    pub id: String,
    /// Position in the plan order
    #[allow(dead_code)]
    pub _position: usize,
    pub title: String,
    #[allow(dead_code)]
    pub _description: Option<String>,
    pub category: Category,
    #[allow(dead_code)]
    pub _note: Option<String>,
    pub song: Option<Song>,
    #[allow(dead_code)]
    pub _scripture: Option<Scripture>,
}

/// Classifies the type of an Item for application purposes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Text,
    Graphic,
    Title,
    Song,
    /// Structural items like headers
    Other,
}

/// Song metadata from Planning Center
#[derive(Debug, Clone)]
pub struct Song {
    pub title: String,
    pub author: Option<String>,
    #[allow(dead_code)]
    pub _copyright: Option<String>,
    #[allow(dead_code)]
    pub _ccli: Option<String>,
    #[allow(dead_code)]
    pub _themes: Option<Vec<String>>,
    pub lyrics: Option<String>,
    #[allow(dead_code)]
    pub _arrangement: Option<String>,
}

/// Scripture reference
#[derive(Debug, Clone)]
pub struct Scripture {
    #[allow(dead_code)]
    pub _reference: String,
    #[allow(dead_code)]
    pub _text: Option<String>,
    #[allow(dead_code)]
    pub _translation: Option<String>,
}
