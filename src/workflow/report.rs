//! Structured workflow result summaries.
use serde::Serialize;

use crate::propresenter::package::PlaylistPackageMode;

/// Summary of a single item processed by the service build workflow.
#[derive(Debug, Serialize)]
pub struct BuildServiceEntry {
    /// Stable source-item identity used by preview overrides.
    pub output_key: String,
    /// Zero-based source-plan position.
    pub position: usize,
    /// Operator-visible presentation name.
    pub name: String,
    /// Concise description of the operation performed.
    pub action: String,
    /// Existing or generated presentation path, when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    /// Number of rendered cues, when a presentation was generated or edited.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slides: Option<usize>,
    /// Entry-specific warnings produced by the build.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Result summary from a complete service build.
#[derive(Debug, Serialize)]
pub struct BuildServiceResult {
    /// Final playlist package path.
    pub playlist_path: String,
    /// Native package shape written by the build.
    pub package_mode: PlaylistPackageMode,
    /// Number of explicit portable media assets included.
    pub media_asset_count: usize,
    /// Results in reviewed plan order.
    pub entries: Vec<BuildServiceEntry>,
    /// Number of presentation items written to the playlist.
    pub total_items: usize,
    /// Number of generated or edited presentations.
    pub generated_count: usize,
    /// Number of existing library presentations reused.
    pub library_count: usize,
    /// Number of reviewed items intentionally skipped.
    pub skipped_count: usize,
    /// Aggregate operator-visible warnings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}
