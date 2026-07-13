//! Structured workflow result summaries.
#![allow(missing_docs)]

use serde::Serialize;

use crate::propresenter::package::PlaylistPackageMode;

/// Summary of a single item processed by the service build workflow.
#[derive(Debug, Serialize)]
pub struct BuildServiceEntry {
    pub output_key: String,
    pub position: usize,
    pub name: String,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slides: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Result summary from a complete service build.
#[derive(Debug, Serialize)]
pub struct BuildServiceResult {
    pub playlist_path: String,
    pub package_mode: PlaylistPackageMode,
    pub media_asset_count: usize,
    pub entries: Vec<BuildServiceEntry>,
    pub total_items: usize,
    pub generated_count: usize,
    pub library_count: usize,
    pub skipped_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}
