//! Structured workflow result summaries.
use std::path::PathBuf;

use serde::Serialize;

use crate::workflow::ExpectedPresentationContract;
use proflow_propresenter::inspection::PresentationStructureSummary;
use proflow_propresenter::playlist::PlaylistExportMode;
use proflow_propresenter::text_fit::{CueTextFitSummary, TextFitContractSummary};

/// What the build did for one plan entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildEntryAction {
    /// A new native presentation was rendered from plan content.
    Generated,
    /// An existing library presentation is linked unchanged.
    Library,
    /// An existing presentation had plan content rendered into it.
    Edited,
    /// An existing presentation had configured transforms applied.
    Restyled,
    /// The entry produced no output.
    Skipped {
        /// Operator-facing reason recorded on the plan.
        reason: String,
    },
}

/// Summary of a single item processed by the service build workflow.
#[derive(Debug, Clone, Serialize)]
pub struct BuildServiceEntry {
    /// Stable source-item identity used by preview overrides.
    pub output_key: String,
    /// Zero-based source-plan position.
    pub position: usize,
    /// Operator-visible presentation name.
    pub name: String,
    /// The operation performed.
    pub action: BuildEntryAction,
    /// Existing or generated presentation path, when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<PathBuf>,
    /// Number of rendered cues, when a presentation was generated or edited.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slides: Option<usize>,
    /// Semantic native-output promise derived from the reviewed plan.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_presentation: Option<ExpectedPresentationContract>,
    /// Semantic inspection of the exact final presentation bytes carried by
    /// the playlist entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presentation_structure: Option<PresentationStructureSummary>,
    /// Playlist-level arrangement selection and the exact operator traversal
    /// it activates. This is distinct from the selected arrangement stored
    /// inside the embedded presentation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playlist_selection: Option<PlaylistSelectionSummary>,
    /// Native `TextKit` layout evidence for generated text cues and every
    /// macro-selected audience-screen destination.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub text_fit_evidence: Vec<CueTextFitSummary>,
    /// Entry-specific warnings produced by the build.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Effective arrangement override carried by one playlist item.
#[derive(Debug, Clone, Serialize)]
pub struct PlaylistSelectionSummary {
    /// Selected native arrangement UUID.
    pub arrangement_uuid: String,
    /// Exact selected native arrangement display name.
    pub arrangement_name: String,
    /// Cue indexes reached when the playlist selection is applied.
    pub operator_cue_indexes: Vec<usize>,
}

/// Result summary from a complete service build.
#[derive(Debug, Clone, Serialize)]
pub struct BuildServiceResult {
    /// Final playlist package path.
    pub playlist_path: PathBuf,
    /// Atomic machine-readable evidence sidecar committed with the playlist.
    pub receipt_path: PathBuf,
    /// Aggregate content revision recorded inside the receipt.
    pub receipt_revision: String,
    /// Native layout implementation identity bound into the receipt.
    pub text_fit_contract: TextFitContractSummary,
    /// Native package shape written by the build.
    pub package_mode: PlaylistExportMode,
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
