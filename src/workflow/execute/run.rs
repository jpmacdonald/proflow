//! Execution of an approved plan through one filesystem transaction.

use std::path::{Path, PathBuf};

use crate::propresenter::library::PreparedLibraryUpdate;
use crate::propresenter::playlist::{PlaylistEntry, PlaylistError};
use crate::propresenter::PresentationSize;
use crate::workflow::approval::CapturedSources;
use crate::workflow::description_parser::ParsedContent;
use crate::workflow::plan::{
    PlanDisposition, ReadyAction, RenderStyle, ResolvedItemPlan, ScriptureContent,
};
use crate::workflow::report::{BuildServiceEntry, BuildServiceResult};
use crate::workflow::transaction::BuildFileTransaction;
use crate::workflow::transaction::PreparedFileTransaction;

use super::presentation_output::{ReviewedBackgroundAsset, ReviewedRenderTarget};
use super::review::{PreparedBuildRequest, ReviewedBackgroundPath};
use super::{
    captured_source_bytes, unresolved_plan_error, BuildServiceError, ServiceBuildExecutor,
};

impl ServiceBuildExecutor<'_> {
    /// Materialize every native artifact while the reviewed source and output
    /// snapshots are still authoritative.
    pub(super) async fn prepare_reviewed_service(
        &self,
        inputs: super::review::ReviewedBuildInputs,
    ) -> Result<PreparedBuildRequest, BuildServiceError> {
        let super::review::ReviewedBuildInputs {
            mut request,
            reviewed,
            presentation_size,
            backgrounds,
            outputs,
        } = inputs;
        let transaction = BuildFileTransaction::from_reviewed(outputs);
        let mut rendered = self
            .render_plans(
                reviewed.plans(),
                presentation_size,
                &backgrounds,
                reviewed.sources(),
                transaction,
            )
            .await?;
        let playlist_export = self.stage_playlist(
            &mut request,
            reviewed.sources(),
            &rendered.playlist_entries,
            &mut rendered.transaction,
        )?;

        let mut warnings = collect_build_warnings(&rendered.summary_entries);
        warnings.extend(playlist_export.warnings);
        let result = BuildServiceResult {
            playlist_path: playlist_export.path.display().to_string(),
            package_mode: request.playlist_package_mode,
            media_asset_count: playlist_export.media_asset_count,
            total_items: rendered.playlist_entries.len(),
            entries: rendered.summary_entries,
            generated_count: rendered.counts.generated,
            library_count: rendered.counts.library,
            skipped_count: rendered.counts.skipped,
            warnings,
        };

        let transaction = rendered.transaction.seal()?;
        let artifacts = transaction.presentation_artifacts()?;
        let catalog_updates = {
            let catalog = self.file_index.lock().await;
            artifacts
                .iter()
                .map(|(path, bytes)| catalog.prepare_owned_update(path, bytes))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .flatten()
                .collect()
        };

        let prepared = PreparedService {
            transaction,
            catalog_updates,
            result,
        };
        let (plans, sources) = reviewed.into_verified_parts()?;
        Ok(PreparedBuildRequest::from_materialized(
            request, plans, sources, prepared,
        ))
    }

    pub(super) async fn commit_prepared_service(
        &self,
        reviewed: PreparedBuildRequest,
    ) -> Result<BuildServiceResult, BuildServiceError> {
        let (sources, prepared) = reviewed.into_commit_parts();
        let PreparedService {
            transaction,
            catalog_updates,
            result,
        } = prepared;
        let mut catalog = self.file_index.lock().await;
        let committed_catalog = catalog.with_prepared_updates(&catalog_updates)?;
        sources.verify()?;
        transaction.commit()?;
        *catalog = committed_catalog;
        drop(catalog);
        Ok(result)
    }

    async fn render_plans(
        &self,
        plans: &[ResolvedItemPlan],
        presentation_size: PresentationSize,
        backgrounds: &[ReviewedBackgroundPath],
        sources: &CapturedSources,
        transaction: BuildFileTransaction,
    ) -> Result<RenderedService, BuildServiceError> {
        let mut rendered = RenderedService::new(transaction);
        for plan in plans {
            let background = reviewed_background(plan, backgrounds, sources)?;
            let output = self
                .render_plan(
                    plan,
                    PlanExecutionInputs {
                        presentation_size,
                        background,
                        sources,
                        transaction: &mut rendered.transaction,
                    },
                )
                .await?;
            rendered.record(output);
        }
        Ok(rendered)
    }

    async fn render_plan(
        &self,
        plan: &ResolvedItemPlan,
        inputs: PlanExecutionInputs<'_>,
    ) -> Result<RenderedPlan, BuildServiceError> {
        match &plan.disposition {
            PlanDisposition::Skip => Ok(RenderedPlan::Skipped(skipped_summary(plan))),
            PlanDisposition::NeedsReview(_) => Err(unresolved_plan_error(plan)),
            PlanDisposition::Ready(ReadyAction::UseExisting {
                file_path,
                arrangement,
            }) => Self::render_existing(
                plan,
                file_path,
                arrangement.as_deref(),
                inputs.sources,
                inputs.presentation_size,
            ),
            PlanDisposition::Ready(ReadyAction::RestyleExisting {
                file_path,
                arrangement,
                transform,
            }) => self.render_restyled_existing(
                plan,
                file_path,
                arrangement.as_deref(),
                transform,
                inputs,
            ),
            PlanDisposition::Ready(ReadyAction::EditDescription {
                file_path,
                parsed_content,
                style,
            }) => self.render_edited_description(plan, file_path, parsed_content, style, inputs),
            PlanDisposition::Ready(ReadyAction::GenerateDescription {
                parsed_content,
                style,
            }) => self.render_generated_description(plan, parsed_content, style, inputs),
            PlanDisposition::Ready(ReadyAction::GenerateScripture { scripture, style }) => {
                self.render_generated_scripture(plan, scripture, style, inputs)
                    .await
            }
            PlanDisposition::Ready(ReadyAction::GenerateTitle { text, style }) => {
                self.render_generated_title(plan, text, style, inputs)
            }
        }
    }

    fn render_existing(
        plan: &ResolvedItemPlan,
        file_path: &Path,
        arrangement: Option<&str>,
        sources: &CapturedSources,
        presentation_size: PresentationSize,
    ) -> Result<RenderedPlan, BuildServiceError> {
        let source_bytes = captured_source_bytes(sources, file_path)?;
        let prepared = Self::prepare_existing_presentation(
            plan.output_key.as_str(),
            file_path,
            arrangement,
            source_bytes,
            presentation_size,
        )?;
        let prepared_path = prepared.file_path.display().to_string();
        let playlist_entry = PlaylistEntry::embedded(
            plan.playlist_name.clone(),
            prepared_path.clone(),
            prepared.embedded_data,
        )
        .map_err(PlaylistError::from)?
        .with_selected_arrangement(prepared.selected_arrangement)
        .map_err(PlaylistError::from)?;
        Ok(RenderedPlan::Library {
            playlist_entry,
            summary: library_summary(plan, prepared_path),
        })
    }

    fn render_edited_description(
        &self,
        plan: &ResolvedItemPlan,
        file_path: &Path,
        content: &ParsedContent,
        style: &RenderStyle,
        inputs: PlanExecutionInputs<'_>,
    ) -> Result<RenderedPlan, BuildServiceError> {
        let PlanExecutionInputs {
            presentation_size,
            background,
            sources,
            transaction,
        } = inputs;
        let staged_path = transaction.stage_reviewed(file_path)?;
        let target = ReviewedRenderTarget {
            write_path: &staged_path,
            final_path: file_path,
            existing_bytes: Some(captured_source_bytes(sources, file_path)?),
            presentation_size,
            background,
        };
        let (playlist_entry, slides) = self.edit_description(plan, content, style, target)?;
        let file_path = playlist_entry.presentation_path().to_string();
        Ok(RenderedPlan::Generated {
            playlist_entry,
            summary: edited_summary(plan, file_path, slides),
        })
    }

    fn render_restyled_existing(
        &self,
        plan: &ResolvedItemPlan,
        file_path: &Path,
        arrangement: Option<&str>,
        transform: &crate::workflow::plan::ExistingTransform,
        inputs: PlanExecutionInputs<'_>,
    ) -> Result<RenderedPlan, BuildServiceError> {
        let PlanExecutionInputs {
            presentation_size,
            background,
            sources,
            transaction,
        } = inputs;
        let source_bytes = captured_source_bytes(sources, file_path)?;
        let staged = StagedPresentation {
            final_path: file_path.to_path_buf(),
            write_path: transaction.stage_reviewed(file_path)?,
        };
        let (playlist_entry, slides) = self.restyle_existing_presentation(
            plan,
            file_path,
            arrangement,
            transform,
            source_bytes,
            staged.reviewed_target(sources, presentation_size, background),
        )?;
        let file_path = playlist_entry.presentation_path().to_string();
        Ok(RenderedPlan::Generated {
            playlist_entry,
            summary: restyled_summary(plan, file_path, slides),
        })
    }

    fn render_generated_description(
        &self,
        plan: &ResolvedItemPlan,
        content: &ParsedContent,
        style: &RenderStyle,
        inputs: PlanExecutionInputs<'_>,
    ) -> Result<RenderedPlan, BuildServiceError> {
        let PlanExecutionInputs {
            presentation_size,
            background,
            sources,
            transaction,
        } = inputs;
        let staged = self.stage_generated_presentation(plan, transaction)?;
        let target = staged.reviewed_target(sources, presentation_size, background);
        let (playlist_entry, slides) = self.generate_description(plan, content, style, target)?;
        Ok(generated_plan(plan, playlist_entry, slides))
    }

    async fn render_generated_scripture(
        &self,
        plan: &ResolvedItemPlan,
        scripture: &ScriptureContent,
        style: &RenderStyle,
        inputs: PlanExecutionInputs<'_>,
    ) -> Result<RenderedPlan, BuildServiceError> {
        let PlanExecutionInputs {
            presentation_size,
            background,
            sources,
            transaction,
        } = inputs;
        let staged = self.stage_generated_presentation(plan, transaction)?;
        let target = staged.reviewed_target(sources, presentation_size, background);
        let (playlist_entry, slides) = self
            .generate_scripture(plan, scripture, style, target, sources)
            .await?;
        Ok(generated_plan(plan, playlist_entry, slides))
    }

    fn render_generated_title(
        &self,
        plan: &ResolvedItemPlan,
        text: &str,
        style: &RenderStyle,
        inputs: PlanExecutionInputs<'_>,
    ) -> Result<RenderedPlan, BuildServiceError> {
        let PlanExecutionInputs {
            presentation_size,
            background,
            sources,
            transaction,
        } = inputs;
        let staged = self.stage_generated_presentation(plan, transaction)?;
        let target = staged.reviewed_target(sources, presentation_size, background);
        let (playlist_entry, slides) = self.generate_title(plan, text, style, target)?;
        Ok(generated_plan(plan, playlist_entry, slides))
    }

    fn stage_generated_presentation(
        &self,
        plan: &ResolvedItemPlan,
        transaction: &mut BuildFileTransaction,
    ) -> Result<StagedPresentation, BuildServiceError> {
        let final_path = self.presentation_target(plan)?;
        let write_path = transaction.stage_reviewed(&final_path)?;
        Ok(StagedPresentation {
            final_path,
            write_path,
        })
    }
}

pub(super) struct PreparedService {
    transaction: PreparedFileTransaction,
    catalog_updates: Vec<PreparedLibraryUpdate>,
    result: BuildServiceResult,
}

#[cfg(test)]
impl PreparedService {
    pub(super) fn artifact_bytes(&self, target: &Path) -> std::io::Result<Option<Vec<u8>>> {
        self.transaction.staged_bytes_for(target)
    }

    pub(super) fn offline_test(transaction: PreparedFileTransaction) -> Self {
        Self {
            transaction,
            catalog_updates: Vec::new(),
            result: BuildServiceResult {
                playlist_path: String::new(),
                package_mode: crate::propresenter::package::PlaylistPackageMode::LibraryLocal,
                media_asset_count: 0,
                total_items: 0,
                entries: Vec::new(),
                generated_count: 0,
                library_count: 0,
                skipped_count: 0,
                warnings: Vec::new(),
            },
        }
    }
}

impl std::fmt::Debug for PreparedService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedService")
            .field("transaction", &self.transaction)
            .field("playlist_path", &self.result.playlist_path)
            .finish_non_exhaustive()
    }
}

struct PlanExecutionInputs<'a> {
    presentation_size: PresentationSize,
    background: Option<ReviewedBackgroundAsset<'a>>,
    sources: &'a CapturedSources,
    transaction: &'a mut BuildFileTransaction,
}

struct StagedPresentation {
    final_path: PathBuf,
    write_path: PathBuf,
}

impl StagedPresentation {
    fn reviewed_target<'a>(
        &'a self,
        sources: &'a CapturedSources,
        presentation_size: PresentationSize,
        background: Option<ReviewedBackgroundAsset<'a>>,
    ) -> ReviewedRenderTarget<'a> {
        ReviewedRenderTarget {
            write_path: &self.write_path,
            final_path: &self.final_path,
            existing_bytes: sources.bytes(&self.final_path),
            presentation_size,
            background,
        }
    }
}

struct RenderedService {
    transaction: BuildFileTransaction,
    playlist_entries: Vec<PlaylistEntry>,
    summary_entries: Vec<BuildServiceEntry>,
    counts: BuildCounts,
}

impl RenderedService {
    fn new(transaction: BuildFileTransaction) -> Self {
        Self {
            transaction,
            playlist_entries: Vec::new(),
            summary_entries: Vec::new(),
            counts: BuildCounts::default(),
        }
    }

    fn record(&mut self, rendered: RenderedPlan) {
        match rendered {
            RenderedPlan::Generated {
                playlist_entry,
                summary,
            } => {
                self.playlist_entries.push(playlist_entry);
                self.summary_entries.push(summary);
                self.counts.generated += 1;
            }
            RenderedPlan::Library {
                playlist_entry,
                summary,
            } => {
                self.playlist_entries.push(playlist_entry);
                self.summary_entries.push(summary);
                self.counts.library += 1;
            }
            RenderedPlan::Skipped(summary) => {
                self.summary_entries.push(summary);
                self.counts.skipped += 1;
            }
        }
    }
}

#[derive(Default)]
struct BuildCounts {
    generated: usize,
    library: usize,
    skipped: usize,
}

enum RenderedPlan {
    Generated {
        playlist_entry: PlaylistEntry,
        summary: BuildServiceEntry,
    },
    Library {
        playlist_entry: PlaylistEntry,
        summary: BuildServiceEntry,
    },
    Skipped(BuildServiceEntry),
}

fn reviewed_background<'a>(
    plan: &ResolvedItemPlan,
    backgrounds: &'a [ReviewedBackgroundPath],
    sources: &'a CapturedSources,
) -> Result<Option<ReviewedBackgroundAsset<'a>>, BuildServiceError> {
    backgrounds
        .iter()
        .find(|background| background.output_key == plan.output_key.as_str())
        .map(|background| {
            Ok(ReviewedBackgroundAsset {
                path: &background.path,
                data: captured_source_bytes(sources, &background.path)?,
            })
        })
        .transpose()
}

fn skipped_summary(plan: &ResolvedItemPlan) -> BuildServiceEntry {
    BuildServiceEntry {
        output_key: plan.output_key.to_string(),
        position: plan.position,
        name: plan.pco_title.clone(),
        action: format!("skipped: {}", plan.reason),
        file_path: None,
        slides: None,
        warnings: Vec::new(),
    }
}

fn library_summary(plan: &ResolvedItemPlan, file_path: String) -> BuildServiceEntry {
    BuildServiceEntry {
        output_key: plan.output_key.to_string(),
        position: plan.position,
        name: plan.playlist_name.clone(),
        action: "library".to_string(),
        file_path: Some(file_path),
        slides: None,
        warnings: Vec::new(),
    }
}

fn edited_summary(plan: &ResolvedItemPlan, file_path: String, slides: usize) -> BuildServiceEntry {
    BuildServiceEntry {
        output_key: plan.output_key.to_string(),
        position: plan.position,
        name: plan.playlist_name.clone(),
        action: "edited".to_string(),
        file_path: Some(file_path),
        slides: Some(slides),
        warnings: zero_slide_warnings(slides),
    }
}

fn restyled_summary(
    plan: &ResolvedItemPlan,
    file_path: String,
    slides: usize,
) -> BuildServiceEntry {
    BuildServiceEntry {
        output_key: plan.output_key.to_string(),
        position: plan.position,
        name: plan.playlist_name.clone(),
        action: "restyled".to_string(),
        file_path: Some(file_path),
        slides: Some(slides),
        warnings: zero_slide_warnings(slides),
    }
}

fn generated_plan(
    plan: &ResolvedItemPlan,
    playlist_entry: PlaylistEntry,
    slides: usize,
) -> RenderedPlan {
    let file_path = playlist_entry.presentation_path().to_string();
    RenderedPlan::Generated {
        playlist_entry,
        summary: BuildServiceEntry {
            output_key: plan.output_key.to_string(),
            position: plan.position,
            name: plan.playlist_name.clone(),
            action: "generated".to_string(),
            file_path: Some(file_path),
            slides: Some(slides),
            warnings: zero_slide_warnings(slides),
        },
    }
}

fn zero_slide_warnings(slides: usize) -> Vec<String> {
    if slides == 0 {
        vec!["presentation has zero slides".to_string()]
    } else {
        Vec::new()
    }
}

fn collect_build_warnings(entries: &[BuildServiceEntry]) -> Vec<String> {
    entries
        .iter()
        .flat_map(|entry| {
            entry
                .warnings
                .iter()
                .map(|warning| format!("{}: {warning}", entry.output_key))
        })
        .collect()
}
