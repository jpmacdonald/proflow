//! MCP server for autonomous service preparation.
//!
//! Exposes `ProFlow` capabilities (plan fetching, library search, slide generation,
//! playlist building) as MCP tools so an LLM can prep a service end-to-end.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Utc;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, Implementation, ServerCapabilities, ServerInfo};
use rmcp::{schemars, tool, tool_handler, tool_router, ServerHandler};
use serde::Serialize;
use tokio::sync::Mutex;

use crate::bible::BibleService;
use crate::config::Config;
use crate::paths::{find_data_subdir, project_config_path};
use crate::planning_center::PlanningCenterClient;
use crate::project_config::{
    load_project_config, parse_project_config_value, validate_project_config, write_project_config,
    ConfigValidationIssue, ProjectConfig,
};
use crate::propresenter::macros::MacroCache;
use crate::propresenter::playlist::playlist_output_path;
use crate::propresenter::playlist::{build_playlist, write_playlist_file, PlaylistEntry};
use crate::propresenter::rtf::StyledSegment;
use crate::propresenter::template::ThemeCache;
use crate::propresenter::SlideType;
use crate::setup;
use crate::utils::file_index::FileIndex;
use crate::workflow::classify;
use crate::workflow::execute::{
    BuildRequest, EntryOverride as WorkflowEntryOverride, ServiceBuildExecutor,
    SingleGenerateRequest,
};

// ---------------------------------------------------------------------------
// Server struct
// ---------------------------------------------------------------------------

/// Shared MCP server state holding all service clients and caches.
#[derive(Clone)]
pub struct ProFlowServer {
    config: Arc<Config>,
    pco_client: Arc<PlanningCenterClient>,
    bible_service: Arc<Mutex<BibleService>>,
    file_index: Arc<Mutex<Option<FileIndex>>>,
    template_cache: Arc<Mutex<ThemeCache>>,
    macro_cache: Arc<MacroCache>,
    library_path: Option<PathBuf>,
    tool_router: ToolRouter<Self>,
}

impl ProFlowServer {
    /// Build a new server from loaded configuration.
    ///
    /// Returns `None` if Planning Center credentials are missing.
    pub fn new(config: Config) -> Option<Self> {
        if !config.has_planning_center_credentials() {
            return None;
        }

        let pco_client = PlanningCenterClient::new(&config);

        let bible_path = find_data_subdir("bibles");
        let bible_service = BibleService::new(bible_path);

        let library_path = std::env::var("LIBRARY_DIR")
            .ok()
            .map(|s| PathBuf::from(shellexpand::tilde(&s).to_string()))
            .or_else(crate::utils::file_index::get_default_library_path)
            .or_else(|| {
                config.propresenter_path.as_ref().and_then(|pro_dir| {
                    let path = PathBuf::from(shellexpand::tilde(pro_dir).to_string())
                        .join("Libraries/Default");
                    path.exists().then_some(path)
                })
            });

        let file_index = library_path.as_ref().and_then(|p| FileIndex::build(p).ok());

        let mut template_paths = Vec::new();
        if let Some(ref lib) = library_path {
            template_paths.push(lib.clone());
        }
        template_paths.push(find_data_subdir("templates"));

        // Load and validate config at startup — fail fast on bad config
        let mappings = load_service_config();
        let issues = validate_project_config(&mappings);
        if !issues.is_empty() {
            for issue in &issues {
                eprintln!("Config error at {}: {}", issue.path, issue.message);
            }
            eprintln!(
                "Fix config issues in {} before starting",
                project_config_path().display()
            );
            return None;
        }
        let theme_name = mappings.defaults.theme.as_deref();

        let mut template_cache = ThemeCache::new(theme_name, template_paths);
        let macro_cache = MacroCache::load_default();

        if macro_cache.is_empty() {
            eprintln!("Warning: no macros loaded — macro triggers in config will be no-ops");
        }

        // Validate that config template/macro references resolve
        for (type_key, ptype) in &mappings.presentation_types {
            if let Some(ref tmpl) = ptype.template {
                if template_cache.get(tmpl).is_none() {
                    eprintln!(
                        "Warning: presentation_type '{type_key}' references template '{tmpl}' not found in theme"
                    );
                }
            }
            if let Some(ref tmpl) = ptype.title_template {
                if template_cache.get(tmpl).is_none() {
                    eprintln!(
                        "Warning: presentation_type '{type_key}' references title_template '{tmpl}' not found in theme"
                    );
                }
            }
            if let Some(ref mac) = ptype.macro_name {
                if macro_cache.find(mac).is_none() {
                    eprintln!(
                        "Warning: presentation_type '{type_key}' references macro '{mac}' not found"
                    );
                }
            }
            if let Some(ref mac) = ptype.content_macro {
                if macro_cache.find(mac).is_none() {
                    eprintln!(
                        "Warning: presentation_type '{type_key}' references content_macro '{mac}' not found"
                    );
                }
            }
        }

        Some(Self {
            config: Arc::new(config),
            pco_client: Arc::new(pco_client),
            bible_service: Arc::new(Mutex::new(bible_service)),
            file_index: Arc::new(Mutex::new(file_index)),
            template_cache: Arc::new(Mutex::new(template_cache)),
            macro_cache: Arc::new(macro_cache),
            library_path,
            tool_router: Self::tool_router(),
        })
    }
}

// ---------------------------------------------------------------------------
// Tool argument structs
// ---------------------------------------------------------------------------

/// Arguments for the `preview_playlist` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct PreviewPlaylistArgs {
    /// Plan ID from `fetch_plan` results.
    #[schemars(description = "Plan ID to preview (from fetch_plan results)")]
    pub plan_id: String,
    /// Service type name (for context-aware defaults).
    #[schemars(
        description = "Service type name (e.g. '10:30am traditional') for context-aware defaults"
    )]
    pub service_name: Option<String>,
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
    pub category: Option<String>,
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

/// Arguments for the `find_unmapped_items` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct FindUnmappedItemsArgs {
    /// Plan ID from `fetch_plan` results.
    #[schemars(description = "Plan ID to inspect for skipped or uncertain items")]
    pub plan_id: String,
    /// Service type name (for context-aware defaults).
    #[schemars(
        description = "Service type name (e.g. '10:30am traditional') for context-aware defaults"
    )]
    pub service_name: Option<String>,
}

/// Arguments for the `catalog_assets` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CatalogAssetsArgs {
    /// Maximum number of sample library files to include. Defaults to 40.
    #[schemars(description = "Maximum number of sample library files to include (default: 40)")]
    pub sample_limit: Option<usize>,
}

/// Arguments for the `analyze_recent_plans` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct AnalyzeRecentPlansArgs {
    /// Optional service type name to filter by.
    #[schemars(
        description = "Optional service type name filter (case-insensitive substring match)"
    )]
    pub service_type: Option<String>,
    /// How many days ahead to inspect. Defaults to project/default config.
    #[schemars(description = "Days ahead to inspect for plans (defaults to config value)")]
    pub days_ahead: Option<i64>,
    /// Maximum number of plans to analyze after filtering. Defaults to 12.
    #[schemars(description = "Maximum number of plans to analyze after filtering (default: 12)")]
    pub max_plans: Option<usize>,
    /// Maximum number of recurring patterns to return per section. Defaults to 20.
    #[schemars(
        description = "Maximum number of recurring patterns to return per section (default: 20)"
    )]
    pub max_patterns: Option<usize>,
}

/// Arguments for the `suggest_config_patch` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SuggestConfigPatchArgs {
    /// Optional service type name to filter by.
    #[schemars(
        description = "Optional service type name filter (case-insensitive substring match)"
    )]
    pub service_type: Option<String>,
    /// How many days ahead to inspect. Defaults to project/default config.
    #[schemars(description = "Days ahead to inspect for plans (defaults to config value)")]
    pub days_ahead: Option<i64>,
    /// Maximum number of plans to analyze after filtering. Defaults to 12.
    #[schemars(description = "Maximum number of plans to analyze after filtering (default: 12)")]
    pub max_plans: Option<usize>,
    /// Maximum number of unresolved patterns to turn into suggestions. Defaults to 20.
    #[schemars(
        description = "Maximum number of unresolved patterns to turn into suggestions (default: 20)"
    )]
    pub max_suggestions: Option<usize>,
}

/// Arguments for the `draft_project_config` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DraftProjectConfigArgs {
    /// Optional project/church name for metadata.
    #[schemars(description = "Optional project or church name to include in config metadata")]
    pub project_name: Option<String>,
    /// Optional service type name to filter by.
    #[schemars(
        description = "Optional service type name filter (case-insensitive substring match)"
    )]
    pub service_type: Option<String>,
    /// How many days ahead to inspect. Defaults to project/default config.
    #[schemars(description = "Days ahead to inspect for plans (defaults to config value)")]
    pub days_ahead: Option<i64>,
    /// Maximum number of plans to analyze after filtering. Defaults to 12.
    #[schemars(description = "Maximum number of plans to analyze after filtering (default: 12)")]
    pub max_plans: Option<usize>,
}

/// Arguments for the `write_project_config` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct WriteProjectConfigArgs {
    /// Full v2 project config object to validate and write.
    #[schemars(description = "Full v2 project config object to validate and write")]
    pub config: serde_json::Value,
    /// Whether to activate this config at the live project config path.
    #[schemars(
        description = "Activate the config at the live project config path. If false, writes to a candidate file."
    )]
    pub activate: Option<bool>,
    /// Optional candidate filename label when not activating.
    #[schemars(
        description = "Optional filename label for candidate writes (for example 'starter-v2')"
    )]
    pub name: Option<String>,
}

/// Arguments for the `apply_config_patch` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ApplyConfigPatchArgs {
    /// Suggested config patch object, usually copied from `suggest_config_patch.patch`.
    #[schemars(
        description = "Suggested config patch object, usually copied from suggest_config_patch.patch"
    )]
    pub patch: serde_json::Value,
    /// Whether to activate the merged config at the live project config path.
    #[schemars(
        description = "Activate the merged config at the live project config path. If false, writes to a candidate file."
    )]
    pub activate: Option<bool>,
    /// Optional candidate filename label when not activating.
    #[schemars(
        description = "Optional filename label for candidate writes (for example 'patch-2026-04-13')"
    )]
    pub name: Option<String>,
}

/// Arguments for the `generate_slides` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct GenerateSlidesArgs {
    /// Type of slide to generate.
    #[schemars(description = "Slide type: 'scripture', 'text', 'title', or 'lyrics'")]
    pub slide_type: String,
    /// Name for the presentation file.
    #[schemars(description = "Name for the generated presentation")]
    pub name: String,
    /// Scripture reference (required when `slide_type` is 'scripture').
    #[schemars(
        description = "Scripture reference, e.g. 'Isaiah 35:1-6' (required for scripture type)"
    )]
    pub scripture_reference: Option<String>,
    /// Bible version (optional, defaults to `NRSVue`).
    #[schemars(
        description = "Bible version: NRSVue, NRSV, NIV, KJV, NKJV, NLT, NASB (default: NRSVue)"
    )]
    pub bible_version: Option<String>,
    /// Content lines (required for text/title/lyrics types).
    #[schemars(description = "Content lines for the slides (required for text/title/lyrics)")]
    pub content: Option<Vec<String>>,
    /// Title text for a title slide prepended before content.
    #[schemars(description = "Optional title slide text prepended before content slides")]
    pub title_text: Option<String>,
    /// Per-segment style overrides, parallel to content entries.
    /// Each entry can override color, bold, italic independently.
    /// `null` entries use template defaults for that segment.
    #[schemars(
        description = "Optional style overrides parallel to content. Each entry: {color: '#FFFF00', bold: true, italic: false} or null for template defaults."
    )]
    pub styles: Option<Vec<Option<SegmentStyle>>>,
    /// Background image category: 'default' or 'sermon'. Omit for no background.
    #[schemars(
        description = "Background image: 'default' or 'sermon'. Omit for no background image."
    )]
    pub background: Option<String>,
    /// Arrangement name to select (e.g. 'Default'). Only applies to existing library files read back.
    #[schemars(description = "Set the active arrangement by name (e.g. 'Default').")]
    pub arrangement: Option<String>,
}

/// Style overrides for a single content segment.
///
/// All fields are optional — omitted fields use the template default.
#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
pub struct SegmentStyle {
    /// Hex color override (e.g. "#FFFF00").
    #[schemars(description = "Hex color (e.g. '#FFFF00'). Omit to use template default.")]
    pub color: Option<String>,
    /// Bold override.
    #[schemars(description = "Bold text. Omit to use template default.")]
    pub bold: Option<bool>,
    /// Italic override.
    #[schemars(description = "Italic text. Omit to use template default.")]
    pub italic: Option<bool>,
}

/// A single entry in the playlist to build.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct PlaylistItem {
    /// Display name.
    #[schemars(description = "Display name for this playlist item")]
    pub name: String,
    /// Path to the .pro file on disk.
    #[schemars(description = "Absolute path to the .pro file")]
    pub file_path: String,
    /// Slide type for filename sanitization.
    #[schemars(description = "Slide type: 'scripture', 'text', 'title', 'lyrics', or 'graphic'")]
    pub slide_type: String,
}

/// Arguments for the `build_playlist` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct BuildPlaylistArgs {
    /// Playlist name (used for the output filename).
    #[schemars(description = "Name for the playlist file")]
    pub name: String,
    /// Ordered list of items to include.
    #[schemars(description = "Ordered list of presentation items for the playlist")]
    pub items: Vec<PlaylistItem>,
}

/// Arguments for the `build_service` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct BuildServiceArgs {
    /// Plan ID from `fetch_plan` results.
    #[schemars(description = "Plan ID (from fetch_plan/preview_playlist)")]
    pub plan_id: String,
    /// Service type name for service-specific overrides.
    #[schemars(description = "Service type name (e.g. '10:30am traditional')")]
    pub service_name: Option<String>,
    /// Optional playlist name. Defaults to the service name + date.
    #[schemars(description = "Custom playlist name (default: auto-generated from service)")]
    pub playlist_name: Option<String>,
    /// Output keys to skip from preview output.
    #[schemars(description = "Output keys to skip from the preview output")]
    pub skip_output_keys: Option<Vec<String>>,
    /// Per-entry overrides (by output_key).
    #[schemars(description = "Per-entry overrides by output_key")]
    pub overrides: Option<Vec<EntryOverride>>,
}

/// Override for a single preview entry (by output_key).
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct EntryOverride {
    /// Stable output key from preview output.
    #[schemars(description = "Stable output_key from preview output")]
    pub output_key: String,
    /// Override the playlist name.
    #[schemars(description = "Override the playlist/file name")]
    pub playlist_name: Option<String>,
    /// Override the background category.
    #[schemars(description = "Override background: 'default', 'sermon', or null")]
    pub background: Option<String>,
    /// Override the arrangement name.
    #[schemars(description = "Override arrangement name")]
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
struct ServiceGroupResponse {
    name: String,
    service_types: Vec<String>,
}

#[derive(Serialize)]
struct ProfileResponse {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    service_groups: Vec<String>,
    service_types: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    days_ahead: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    review_policy: Option<String>,
}

#[derive(Serialize)]
struct ConfigProfilesResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    default_theme: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_days_ahead: Option<i64>,
    service_groups: Vec<ServiceGroupResponse>,
    profiles: Vec<ProfileResponse>,
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
struct ConfigWriteResponse {
    path: String,
    activated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    backup_path: Option<String>,
    validation: ConfigValidationResponse,
}

#[derive(Serialize)]
struct ConfigPatchApplyResponse {
    path: String,
    activated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    backup_path: Option<String>,
    added_presentation_types: Vec<String>,
    updated_presentation_types: Vec<String>,
    added_rule_ids: Vec<String>,
    updated_rule_ids: Vec<String>,
    validation: ConfigValidationResponse,
}

#[derive(Serialize)]
struct ExplainRuleMatchResponse {
    input: ExplainRuleMatchInput,
    entries: Vec<classify::PreviewEntry>,
}

#[derive(Serialize)]
struct UnmappedItemsResponse {
    plan_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    service_name: Option<String>,
    skipped_count: usize,
    uncertain_count: usize,
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

fn parse_slide_type(s: &str) -> SlideType {
    match s.to_lowercase().as_str() {
        "scripture" => SlideType::Scripture,
        "lyrics" => SlideType::Lyrics,
        "title" => SlideType::Title,
        "graphic" => SlideType::Graphic,
        _ => SlideType::Text,
    }
}

fn parse_category(s: Option<&str>) -> crate::planning_center::types::Category {
    match s.unwrap_or("text").to_lowercase().as_str() {
        "graphic" => crate::planning_center::types::Category::Graphic,
        "title" => crate::planning_center::types::Category::Title,
        "song" => crate::planning_center::types::Category::Song,
        "other" => crate::planning_center::types::Category::Other,
        _ => crate::planning_center::types::Category::Text,
    }
}

/// Parse a hex color string like "#FFFF00" or "FFFF00" into RGB.
fn parse_hex_color(s: &str) -> Option<(u8, u8, u8)> {
    let hex = s.strip_prefix('#').unwrap_or(s);
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some((r, g, b))
}

fn load_service_config() -> ProjectConfig {
    let path = project_config_path();
    match load_project_config(&path) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Warning: failed to load {}: {e}", path.display());
            ProjectConfig::default()
        }
    }
}

fn mcp_err(msg: impl Into<String>) -> rmcp::ErrorData {
    rmcp::ErrorData::internal_error(msg.into(), None)
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

struct PatchApplySummary {
    added_presentation_types: Vec<String>,
    updated_presentation_types: Vec<String>,
    added_rule_ids: Vec<String>,
    updated_rule_ids: Vec<String>,
}

fn config_validation_or_err(
    config: &ProjectConfig,
) -> Result<ConfigValidationResponse, rmcp::ErrorData> {
    let issues = validate_project_config(config);
    if !issues.is_empty() {
        return Err(mcp_err(format!(
            "project config validation failed: {}",
            serde_json::to_string(&issues).unwrap_or_else(|_| "validation issues present".into())
        )));
    }
    Ok(ConfigValidationResponse {
        valid: true,
        issues,
    })
}

fn write_config_reviewed(
    config: &ProjectConfig,
    activate: bool,
    name: Option<&str>,
) -> Result<(ConfigWriteOutcome, ConfigValidationResponse), rmcp::ErrorData> {
    let validation = config_validation_or_err(config)?;
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

fn apply_suggested_patch(
    mut config: ProjectConfig,
    patch: setup::SuggestedConfigPatch,
) -> (ProjectConfig, PatchApplySummary) {
    let mut added_presentation_types = Vec::new();
    let mut updated_presentation_types = Vec::new();
    for (name, ptype) in patch.presentation_types {
        if config
            .presentation_types
            .insert(name.clone(), ptype)
            .is_some()
        {
            updated_presentation_types.push(name);
        } else {
            added_presentation_types.push(name);
        }
    }

    let mut added_rule_ids = Vec::new();
    let mut updated_rule_ids = Vec::new();
    for rule in patch.item_rules {
        if let Some(existing) = config
            .item_rules
            .iter_mut()
            .find(|current| current.id == rule.id)
        {
            *existing = rule.clone();
            updated_rule_ids.push(rule.id);
        } else {
            added_rule_ids.push(rule.id.clone());
            config.item_rules.push(rule);
        }
    }

    (
        config,
        PatchApplySummary {
            added_presentation_types,
            updated_presentation_types,
            added_rule_ids,
            updated_rule_ids,
        },
    )
}

fn candidate_config_path(name: Option<&str>) -> PathBuf {
    let base_dir = project_config_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("data"));
    let dir = base_dir.join("config-candidates");
    let stamp = Utc::now().format("%Y%m%d-%H%M%S");
    let label = name
        .map(config_file_slug)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "candidate".to_string());
    dir.join(format!("{label}-{stamp}.json"))
}

fn backup_config_path(live_path: &Path) -> PathBuf {
    let base_dir = live_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("data"));
    let dir = base_dir.join("config-backups");
    let stem = live_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("proflow.config");
    let stamp = Utc::now().format("%Y%m%d-%H%M%S");
    dir.join(format!("{stem}-{stamp}.json"))
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
    /// Get context rules for service preparation.
    #[tool(
        description = "Get formatting rules and context for preparing worship service slides. Call this FIRST before processing any plan — it explains how to handle responsive readings, scripture, songs, and other item types."
    )]
    async fn get_context(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        let mappings = load_service_config();

        let cache = self.template_cache.lock().await;
        let theme_name = cache.theme_name().map(String::from);
        let slide_names = cache
            .theme_slide_names()
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();
        drop(cache);

        let macro_names = self
            .macro_cache
            .names()
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();

        Ok(text_result(render_mcp_context(
            &mappings,
            theme_name.as_deref(),
            &slide_names,
            &macro_names,
        )))
    }

    /// List configured service groups and build profiles from project config.
    #[tool(
        description = "List configured service groups and build profiles from project config. Use this to discover named workflows like weekly or seasonal before fetching plans or building services."
    )]
    async fn list_profiles(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        let mappings = load_service_config();

        let mut service_groups: Vec<_> = mappings
            .service_groups
            .iter()
            .map(|(name, group)| ServiceGroupResponse {
                name: name.clone(),
                service_types: group.service_types.clone(),
            })
            .collect();
        service_groups.sort_by(|a, b| a.name.cmp(&b.name));

        let mut profiles: Vec<_> = mappings
            .profiles
            .iter()
            .map(|(name, profile)| ProfileResponse {
                name: name.clone(),
                description: profile.description.clone(),
                service_groups: profile.service_groups.clone(),
                service_types: profile.service_types.clone(),
                days_ahead: profile.days_ahead,
                review_policy: profile
                    .review_policy
                    .map(|p| format!("{p:?}").to_lowercase()),
            })
            .collect();
        profiles.sort_by(|a, b| a.name.cmp(&b.name));

        json_result(&ConfigProfilesResponse {
            default_theme: mappings.defaults.theme.clone(),
            default_days_ahead: mappings.defaults.days_ahead,
            service_groups,
            profiles,
        })
    }

    /// Validate the project config for missing references and inconsistent wiring.
    #[tool(
        description = "Validate project config references and report missing service groups, unknown presentation types, or inconsistent rule wiring. Use this before relying on new config changes."
    )]
    async fn validate_config(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        let mappings = load_service_config();
        let issues = validate_project_config(&mappings);

        json_result(&ConfigValidationResponse {
            valid: issues.is_empty(),
            issues,
        })
    }

    /// Show the normalized config the runtime is actually using.
    #[tool(
        description = "Show the project config the runtime is currently using, alongside validation results. Use this to inspect the effective config state before debugging rule behavior."
    )]
    async fn show_effective_config(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        let mappings = load_service_config();
        let issues = validate_project_config(&mappings);

        json_result(&EffectiveConfigResponse {
            config: mappings,
            validation: ConfigValidationResponse {
                valid: issues.is_empty(),
                issues,
            },
        })
    }

    /// Validate and write a reviewed full project config.
    #[tool(
        description = "Validate and write a reviewed full v2 project config. If activate=true, backs up the current live config and replaces it. If activate=false, writes a candidate file under a config-candidates directory."
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
            backup_path: outcome
                .backup_path
                .as_ref()
                .map(|path| path.display().to_string()),
            validation,
        })
    }

    /// Apply a reviewed patch onto the current config and write the merged result.
    #[tool(
        description = "Apply a reviewed config patch onto the current project config. The patch should usually be copied from suggest_config_patch.patch. If activate=true, backs up and replaces the live config; otherwise writes a candidate merged config file."
    )]
    async fn apply_config_patch(
        &self,
        Parameters(args): Parameters<ApplyConfigPatchArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let patch: setup::SuggestedConfigPatch =
            serde_json::from_value(args.patch).map_err(|e| mcp_err(e.to_string()))?;
        let live_path = project_config_path();
        let current = load_project_config(&live_path).map_err(|e| mcp_err(e.to_string()))?;
        let (merged, summary) = apply_suggested_patch(current, patch);
        let (outcome, validation) = write_config_reviewed(
            &merged,
            args.activate.unwrap_or(false),
            args.name.as_deref(),
        )?;

        json_result(&ConfigPatchApplyResponse {
            path: outcome.path.display().to_string(),
            activated: outcome.activated,
            backup_path: outcome
                .backup_path
                .as_ref()
                .map(|path| path.display().to_string()),
            added_presentation_types: summary.added_presentation_types,
            updated_presentation_types: summary.updated_presentation_types,
            added_rule_ids: summary.added_rule_ids,
            updated_rule_ids: summary.updated_rule_ids,
            validation,
        })
    }

    /// Inspect the local ProPresenter installation and current project config.
    #[tool(
        description = "Catalog the local ProPresenter assets and current project config. Returns available theme slides, macros, library folders/files, service groups, profiles, and presentation types. Use this before drafting or patching config for a new church."
    )]
    async fn catalog_assets(
        &self,
        Parameters(args): Parameters<CatalogAssetsArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let sample_limit = args.sample_limit.unwrap_or(40).clamp(1, 200);
        let mappings = load_service_config();
        let template_cache = self.template_cache.lock().await;
        let file_index = self.file_index.lock().await;

        let catalog = setup::catalog_assets(
            &mappings,
            &template_cache,
            self.macro_cache.as_ref(),
            file_index.as_ref(),
            self.library_path.as_deref(),
            sample_limit,
        );

        drop(file_index);
        drop(template_cache);

        json_result(&catalog)
    }

    /// Analyze recent plan patterns to help author config.
    #[tool(
        description = "Analyze upcoming plans and summarize recurring Planning Center patterns useful for config authoring. Returns service/category breakdowns, recurring titles, normalized recurring patterns, scripture patterns, speaker candidates, and candidate rule hints."
    )]
    async fn analyze_recent_plans(
        &self,
        Parameters(args): Parameters<AnalyzeRecentPlansArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let mappings = load_service_config();
        let days = args
            .days_ahead
            .or(mappings.defaults.days_ahead)
            .unwrap_or(self.config.days_ahead);
        let (_, plans) = self
            .pco_client
            .get_upcoming_services(days)
            .await
            .map_err(|e| mcp_err(e.to_string()))?;

        let filter = args.service_type.map(|s| s.to_lowercase());
        let max_plans = args.max_plans.unwrap_or(12);
        let max_patterns = args.max_patterns.unwrap_or(20).clamp(1, 50);

        let selected_plans: Vec<_> = plans
            .into_iter()
            .filter(|plan| {
                filter
                    .as_ref()
                    .is_none_or(|value| plan.service_name.to_lowercase().contains(value.as_str()))
            })
            .take(max_plans)
            .collect();

        let item_futures: Vec<_> = selected_plans
            .iter()
            .map(|plan| {
                let client = Arc::clone(&self.pco_client);
                let plan_id = plan.id.clone();
                async move { client.get_service_items(&plan_id).await }
            })
            .collect();
        let item_results = futures::future::join_all(item_futures).await;

        let mut item_sets = Vec::with_capacity(item_results.len());
        for items in item_results {
            item_sets.push(items.map_err(|e| mcp_err(e.to_string()))?);
        }

        let analysis = setup::analyze_recent_plans(&selected_plans, &item_sets, max_patterns);
        json_result(&analysis)
    }

    /// Suggest deterministic project config changes from unresolved plan items.
    #[tool(
        description = "Suggest deterministic project config changes from unresolved upcoming plan items. Returns candidate presentation_types and item_rules additions based on uncertain preview entries, exact library matches, and existing config types. Use this to draft config patches, not to bypass review."
    )]
    async fn suggest_config_patch(
        &self,
        Parameters(args): Parameters<SuggestConfigPatchArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let mappings = load_service_config();
        let days = args
            .days_ahead
            .or(mappings.defaults.days_ahead)
            .unwrap_or(self.config.days_ahead);
        let (_, plans) = self
            .pco_client
            .get_upcoming_services(days)
            .await
            .map_err(|e| mcp_err(e.to_string()))?;

        let filter = args.service_type.map(|s| s.to_lowercase());
        let max_plans = args.max_plans.unwrap_or(12);
        let max_suggestions = args.max_suggestions.unwrap_or(20).clamp(1, 50);

        let selected_plans: Vec<_> = plans
            .into_iter()
            .filter(|plan| {
                filter
                    .as_ref()
                    .is_none_or(|value| plan.service_name.to_lowercase().contains(value.as_str()))
            })
            .take(max_plans)
            .collect();

        let item_futures: Vec<_> = selected_plans
            .iter()
            .map(|plan| {
                let client = Arc::clone(&self.pco_client);
                let plan_id = plan.id.clone();
                async move { client.get_service_items(&plan_id).await }
            })
            .collect();
        let item_results = futures::future::join_all(item_futures).await;

        let mut item_sets = Vec::with_capacity(item_results.len());
        for items in item_results {
            item_sets.push(items.map_err(|e| mcp_err(e.to_string()))?);
        }

        let template_cache = self.template_cache.lock().await;
        let file_index = self.file_index.lock().await;
        let suggestion = setup::suggest_config_patch(
            &mappings,
            &selected_plans,
            &item_sets,
            file_index.as_ref(),
            &template_cache,
            self.macro_cache.as_ref(),
            max_suggestions,
        );
        drop(file_index);
        drop(template_cache);

        json_result(&suggestion)
    }

    /// Draft a starter v2 project config from assets and recent plans.
    #[tool(
        description = "Draft a starter v2 project config from local ProPresenter assets and recent Planning Center plans. Returns a conservative scaffold with service groups, profiles, starter presentation_types, and starter item_rules. Review before writing it to disk."
    )]
    async fn draft_project_config(
        &self,
        Parameters(args): Parameters<DraftProjectConfigArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let current = load_service_config();
        let days = args
            .days_ahead
            .or(current.defaults.days_ahead)
            .unwrap_or(self.config.days_ahead);
        let (_, plans) = self
            .pco_client
            .get_upcoming_services(days)
            .await
            .map_err(|e| mcp_err(e.to_string()))?;

        let filter = args.service_type.map(|s| s.to_lowercase());
        let max_plans = args.max_plans.unwrap_or(12);
        let selected_plans: Vec<_> = plans
            .into_iter()
            .filter(|plan| {
                filter
                    .as_ref()
                    .is_none_or(|value| plan.service_name.to_lowercase().contains(value.as_str()))
            })
            .take(max_plans)
            .collect();

        let item_futures: Vec<_> = selected_plans
            .iter()
            .map(|plan| {
                let client = Arc::clone(&self.pco_client);
                let plan_id = plan.id.clone();
                async move { client.get_service_items(&plan_id).await }
            })
            .collect();
        let item_results = futures::future::join_all(item_futures).await;

        let mut item_sets = Vec::with_capacity(item_results.len());
        for items in item_results {
            item_sets.push(items.map_err(|e| mcp_err(e.to_string()))?);
        }

        let template_cache = self.template_cache.lock().await;
        let file_index = self.file_index.lock().await;
        let draft = setup::draft_project_config(
            args.project_name.as_deref(),
            &selected_plans,
            &item_sets,
            file_index.as_ref(),
            &template_cache,
            self.macro_cache.as_ref(),
            days,
        );
        drop(file_index);
        drop(template_cache);

        json_result(&draft)
    }

    /// Fetch upcoming service plans from Planning Center.
    #[tool(
        description = "Fetch upcoming service plans from Planning Center Online. Returns plans with their items (title, description, notes, category, song data, scripture references). Use this to see the full service order."
    )]
    async fn fetch_plan(
        &self,
        Parameters(args): Parameters<FetchPlanArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let days = args.days_ahead.unwrap_or(self.config.days_ahead);
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
        description = "Analyze a PCO plan and propose playlist entries. Returns classified items with stable output_key values, status (used/edited/created/skipped/uncertain), parsed description content, backgrounds, and arrangements. IMPORTANT: Present the results to the user and ask about any 'uncertain' items or missing songs before calling build_service. Items the user wants removed should be passed as skip_output_keys to build_service."
    )]
    async fn preview_playlist(
        &self,
        Parameters(args): Parameters<PreviewPlaylistArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        // Fetch items for the plan
        let items = self
            .pco_client
            .get_service_items(&args.plan_id)
            .await
            .map_err(|e| mcp_err(e.to_string()))?;

        // Load mappings
        let mappings = load_service_config();

        // Build preview
        let index_guard = self.file_index.lock().await;
        let entries = classify::build_preview(
            &items,
            &mappings,
            index_guard.as_ref(),
            args.service_name.as_deref(),
        );
        drop(index_guard);

        // Build summary
        let used_count = entries
            .iter()
            .filter(|e| matches!(e.status, classify::PreviewStatus::Used))
            .count();
        let created_count = entries
            .iter()
            .filter(|e| matches!(e.status, classify::PreviewStatus::Created))
            .count();
        let edited_count = entries
            .iter()
            .filter(|e| matches!(e.status, classify::PreviewStatus::Edited))
            .count();
        let skip_count = entries
            .iter()
            .filter(|e| matches!(e.status, classify::PreviewStatus::Skipped))
            .count();
        let uncertain_count = entries
            .iter()
            .filter(|e| matches!(e.status, classify::PreviewStatus::Uncertain))
            .count();

        let result = classify::PreviewResult {
            plan_title: args.plan_id.clone(),
            service_name: args.service_name.unwrap_or_default(),
            date: String::new(),
            entries,
            summary: classify::PreviewSummary {
                used_count,
                created_count,
                edited_count,
                skip_count,
                uncertain_count,
                total_playlist_items: used_count + created_count + edited_count,
            },
        };

        json_result(&result)
    }

    /// Explain how the current config/classifier would handle a single item.
    #[tool(
        description = "Explain how the current config and classifier would handle a single Planning Center item title. Returns the same preview entries that the real workflow would produce, including expansions, nametags, skips, uncertainty, templates, backgrounds, and arrangements."
    )]
    async fn explain_rule_match(
        &self,
        Parameters(args): Parameters<ExplainRuleMatchArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let category = parse_category(args.category.as_deref());
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

        let mappings = load_service_config();
        let index_guard = self.file_index.lock().await;
        let entries = classify::build_preview(
            &[item],
            &mappings,
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

    /// Return only the skipped or uncertain items from a plan preview.
    #[tool(
        description = "Find only the skipped or uncertain items from a plan preview. Use this to surface config gaps, ambiguous song matches, and items that still need explicit rules."
    )]
    async fn find_unmapped_items(
        &self,
        Parameters(args): Parameters<FindUnmappedItemsArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let items = self
            .pco_client
            .get_service_items(&args.plan_id)
            .await
            .map_err(|e| mcp_err(e.to_string()))?;

        let mappings = load_service_config();
        let index_guard = self.file_index.lock().await;
        let entries = classify::build_preview(
            &items,
            &mappings,
            index_guard.as_ref(),
            args.service_name.as_deref(),
        );
        drop(index_guard);

        let filtered: Vec<_> = entries
            .into_iter()
            .filter(|entry| {
                matches!(
                    entry.status,
                    classify::PreviewStatus::Skipped | classify::PreviewStatus::Uncertain
                )
            })
            .collect();

        let skipped_count = filtered
            .iter()
            .filter(|entry| matches!(entry.status, classify::PreviewStatus::Skipped))
            .count();
        let uncertain_count = filtered
            .iter()
            .filter(|entry| matches!(entry.status, classify::PreviewStatus::Uncertain))
            .count();

        json_result(&UnmappedItemsResponse {
            plan_id: args.plan_id,
            service_name: args.service_name,
            skipped_count,
            uncertain_count,
            entries: filtered,
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
        let max = args.max_results.unwrap_or(10);
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

    /// Generate a `ProPresenter` presentation file.
    #[tool(
        description = "Generate a ProPresenter .pro presentation file. For scripture: provide scripture_reference (e.g. 'Isaiah 35:1-6') and optionally bible_version. For text/title/lyrics: provide content lines. Returns the path to the generated file."
    )]
    async fn generate_slides(
        &self,
        Parameters(args): Parameters<GenerateSlidesArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let slide_type = parse_slide_type(&args.slide_type);

        let content = args.content.map(|lines| {
            lines
                .iter()
                .enumerate()
                .map(|(i, text)| {
                    let style = args
                        .styles
                        .as_ref()
                        .and_then(|s| s.get(i))
                        .and_then(|opt| opt.as_ref());
                    StyledSegment {
                        text: text.clone(),
                        color: style
                            .and_then(|s| s.color.as_deref())
                            .and_then(parse_hex_color),
                        bold: style.and_then(|s| s.bold),
                        italic: style.and_then(|s| s.italic),
                    }
                })
                .collect()
        });

        let executor = ServiceBuildExecutor::new(
            self.pco_client.as_ref(),
            &self.bible_service,
            &self.file_index,
            &self.template_cache,
            self.macro_cache.as_ref(),
            self.library_path.as_deref(),
        );

        let result = executor
            .generate_single(&SingleGenerateRequest {
                slide_type,
                name: args.name,
                scripture_reference: args.scripture_reference,
                bible_version: args.bible_version,
                content,
                title_text: args.title_text,
                background: args.background,
                arrangement: args.arrangement,
            })
            .await
            .map_err(|e| mcp_err(e.to_string()))?;

        Ok(text_result(format!(
            "Generated: {} ({} slides)",
            result.output_path.display(),
            result.slide_count
        )))
    }

    /// Generate all slides and build a playlist for an entire service in one call.
    #[tool(
        description = "One-shot service builder: generates all slides, applies backgrounds and arrangements, assembles .proplaylist. Call ONLY after preview_playlist and user confirmation. Uncertain items are automatically skipped. Pass skip_output_keys for user-rejected items and overrides keyed by output_key for corrections."
    )]
    async fn build_service(
        &self,
        Parameters(args): Parameters<BuildServiceArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let mappings = load_service_config();
        let request = BuildRequest {
            plan_id: args.plan_id,
            service_name: args.service_name,
            playlist_name: args.playlist_name,
            skip_output_keys: args.skip_output_keys.unwrap_or_default(),
            overrides: args
                .overrides
                .unwrap_or_default()
                .into_iter()
                .map(|entry| WorkflowEntryOverride {
                    output_key: entry.output_key,
                    playlist_name: entry.playlist_name,
                    background: entry.background,
                    arrangement: entry.arrangement,
                })
                .collect(),
        };
        let executor = ServiceBuildExecutor::new(
            self.pco_client.as_ref(),
            &self.bible_service,
            &self.file_index,
            &self.template_cache,
            self.macro_cache.as_ref(),
            self.library_path.as_deref(),
        );
        let result = executor
            .build_service(&request, &mappings)
            .await
            .map_err(|e| mcp_err(e.to_string()))?;
        json_result(&result)
    }

    /// Build a `ProPresenter` playlist from an ordered list of presentations.
    #[tool(
        description = "Build a ProPresenter .proplaylist file from an ordered list of presentations. Each item needs a name, file_path to its .pro file, and slide_type. Returns the path to the generated playlist."
    )]
    async fn build_service_playlist(
        &self,
        Parameters(args): Parameters<BuildPlaylistArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let entries: Vec<PlaylistEntry> = args
            .items
            .iter()
            .map(|item| {
                let slide_type = parse_slide_type(&item.slide_type);
                let embedded_data = std::fs::read(&item.file_path).ok();
                let file_stem = std::path::Path::new(&item.file_path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&item.name);

                PlaylistEntry {
                    name: file_stem.to_string(),
                    slide_type,
                    from_matched_file: true,
                    presentation_path: item.file_path.clone(),
                    arrangement_uuid: None,
                    embedded_data,
                }
            })
            .collect();

        let playlist = build_playlist(&args.name, &entries);
        let output_path = playlist_output_path(self.library_path.as_deref(), &args.name);

        write_playlist_file(&playlist, &entries, &output_path)
            .map_err(|e| mcp_err(e.to_string()))?;

        Ok(text_result(format!(
            "Playlist saved: {} ({} items)",
            output_path.display(),
            entries.len()
        )))
    }
}

// ---------------------------------------------------------------------------
// Context rendering — single source of truth from proflow.config.json
// ---------------------------------------------------------------------------

/// Render context documentation from the mappings config.
///
/// Replaces `mcp_context.md` — everything derives from `proflow.config.json`.
#[allow(clippy::too_many_lines)]
fn render_mcp_context(
    mappings: &ProjectConfig,
    theme_name: Option<&str>,
    theme_slides: &[String],
    macro_names: &[String],
) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(4096);

    out.push_str("# ProFlow Service Preparation Context\n\n");

    // Theme info
    if let Some(name) = theme_name {
        let _ = writeln!(out, "## Theme\n\nActive theme: **{name}**\n");
        if !theme_slides.is_empty() {
            out.push_str("Available slides:\n");
            for slide in theme_slides {
                let _ = writeln!(out, "- `{slide}`");
            }
            out.push('\n');
        }
    } else {
        out.push_str("## Theme\n\nNo theme configured — using legacy `.pro` template files.\n\n");
    }

    // Available macros
    if !macro_names.is_empty() {
        out.push_str("## Available Macros\n\n");
        for name in macro_names {
            let _ = writeln!(out, "- `{name}`");
        }
        out.push('\n');
    }
    out.push_str("## Workflow\n\n");
    out.push_str("1. `get_context` (this) — understand type system and rules\n");
    out.push_str("2. `fetch_plan` — get service order from Planning Center\n");
    out.push_str(
        "3. `preview_playlist` — classified items with parsed content, backgrounds, arrangements\n",
    );
    out.push_str("4. Present preview to user (see Review Protocol below), get confirmation\n");
    out.push_str(
        "5. `build_service` — generates all slides, applies styling, builds .proplaylist\n\n",
    );

    // Review protocol
    out.push_str("## Review Protocol\n\n");
    out.push_str("After `preview_playlist`, present a table to the user showing every entry.\n");
    out.push_str("Flag items that need attention:\n\n");
    out.push_str("### Must ask the user about:\n");
    out.push_str(
        "- **Uncertain** items (status=uncertain) — nametag not found, library miss, etc.\n",
    );
    out.push_str("  Ask: skip it, use a different file, or create it?\n");
    out.push_str(
        "- **Edited items with no parsed_content** — description was empty or unparseable.\n",
    );
    out.push_str(
        "  The library file will be used as-is. Flag if the item normally has weekly content.\n",
    );
    out.push_str(
        "- **Songs not found** (status=skipped, type=song) — expected in library but missing.\n",
    );
    out.push_str("  Ask: is this a known song under a different name? Should it be skipped?\n\n");
    out.push_str("### Surface for awareness (don't block on these):\n");
    out.push_str("- Items with parsed_content — show a brief preview of what will be generated\n");
    out.push_str("- Scripture references — confirm book/chapter/verse and version look correct\n");
    out.push_str("- Total item count — does the playlist length look right for this service?\n\n");
    out.push_str("### After confirmation:\n");
    out.push_str(
        "Call `build_service` with `skip_output_keys` for any items the user wants removed,\n",
    );
    out.push_str(
        "and `overrides` for any corrections (different name, background, arrangement).\n\n",
    );
    out.push_str("### Learning:\n");
    out.push_str("If the user says \"X always maps to Y\", update `data/proflow.config.json` so\n");
    out.push_str("the mapping is permanent. This reduces future uncertain items.\n\n");

    // Presentation types table
    out.push_str("## Presentation Types\n\n");
    out.push_str(
        "| Type | Kind | Content | Output | Template | Background | Macro | Arrangement |\n",
    );
    out.push_str(
        "|------|------|---------|--------|----------|------------|-------|-------------|\n",
    );

    let mut type_keys: Vec<_> = mappings.presentation_types.keys().collect();
    type_keys.sort();
    for key in &type_keys {
        let pt = &mappings.presentation_types[*key];
        let _ = writeln!(
            out,
            "| `{key}` | {kind:?} | {source:?} | {strategy:?} | {template} | {bg} | {mac} | {arr} |",
            kind = pt.kind,
            source = pt.content_source,
            strategy = pt.output_strategy,
            template = pt.template.as_deref().unwrap_or("—"),
            bg = pt.background.as_deref().unwrap_or("—"),
            mac = pt.macro_name.as_deref().unwrap_or("—"),
            arr = pt.arrangement.as_deref().unwrap_or("—"),
        );
    }

    // Item rules
    if !mappings.item_rules.is_empty() {
        out.push_str("\n## Item Rules\n\n");
        for rule in &mappings.item_rules {
            let match_desc = if !rule.match_spec.title_prefix.is_empty() {
                format!("prefix: {}", rule.match_spec.title_prefix.join(", "))
            } else if !rule.match_spec.title_contains.is_empty() {
                format!("contains: {}", rule.match_spec.title_contains.join(", "))
            } else if rule.match_spec.category.is_some() {
                format!(
                    "category: {}",
                    rule.match_spec.category.as_deref().unwrap_or("")
                )
            } else {
                "catch-all".to_string()
            };

            if let Some(ref action) = rule.action {
                match action {
                    crate::project_config::RuleAction::Skip { reason } => {
                        let _ =
                            writeln!(out, "- `{}` ({match_desc}) → **skip**: {reason}", rule.id);
                    }
                    crate::project_config::RuleAction::Review { reason } => {
                        let _ =
                            writeln!(out, "- `{}` ({match_desc}) → **review**: {reason}", rule.id);
                    }
                }
            } else if !rule.expand.is_empty() {
                let steps: Vec<_> = rule
                    .expand
                    .iter()
                    .map(|s| {
                        if s.speaker.is_some() {
                            "speaker nametag".to_string()
                        } else {
                            s.use_type.clone()
                        }
                    })
                    .collect();
                let _ = writeln!(
                    out,
                    "- `{}` ({match_desc}) → expand: {}",
                    rule.id,
                    steps.join(" + ")
                );
            } else if let Some(ref use_type) = rule.use_type {
                let target = rule
                    .target
                    .as_ref()
                    .and_then(|t| t.library_file.as_deref())
                    .unwrap_or("—");
                let _ = writeln!(
                    out,
                    "- `{}` ({match_desc}) → type `{use_type}`, target: {target}",
                    rule.id
                );
            }
        }
    }

    if !mappings.service_groups.is_empty() {
        out.push_str("\n## Service Groups\n\n");
        for (group, config) in &mappings.service_groups {
            let _ = writeln!(
                out,
                "- `{group}`: {}",
                if config.service_types.is_empty() {
                    "no service types configured".to_string()
                } else {
                    config.service_types.join(", ")
                }
            );
        }
    }

    if !mappings.profiles.is_empty() {
        out.push_str("\n## Profiles\n\n");
        for (name, profile) in &mappings.profiles {
            let mut parts = Vec::new();
            if !profile.service_groups.is_empty() {
                parts.push(format!("groups: {}", profile.service_groups.join(", ")));
            }
            if !profile.service_types.is_empty() {
                parts.push(format!(
                    "service types: {}",
                    profile.service_types.join(", ")
                ));
            }
            if let Some(days_ahead) = profile.days_ahead {
                parts.push(format!("days_ahead: {days_ahead}"));
            }
            if let Some(review_policy) = profile.review_policy {
                parts.push(format!("review_policy: {review_policy:?}"));
            }
            let summary = if parts.is_empty() {
                "no profile selectors configured".to_string()
            } else {
                parts.join("; ")
            };
            let _ = writeln!(out, "- `{name}`: {summary}");
        }
    }

    // People
    if !mappings.people.is_empty() {
        out.push_str("\n## People\n\n");
        out.push_str("| Name | Role | Nametag |\n");
        out.push_str("|------|------|---------|\n");
        for (name, person) in &mappings.people {
            let _ = writeln!(
                out,
                "| {name} {} | {} | {} |",
                person.last.as_deref().unwrap_or(""),
                person.role.as_deref().unwrap_or(""),
                person.nametag.as_deref().unwrap_or("—"),
            );
        }
    }

    // Override rules
    if !mappings.overrides.is_empty() {
        out.push_str("\n## Override Rules\n\n");
        for ovr in &mappings.overrides {
            let mut when_parts = Vec::new();
            if let Some(ref group) = ovr.when.service_group {
                when_parts.push(format!("group={group}"));
            }
            if let Some(ref stype) = ovr.when.service_type {
                when_parts.push(format!("service={stype}"));
            }
            if let Some(ref ptype) = ovr.when.presentation_type {
                when_parts.push(format!("type={ptype}"));
            }
            let mut apply_parts = Vec::new();
            if let Some(ref arr) = ovr.arrangement {
                apply_parts.push(format!("arrangement={arr}"));
            }
            if let Some(ref bg) = ovr.background {
                apply_parts.push(format!("background={bg}"));
            }
            if let Some(ref tmpl) = ovr.template {
                apply_parts.push(format!("template={tmpl}"));
            }
            let _ = writeln!(
                out,
                "- when {} → {}",
                when_parts.join(", "),
                apply_parts.join(", ")
            );
        }
    }

    // Description parsing rules (stable preamble)
    out.push_str("\n## Description Parsing\n\n");
    out.push_str("Descriptions are auto-parsed by `build_service`. The parser handles:\n\n");
    out.push_str("### Marker-based (Prayer of Confession, etc.)\n");
    out.push_str("- `[SLIDE/ALL]` → banana yellow (#FEFC8B), congregation reads\n");
    out.push_str("- `[SLIDE]` → default color\n");
    out.push_str("- `[no slide]`, `[SILENT CONFESSION]` → skipped\n");
    out.push_str("- Content inside `[brackets]` after markers is extracted\n\n");
    out.push_str("### Responsive readings (Call to Worship, etc.)\n");
    out.push_str("- `Leader:` / `L:` → white (default)\n");
    out.push_str("- `People:` / `All:` / `P:` / `Unison:` → banana yellow (#FEFC8B)\n");
    out.push_str("- Continuation lines inherit previous color\n");
    out.push_str("- Prefixes are stripped from displayed text\n\n");
    out.push_str("### Content nametags (Prelude, Postlude, etc.)\n");
    out.push_str("- Piece title extracted from item title after colon\n");
    out.push_str("- Description parsed for composer/performer (split on `/`)\n\n");
    out.push_str("### Scripture\n");
    out.push_str("- Reference parsed from title, version auto-detected\n");
    out.push_str("- Title slide with reference + version auto-generated\n");
    out.push_str("- Verse text looked up from Bible data files\n");

    out
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
                 Use fetch_plan to see upcoming services, search_library to find existing \
                 presentations, generate_slides to create new ones, and build_service_playlist \
                 to assemble the final playlist. For end-to-end automation: \
                 preview_playlist → user confirms → build_service does everything."
                    .to_string(),
            )
    }
}
