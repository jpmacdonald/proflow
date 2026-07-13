//! MCP server for autonomous service preparation.
//!
//! Exposes the reviewed service workflow as MCP tools so an LLM can prep a
//! service without bypassing project configuration or preview approval.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Utc;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, Implementation, ServerCapabilities, ServerInfo};
use rmcp::{schemars, tool, tool_handler, tool_router, ServerHandler};
use serde::Serialize;
use tokio::sync::Mutex;

use crate::bible::BibleService;
use crate::config::Config;
use crate::paths::{data_root, find_data_subdir, project_config_path};
use crate::planning_center::types::{Plan, Service};
use crate::planning_center::PlanningCenterClient;
use crate::project_config::{
    load_project_config, parse_project_config_value, validate_project_config, write_project_config,
    BackgroundId, ConfigValidationIssue, ProjectConfig,
};
use crate::propresenter::background::resolve_background_image;
use crate::propresenter::macros::{MacroCache, MacroCacheLoadError};
use crate::propresenter::package::PlaylistPackageMode;
use crate::propresenter::playlist::{PlaylistMediaAsset, PlaylistMetadata};
use crate::propresenter::template::{ThemeCache, ThemeCacheLoadError};
use crate::setup;
use crate::utils::file_index::FileIndex;
use crate::workflow::classify;
use crate::workflow::execute::{
    BuildRequest, EntryOverride as WorkflowEntryOverride, OverrideSlideType, ReviewedBuildRequest,
    ServiceBuildExecutor,
};
use crate::workflow::plan::ResolvedBackground;

const DEFAULT_DAYS_AHEAD: i64 = 30;
const MIN_PREVIEW_LOOKAHEAD_DAYS: i64 = 60;
const MAX_DAYS_AHEAD: i64 = 365;

// ---------------------------------------------------------------------------
// Server struct
// ---------------------------------------------------------------------------

/// Shared MCP server state holding all service clients and caches.
#[derive(Clone)]
pub struct ProFlowServer {
    mappings: Arc<ProjectConfig>,
    pco_client: Arc<PlanningCenterClient>,
    bible_service: Arc<Mutex<BibleService>>,
    file_index: Arc<Mutex<Option<FileIndex>>>,
    template_cache: Arc<ThemeCache>,
    macro_cache: Arc<MacroCache>,
    playlist_metadata: Arc<PlaylistMetadata>,
    library_path: Option<PathBuf>,
    playlist_output_dir: Option<PathBuf>,
    generated_presentation_dir: Option<PathBuf>,
    reviewed_plans: Arc<Mutex<HashMap<String, ReviewedPlanSnapshot>>>,
}

struct ReviewedPlanSnapshot {
    revision: String,
    reviewed: ReviewedBuildRequest,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
enum PreviewPlanError {
    #[error("plan '{plan_id}' was not found in the next {days_ahead} days")]
    NotFound { plan_id: String, days_ahead: i64 },
    #[error("service_name mismatch for plan '{plan_id}': caller supplied '{supplied}', Planning Center reports '{actual}'")]
    ServiceNameMismatch {
        plan_id: String,
        supplied: String,
        actual: String,
    },
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
enum ReviewedPlanError {
    #[error("plan '{plan_id}' has no unconsumed reviewed preview in this server process")]
    Missing { plan_id: String },
    #[error("preview_revision for plan '{plan_id}' is stale or invalid")]
    RevisionMismatch { plan_id: String },
    #[error(
        "service_name must match Planning Center metadata '{actual}' for the reviewed preview"
    )]
    ServiceNameMismatch { actual: String },
}

#[derive(Debug, thiserror::Error)]
enum ConfigAssetValidationError {
    #[error("failed to load configured theme: {0}")]
    Theme(#[from] ThemeCacheLoadError),
    #[error("failed to load installed macros: {0}")]
    Macros(#[from] MacroCacheLoadError),
    #[error("candidate config references unavailable assets: {details}")]
    Unresolved { details: String },
}

struct ResolvedPlanMetadata {
    service_name: String,
    plan_title: String,
    date: String,
    default_playlist_name: String,
}

/// Errors that prevent the MCP server from starting in a coherent state.
#[derive(Debug, thiserror::Error)]
pub enum ProFlowServerInitError {
    /// Planning Center credentials are required by every operational workflow.
    #[error("Planning Center credentials not configured; set PCO_APP_ID and PCO_SECRET")]
    MissingCredentials,
    /// The MCP workflow requires a concrete `ProPresenter` library root.
    #[error("ProPresenter library not found; set LIBRARY_DIR to the library folder")]
    MissingLibrary,
    /// The configured project file could not be loaded.
    #[error("failed to load project config at {path}: {message}")]
    ProjectConfig {
        /// Config path that could not be loaded.
        path: PathBuf,
        /// Underlying load failure.
        message: String,
    },
    /// The configured project file parsed but violated runtime invariants.
    #[error("project config at {path} is invalid: {message}")]
    InvalidProjectConfig {
        /// Config path that failed validation.
        path: PathBuf,
        /// Collected validation failures.
        message: String,
    },
    /// The configured `ProPresenter` library could not be indexed.
    #[error("failed to index ProPresenter library at {path}: {message}")]
    Library {
        /// Library root that could not be indexed.
        path: PathBuf,
        /// Underlying index failure.
        message: String,
    },
    /// The live playlist library did not provide a valid producer profile.
    #[error("failed to read playlist metadata for ProPresenter library at {path}: {message}")]
    PlaylistMetadata {
        /// Configured library used to find `Playlists/Library`.
        path: PathBuf,
        /// Underlying metadata read or decode failure.
        message: String,
    },
    /// A configured theme or macro document could not be loaded.
    #[error("failed to load configured display assets: {message}")]
    DisplayAssets {
        /// Underlying theme or macro load failure.
        message: String,
    },
    /// A configured cue role or background references an unavailable asset.
    #[error("project config references unavailable assets: {message}")]
    UnresolvedAssets {
        /// Collected unresolved references.
        message: String,
    },
}

fn unresolved_assets(
    mappings: &ProjectConfig,
    theme_cache: &ThemeCache,
    macro_cache: &MacroCache,
    project_data_root: &Path,
) -> Vec<String> {
    let mut issues = Vec::new();
    for (role_key, role) in &mappings.cue_roles {
        match theme_cache.text_template(&role.slide) {
            Ok(slide) => {
                if let Some(issue) = theme_slide_size_issue(
                    role_key,
                    &role.slide,
                    slide,
                    mappings.defaults.presentation_size,
                ) {
                    issues.push(issue);
                }
            }
            Err(error) => issues.push(format!("cue_role '{role_key}': {error}")),
        }
        for (field, macro_name) in [
            ("enter_macro", role.enter_macro.as_deref()),
            (
                "all_content_colored_macro",
                role.all_content_colored_macro.as_deref(),
            ),
        ] {
            if let Some(name) = macro_name {
                if macro_cache.find(name).is_none() {
                    issues.push(format!(
                        "cue_role '{role_key}' references {field} '{name}' not found"
                    ));
                }
            }
        }
    }

    for (background_id, relative_path) in &mappings.backgrounds {
        if let Err(error) = resolve_background_image(project_data_root, relative_path.as_path()) {
            issues.push(format!(
                "background '{background_id}' at '{}': {error}",
                relative_path.as_path().display()
            ));
        }
    }

    issues.sort();
    issues
}

fn theme_slide_size_issue(
    role_key: &str,
    slide_name: &str,
    slide: &crate::propresenter::generated::rv_data::PresentationSlide,
    expected: crate::propresenter::PresentationSize,
) -> Option<String> {
    match crate::propresenter::resolution::inspect_slide_size(slide) {
        Ok(actual) if actual == expected => None,
        Ok(actual) => Some(format!(
            "cue_role '{role_key}' theme slide '{slide_name}' is {actual}; expected {expected}"
        )),
        Err(error) => Some(format!(
            "cue_role '{role_key}' theme slide '{slide_name}' has no valid {expected} canvas: {error}"
        )),
    }
}

impl ProFlowServer {
    /// Build a new server from loaded configuration.
    ///
    /// Startup succeeds only after credentials, project config, and configured
    /// library state have been loaded successfully.
    pub fn new(config: &Config) -> Result<Self, ProFlowServerInitError> {
        if !config.has_planning_center_credentials() {
            return Err(ProFlowServerInitError::MissingCredentials);
        }

        let pco_client = PlanningCenterClient::new(config);

        let bible_path = find_data_subdir("bibles");
        let bible_service = BibleService::new(bible_path);

        let library_path = env_path("LIBRARY_DIR")
            .or_else(crate::utils::file_index::get_default_library_path)
            .ok_or(ProFlowServerInitError::MissingLibrary)?;
        let playlist_output_dir =
            Some(env_path("PLAYLIST_DIR").unwrap_or_else(|| library_path.clone()));
        let generated_presentation_dir = Some(
            env_path("GENERATED_PRESENTATIONS_DIR")
                .unwrap_or_else(|| default_generated_presentation_dir(&library_path)),
        );
        let playlist_metadata =
            PlaylistMetadata::read_from_library_dir(&library_path).map_err(|error| {
                ProFlowServerInitError::PlaylistMetadata {
                    path: library_path.clone(),
                    message: error.to_string(),
                }
            })?;

        let file_index = Some(FileIndex::build(&library_path).map_err(|error| {
            ProFlowServerInitError::Library {
                path: library_path.clone(),
                message: error.to_string(),
            }
        })?);

        // Load and validate one immutable runtime snapshot. Config activation
        // writes a new snapshot for the next server process; it never mixes new
        // mappings with caches created from an older config.
        let mappings_path = project_config_path();
        let mappings = load_project_config(&mappings_path).map_err(|error| {
            ProFlowServerInitError::ProjectConfig {
                path: mappings_path.clone(),
                message: error.to_string(),
            }
        })?;
        let issues = validate_project_config(&mappings);
        if !issues.is_empty() {
            let message = issues
                .iter()
                .map(|issue| format!("{}: {}", issue.path, issue.message))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(ProFlowServerInitError::InvalidProjectConfig {
                path: mappings_path,
                message,
            });
        }
        let theme_name = mappings.defaults.theme.as_deref();

        let template_cache = ThemeCache::load(theme_name).map_err(|error| {
            ProFlowServerInitError::DisplayAssets {
                message: error.to_string(),
            }
        })?;
        let macro_cache =
            MacroCache::load_default().map_err(|error| ProFlowServerInitError::DisplayAssets {
                message: error.to_string(),
            })?;
        let project_data_root = data_root();
        let unresolved =
            unresolved_assets(&mappings, &template_cache, &macro_cache, &project_data_root);
        if !unresolved.is_empty() {
            return Err(ProFlowServerInitError::UnresolvedAssets {
                message: unresolved.join("; "),
            });
        }

        Ok(Self {
            mappings: Arc::new(mappings),
            pco_client: Arc::new(pco_client),
            bible_service: Arc::new(Mutex::new(bible_service)),
            file_index: Arc::new(Mutex::new(file_index)),
            template_cache: Arc::new(template_cache),
            macro_cache: Arc::new(macro_cache),
            playlist_metadata: Arc::new(playlist_metadata),
            library_path: Some(library_path),
            playlist_output_dir,
            generated_presentation_dir,
            reviewed_plans: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    fn service_build_executor(&self) -> ServiceBuildExecutor<'_> {
        ServiceBuildExecutor::new(
            self.pco_client.as_ref(),
            &self.bible_service,
            &self.file_index,
            self.template_cache.as_ref(),
            self.macro_cache.as_ref(),
            self.playlist_metadata.as_ref(),
            self.playlist_output_dir.as_deref(),
            self.generated_presentation_dir.as_deref(),
        )
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var(name)
        .ok()
        .map(|value| PathBuf::from(shellexpand::tilde(&value).to_string()))
}

fn default_generated_presentation_dir(library_path: &Path) -> PathBuf {
    let default_library = library_path.join("Default");
    if default_library.is_dir() {
        default_library
    } else {
        library_path.to_path_buf()
    }
}

// ---------------------------------------------------------------------------
// Tool argument structs
// ---------------------------------------------------------------------------

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
    /// Package mode. Defaults to portable when the reviewed plan references media.
    #[schemars(
        description = "Package mode: library_local or export_portable; plans with managed media default to export_portable"
    )]
    pub package_mode: Option<PlaylistPackageMode>,
    /// Extra media files to bind to an `export_portable` preview/build.
    #[schemars(description = "Extra media files to bind to an export_portable build")]
    pub media_assets: Option<Vec<PlaylistMediaAssetArg>>,
}

/// Arguments for the `fetch_plan` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
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
pub struct ExplainRuleMatchArgs {
    /// Item title as it appears in Planning Center.
    #[schemars(description = "Planning Center item title to classify/explain")]
    pub title: String,
    /// Optional description/body text.
    #[schemars(description = "Optional Planning Center item description/body text")]
    pub description: Option<String>,
    /// Optional category string: text, graphic, title, song, or other.
    #[schemars(description = "Optional item category: text, graphic, title, song, or other")]
    pub category: Option<crate::planning_center::types::Category>,
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
pub struct EntryOverride {
    /// Stable output key from preview output.
    #[schemars(description = "Stable output_key from preview output")]
    pub output_key: String,
    /// Override the playlist name.
    #[schemars(description = "Override the playlist/file name")]
    pub playlist_name: Option<String>,
    /// Use this existing `.pro` file for the entry instead of the preview action.
    #[schemars(description = "Use this existing .pro file for the entry")]
    pub file_path: Option<String>,
    /// Override slide type for the entry: text, lyrics/song, scripture, or nametag.
    #[schemars(description = "Override slide type: text, lyrics/song, scripture, or nametag")]
    pub slide_type: Option<OverrideSlideType>,
    /// Override the background using a configured background identifier.
    #[schemars(
        description = "Override background using an ID from project config backgrounds",
        with = "Option<String>"
    )]
    pub background: Option<BackgroundId>,
    /// Override the arrangement for a read-only existing presentation.
    #[schemars(description = "Override the named arrangement for a use_existing presentation")]
    pub arrangement: Option<String>,
}

// ---------------------------------------------------------------------------
// JSON response helpers
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct PlanResponse {
    service_name: String,
    plan_id: String,
    plan_title: String,
    date: String,
    items: Vec<ItemResponse>,
}

#[derive(Serialize)]
struct ItemResponse {
    id: String,
    position: usize,
    title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    category: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    song: Option<SongResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scripture: Option<ScriptureResponse>,
}

#[derive(Serialize)]
struct SongResponse {
    title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lyrics: Option<String>,
}

#[derive(Serialize)]
struct ScriptureResponse {
    reference: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
}

#[derive(Serialize)]
struct FileMatch {
    name: String,
    path: String,
}

#[derive(Serialize)]
struct ConfigValidationResponse {
    valid: bool,
    issues: Vec<ConfigValidationIssue>,
}

#[derive(Serialize)]
struct EffectiveConfigResponse {
    config: ProjectConfig,
    validation: ConfigValidationResponse,
}

#[derive(Serialize)]
struct ReviewedPreviewResponse {
    preview_revision: String,
    playlist_name: String,
    package_mode: PlaylistPackageMode,
    media_assets: Vec<ReviewedMediaAssetResponse>,
    #[serde(flatten)]
    preview: classify::PreviewResult,
}

#[derive(Serialize)]
struct ReviewedMediaAssetResponse {
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    archive_path: Option<String>,
}

#[derive(Serialize)]
struct ConfigWriteResponse {
    path: String,
    activated: bool,
    restart_required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    backup_path: Option<String>,
    validation: ConfigValidationResponse,
}

#[derive(Serialize)]
struct ExplainRuleMatchResponse {
    input: ExplainRuleMatchInput,
    entries: Vec<classify::PreviewEntry>,
}

#[derive(Serialize)]
struct ExplainRuleMatchInput {
    title: String,
    category: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    service_name: Option<String>,
}

const fn format_category(cat: crate::planning_center::types::Category) -> &'static str {
    match cat {
        crate::planning_center::types::Category::Text => "text",
        crate::planning_center::types::Category::Graphic => "graphic",
        crate::planning_center::types::Category::Title => "title",
        crate::planning_center::types::Category::Song => "song",
        crate::planning_center::types::Category::Other => "other",
    }
}

fn resolve_package_mode(
    value: Option<PlaylistPackageMode>,
    requires_portable_media: bool,
) -> PlaylistPackageMode {
    let default_mode = if requires_portable_media {
        PlaylistPackageMode::ExportPortable
    } else {
        PlaylistPackageMode::LibraryLocal
    };
    value.unwrap_or(default_mode)
}

fn parse_media_assets(args: Option<Vec<PlaylistMediaAssetArg>>) -> Vec<PlaylistMediaAsset> {
    args.unwrap_or_default()
        .into_iter()
        .map(|asset| PlaylistMediaAsset {
            source_path: PathBuf::from(asset.path),
            archive_path: asset.archive_path,
        })
        .collect()
}

fn resolve_entry_override(
    mappings: &ProjectConfig,
    entry: EntryOverride,
) -> Result<WorkflowEntryOverride, rmcp::ErrorData> {
    let background = match entry.background {
        Some(id) => {
            let file = mappings.backgrounds.get(&id).cloned().ok_or_else(|| {
                let mut available: Vec<_> = mappings
                    .backgrounds
                    .keys()
                    .map(std::string::ToString::to_string)
                    .collect();
                available.sort();
                mcp_err(format!(
                    "unknown background id '{id}'; configured backgrounds: {}",
                    available.join(", ")
                ))
            })?;
            Some(ResolvedBackground::new(id, file))
        }
        None => None,
    };

    Ok(WorkflowEntryOverride {
        output_key: entry.output_key,
        action: None,
        playlist_name: entry.playlist_name,
        file_path: entry.file_path,
        slide_type: entry.slide_type,
        background,
        arrangement: entry.arrangement,
    })
}

fn mcp_err(msg: impl Into<String>) -> rmcp::ErrorData {
    rmcp::ErrorData::internal_error(msg.into(), None)
}

fn bounded_usize(
    name: &str,
    value: Option<usize>,
    default: usize,
    maximum: usize,
) -> Result<usize, rmcp::ErrorData> {
    let value = value.unwrap_or(default);
    if (1..=maximum).contains(&value) {
        Ok(value)
    } else {
        Err(mcp_err(format!(
            "{name} must be between 1 and {maximum}, got {value}"
        )))
    }
}

fn bounded_days(value: Option<i64>, default: i64) -> Result<i64, rmcp::ErrorData> {
    let value = value.unwrap_or(default);
    if (1..=MAX_DAYS_AHEAD).contains(&value) {
        Ok(value)
    } else {
        Err(mcp_err(format!(
            "days_ahead must be between 1 and {MAX_DAYS_AHEAD}, got {value}"
        )))
    }
}

fn preview_lookahead_days(configured_days: Option<i64>) -> i64 {
    configured_days
        .unwrap_or(DEFAULT_DAYS_AHEAD)
        .clamp(MIN_PREVIEW_LOOKAHEAD_DAYS, MAX_DAYS_AHEAD)
}

fn resolve_plan_metadata(
    services: &[Service],
    plans: &[Plan],
    plan_id: &str,
    supplied_service_name: Option<&str>,
    days_ahead: i64,
) -> Result<ResolvedPlanMetadata, PreviewPlanError> {
    let plan = plans
        .iter()
        .find(|plan| plan.id == plan_id)
        .ok_or_else(|| PreviewPlanError::NotFound {
            plan_id: plan_id.to_string(),
            days_ahead,
        })?;
    let service_name = services
        .iter()
        .find(|service| service.id == plan.service_id)
        .map_or_else(|| plan.service_name.clone(), |service| service.name.clone());

    if let Some(supplied) = supplied_service_name {
        if supplied != service_name {
            return Err(PreviewPlanError::ServiceNameMismatch {
                plan_id: plan_id.to_string(),
                supplied: supplied.to_string(),
                actual: service_name,
            });
        }
    }

    let default_playlist_name = format!("{} - {service_name}", plan.date.format("%B %-d, %Y"));
    Ok(ResolvedPlanMetadata {
        service_name,
        plan_title: plan.title.clone(),
        date: plan.date.format("%Y-%m-%d").to_string(),
        default_playlist_name,
    })
}

fn consume_reviewed_plan(
    snapshots: &mut HashMap<String, ReviewedPlanSnapshot>,
    plan_id: &str,
    preview_revision: &str,
    supplied_service_name: Option<&str>,
) -> Result<ReviewedPlanSnapshot, ReviewedPlanError> {
    let reviewed = snapshots
        .get(plan_id)
        .ok_or_else(|| ReviewedPlanError::Missing {
            plan_id: plan_id.to_string(),
        })?;
    if reviewed.revision != preview_revision {
        return Err(ReviewedPlanError::RevisionMismatch {
            plan_id: plan_id.to_string(),
        });
    }
    if let Some(supplied) = supplied_service_name {
        if supplied != reviewed.reviewed.service_name() {
            return Err(ReviewedPlanError::ServiceNameMismatch {
                actual: reviewed.reviewed.service_name().to_string(),
            });
        }
    }

    snapshots
        .remove(plan_id)
        .ok_or_else(|| ReviewedPlanError::Missing {
            plan_id: plan_id.to_string(),
        })
}

fn text_result(text: impl Into<String>) -> CallToolResult {
    CallToolResult::success(vec![Content::text(text.into())])
}

fn json_result(value: &impl Serialize) -> Result<CallToolResult, rmcp::ErrorData> {
    let json = serde_json::to_string_pretty(value).map_err(|e| mcp_err(e.to_string()))?;
    Ok(text_result(json))
}

struct ConfigWriteOutcome {
    path: PathBuf,
    backup_path: Option<PathBuf>,
    activated: bool,
}

fn config_validation_or_err(
    config: &ProjectConfig,
) -> Result<ConfigValidationResponse, rmcp::ErrorData> {
    let issues = validate_project_config(config);
    if !issues.is_empty() {
        let details = issues
            .iter()
            .map(|issue| format!("{}: {}", issue.path, issue.message))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(mcp_err(format!(
            "project config validation failed: {details}"
        )));
    }
    Ok(ConfigValidationResponse {
        valid: true,
        issues,
    })
}

fn validate_candidate_assets(config: &ProjectConfig) -> Result<(), ConfigAssetValidationError> {
    let theme_cache = ThemeCache::load(config.defaults.theme.as_deref())?;
    let macro_cache = MacroCache::load_default()?;
    let issues = unresolved_assets(config, &theme_cache, &macro_cache, &data_root());
    if issues.is_empty() {
        Ok(())
    } else {
        Err(ConfigAssetValidationError::Unresolved {
            details: issues.join("; "),
        })
    }
}

fn write_config_reviewed(
    config: &ProjectConfig,
    activate: bool,
    name: Option<&str>,
) -> Result<(ConfigWriteOutcome, ConfigValidationResponse), rmcp::ErrorData> {
    let validation = config_validation_or_err(config)?;
    validate_candidate_assets(config).map_err(|error| mcp_err(error.to_string()))?;
    let live_path = project_config_path();
    let write_path = if activate {
        live_path.clone()
    } else {
        candidate_config_path(name)
    };

    let backup_path = if activate && live_path.is_file() {
        let backup = backup_config_path(&live_path);
        let parent = backup.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent).map_err(|e| mcp_err(e.to_string()))?;
        std::fs::copy(&live_path, &backup).map_err(|e| mcp_err(e.to_string()))?;
        Some(backup)
    } else {
        None
    };

    write_project_config(&write_path, config).map_err(|e| mcp_err(e.to_string()))?;

    Ok((
        ConfigWriteOutcome {
            path: write_path,
            backup_path,
            activated: activate,
        },
        validation,
    ))
}

fn candidate_config_path(name: Option<&str>) -> PathBuf {
    let base_dir = project_config_path()
        .parent()
        .map_or_else(|| PathBuf::from("data"), Path::to_path_buf);
    let dir = base_dir.join("config-candidates");
    let stamp = Utc::now().format("%Y%m%d-%H%M%S-%3f");
    let label = name
        .map(config_file_slug)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "candidate".to_string());
    dir.join(format!("{label}-{stamp}-{}.json", uuid::Uuid::new_v4()))
}

fn backup_config_path(live_path: &Path) -> PathBuf {
    let base_dir = live_path
        .parent()
        .map_or_else(|| PathBuf::from("data"), Path::to_path_buf);
    let dir = base_dir.join("config-backups");
    let stem = live_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("proflow.config");
    let stamp = Utc::now().format("%Y%m%d-%H%M%S-%3f");
    dir.join(format!("{stem}-{stamp}-{}.json", uuid::Uuid::new_v4()))
}

fn config_file_slug(name: &str) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            slug.push('-');
            prev_dash = true;
        }
    }
    slug.trim_matches('-').to_string()
}

// ---------------------------------------------------------------------------
// Tool implementations
// ---------------------------------------------------------------------------

#[tool_router]
impl ProFlowServer {
    /// Show the normalized config the runtime is actually using.
    #[tool(
        description = "Show the project config the runtime is currently using, alongside validation results. Use this to inspect the effective config state before debugging rule behavior."
    )]
    async fn show_effective_config(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        let mappings = self.mappings.as_ref();
        let issues = validate_project_config(mappings);

        json_result(&EffectiveConfigResponse {
            config: mappings.clone(),
            validation: ConfigValidationResponse {
                valid: issues.is_empty(),
                issues,
            },
        })
    }

    /// Validate and write a reviewed full project config.
    #[tool(
        description = "Validate and write a reviewed full project config. Before any candidate, backup, or live write, reloads the configured theme and installed macros and verifies every exact cue-role slide, macro, and background file. If activate=true, backs up and replaces the live file for the next server process; restart the MCP server to activate the new immutable runtime snapshot."
    )]
    async fn write_project_config(
        &self,
        Parameters(args): Parameters<WriteProjectConfigArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let config = parse_project_config_value(args.config).map_err(|e| mcp_err(e.to_string()))?;
        let (outcome, validation) = write_config_reviewed(
            &config,
            args.activate.unwrap_or(false),
            args.name.as_deref(),
        )?;

        json_result(&ConfigWriteResponse {
            path: outcome.path.display().to_string(),
            activated: outcome.activated,
            restart_required: outcome.activated,
            backup_path: outcome
                .backup_path
                .as_ref()
                .map(|path| path.display().to_string()),
            validation,
        })
    }

    /// Inspect the local `ProPresenter` installation and current project config.
    #[tool(
        description = "Catalog local ProPresenter assets and current project config. Optionally load one exact installed theme ephemerally for discovery without changing the runtime snapshot. Returns theme slides, macros, configured backgrounds and cue roles, library files, service groups, and presentation types."
    )]
    async fn catalog_assets(
        &self,
        Parameters(args): Parameters<CatalogAssetsArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let sample_limit = bounded_usize("sample_limit", args.sample_limit, 40, 200)?;
        let mappings = self.mappings.as_ref();
        let requested_theme = args
            .theme_name
            .as_deref()
            .map(|theme_name| ThemeCache::load(Some(theme_name)))
            .transpose()
            .map_err(|error| mcp_err(format!("failed to inspect requested theme: {error}")))?;
        let theme_cache = requested_theme
            .as_ref()
            .unwrap_or_else(|| self.template_cache.as_ref());
        let file_index = self.file_index.lock().await;

        let catalog = setup::catalog_assets(
            mappings,
            theme_cache,
            self.macro_cache.as_ref(),
            file_index.as_ref(),
            self.library_path.as_deref(),
            sample_limit,
        );

        drop(file_index);
        json_result(&catalog)
    }

    /// Fetch upcoming service plans from Planning Center.
    #[tool(
        description = "Fetch upcoming service plans from Planning Center Online. Returns plans with their items (title, description, notes, category, song data, scripture references). Use this to see the full service order."
    )]
    async fn fetch_plan(
        &self,
        Parameters(args): Parameters<FetchPlanArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let days = bounded_days(
            args.days_ahead.or(self.mappings.defaults.days_ahead),
            DEFAULT_DAYS_AHEAD,
        )?;
        let (services, plans) = self
            .pco_client
            .get_upcoming_services(days)
            .await
            .map_err(|e| mcp_err(e.to_string()))?;

        let filter = args.service_type.map(|s| s.to_lowercase());
        let filtered_plans: Vec<_> = plans
            .iter()
            .filter(|p| {
                filter
                    .as_ref()
                    .is_none_or(|f| p.service_name.to_lowercase().contains(f.as_str()))
            })
            .collect();

        // Fetch items for each plan concurrently
        let item_futures: Vec<_> = filtered_plans
            .iter()
            .map(|p| {
                let client = Arc::clone(&self.pco_client);
                let plan_id = p.id.clone();
                async move { client.get_service_items(&plan_id).await }
            })
            .collect();

        let item_results = futures::future::join_all(item_futures).await;

        let mut response: Vec<PlanResponse> = Vec::new();
        for (plan, items_result) in filtered_plans.iter().zip(item_results) {
            let service_name = services
                .iter()
                .find(|s| s.id == plan.service_id)
                .map_or(plan.service_name.as_str(), |s| s.name.as_str());

            let items = items_result.map_err(|e| mcp_err(e.to_string()))?;
            let item_responses: Vec<ItemResponse> = items
                .iter()
                .map(|item| ItemResponse {
                    id: item.id.clone(),
                    position: item.position,
                    title: item.title.clone(),
                    description: item.description.clone(),
                    category: format_category(item.category).to_string(),
                    note: item.note.clone(),
                    song: item.song.as_ref().map(|s| SongResponse {
                        title: s.title.clone(),
                        author: s.author.clone(),
                        lyrics: s.lyrics.clone(),
                    }),
                    scripture: item.scripture.as_ref().map(|s| ScriptureResponse {
                        reference: s.reference.clone(),
                        text: s.text.clone(),
                    }),
                })
                .collect();

            response.push(PlanResponse {
                service_name: service_name.to_string(),
                plan_id: plan.id.clone(),
                plan_title: plan.title.clone(),
                date: plan.date.format("%Y-%m-%d").to_string(),
                items: item_responses,
            });
        }

        json_result(&response)
    }

    /// Preview the proposed playlist for a service plan.
    #[tool(
        description = "Resolve a plan's title, date, and service type from Planning Center, apply any playlist name, skip, entry override, package-mode, and media choices, then return the exact effective playlist for review. The response surfaces the final playlist identity and complete portable-media set. build_service can only consume this immutable request; the returned revision is one-time."
    )]
    async fn preview_playlist(
        &self,
        Parameters(args): Parameters<PreviewPlaylistArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let days_ahead = preview_lookahead_days(self.mappings.defaults.days_ahead);
        let (services, upcoming_plans) = self
            .pco_client
            .get_upcoming_services(days_ahead)
            .await
            .map_err(|error| mcp_err(error.to_string()))?;
        let metadata = resolve_plan_metadata(
            &services,
            &upcoming_plans,
            &args.plan_id,
            args.service_name.as_deref(),
            days_ahead,
        )
        .map_err(|error| mcp_err(error.to_string()))?;

        let items = self
            .pco_client
            .get_service_items(&args.plan_id)
            .await
            .map_err(|e| mcp_err(e.to_string()))?;

        // Load mappings
        let mappings = self.mappings.as_ref();

        // Resolve every output-affecting option before producing the preview.
        let index_guard = self.file_index.lock().await;
        let plans = classify::build_plan(
            &items,
            mappings,
            index_guard.as_ref(),
            Some(metadata.service_name.as_str()),
        );
        drop(index_guard);
        let media_assets = parse_media_assets(args.media_assets);
        let overrides = args
            .overrides
            .unwrap_or_default()
            .into_iter()
            .map(|entry| resolve_entry_override(self.mappings.as_ref(), entry))
            .collect::<Result<Vec<_>, _>>()?;
        let has_managed_background = plans.iter().any(|plan| plan.style.background.is_some())
            || overrides.iter().any(|entry| entry.background.is_some());
        let playlist_package_mode = resolve_package_mode(
            args.package_mode,
            !media_assets.is_empty() || has_managed_background,
        );
        let request = BuildRequest {
            plan_id: args.plan_id.clone(),
            service_name: Some(metadata.service_name.clone()),
            playlist_name: Some(
                args.playlist_name
                    .unwrap_or_else(|| metadata.default_playlist_name.clone()),
            ),
            skip_output_keys: args.skip_output_keys.unwrap_or_default(),
            overrides,
            playlist_package_mode,
            media_assets,
        };
        let reviewed = self
            .service_build_executor()
            .review_build_request(request, &plans, self.mappings.defaults.presentation_size)
            .map_err(|error| mcp_err(error.to_string()))?;
        let entries = classify::render_preview(reviewed.plans());
        let playlist_name = reviewed.playlist_name().to_string();
        let package_mode = reviewed.playlist_package_mode();
        let media_assets = reviewed
            .media_assets()
            .iter()
            .map(|asset| ReviewedMediaAssetResponse {
                path: asset.source_path.display().to_string(),
                archive_path: asset.archive_path.clone(),
            })
            .collect();

        let summary = classify::PreviewSummary::from_entries(&entries);

        let result = classify::PreviewResult {
            plan_title: metadata.plan_title,
            service_name: metadata.service_name.clone(),
            date: metadata.date,
            entries,
            summary,
        };

        let preview_revision = uuid::Uuid::new_v4().to_string();
        self.reviewed_plans.lock().await.insert(
            args.plan_id,
            ReviewedPlanSnapshot {
                revision: preview_revision.clone(),
                reviewed,
            },
        );

        json_result(&ReviewedPreviewResponse {
            preview_revision,
            playlist_name,
            package_mode,
            media_assets,
            preview: result,
        })
    }

    /// Explain how the current config/classifier would handle a single item.
    #[tool(
        description = "Explain how the current config and classifier would handle one Planning Center item. Returns the same preview entries as the real workflow, including expansions, cue-role slides, nametags, skips, uncertainty, backgrounds, and existing-file arrangements."
    )]
    async fn explain_rule_match(
        &self,
        Parameters(args): Parameters<ExplainRuleMatchArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let category = args
            .category
            .unwrap_or(crate::planning_center::types::Category::Text);
        let item = crate::planning_center::types::Item {
            id: "__explain__".to_string(),
            position: 1,
            title: args.title.clone(),
            description: args.description.clone(),
            category,
            note: None,
            song: args
                .song_title
                .as_ref()
                .map(|title| crate::planning_center::types::Song {
                    title: title.clone(),
                    author: None,
                    copyright: None,
                    ccli: None,
                    themes: None,
                    lyrics: None,
                    arrangement: None,
                }),
            scripture: args.scripture_reference.as_ref().map(|reference| {
                crate::planning_center::types::Scripture {
                    reference: reference.clone(),
                    text: None,
                    translation: None,
                }
            }),
        };

        let mappings = self.mappings.as_ref();
        let index_guard = self.file_index.lock().await;
        let entries = classify::build_preview(
            &[item],
            mappings,
            index_guard.as_ref(),
            args.service_name.as_deref(),
        );
        drop(index_guard);

        json_result(&ExplainRuleMatchResponse {
            input: ExplainRuleMatchInput {
                title: args.title,
                category: format_category(category).to_string(),
                service_name: args.service_name,
            },
            entries,
        })
    }

    /// Search the `ProPresenter` library for matching files.
    #[tool(
        description = "Search the ProPresenter library for .pro files matching a query. Returns file names and paths sorted by relevance. Use this to find existing presentations before generating new ones."
    )]
    async fn search_library(
        &self,
        Parameters(args): Parameters<SearchLibraryArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let max = bounded_usize("max_results", args.max_results, 10, 100)?;
        let index = self.file_index.lock().await;
        let Some(ref idx) = *index else {
            return Err(mcp_err(
                "Library not indexed. Set LIBRARY_DIR to your ProPresenter library path.",
            ));
        };

        let matches = idx.find_matches(&args.query, max);
        let results: Vec<FileMatch> = matches
            .iter()
            .map(|entry| FileMatch {
                name: entry.file_name.clone(),
                path: entry.full_path.to_string_lossy().to_string(),
            })
            .collect();

        drop(index);
        json_result(&results)
    }

    /// Generate all slides and build a playlist for an entire service in one call.
    #[tool(
        description = "Consume exactly the playlist identity, skips, overrides, package mode, media set, decisions, and source bytes captured by preview_playlist. build_service accepts no new output options. Changed sources require a new preview. A matching preview_revision is atomically consumed before build side effects, so it is one-time even when the build fails; re-preview before retrying."
    )]
    async fn build_service(
        &self,
        Parameters(args): Parameters<BuildServiceArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let reviewed = {
            let mut reviewed_plans = self.reviewed_plans.lock().await;
            consume_reviewed_plan(
                &mut reviewed_plans,
                &args.plan_id,
                &args.preview_revision,
                args.service_name.as_deref(),
            )
            .map_err(|error| mcp_err(error.to_string()))?
        };
        let result = self
            .service_build_executor()
            .build_reviewed_request(reviewed.reviewed)
            .await
            .map_err(|e| mcp_err(e.to_string()))?;
        json_result(&result)
    }
}

// ---------------------------------------------------------------------------
// `ServerHandler` impl — wires up tool routing
// ---------------------------------------------------------------------------

#[tool_handler]
impl ServerHandler for ProFlowServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::from_build_env())
            .with_instructions(
                "ProFlow MCP server for worship service preparation. \
                 Use catalog_assets (optionally with an exact theme_name), fetch_plan, and \
                 show_effective_config when authoring configuration. \
                 The only production write workflow is: preview_playlist → user confirms → \
                 build_service. Put all skips, overrides, package, media, and naming choices in \
                 preview_playlist; build_service accepts no new output decisions. Preview metadata \
                 always comes from Planning Center, and each preview_revision is consumed by one \
                 matching build attempt. Use \
                 explain_rule_match and search_library for read-only inspection."
                    .to_string(),
            )
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{
        backup_config_path, bounded_days, bounded_usize, consume_reviewed_plan,
        preview_lookahead_days, resolve_entry_override, resolve_package_mode,
        resolve_plan_metadata, theme_slide_size_issue, unresolved_assets, BuildServiceArgs,
        EntryOverride, PreviewPlanError, ReviewedPlanError, ReviewedPlanSnapshot,
    };
    use crate::planning_center::types::{Plan, Service};
    use crate::project_config::{BackgroundAssetPath, BackgroundId, CueRoleConfig, ProjectConfig};
    use crate::propresenter::macros::MacroCache;
    use crate::propresenter::package::PlaylistPackageMode;
    use crate::propresenter::template::ThemeCache;
    use crate::workflow::execute::ReviewedBuildRequest;
    use chrono::Utc;
    use std::collections::HashMap;
    use std::path::Path;

    #[test]
    fn bounded_arguments_reject_zero_and_oversized_values() {
        assert!(bounded_usize("max_results", Some(0), 10, 100).is_err());
        assert!(bounded_usize("max_results", Some(101), 10, 100).is_err());
        assert!(bounded_days(Some(0), 30).is_err());
        assert!(bounded_days(Some(366), 30).is_err());
    }

    #[test]
    fn bounded_arguments_accept_defaults_and_limits() {
        assert_eq!(bounded_usize("max_results", None, 10, 100).ok(), Some(10));
        assert_eq!(
            bounded_usize("max_results", Some(100), 10, 100).ok(),
            Some(100)
        );
        assert_eq!(bounded_days(None, 30).ok(), Some(30));
        assert_eq!(bounded_days(Some(365), 30).ok(), Some(365));
    }

    #[test]
    fn preview_lookahead_uses_at_least_sixty_days_and_never_exceeds_limit() {
        assert_eq!(preview_lookahead_days(None), 60);
        assert_eq!(preview_lookahead_days(Some(14)), 60);
        assert_eq!(preview_lookahead_days(Some(90)), 90);
        assert_eq!(preview_lookahead_days(Some(500)), 365);
    }

    #[test]
    fn managed_media_defaults_to_portable_without_overriding_explicit_mode() {
        assert_eq!(
            resolve_package_mode(None, true),
            PlaylistPackageMode::ExportPortable
        );
        assert_eq!(
            resolve_package_mode(None, false),
            PlaylistPackageMode::LibraryLocal
        );
        assert_eq!(
            resolve_package_mode(Some(PlaylistPackageMode::LibraryLocal), true),
            PlaylistPackageMode::LibraryLocal
        );
    }

    #[test]
    fn cue_role_theme_slide_must_match_project_presentation_size() {
        let slide = crate::propresenter::generated::rv_data::PresentationSlide {
            base_slide: Some(crate::propresenter::generated::rv_data::Slide {
                size: Some(crate::propresenter::generated::rv_data::graphics::Size {
                    width: 1280.0,
                    height: 720.0,
                }),
                ..crate::propresenter::generated::rv_data::Slide::default()
            }),
            ..crate::propresenter::generated::rv_data::PresentationSlide::default()
        };
        let expected =
            crate::propresenter::PresentationSize::new(1920, 1080).expect("valid expected size");

        let issue = theme_slide_size_issue("content", "Content", &slide, expected)
            .expect("legacy theme slide must be rejected");

        assert!(issue.contains("1280x720"));
        assert!(issue.contains("1920x1080"));
    }

    #[test]
    fn config_backups_never_reuse_a_path() {
        let live = Path::new("/tmp/proflow/proflow.config.json");
        let first = backup_config_path(live);
        let second = backup_config_path(live);

        assert_ne!(first, second);
        assert_eq!(
            first.parent(),
            Some(Path::new("/tmp/proflow/config-backups"))
        );
    }

    #[test]
    fn preview_metadata_comes_from_planning_center_and_rejects_mismatch() {
        let services = vec![Service {
            id: "service-1".to_string(),
            name: "Sunday Morning".to_string(),
        }];
        let plans = vec![Plan {
            id: "plan-1".to_string(),
            service_id: "service-1".to_string(),
            service_name: "stale embedded name".to_string(),
            date: Utc::now(),
            title: "Fourth Sunday".to_string(),
            items: Vec::new(),
        }];

        let resolved =
            resolve_plan_metadata(&services, &plans, "plan-1", Some("Sunday Morning"), 60)
                .expect("matching Planning Center metadata should resolve");
        assert_eq!(resolved.service_name, "Sunday Morning");
        assert_eq!(resolved.plan_title, "Fourth Sunday");
        assert!(!resolved.date.is_empty());
        assert!(resolved
            .default_playlist_name
            .ends_with(" - Sunday Morning"));

        let mismatch =
            resolve_plan_metadata(&services, &plans, "plan-1", Some("Christmas Eve"), 60);
        assert!(matches!(
            mismatch,
            Err(PreviewPlanError::ServiceNameMismatch { .. })
        ));
        let missing = resolve_plan_metadata(&services, &plans, "missing", None, 60);
        assert_eq!(
            missing.err(),
            Some(PreviewPlanError::NotFound {
                plan_id: "missing".to_string(),
                days_ahead: 60,
            })
        );
    }

    #[test]
    fn build_tool_rejects_output_options_that_were_not_bound_by_preview() {
        let error = serde_json::from_value::<BuildServiceArgs>(serde_json::json!({
            "plan_id": "plan-1",
            "preview_revision": "revision-1",
            "playlist_name": "Changed after preview"
        }))
        .expect_err("build_service must not accept new output-affecting options");

        assert!(error.to_string().contains("unknown field `playlist_name`"));
    }

    #[test]
    fn reviewed_revision_is_consumed_once_and_stale_calls_preserve_current_preview() {
        let mut snapshots = HashMap::from([(
            "plan-1".to_string(),
            ReviewedPlanSnapshot {
                revision: "revision-2".to_string(),
                reviewed: ReviewedBuildRequest::offline_test(
                    "plan-1",
                    "Sunday Morning",
                    "May 24, 2026 - Sunday Morning",
                )
                .expect("empty reviewed request should capture"),
            },
        )]);

        let stale = consume_reviewed_plan(
            &mut snapshots,
            "plan-1",
            "revision-1",
            Some("Sunday Morning"),
        );
        assert_eq!(
            stale.err(),
            Some(ReviewedPlanError::RevisionMismatch {
                plan_id: "plan-1".to_string(),
            })
        );
        assert!(snapshots.contains_key("plan-1"));

        let mismatch = consume_reviewed_plan(
            &mut snapshots,
            "plan-1",
            "revision-2",
            Some("Christmas Eve"),
        );
        assert_eq!(
            mismatch.err(),
            Some(ReviewedPlanError::ServiceNameMismatch {
                actual: "Sunday Morning".to_string(),
            })
        );
        assert!(snapshots.contains_key("plan-1"));

        let consumed = consume_reviewed_plan(
            &mut snapshots,
            "plan-1",
            "revision-2",
            Some("Sunday Morning"),
        )
        .expect("matching revision should be consumed");
        assert_eq!(consumed.reviewed.service_name(), "Sunday Morning");
        assert!(!snapshots.contains_key("plan-1"));

        let reused = consume_reviewed_plan(
            &mut snapshots,
            "plan-1",
            "revision-2",
            Some("Sunday Morning"),
        );
        assert_eq!(
            reused.err(),
            Some(ReviewedPlanError::Missing {
                plan_id: "plan-1".to_string(),
            })
        );
    }

    #[test]
    fn startup_asset_check_covers_every_cue_role_binding_and_background() {
        let root = tempfile::tempdir().expect("temporary data root should exist");
        std::fs::create_dir(root.path().join("backgrounds"))
            .expect("background directory should be created");
        std::fs::write(root.path().join("backgrounds/empty.png"), [])
            .expect("empty test background should be written");

        let mut config = ProjectConfig::default();
        config.backgrounds.insert(
            BackgroundId::new("default").expect("valid background id"),
            BackgroundAssetPath::new("backgrounds/empty.png").expect("valid relative path"),
        );
        config.cue_roles.insert(
            "scripture".to_string(),
            CueRoleConfig {
                slide: "Scripture".to_string(),
                enter_macro: Some("Scripture/Prayer".to_string()),
                all_content_colored_macro: Some("Scripture/Prayer (Highlighted)".to_string()),
            },
        );

        let themes = ThemeCache::load(None).expect("empty theme cache should load");
        let issues = unresolved_assets(&config, &themes, &MacroCache::empty(), root.path());

        assert!(issues
            .iter()
            .any(|issue| issue.contains("theme slide 'Scripture' was not found")));
        assert!(issues
            .iter()
            .any(|issue| issue.contains("enter_macro 'Scripture/Prayer' not found")));
        assert!(issues.iter().any(|issue| issue
            .contains("all_content_colored_macro 'Scripture/Prayer (Highlighted)' not found")));
        assert!(issues
            .iter()
            .any(|issue| issue.contains("background 'default'") && issue.contains("empty")));
    }

    #[test]
    fn build_override_resolves_registered_background_once() {
        let id = BackgroundId::new("sermon").expect("valid background id");
        let path =
            BackgroundAssetPath::new("backgrounds/sermon.png").expect("valid background path");
        let mut config = ProjectConfig::default();
        config.backgrounds.insert(id.clone(), path.clone());

        let resolved = resolve_entry_override(
            &config,
            EntryOverride {
                output_key: "item-1:0".to_string(),
                playlist_name: None,
                file_path: None,
                slide_type: None,
                background: Some(id.clone()),
                arrangement: None,
            },
        )
        .expect("registered background should resolve");

        let background = resolved
            .background
            .expect("resolved override should carry background");
        assert_eq!(background.id(), &id);
        assert_eq!(background.file(), &path);
    }

    #[test]
    fn build_override_rejects_unknown_background_id() {
        let result = resolve_entry_override(
            &ProjectConfig::default(),
            EntryOverride {
                output_key: "item-1:0".to_string(),
                playlist_name: None,
                file_path: None,
                slide_type: None,
                background: Some(BackgroundId::new("missing").expect("valid background id")),
                arrangement: None,
            },
        );

        assert!(result.is_err());
    }
}
