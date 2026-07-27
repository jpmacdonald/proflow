//! Per-plan rendering and presentation staging.

use std::path::{Path, PathBuf};

use crate::propresenter::playlist::{PlaylistEntry, PlaylistError};
use crate::propresenter::text_fit::NativeTextFitOracle;
use crate::propresenter::PresentationSize;
use crate::workflow::approval::CapturedSources;
use crate::workflow::description_parser::ParsedContent;
use crate::workflow::plan::{
    PlanDisposition, ReadyAction, RenderStyle, ResolvedItemPlan, ScriptureContent,
};
use crate::workflow::transaction::BuildFileTransaction;

use super::presentation_output::{ReviewedBackgroundAsset, ReviewedRenderTarget};
use super::rendered_service::{
    edited_summary, generated_plan, library_summary, restyled_summary, skipped_summary,
    RenderedPlan, RenderedService,
};
use super::review::ReviewedBackgroundPath;
use super::{
    captured_source_bytes, unresolved_plan_error, BuildServiceError, ServiceBuildExecutor,
};

impl ServiceBuildExecutor<'_> {
    pub(super) fn render_plans(
        &self,
        plans: &[ResolvedItemPlan],
        presentation_size: PresentationSize,
        backgrounds: &[ReviewedBackgroundPath],
        sources: &CapturedSources,
        transaction: BuildFileTransaction,
    ) -> Result<RenderedService, BuildServiceError> {
        let mut text_fit = NativeTextFitOracle::start_bundled()
            .map_err(crate::workflow::presentation_render::PresentationRenderError::from)?;
        let mut rendered = RenderedService::new(transaction, text_fit.contract().clone());
        for plan in plans {
            let background = reviewed_background(plan, backgrounds, sources)?;
            let output = self.render_plan(
                plan,
                PlanExecutionInputs {
                    presentation_size,
                    background,
                    sources,
                    transaction: &mut rendered.transaction,
                    text_fit: &mut text_fit,
                },
            )?;
            rendered.record(plan, presentation_size, output)?;
        }
        Ok(rendered)
    }

    fn render_plan(
        &self,
        plan: &ResolvedItemPlan,
        inputs: PlanExecutionInputs<'_>,
    ) -> Result<RenderedPlan, BuildServiceError> {
        match plan.disposition() {
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
        let playlist_entry =
            PlaylistEntry::embedded(prepared.name, prepared_path.clone(), prepared.embedded_data)
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
            text_fit,
        } = inputs;
        let staged_path = transaction.stage_reviewed(file_path)?;
        let target = ReviewedRenderTarget {
            write_path: &staged_path,
            final_path: file_path,
            existing_bytes: Some(captured_source_bytes(sources, file_path)?),
            presentation_size,
            background,
        };
        let (playlist_entry, slides, text_fit_evidence, resolved_macro_regions) =
            self.edit_description(plan, content, style, target, text_fit)?;
        let file_path = playlist_entry.presentation_path().to_string();
        Ok(RenderedPlan::Generated {
            playlist_entry,
            summary: edited_summary(plan, file_path, slides),
            text_fit_evidence,
            resolved_macro_regions: Some(resolved_macro_regions),
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
            text_fit,
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
        let final_bytes = playlist_entry.embedded_data().ok_or_else(|| {
            BuildServiceError::MissingPresentationEvidence {
                output_key: plan.output_key.to_string(),
            }
        })?;
        let final_presentation = crate::propresenter::deserialize::decode_presentation_bytes(
            final_bytes,
            playlist_entry.presentation_path(),
        )?;
        let text_fit_evidence = super::restyle_text_fit::prove_restyled_text_fit(
            &final_presentation,
            self.render_assets,
            text_fit,
        )
        .map_err(|source| BuildServiceError::RestyleTextFit {
            presentation: plan.playlist_name.clone(),
            reason: source.to_string(),
        })?;
        let file_path = playlist_entry.presentation_path().to_string();
        Ok(RenderedPlan::Generated {
            playlist_entry,
            summary: restyled_summary(plan, file_path, slides),
            text_fit_evidence,
            resolved_macro_regions: None,
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
            text_fit,
        } = inputs;
        let staged = self.stage_generated_presentation(plan, transaction)?;
        let target = staged.reviewed_target(sources, presentation_size, background);
        let (playlist_entry, slides, text_fit_evidence, resolved_macro_regions) =
            self.generate_description(plan, content, style, target, text_fit)?;
        Ok(generated_plan(
            plan,
            playlist_entry,
            slides,
            text_fit_evidence,
            resolved_macro_regions,
        ))
    }

    fn render_generated_scripture(
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
            text_fit,
        } = inputs;
        let staged = self.stage_generated_presentation(plan, transaction)?;
        let target = staged.reviewed_target(sources, presentation_size, background);
        let (playlist_entry, slides, text_fit_evidence, resolved_macro_regions) =
            self.generate_scripture(plan, scripture, style, target, sources, text_fit)?;
        Ok(generated_plan(
            plan,
            playlist_entry,
            slides,
            text_fit_evidence,
            resolved_macro_regions,
        ))
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
            text_fit,
        } = inputs;
        let staged = self.stage_generated_presentation(plan, transaction)?;
        let target = staged.reviewed_target(sources, presentation_size, background);
        let (playlist_entry, slides, text_fit_evidence, resolved_macro_regions) =
            self.generate_title(plan, text, style, target, text_fit)?;
        Ok(generated_plan(
            plan,
            playlist_entry,
            slides,
            text_fit_evidence,
            resolved_macro_regions,
        ))
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

struct PlanExecutionInputs<'a> {
    presentation_size: PresentationSize,
    background: Option<ReviewedBackgroundAsset<'a>>,
    sources: &'a CapturedSources,
    transaction: &'a mut BuildFileTransaction,
    text_fit: &'a mut NativeTextFitOracle,
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
