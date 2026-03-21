//! MCP server for autonomous service preparation.
//!
//! Exposes `ProFlow` capabilities (plan fetching, library search, slide generation,
//! playlist building) as MCP tools so an LLM can prep a service end-to-end.

mod description_parser;
mod preview;

use std::path::PathBuf;
use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, Content, Implementation, ServerCapabilities, ServerInfo,
};
use rmcp::{schemars, tool, tool_handler, tool_router, ServerHandler};
use serde::Serialize;
use tokio::sync::Mutex;

use crate::app::find_data_subdir;
use crate::bible::{parse_scripture_ref, BibleService, BibleVersion};
use crate::config::Config;
use crate::planning_center::PlanningCenterClient;
use crate::playlist_gen::{canonical_presentation_name, playlist_output_path};
use crate::propresenter::playlist::{build_playlist, write_playlist_file, PlaylistEntry};
use crate::propresenter::rtf::StyledSegment;
use crate::propresenter::serialize::write_presentation_file;
use crate::propresenter::macros::MacroCache;
use crate::propresenter::template::{
    build_combined_scripture_presentation, build_presentation_from_template_with_options,
    build_scripture_presentation, ScripturePassage, ThemeCache, DEFAULT_MAX_LINES_PER_SLIDE,
};
use crate::types::SlideType;
use crate::utils::file_matcher::FileIndex;

use description_parser::to_styled_segments;

/// Path to the item mappings file relative to the data directory.
const ITEM_MAPPINGS_FILE: &str = "item_mappings.json";

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
            .or_else(crate::utils::file_matcher::get_default_library_path)
            .or_else(|| {
                config.propresenter_path.as_ref().and_then(|pro_dir| {
                    let path = PathBuf::from(shellexpand::tilde(pro_dir).to_string())
                        .join("Libraries/Default");
                    path.exists().then_some(path)
                })
            });

        let file_index = library_path
            .as_ref()
            .and_then(|p| FileIndex::build(p).ok());

        let mut template_paths = Vec::new();
        if let Some(ref lib) = library_path {
            template_paths.push(lib.clone());
        }
        template_paths.push(find_data_subdir("templates"));

        // Load mappings to get theme name
        let data_dir = find_data_subdir("");
        let mappings: preview::ItemMappings =
            std::fs::read_to_string(data_dir.join(ITEM_MAPPINGS_FILE))
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();
        let theme_name = mappings.theme.as_deref();

        let macro_cache = MacroCache::load_default();

        Some(Self {
            config: Arc::new(config),
            pco_client: Arc::new(pco_client),
            bible_service: Arc::new(Mutex::new(bible_service)),
            file_index: Arc::new(Mutex::new(file_index)),
            template_cache: Arc::new(Mutex::new(ThemeCache::new(theme_name, template_paths))),
            macro_cache: Arc::new(macro_cache),
            library_path,
            tool_router: Self::tool_router(),
        })
    }
}

// ---------------------------------------------------------------------------
// build_service helpers
// ---------------------------------------------------------------------------

impl ProFlowServer {
    /// Infer the `SlideType` from a preview entry's type key.
    fn infer_slide_type(entry: &preview::PreviewEntry) -> SlideType {
        match entry.item_type.as_deref() {
            Some("scripture") => SlideType::Scripture,
            Some("song") => SlideType::Lyrics,
            _ => SlideType::Text,
        }
    }

    /// Look up an arrangement UUID from a .pro file by name.
    fn resolve_arrangement_uuid(
        file_path: &str,
        arrangement_name: Option<&str>,
    ) -> Option<uuid::Uuid> {
        let name = arrangement_name?;
        let data = std::fs::read(file_path).ok()?;
        let presentation =
            <crate::propresenter::generated::rv_data::Presentation as prost::Message>::decode(
                data.as_slice(),
            )
            .ok()?;

        let target = name.to_lowercase();
        presentation
            .arrangements
            .iter()
            .find(|a| a.name.to_lowercase() == target)
            .and_then(|a| a.uuid.as_ref())
            .and_then(|u| uuid::Uuid::parse_str(&u.string).ok())
    }

    /// Generate a presentation for an "edited" preview entry.
    ///
    /// Uses `parsed_content` if available, otherwise falls back to raw description.
    #[allow(clippy::too_many_lines)]
    async fn generate_edited_entry(
        &self,
        entry: &preview::PreviewEntry,
        bg_override: Option<&str>,
        arr_override: Option<&str>,
    ) -> Result<(PlaylistEntry, usize), rmcp::ErrorData> {
        let segments: Vec<StyledSegment> = entry
            .parsed_content
            .as_ref()
            .map(to_styled_segments)
            .unwrap_or_default();

        // No parsed content — fall back to using library file as-is
        if segments.is_empty() {
            if let Some(ref file_path) = entry.file_path {
                let embedded_data = std::fs::read(file_path).ok();
                let file_stem = std::path::Path::new(file_path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&entry.playlist_name);
                return Ok((
                    PlaylistEntry {
                        name: file_stem.to_string(),
                        slide_type: Self::infer_slide_type(entry),
                        from_matched_file: true,
                        presentation_path: file_path.clone(),
                        arrangement_uuid: None,
                        embedded_data,
                    },
                    0,
                ));
            }
            return Err(mcp_err(format!(
                "No parsed content and no library file for edited item '{}'",
                entry.pco_title
            )));
        }

        let title_text = entry
            .parsed_content
            .as_ref()
            .and_then(|pc| pc.title_text.clone());

        // Resolve slide name: prefer the entry's template field, fall back to type-based default
        let slide_name = entry.template_name.clone().unwrap_or_else(|| {
            match entry.item_type.as_deref() {
                Some("scripture") => "scripture",
                Some("song") => "song",
                _ => "info",
            }
            .to_string()
        });

        let template_slide = self
            .template_cache
            .lock()
            .await
            .get(&slide_name)
            .cloned()
            .ok_or_else(|| mcp_err(format!("No template slide: {slide_name}")))?;

        let presentation_name =
            canonical_presentation_name(&entry.playlist_name, Self::infer_slide_type(entry));

        let mut presentation = build_presentation_from_template_with_options(
            &presentation_name,
            &template_slide,
            &segments,
            45,
            DEFAULT_MAX_LINES_PER_SLIDE,
            title_text.as_deref(),
        )
        .ok_or_else(|| mcp_err("Failed to build presentation from template"))?;

        // Apply background
        let bg = bg_override
            .map(String::from)
            .or_else(|| entry.background.clone());
        if let Some(ref bg_cat) = bg {
            Self::apply_background(&mut presentation, bg_cat);
        }

        // Apply macro
        if let Some(ref macro_name) = entry.macro_name {
            crate::propresenter::macros::add_macro_to_first_cue(
                &mut presentation,
                macro_name,
                &self.macro_cache,
            );
        }

        // Apply arrangement
        let arr = arr_override
            .map(String::from)
            .or_else(|| entry.arrangement.clone());
        if let Some(ref arr_name) = arr {
            crate::propresenter::arrangement::select_arrangement_by_name(
                &mut presentation,
                arr_name,
            );
        }

        // Write to library
        let output_path = self
            .library_path
            .as_deref()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join(format!("{presentation_name}.pro"));

        write_presentation_file(&presentation, &output_path)
            .map_err(|e| mcp_err(e.to_string()))?;

        // Update file index
        if let Some(ref mut idx) = *self.file_index.lock().await {
            idx.add_entry(&output_path);
        }

        let slide_count = presentation.cues.len();
        let embedded_data = std::fs::read(&output_path).ok();

        Ok((
            PlaylistEntry {
                name: presentation_name,
                slide_type: Self::infer_slide_type(entry),
                from_matched_file: false,
                presentation_path: output_path.display().to_string(),
                arrangement_uuid: None,
                embedded_data,
            },
            slide_count,
        ))
    }

    /// Generate a presentation for a "created" preview entry (typically scripture).
    ///
    /// Handles both single-reference and multi-reference items. Multi-ref entries
    /// (identified by `scripture_refs`) produce a combined presentation with
    /// title → verses → blank divider for each passage.
    #[allow(clippy::too_many_lines)]
    async fn generate_created_entry(
        &self,
        entry: &preview::PreviewEntry,
        bg_override: Option<&str>,
    ) -> Result<(PlaylistEntry, usize), rmcp::ErrorData> {
        if entry.item_type.as_deref() != Some("scripture") {
            return Err(mcp_err(format!(
                "Unknown created type for '{}'",
                entry.pco_title
            )));
        }

        let slide_name = entry
            .template_name
            .clone()
            .unwrap_or_else(|| "scripture".to_string());

        let template_slide = self
            .template_cache
            .lock()
            .await
            .get(&slide_name)
            .cloned()
            .ok_or_else(|| mcp_err(format!("No template slide: {slide_name}")))?;

        let presentation_name =
            canonical_presentation_name(&entry.playlist_name, SlideType::Scripture);

        let mut missing_warnings: Vec<String> = Vec::new();

        let mut presentation = if let Some(ref refs) = entry.scripture_refs {
            // Multi-reference: build combined presentation
            let mut passages = Vec::new();
            let mut bible = self.bible_service.lock().await;

            for ref_info in refs {
                let reference = parse_scripture_ref(&ref_info.reference)
                    .ok_or_else(|| mcp_err(format!("Cannot parse: {}", ref_info.reference)))?;
                let version = parse_bible_version(Some(&ref_info.version));

                let (header, verses) = bible
                    .lookup_verses(&reference, version)
                    .map_err(|e| mcp_err(e.to_string()))?;

                if !header.missing_verses.is_empty() {
                    missing_warnings.push(format!(
                        "{}: missing verses {:?}",
                        ref_info.reference, header.missing_verses
                    ));
                }

                passages.push(ScripturePassage {
                    title: header.display(),
                    verses,
                });
            }
            drop(bible);

            build_combined_scripture_presentation(&presentation_name, &template_slide, &passages)
                .ok_or_else(|| mcp_err("Failed to build combined scripture presentation"))?
        } else {
            // Single reference
            let ref_str = entry.scripture_reference.as_deref().ok_or_else(|| {
                mcp_err(format!(
                    "No scripture reference for '{}'",
                    entry.pco_title
                ))
            })?;
            let reference = parse_scripture_ref(ref_str)
                .ok_or_else(|| mcp_err(format!("Cannot parse reference: {ref_str}")))?;
            let version = parse_bible_version(entry.bible_version.as_deref());

            let (header, verses) = self
                .bible_service
                .lock()
                .await
                .lookup_verses(&reference, version)
                .map_err(|e| mcp_err(e.to_string()))?;

            if !header.missing_verses.is_empty() {
                missing_warnings.push(format!(
                    "{ref_str}: missing verses {:?}",
                    header.missing_verses
                ));
            }

            build_scripture_presentation(
                &presentation_name,
                &template_slide,
                &verses,
                Some(&header.display()),
            )
            .ok_or_else(|| mcp_err("Failed to build scripture presentation"))?
        };

        // Apply background
        let bg = bg_override
            .map(String::from)
            .or_else(|| entry.background.clone());
        if let Some(ref bg_cat) = bg {
            Self::apply_background(&mut presentation, bg_cat);
        }

        // Apply macro
        if let Some(ref macro_name) = entry.macro_name {
            crate::propresenter::macros::add_macro_to_first_cue(
                &mut presentation,
                macro_name,
                &self.macro_cache,
            );
        }

        // Write to library
        let output_path = self
            .library_path
            .as_deref()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join(format!("{presentation_name}.pro"));

        write_presentation_file(&presentation, &output_path)
            .map_err(|e| mcp_err(e.to_string()))?;

        if let Some(ref mut idx) = *self.file_index.lock().await {
            idx.add_entry(&output_path);
        }

        let slide_count = presentation.cues.len();
        let embedded_data = std::fs::read(&output_path).ok();

        if !missing_warnings.is_empty() {
            eprintln!("Warning: {}", missing_warnings.join("; "));
        }

        Ok((
            PlaylistEntry {
                name: presentation_name,
                slide_type: SlideType::Scripture,
                from_matched_file: false,
                presentation_path: output_path.display().to_string(),
                arrangement_uuid: None,
                embedded_data,
            },
            slide_count,
        ))
    }

    /// Apply a background image to the first cue of a presentation.
    fn apply_background(
        presentation: &mut crate::propresenter::generated::rv_data::Presentation,
        bg_category: &str,
    ) {
        let category = match bg_category.to_lowercase().as_str() {
            "sermon" => crate::propresenter::background::BackgroundCategory::Sermon,
            _ => crate::propresenter::background::BackgroundCategory::Default,
        };
        let data_dir = find_data_subdir("");
        if let Some(image_path) =
            crate::propresenter::background::resolve_background_image(&data_dir, category)
        {
            crate::propresenter::background::add_background_to_first_cue(
                presentation,
                &image_path,
            );
        }
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
    #[schemars(description = "Service type name (e.g. '10:30am traditional') for context-aware defaults")]
    pub service_name: Option<String>,
}

/// Arguments for the `fetch_plan` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct FetchPlanArgs {
    /// Optional service type name to filter by (e.g. "Sunday Morning").
    #[schemars(description = "Filter plans to this service type name (case-insensitive substring match)")]
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
    #[schemars(description = "Scripture reference, e.g. 'Isaiah 35:1-6' (required for scripture type)")]
    pub scripture_reference: Option<String>,
    /// Bible version (optional, defaults to `NRSVue`).
    #[schemars(description = "Bible version: NRSVue, NRSV, NIV, KJV, NKJV, NLT, NASB (default: NRSVue)")]
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
    #[schemars(description = "Optional style overrides parallel to content. Each entry: {color: '#FFFF00', bold: true, italic: false} or null for template defaults.")]
    pub styles: Option<Vec<Option<SegmentStyle>>>,
    /// Background image category: 'default' or 'sermon'. Omit for no background.
    #[schemars(description = "Background image: 'default' or 'sermon'. Omit for no background image.")]
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
    /// Positions to skip (1-based, from preview output).
    #[schemars(description = "Positions to skip from the preview (1-based position numbers)")]
    pub skip_positions: Option<Vec<usize>>,
    /// Per-entry overrides (by position).
    #[schemars(description = "Per-entry overrides by position")]
    pub overrides: Option<Vec<EntryOverride>>,
}

/// Override for a single preview entry (by position).
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct EntryOverride {
    /// 1-based position of the entry to override.
    #[schemars(description = "1-based position number from preview")]
    pub position: usize,
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

/// Summary of a single item processed by `build_service`.
#[derive(Serialize)]
struct BuildServiceEntry {
    position: usize,
    name: String,
    action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    slides: Option<usize>,
}

/// Result summary from `build_service`.
#[derive(Serialize)]
struct BuildServiceResult {
    playlist_path: String,
    entries: Vec<BuildServiceEntry>,
    total_items: usize,
    generated_count: usize,
    library_count: usize,
    skipped_count: usize,
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

fn parse_bible_version(s: Option<&str>) -> BibleVersion {
    s.and_then(BibleVersion::from_text).unwrap_or_default()
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

// ---------------------------------------------------------------------------
// Tool implementations
// ---------------------------------------------------------------------------

#[tool_router]
impl ProFlowServer {
    /// Get context rules for service preparation.
    #[tool(description = "Get formatting rules and context for preparing worship service slides. Call this FIRST before processing any plan — it explains how to handle responsive readings, scripture, songs, and other item types.")]
    async fn get_context(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        let data_dir = find_data_subdir("");
        let mappings: preview::ItemMappings =
            std::fs::read_to_string(data_dir.join(ITEM_MAPPINGS_FILE))
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();

        let cache = self.template_cache.lock().await;
        let theme_name = cache.theme_name().map(String::from);
        let slide_names = cache.theme_slide_names().into_iter().map(String::from).collect::<Vec<_>>();
        drop(cache);

        let macro_names = self.macro_cache.names().into_iter().map(String::from).collect::<Vec<_>>();

        Ok(text_result(render_context(&mappings, theme_name.as_deref(), &slide_names, &macro_names)))
    }

    /// Fetch upcoming service plans from Planning Center.
    #[tool(description = "Fetch upcoming service plans from Planning Center Online. Returns plans with their items (title, description, notes, category, song data, scripture references). Use this to see the full service order.")]
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
    #[tool(description = "Analyze a PCO plan and propose playlist entries. Returns classified items with status (used/edited/created/skipped/uncertain), parsed description content, backgrounds, and arrangements. IMPORTANT: Present the results to the user and ask about any 'uncertain' items or missing songs before calling build_service. Items the user wants removed should be passed as skip_positions to build_service.")]
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
        let data_dir = find_data_subdir("");
        let mappings: preview::ItemMappings =
            std::fs::read_to_string(data_dir.join(ITEM_MAPPINGS_FILE))
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();

        // Build preview
        let index_guard = self.file_index.lock().await;
        let entries =
            preview::build_preview(&items, &mappings, index_guard.as_ref(), args.service_name.as_deref());
        drop(index_guard);

        // Build summary
        let used_count = entries
            .iter()
            .filter(|e| matches!(e.status, preview::PreviewStatus::Used))
            .count();
        let created_count = entries
            .iter()
            .filter(|e| matches!(e.status, preview::PreviewStatus::Created))
            .count();
        let edited_count = entries
            .iter()
            .filter(|e| matches!(e.status, preview::PreviewStatus::Edited))
            .count();
        let skip_count = entries
            .iter()
            .filter(|e| matches!(e.status, preview::PreviewStatus::Skipped))
            .count();
        let uncertain_count = entries
            .iter()
            .filter(|e| matches!(e.status, preview::PreviewStatus::Uncertain))
            .count();

        let result = preview::PreviewResult {
            plan_title: args.plan_id.clone(),
            service_name: args.service_name.unwrap_or_default(),
            date: String::new(),
            entries,
            summary: preview::PreviewSummary {
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

    /// Search the `ProPresenter` library for matching files.
    #[tool(description = "Search the ProPresenter library for .pro files matching a query. Returns file names and paths sorted by relevance. Use this to find existing presentations before generating new ones.")]
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
    #[tool(description = "Generate a ProPresenter .pro presentation file. For scripture: provide scripture_reference (e.g. 'Isaiah 35:1-6') and optionally bible_version. For text/title/lyrics: provide content lines. Returns the path to the generated file.")]
    async fn generate_slides(
        &self,
        Parameters(args): Parameters<GenerateSlidesArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let slide_type = parse_slide_type(&args.slide_type);

        let slide_name = match slide_type {
            SlideType::Scripture => "scripture",
            SlideType::Lyrics => "song",
            SlideType::Title | SlideType::Text | SlideType::Graphic => "info",
        };

        let template_slide = self
            .template_cache
            .lock()
            .await
            .get(slide_name)
            .cloned()
            .ok_or_else(|| mcp_err(format!("No template slide: {slide_name}")))?;

        let presentation_name = canonical_presentation_name(&args.name, slide_type);

        let mut presentation = if slide_type == SlideType::Scripture {
            // Verse-aware scripture pipeline
            let ref_str = args.scripture_reference.as_deref().ok_or_else(|| {
                mcp_err("scripture_reference is required for scripture slide type")
            })?;
            let reference = parse_scripture_ref(ref_str)
                .ok_or_else(|| mcp_err("Could not parse scripture reference"))?;
            let version = parse_bible_version(args.bible_version.as_deref());

            let (header, verses) = self
                .bible_service
                .lock()
                .await
                .lookup_verses(&reference, version)
                .map_err(|e| mcp_err(e.to_string()))?;

            if !header.missing_verses.is_empty() {
                eprintln!(
                    "Warning: {ref_str}: missing verses {:?}",
                    header.missing_verses
                );
            }

            build_scripture_presentation(
                &presentation_name,
                &template_slide,
                &verses,
                Some(&header.display()),
            )
            .ok_or_else(|| mcp_err("Failed to build scripture presentation"))?
        } else {
            // Non-scripture: existing styled-segment pipeline
            let content = args
                .content
                .ok_or_else(|| mcp_err("content is required for non-scripture slide types"))?;
            let title_slide = args.title_text;

            let segments: Vec<StyledSegment> = content
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
                        color: style.and_then(|s| s.color.as_deref()).and_then(parse_hex_color),
                        bold: style.and_then(|s| s.bold),
                        italic: style.and_then(|s| s.italic),
                    }
                })
                .collect();

            build_presentation_from_template_with_options(
                &presentation_name,
                &template_slide,
                &segments,
                45,
                DEFAULT_MAX_LINES_PER_SLIDE,
                title_slide.as_deref(),
            )
            .ok_or_else(|| mcp_err("Failed to build presentation from template"))?
        };

        // Apply background image to first cue if requested
        if let Some(ref bg_category) = args.background {
            let category = match bg_category.to_lowercase().as_str() {
                "sermon" => crate::propresenter::background::BackgroundCategory::Sermon,
                _ => crate::propresenter::background::BackgroundCategory::Default,
            };
            let data_dir = find_data_subdir("");
            if let Some(image_path) =
                crate::propresenter::background::resolve_background_image(&data_dir, category)
            {
                crate::propresenter::background::add_background_to_first_cue(
                    &mut presentation,
                    &image_path,
                );
            }
        }

        // Select arrangement if requested
        if let Some(ref arr_name) = args.arrangement {
            crate::propresenter::arrangement::select_arrangement_by_name(
                &mut presentation,
                arr_name,
            );
        }

        let output_path = self
            .library_path
            .as_deref()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join(format!("{presentation_name}.pro"));

        write_presentation_file(&presentation, &output_path)
            .map_err(|e| mcp_err(e.to_string()))?;

        // Add to file index so subsequent searches find it
        if let Some(ref mut idx) = *self.file_index.lock().await {
            idx.add_entry(&output_path);
        }

        Ok(text_result(format!(
            "Generated: {} ({} slides)",
            output_path.display(),
            presentation.cues.len()
        )))
    }

    /// Generate all slides and build a playlist for an entire service in one call.
    #[tool(description = "One-shot service builder: generates all slides, applies backgrounds and arrangements, assembles .proplaylist. Call ONLY after preview_playlist and user confirmation. Uncertain items are automatically skipped. Pass skip_positions for user-rejected items and overrides for corrections.")]
    async fn build_service(
        &self,
        Parameters(args): Parameters<BuildServiceArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        // 1. Fetch items
        let items = self
            .pco_client
            .get_service_items(&args.plan_id)
            .await
            .map_err(|e| mcp_err(e.to_string()))?;

        // 2. Load mappings and build preview (re-derive everything — no stale state)
        let data_dir = find_data_subdir("");
        let mappings: preview::ItemMappings =
            std::fs::read_to_string(data_dir.join(ITEM_MAPPINGS_FILE))
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();

        let index_guard = self.file_index.lock().await;
        let entries = preview::build_preview(
            &items,
            &mappings,
            index_guard.as_ref(),
            args.service_name.as_deref(),
        );
        drop(index_guard);

        // 3. Apply skip_positions
        let skip_set: std::collections::HashSet<usize> = args
            .skip_positions
            .unwrap_or_default()
            .into_iter()
            .collect();

        // Index overrides by position
        let override_map: std::collections::HashMap<usize, &EntryOverride> = args
            .overrides
            .as_ref()
            .map(|ovrs| ovrs.iter().map(|o| (o.position, o)).collect())
            .unwrap_or_default();

        // 4. Process each entry
        let mut playlist_entries: Vec<PlaylistEntry> = Vec::new();
        let mut summary_entries: Vec<BuildServiceEntry> = Vec::new();
        let mut generated_count: usize = 0;
        let mut library_count: usize = 0;
        let mut skipped_count: usize = 0;

        for entry in &entries {
            // User-requested skip
            if skip_set.contains(&entry.position) {
                skipped_count += 1;
                summary_entries.push(BuildServiceEntry {
                    position: entry.position,
                    name: entry.pco_title.clone(),
                    action: "skipped (user)".to_string(),
                    file_path: None,
                    slides: None,
                });
                continue;
            }

            // Apply overrides
            let ovr = override_map.get(&entry.position);
            let bg_override = ovr.and_then(|o| o.background.as_deref());
            let arr_override = ovr.and_then(|o| o.arrangement.as_deref());

            match entry.status {
                preview::PreviewStatus::Skipped => {
                    skipped_count += 1;
                    summary_entries.push(BuildServiceEntry {
                        position: entry.position,
                        name: entry.pco_title.clone(),
                        action: format!("skipped: {}", entry.reason),
                        file_path: None,
                        slides: None,
                    });
                }
                preview::PreviewStatus::Uncertain => {
                    skipped_count += 1;
                    summary_entries.push(BuildServiceEntry {
                        position: entry.position,
                        name: entry.pco_title.clone(),
                        action: format!("uncertain: {}", entry.reason),
                        file_path: None,
                        slides: None,
                    });
                }
                preview::PreviewStatus::Used => {
                    // Reference existing library file
                    let file_path = entry.file_path.clone().unwrap_or_default();
                    let embedded_data = std::fs::read(&file_path).ok();

                    // Apply arrangement if needed
                    let effective_arr = arr_override
                        .map(String::from)
                        .or_else(|| entry.arrangement.clone());
                    let arrangement_uuid = if effective_arr.is_some() && embedded_data.is_some() {
                        // Read presentation, find arrangement UUID
                        Self::resolve_arrangement_uuid(&file_path, effective_arr.as_deref())
                    } else {
                        None
                    };

                    let slide_type = Self::infer_slide_type(entry);
                    let file_stem = std::path::Path::new(&file_path)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or(&entry.playlist_name);

                    playlist_entries.push(PlaylistEntry {
                        name: file_stem.to_string(),
                        slide_type,
                        from_matched_file: true,
                        presentation_path: file_path.clone(),
                        arrangement_uuid,
                        embedded_data,
                    });

                    library_count += 1;
                    summary_entries.push(BuildServiceEntry {
                        position: entry.position,
                        name: entry.playlist_name.clone(),
                        action: "library".to_string(),
                        file_path: Some(file_path),
                        slides: None,
                    });
                }
                preview::PreviewStatus::Edited => {
                    // Generate from parsed content or description
                    let result = self
                        .generate_edited_entry(entry, bg_override, arr_override)
                        .await?;
                    let slides = result.1;
                    playlist_entries.push(result.0);

                    generated_count += 1;
                    summary_entries.push(BuildServiceEntry {
                        position: entry.position,
                        name: entry.playlist_name.clone(),
                        action: "generated".to_string(),
                        file_path: None,
                        slides: Some(slides),
                    });
                }
                preview::PreviewStatus::Created => {
                    // Scripture or other created items
                    let result = self
                        .generate_created_entry(entry, bg_override)
                        .await?;
                    let slides = result.1;
                    playlist_entries.push(result.0);

                    generated_count += 1;
                    summary_entries.push(BuildServiceEntry {
                        position: entry.position,
                        name: entry.playlist_name.clone(),
                        action: "generated".to_string(),
                        file_path: None,
                        slides: Some(slides),
                    });
                }
            }
        }

        // 5. Build playlist
        let playlist_name = args.playlist_name.unwrap_or_else(|| {
            args.service_name
                .as_deref()
                .unwrap_or("Service")
                .to_string()
        });
        let playlist = build_playlist(&playlist_name, &playlist_entries);
        let output_path = playlist_output_path(self.library_path.as_deref(), &playlist_name);
        write_playlist_file(&playlist, &playlist_entries, &output_path)
            .map_err(|e| mcp_err(e.to_string()))?;

        // 6. Return summary
        let result = BuildServiceResult {
            playlist_path: output_path.display().to_string(),
            entries: summary_entries,
            total_items: playlist_entries.len(),
            generated_count,
            library_count,
            skipped_count,
        };

        json_result(&result)
    }

    /// Build a `ProPresenter` playlist from an ordered list of presentations.
    #[tool(description = "Build a ProPresenter .proplaylist file from an ordered list of presentations. Each item needs a name, file_path to its .pro file, and slide_type. Returns the path to the generated playlist.")]
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
// Context rendering — single source of truth from item_mappings.json
// ---------------------------------------------------------------------------

/// Render context documentation from the mappings config.
///
/// Replaces `mcp_context.md` — everything derives from `item_mappings.json`.
#[allow(clippy::too_many_lines)]
fn render_context(mappings: &preview::ItemMappings, theme_name: Option<&str>, theme_slides: &[String], macro_names: &[String]) -> String {
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
    out.push_str("3. `preview_playlist` — classified items with parsed content, backgrounds, arrangements\n");
    out.push_str("4. Present preview to user (see Review Protocol below), get confirmation\n");
    out.push_str("5. `build_service` — generates all slides, applies styling, builds .proplaylist\n\n");

    // Review protocol
    out.push_str("## Review Protocol\n\n");
    out.push_str("After `preview_playlist`, present a table to the user showing every entry.\n");
    out.push_str("Flag items that need attention:\n\n");
    out.push_str("### Must ask the user about:\n");
    out.push_str("- **Uncertain** items (status=uncertain) — nametag not found, library miss, etc.\n");
    out.push_str("  Ask: skip it, use a different file, or create it?\n");
    out.push_str("- **Edited items with no parsed_content** — description was empty or unparseable.\n");
    out.push_str("  The library file will be used as-is. Flag if the item normally has weekly content.\n");
    out.push_str("- **Songs not found** (status=skipped, type=song) — expected in library but missing.\n");
    out.push_str("  Ask: is this a known song under a different name? Should it be skipped?\n\n");
    out.push_str("### Surface for awareness (don't block on these):\n");
    out.push_str("- Items with parsed_content — show a brief preview of what will be generated\n");
    out.push_str("- Scripture references — confirm book/chapter/verse and version look correct\n");
    out.push_str("- Total item count — does the playlist length look right for this service?\n\n");
    out.push_str("### After confirmation:\n");
    out.push_str("Call `build_service` with `skip_positions` for any items the user wants removed,\n");
    out.push_str("and `overrides` for any corrections (different name, background, arrangement).\n\n");
    out.push_str("### Learning:\n");
    out.push_str("If the user says \"X always maps to Y\", update `data/item_mappings.json` so\n");
    out.push_str("the mapping is permanent. This reduces future uncertain items.\n\n");

    // Presentation types table
    out.push_str("## Presentation Types\n\n");
    out.push_str("| Type | Template | Edited | Background | Macro | Arrangement | Description |\n");
    out.push_str("|------|----------|--------|------------|-------|-------------|-------------|\n");

    let mut type_keys: Vec<_> = mappings.presentation_types.keys().collect();
    type_keys.sort();
    for key in &type_keys {
        let pt = &mappings.presentation_types[*key];
        let _ = writeln!(
            out,
            "| `{key}` | {} | {} | {} | {} | {} | {} |",
            pt.template.as_deref().unwrap_or("—"),
            if pt.edited { "yes" } else { "no" },
            pt.background.as_deref().unwrap_or("—"),
            pt.macro_name.as_deref().unwrap_or("—"),
            pt.arrangement.as_deref().unwrap_or("—"),
            pt.description,
        );
    }

    // Item → type mappings
    out.push_str("\n## Item → Type Mappings\n\n");
    out.push_str("| PCO Title Pattern (lowercase prefix) | Type |\n");
    out.push_str("|--------------------------------------|------|\n");
    let mut item_keys: Vec<_> = mappings.item_types.iter().collect();
    item_keys.sort_by_key(|(k, _)| k.as_str());
    for (pattern, type_key) in &item_keys {
        if pattern.starts_with('_') {
            continue;
        }
        let _ = writeln!(out, "| `{pattern}` | `{type_key}` |");
    }

    // Skip rules
    out.push_str("\n## Skip Rules\n\n");
    for skip in &mappings.skip_items {
        let _ = writeln!(out, "- `{skip}`");
    }

    // Multi-expand rules
    out.push_str("\n## Multi-Item Expansion\n\n");
    for (pattern, expansion) in &mappings.multi_expand {
        let _ = writeln!(out, "- `{pattern}` → {}", expansion.join(", "));
    }

    // Nametag pattern
    if let Some(ref pattern) = mappings.nametag_pattern {
        let _ = writeln!(out, "\n## Nametag Pattern\n\n`{pattern}`\n");
    }

    // Staff
    out.push_str("\n## Staff\n\n");
    out.push_str("| Name | Role |\n");
    out.push_str("|------|------|\n");
    for (name, staff) in &mappings.staff {
        let _ = writeln!(out, "| {name} {} | {} |", staff.last, staff.role);
    }

    // Service types
    if let Some(ref st) = mappings.service_types {
        out.push_str("\n## Service Types\n\n");
        if !st.primary.is_empty() {
            let _ = writeln!(out, "**Primary:** {}", st.primary.join(", "));
        }
        if !st.seasonal.is_empty() {
            let _ = writeln!(out, "**Seasonal:** {}", st.seasonal.join(", "));
        }
    }

    // Service overrides
    if !mappings.service_overrides.is_empty() {
        out.push_str("\n## Service Overrides\n\n");
        for (service, overrides) in &mappings.service_overrides {
            let _ = writeln!(out, "**{service}:**");
            for (type_key, ovr) in overrides {
                let mut parts = Vec::new();
                if let Some(ref arr) = ovr.arrangement {
                    parts.push(format!("arrangement={arr}"));
                }
                let _ = writeln!(out, "- `{type_key}`: {}", parts.join(", "));
            }
        }
    }

    // Description parsing rules (stable preamble)
    out.push_str("\n## Description Parsing\n\n");
    out.push_str("Descriptions are auto-parsed by `build_service`. The parser handles:\n\n");
    out.push_str("### Marker-based (Prayer of Confession, etc.)\n");
    out.push_str("- `[SLIDE/ALL]` → yellow (#FFFF00), congregation reads\n");
    out.push_str("- `[SLIDE]` → default color\n");
    out.push_str("- `[no slide]`, `[SILENT CONFESSION]` → skipped\n");
    out.push_str("- Content inside `[brackets]` after markers is extracted\n\n");
    out.push_str("### Responsive readings (Call to Worship, etc.)\n");
    out.push_str("- `Leader:` / `L:` → white (default)\n");
    out.push_str("- `People:` / `All:` / `P:` / `Unison:` → yellow (#FFFF00)\n");
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
