//! MCP tool implementations.
//!
//! This module translates wire arguments into existing domain operations. The
//! server remains the sole owner of clients, caches, paths, and reviewed state.

use std::sync::Arc;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};
use serde::Serialize;

use crate::project_config::{parse_project_config_value, RawProjectConfig};
use crate::propresenter::playlist::{PlaylistExportIntent, PlaylistExportMode};
use crate::propresenter::theme::ThemeCache;
use crate::setup;
use crate::workflow::classify;
use crate::workflow::execute::BuildRequest;

use super::config::write_config_reviewed;
use super::review::{
    bounded_days, bounded_usize, consume_reviewed_plan, parse_media_assets,
    replace_prepared_snapshot, resolve_entry_override,
};
use super::schema::{
    format_category, BuildServiceArgs, CatalogAssetsArgs, ConfigValidationResponse,
    ConfigWriteResponse, EffectiveConfigResponse, ExplainRuleMatchArgs, ExplainRuleMatchInput,
    ExplainRuleMatchResponse, FetchPlanArgs, FileMatch, ItemResponse, PlanResponse,
    PreviewPlaylistArgs, ReviewedMediaAssetResponse, ReviewedPreviewResponse, ScriptureResponse,
    SearchLibraryArgs, SongResponse, WriteProjectConfigArgs,
};
use super::{mcp_err, ProFlowServer};

fn text_result(text: impl Into<String>) -> CallToolResult {
    CallToolResult::success(vec![Content::text(text.into())])
}

fn json_result(value: &impl Serialize) -> Result<CallToolResult, rmcp::ErrorData> {
    let json = serde_json::to_string_pretty(value).map_err(|error| mcp_err(error.to_string()))?;
    Ok(text_result(json))
}

#[tool_router(vis = "pub(super)")]
impl ProFlowServer {
    /// Return the machine-readable v4 project-config contract.
    #[tool(
        description = "Return the complete JSON Schema for proflow.config.json. Use this before authoring or changing presentation types, cue roles, item rules, required playlist items, or overrides."
    )]
    async fn project_config_schema(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        json_result(&schemars::schema_for!(RawProjectConfig))
    }

    /// Show the normalized config the runtime is actually using.
    #[tool(
        description = "Show the project config the runtime is currently using, alongside validation results. Use this to inspect the effective config state before debugging rule behavior."
    )]
    async fn show_effective_config(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        json_result(&EffectiveConfigResponse {
            config: self.render_assets.config().clone(),
            validation: ConfigValidationResponse {
                valid: true,
                issues: Vec::new(),
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
            self.render_assets.locations(),
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
        description = "Catalog local ProPresenter assets and current project config. Optionally load one exact installed theme ephemerally for discovery without changing the runtime snapshot. Returns theme slides, macros, cue groups, configured backgrounds and cue roles, library files, service groups, and presentation types."
    )]
    async fn catalog_assets(
        &self,
        Parameters(args): Parameters<CatalogAssetsArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let sample_limit = bounded_usize("sample_limit", args.sample_limit, 40, 200)?;
        let mappings = self.render_assets.config();
        let requested_theme = args
            .theme_name
            .as_deref()
            .map(|theme_name| {
                ThemeCache::load_from_dir(Some(theme_name), self.render_assets.locations().themes())
            })
            .transpose()
            .map_err(|error| mcp_err(format!("failed to inspect requested theme: {error}")))?;
        let theme_cache = requested_theme
            .as_ref()
            .unwrap_or_else(|| self.render_assets.themes());
        let file_index = self.file_index.lock().await;

        let catalog = setup::catalog_assets(
            mappings,
            theme_cache,
            self.render_assets.macros(),
            self.group_catalog.as_ref(),
            &file_index,
            self.render_assets.locations().presentation_library(),
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
            args.days_ahead,
            self.render_assets.config().plan_lookahead_days(),
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
        description = "Resolve a plan's title, date, and service type from Planning Center, apply any playlist name, skip, entry override, package-mode, and media choices, then return the exact effective playlist for review. Fully resolved previews materialize and seal every native artifact and return a one-time preview_revision. Previews with uncertain items return no revision and cannot be built until re-previewed with decisions."
    )]
    async fn preview_playlist(
        &self,
        Parameters(args): Parameters<PreviewPlaylistArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let media_assets = parse_media_assets(args.media_assets);
        let overrides = args
            .overrides
            .unwrap_or_default()
            .into_iter()
            .map(|entry| resolve_entry_override(self.render_assets.config(), entry))
            .collect::<Result<Vec<_>, _>>()?;
        let playlist_export_mode = args.package_mode.unwrap_or_default();
        let request = BuildRequest {
            plan_id: args.plan_id.clone(),
            service_name: args.service_name,
            playlist_name: args.playlist_name,
            skip_output_keys: args.skip_output_keys.unwrap_or_default(),
            overrides,
            playlist_export: match playlist_export_mode {
                PlaylistExportMode::LibraryLinks => PlaylistExportIntent::library_links(),
                PlaylistExportMode::PortableImport => {
                    PlaylistExportIntent::portable_import(media_assets)
                }
            },
        };
        let reviewed = self
            .service_build_executor()
            .review_service_request(request)
            .await
            .map_err(|error| mcp_err(error.to_string()))?;
        let plan_title = reviewed.plan_title().to_string();
        let service_name = reviewed.service_name().to_string();
        let date = reviewed.date().format("%Y-%m-%d").to_string();
        let entries = classify::render_preview(reviewed.plans());
        let playlist_name = reviewed.playlist_name().to_string();
        let package_mode = reviewed.playlist_export_mode();
        let media_assets = reviewed
            .additional_media_assets()
            .iter()
            .map(|asset| ReviewedMediaAssetResponse {
                path: asset.source_path.display().to_string(),
                archive_path: asset.archive_path.clone(),
            })
            .collect();
        let summary = classify::PreviewSummary::from_entries(&entries);
        let materialized = reviewed.materialized_result().cloned();
        let result = classify::PreviewResult {
            plan_title,
            service_name,
            date,
            entries,
            summary,
        };

        let prepared = match reviewed {
            crate::workflow::execute::BuildReview::Prepared(prepared) => Some(*prepared),
            crate::workflow::execute::BuildReview::NeedsReview(_) => None,
        };
        let preview_revision = {
            let mut snapshots = self.reviewed_plans.lock().await;
            replace_prepared_snapshot(&mut snapshots, args.plan_id, prepared)
        };

        json_result(&ReviewedPreviewResponse {
            preview_revision,
            playlist_name,
            package_mode,
            media_assets,
            materialized,
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

        let mappings = self.render_assets.config();
        let index_guard = self.file_index.lock().await;
        let entries = classify::build_preview(
            &[item],
            mappings,
            Some(&index_guard),
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
        let matches = index.find_matches(&args.query, max);
        let results: Vec<FileMatch> = matches
            .iter()
            .map(|entry| FileMatch {
                name: entry.file_name().to_string(),
                path: entry.full_path().to_string_lossy().to_string(),
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
            .build_prepared_request(reviewed.prepared)
            .await
            .map_err(|e| mcp_err(e.to_string()))?;
        json_result(&result)
    }
}
