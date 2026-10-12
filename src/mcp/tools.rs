//! MCP tool implementations.
//!
//! This module translates wire arguments into existing domain operations. The
//! server remains the sole owner of clients, caches, paths, and reviewed state.

use std::sync::{Arc, MutexGuard};

use proflow_propresenter::library::LibraryCatalog;
use proflow_propresenter::playlist_document::playlist_document_schema;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};
use serde::Serialize;

use crate::project_config::{parse_project_config_value, RawProjectConfig};
use crate::setup;
use crate::workflow::classify;
use proflow_propresenter::theme::ThemeCache;

use super::config::write_config_reviewed;
use super::playlist::{read_playlist_document, read_playlist_err, PlaylistRequest};
use super::review::{bounded_days, bounded_usize, recorded_decisions};
use super::schema::{
    format_category, BuildServiceArgs, CatalogAssetsArgs, ConfigValidationResponse,
    ConfigWriteResponse, EffectiveConfigResponse, ExplainRuleMatchArgs, ExplainRuleMatchInput,
    ExplainRuleMatchResponse, FetchPlanArgs, FileMatch, ItemResponse, PlanResponse,
    PreviewPlaylistArgs, ReadPlaylistArgs, ReviewedMediaAssetResponse, ReviewedPreviewResponse,
    ScriptureResponse, SearchLibraryArgs, SongResponse, WriteProjectConfigArgs,
};
use super::{build_err, mcp_err, ProFlowServer};
use crate::workstation::RecordedBuild;

impl ProFlowServer {
    fn library_catalog(&self) -> Result<MutexGuard<'_, LibraryCatalog>, rmcp::ErrorData> {
        self.workstation.library().lock().map_err(|_| {
            mcp_err("presentation library catalog is unavailable after an earlier failure")
        })
    }
}

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

    /// Return the machine-readable playlist document contract.
    #[tool(
        description = "Return the JSON Schema of the playlist document returned by read_playlist (versioned by its schema_version field). Use this to interpret item kinds, presentation content states, and diagnostics."
    )]
    async fn playlist_document_schema(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        json_result(&playlist_document_schema())
    }

    /// Read an existing playlist as a playlist document.
    #[tool(
        description = "Read an existing ProPresenter playlist as a versioned JSON playlist document: ordered items with kind, source file, arrangement, and a compact content summary (cue groups and labels, scripture reference, media). Pass exactly one of name (a playlist or folder in the ProPresenter playlist library) or path (a .proplaylist file inside the playlist output directory or ProPresenter data root). A presentation that cannot be read stays in the document with a typed unavailable reason. Read-only; the document cannot be used as build input."
    )]
    async fn read_playlist(
        &self,
        Parameters(args): Parameters<ReadPlaylistArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let request = PlaylistRequest::try_from(args).map_err(|error| read_playlist_err(&error))?;
        let document =
            read_playlist_document(self.workstation.render_assets().locations(), &request)
                .map_err(|error| read_playlist_err(&error))?;
        json_result(&document)
    }

    /// Show the normalized config the runtime is actually using.
    #[tool(
        description = "Show the project config the runtime is currently using, alongside validation results. Use this to inspect the effective config state before debugging rule behavior."
    )]
    async fn show_effective_config(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        json_result(&EffectiveConfigResponse {
            config: self.workstation.render_assets().config().clone(),
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
        let config = parse_project_config_value(args.config)
            .map_err(|error| rmcp::ErrorData::invalid_params(error.to_string(), None))?;
        let (outcome, validation) = write_config_reviewed(
            &config,
            self.workstation.render_assets().locations(),
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
        let mappings = self.workstation.render_assets().config();
        let requested_theme = args
            .theme_name
            .as_deref()
            .map(|theme_name| {
                ThemeCache::load_from_dir(
                    Some(theme_name),
                    self.workstation.render_assets().locations().themes(),
                )
            })
            .transpose()
            .map_err(|error| mcp_err(format!("failed to inspect requested theme: {error}")))?;
        let theme_cache = requested_theme
            .as_ref()
            .unwrap_or_else(|| self.workstation.render_assets().themes());
        let file_index = self.library_catalog()?;

        let catalog = setup::catalog_assets(
            mappings,
            theme_cache,
            self.workstation.render_assets().macros(),
            self.workstation.group_catalog(),
            &file_index,
            self.workstation
                .render_assets()
                .locations()
                .presentation_library(),
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
            self.workstation
                .render_assets()
                .config()
                .plan_lookahead_days(),
        )?;
        let (services, plans) = self
            .workstation
            .planning_center()
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
                let workstation = Arc::clone(&self.workstation);
                let plan_id = p.id.clone();
                async move {
                    workstation
                        .planning_center()
                        .get_service_items(&plan_id)
                        .await
                }
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
        description = "Resolve a plan's title, date, and service type from Planning Center, record the given playlist name, skip, entry override, package-mode, and media choices as the plan's decisions (replacing any recorded earlier), then return the exact effective playlist for review. Fully resolved previews render and verify every native artifact and return a preview_revision. Previews with uncertain items return open_decisions and no revision, and cannot be built until re-previewed with decisions. Each open decision lists options, ranked by what was chosen for the same question before. A question the operator set a default for is answered with it and reported in applied_defaults."
    )]
    async fn preview_playlist(
        &self,
        Parameters(args): Parameters<PreviewPlaylistArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let plan_id = args.plan_id.clone();
        let service_name = args.service_name.clone();
        let decisions = recorded_decisions(args);
        self.workstation
            .record_decisions(&plan_id, &decisions)
            .map_err(build_err)?;
        self.workstation
            .remember_decisions(&plan_id, &decisions)
            .map_err(build_err)?;
        let RecordedBuild {
            review: reviewed,
            open_decisions,
            applied_defaults,
            refused_defaults,
        } = self
            .workstation
            .build_recorded(&plan_id, service_name.as_deref())
            .await
            .map_err(build_err)?;
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

        let preview_revision = match reviewed {
            crate::workflow::execute::BuildReview::Prepared(prepared) => Some(prepared.revision()),
            crate::workflow::execute::BuildReview::NeedsReview(_) => None,
        };

        json_result(&ReviewedPreviewResponse {
            preview_revision,
            open_decisions,
            applied_defaults,
            refused_defaults,
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

        let mappings = self.workstation.render_assets().config();
        let index_guard = self.library_catalog()?;
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
        let index = self.library_catalog()?;
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
        description = "Install the verified build that preview_playlist rendered. build_service accepts no new output options. The plan is fetched again: if the plan, configuration, library, recorded decisions, or any source or target changed since the preview, or preview_revision is not the revision of the build on disk, nothing is installed and a new preview is required. A committed build cannot be committed twice."
    )]
    async fn build_service(
        &self,
        Parameters(args): Parameters<BuildServiceArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let result = self
            .workstation
            .commit_recorded(
                &args.plan_id,
                args.service_name.as_deref(),
                Some(&args.preview_revision),
            )
            .await
            .map_err(build_err)?;
        json_result(&result)
    }
}

#[cfg(test)]
mod tests;
