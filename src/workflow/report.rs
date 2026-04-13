//! Structured workflow result summaries.

use serde::Serialize;

/// Summary of a single item processed by the service build workflow.
#[derive(Debug, Serialize)]
pub(crate) struct BuildServiceEntry {
    pub output_key: String,
    pub position: usize,
    pub name: String,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slides: Option<usize>,
}

/// Result summary from a complete service build.
#[derive(Debug, Serialize)]
pub(crate) struct BuildServiceResult {
    pub playlist_path: String,
    pub entries: Vec<BuildServiceEntry>,
    pub total_items: usize,
    pub generated_count: usize,
    pub library_count: usize,
    pub skipped_count: usize,
}
