//! MCP wire contracts.
//!
//! These types describe the JSON accepted and returned by the MCP boundary.
//! Runtime workflow types stay out of this module so changes to internal
//! planning do not silently change the public tool schema. The one shared
//! wire type is [`EntryOverrideAction`]: a plan's `review.toml` records the
//! same action an override carries here, so both read one definition.

use rmcp::schemars;
use serde::Serialize;

pub use crate::workflow::execute::EntryOverrideAction;

use crate::planning_center::types::Category;
use crate::project_config::{ConfigValidationIssue, ProjectConfig};
use crate::workflow::classify;
use crate::workflow::execute::OverrideSlideType;
use proflow_propresenter::playlist::PlaylistExportMode;

/// Arguments for the `preview_playlist` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PreviewPlaylistArgs {
    /// Plan ID from `fetch_plan` results.
    #[schemars(description = "Plan ID to preview (from fetch_plan results)")]
    pub plan_id: String,
    /// Optional assertion about the plan's Planning Center service type.
    #[schemars(
        description = "Optional exact service type assertion. The preview always uses Planning Center metadata and rejects a mismatch."
    )]
    pub service_name: Option<String>,
    /// Optional playlist name. Defaults to the resolved service date and type.
    #[schemars(description = "Custom playlist name (default: resolved service date and type)")]
    pub playlist_name: Option<String>,
    /// Output keys to skip from the reviewed preview.
    #[schemars(description = "Output keys to skip in this reviewed build")]
    pub skip_output_keys: Option<Vec<String>>,
    /// Per-entry decisions applied before the reviewed preview is returned.
    #[schemars(description = "Per-entry overrides applied to this reviewed preview")]
    pub overrides: Option<Vec<EntryOverride>>,
    /// Package mode. Defaults to a portable export.
    #[schemars(description = "Package mode: export_portable (default) or explicit library_local")]
    pub package_mode: Option<PlaylistExportMode>,
    /// Extra media files to bind to an `export_portable` preview/build.
    #[schemars(description = "Extra media files to bind to an export_portable build")]
    pub media_assets: Option<Vec<PlaylistMediaAssetArg>>,
}

/// Arguments for the `fetch_plan` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FetchPlanArgs {
    /// Optional service type name to filter by (e.g. "Sunday Morning").
    #[schemars(
        description = "Filter plans to this service type name (case-insensitive substring match)"
    )]
    pub service_type: Option<String>,
    /// How many days ahead to look. Defaults to 30.
    #[schemars(description = "Number of days ahead to search for plans (default: 30)")]
    pub days_ahead: Option<i64>,
}

/// Arguments for the `search_library` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchLibraryArgs {
    /// Search query string.
    #[schemars(description = "Fuzzy search query for ProPresenter library files")]
    pub query: String,
    /// Maximum results to return. Defaults to 10.
    #[schemars(description = "Maximum number of results (default: 10)")]
    pub max_results: Option<usize>,
}

/// Arguments for the `explain_rule_match` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExplainRuleMatchArgs {
    /// Item title as it appears in Planning Center.
    #[schemars(description = "Planning Center item title to classify/explain")]
    pub title: String,
    /// Optional description/body text.
    #[schemars(description = "Optional Planning Center item description/body text")]
    pub description: Option<String>,
    /// Optional category string: text, graphic, title, song, or other.
    #[schemars(description = "Optional item category: text, graphic, title, song, or other")]
    pub category: Option<Category>,
    /// Optional service type name for service-specific overrides.
    #[schemars(description = "Optional service type name for service-specific overrides")]
    pub service_name: Option<String>,
    /// Optional song title for song items.
    #[schemars(description = "Optional linked song title for song items")]
    pub song_title: Option<String>,
    /// Optional linked scripture reference.
    #[schemars(description = "Optional linked scripture reference")]
    pub scripture_reference: Option<String>,
}

/// Arguments for the `catalog_assets` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CatalogAssetsArgs {
    /// Exact installed theme to inspect without changing the runtime snapshot.
    #[schemars(
        description = "Optional exact installed ProPresenter theme name to inspect ephemerally"
    )]
    pub theme_name: Option<String>,
    /// Maximum number of sample library files to include. Defaults to 40.
    #[schemars(description = "Maximum number of sample library files to include (default: 40)")]
    pub sample_limit: Option<usize>,
}

/// Arguments for the `write_project_config` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WriteProjectConfigArgs {
    /// Full project config object to validate and write.
    #[schemars(description = "Full project config object to validate and write")]
    pub config: serde_json::Value,
    /// Whether to activate this config at the live project config path.
    #[schemars(
        description = "Activate the config at the live project config path. If false, writes to a candidate file."
    )]
    pub activate: Option<bool>,
    /// Optional candidate filename label when not activating.
    #[schemars(
        description = "Optional filename label for candidate writes (for example 'candidate')"
    )]
    pub name: Option<String>,
}

/// A media file to include in a portable `.proplaylist` package.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaylistMediaAssetArg {
    /// Absolute path to a media file on disk.
    #[schemars(description = "Absolute path to a media file on disk")]
    pub path: String,
    /// Optional confined path inside the `.proplaylist` zip. When absent, the
    /// canonical absolute source path matches native portable exports.
    #[schemars(
        description = "Optional confined archive path; defaults to the native canonical absolute source path"
    )]
    pub archive_path: Option<String>,
}

/// Arguments for the `build_service` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BuildServiceArgs {
    /// Plan ID from `fetch_plan` results.
    #[schemars(description = "Plan ID (from fetch_plan/preview_playlist)")]
    pub plan_id: String,
    /// Revision returned by the most recent preview for this plan.
    #[schemars(description = "Required preview_revision returned by preview_playlist")]
    pub preview_revision: String,
    /// Optional assertion matching the service type already bound by preview.
    #[schemars(description = "Optional exact assertion of the previewed service type")]
    pub service_name: Option<String>,
}

/// Override for a single preview entry (by `output_key`).
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EntryOverride {
    /// Stable output key from preview output.
    #[schemars(description = "Stable output_key from preview output")]
    pub output_key: String,
    /// Override the playlist name.
    #[schemars(description = "Override the playlist/file name")]
    pub playlist_name: Option<String>,
    /// Override slide type for the entry: text, lyrics/song, scripture, or nametag.
    #[schemars(description = "Override slide type: text, lyrics/song, scripture, or nametag")]
    pub slide_type: Option<OverrideSlideType>,
    /// One complete semantic action override. Omit this to change only the
    /// playlist name or slide type.
    pub action: Option<EntryOverrideAction>,
}

#[derive(Serialize)]
pub(super) struct PlanResponse {
    pub(super) service_name: String,
    pub(super) plan_id: String,
    pub(super) plan_title: String,
    pub(super) date: String,
    pub(super) items: Vec<ItemResponse>,
}

#[derive(Serialize)]
pub(super) struct ItemResponse {
    pub(super) id: String,
    pub(super) position: usize,
    pub(super) title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) description: Option<String>,
    pub(super) category: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) song: Option<SongResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) scripture: Option<ScriptureResponse>,
}

#[derive(Serialize)]
pub(super) struct SongResponse {
    pub(super) title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) lyrics: Option<String>,
}

#[derive(Serialize)]
pub(super) struct ScriptureResponse {
    pub(super) reference: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) text: Option<String>,
}

#[derive(Serialize)]
pub(super) struct FileMatch {
    pub(super) name: String,
    pub(super) path: String,
}

#[derive(Serialize)]
pub(super) struct ConfigValidationResponse {
    pub(super) valid: bool,
    pub(super) issues: Vec<ConfigValidationIssue>,
}

#[derive(Serialize)]
pub(super) struct EffectiveConfigResponse {
    pub(super) config: ProjectConfig,
    pub(super) validation: ConfigValidationResponse,
}

#[derive(Serialize)]
pub(super) struct ReviewedPreviewResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) preview_revision: Option<super::review::PreviewRevision>,
    /// An earlier executable preview for this plan that this preview replaced;
    /// its revision can no longer be built.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) superseded_preview_revision: Option<super::review::PreviewRevision>,
    pub(super) playlist_name: String,
    pub(super) package_mode: PlaylistExportMode,
    pub(super) media_assets: Vec<ReviewedMediaAssetResponse>,
    /// Exact final native/package evidence for an executable preview.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) materialized: Option<crate::workflow::report::BuildServiceResult>,
    #[serde(flatten)]
    pub(super) preview: classify::PreviewResult,
}

#[derive(Serialize)]
pub(super) struct ReviewedMediaAssetResponse {
    pub(super) path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) archive_path: Option<String>,
}

#[derive(Serialize)]
pub(super) struct ConfigWriteResponse {
    pub(super) path: String,
    pub(super) activated: bool,
    pub(super) restart_required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) backup_path: Option<String>,
    pub(super) validation: ConfigValidationResponse,
}

#[derive(Serialize)]
pub(super) struct ExplainRuleMatchResponse {
    pub(super) input: ExplainRuleMatchInput,
    pub(super) entries: Vec<classify::PreviewEntry>,
}

#[derive(Serialize)]
pub(super) struct ExplainRuleMatchInput {
    pub(super) title: String,
    pub(super) category: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) service_name: Option<String>,
}

pub(super) const fn format_category(category: Category) -> &'static str {
    match category {
        Category::Text => "text",
        Category::Graphic => "graphic",
        Category::Title => "title",
        Category::Song => "song",
        Category::Other => "other",
    }
}

/// Arguments for the `read_playlist` tool. Exactly one of `name` or `path`.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadPlaylistArgs {
    /// Name of a playlist or folder in the live `ProPresenter` playlist library.
    #[schemars(
        description = "Name of a playlist (or folder of playlists) in the ProPresenter playlist library. Exactly one of name or path."
    )]
    pub name: Option<String>,
    /// A `.proplaylist` file inside the configured playlist output directory or
    /// `ProPresenter` data root.
    #[schemars(
        description = "Path to a .proplaylist file inside the configured playlist output directory or ProPresenter data root; relative paths are resolved against the playlist output directory. Exactly one of name or path."
    )]
    pub path: Option<String>,
}
