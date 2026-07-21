//! Operator-facing serialization of typed service plans.

use serde::Serialize;

use super::description_parser::ParsedContent;
use super::plan::{
    PlanDisposition, ReadyAction, ResolvedItemPlan, ScriptureRefInfo, ScriptureRequest,
};
use crate::project_config::BackgroundId;

/// Status of a proposed playlist entry.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewStatus {
    /// Existing library file, no changes needed.
    Used,
    /// New file generated from scratch (scripture, etc.).
    Created,
    /// Library file whose content is refreshed from this week's description.
    Edited,
    /// Not included in the playlist.
    #[default]
    Skipped,
    /// Needs user confirmation.
    Uncertain,
}

/// A single row in the preview table.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PreviewEntry {
    /// Stable key identifying the source item within the service plan.
    pub output_key: String,
    /// Zero-based position of the item in its source service plan.
    pub position: usize,
    /// Item title supplied by Planning Center.
    pub pco_title: String,
    /// Operator-visible name written into the `ProPresenter` playlist.
    pub playlist_name: String,
    /// Existing or generated presentation path, when one is known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    /// Proposed disposition shown to the operator.
    pub status: PreviewStatus,
    /// Human-readable explanation for the proposed disposition.
    pub reason: String,
    /// Configured classification rule that produced this output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub classification_rule: Option<String>,
    /// Normalized configured item type, when classification found one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_type: Option<String>,
    /// Structured description content used to generate or edit a presentation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parsed_content: Option<ParsedContent>,
    /// Registered background selected by policy, when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<BackgroundId>,
    /// Requested song arrangement, when one was selected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arrangement: Option<String>,
    /// Single scripture reference, when the item has exactly one reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scripture_reference: Option<String>,
    /// Bible translation requested for a single scripture reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bible_version: Option<String>,
    /// Individual scripture references for multi-reference items.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub scripture_refs: Option<Vec<ScriptureRefInfo>>,
    /// Cue-role slide name used for generated content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_slide: Option<String>,
    /// Cue-role slide name used for a leading title cue.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_slide: Option<String>,
    /// `ProPresenter` macro triggered on the first operator-visible cue.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_cue_macro: Option<String>,
    /// `ProPresenter` macro triggered on the first content cue after the title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_content_cue_macro: Option<String>,
}

/// Full preview result.
#[derive(Debug, Serialize)]
pub struct PreviewResult {
    /// Title of the source Planning Center plan.
    pub plan_title: String,
    /// Configured service-type name.
    pub service_name: String,
    /// Service date rendered for the operator.
    pub date: String,
    /// Proposed playlist entries in source order.
    pub entries: Vec<PreviewEntry>,
    /// Aggregate counts derived from `entries`.
    pub summary: PreviewSummary,
}

/// Summary counts for the preview.
#[derive(Debug, Serialize)]
pub struct PreviewSummary {
    /// Existing presentations reused without edits.
    pub used_count: usize,
    /// Presentations proposed for creation.
    pub created_count: usize,
    /// Existing presentations proposed for content edits.
    pub edited_count: usize,
    /// Source items intentionally omitted from the playlist.
    pub skip_count: usize,
    /// Source items requiring an explicit operator decision.
    pub uncertain_count: usize,
    /// Number of entries that would be written to the playlist.
    pub total_playlist_items: usize,
}

impl PreviewSummary {
    /// Count one preview using the same definition at every operator boundary.
    #[must_use]
    pub fn from_entries(entries: &[PreviewEntry]) -> Self {
        let mut summary = Self {
            used_count: 0,
            created_count: 0,
            edited_count: 0,
            skip_count: 0,
            uncertain_count: 0,
            total_playlist_items: 0,
        };
        for entry in entries {
            match &entry.status {
                PreviewStatus::Used => summary.used_count += 1,
                PreviewStatus::Created => summary.created_count += 1,
                PreviewStatus::Edited => summary.edited_count += 1,
                PreviewStatus::Skipped => summary.skip_count += 1,
                PreviewStatus::Uncertain => summary.uncertain_count += 1,
            }
        }
        summary.total_playlist_items =
            summary.used_count + summary.created_count + summary.edited_count;
        summary
    }
}

impl From<&PlanDisposition> for PreviewStatus {
    fn from(disposition: &PlanDisposition) -> Self {
        match disposition {
            PlanDisposition::Ready(ReadyAction::UseExisting { .. }) => Self::Used,
            PlanDisposition::Ready(
                ReadyAction::EditDescription { .. } | ReadyAction::RestyleExisting { .. },
            ) => Self::Edited,
            PlanDisposition::Ready(
                ReadyAction::GenerateDescription { .. }
                | ReadyAction::GenerateScripture { .. }
                | ReadyAction::GenerateTitle { .. },
            ) => Self::Created,
            PlanDisposition::Skip => Self::Skipped,
            PlanDisposition::NeedsReview(_) => Self::Uncertain,
        }
    }
}

impl From<ResolvedItemPlan> for PreviewEntry {
    fn from(plan: ResolvedItemPlan) -> Self {
        let parsed_content = plan.parsed_content().cloned();
        let (scripture_reference, bible_version, scripture_refs) =
            plan.scripture_content()
                .map_or((None, None, None), |scripture| match scripture.request() {
                    ScriptureRequest::Single {
                        reference,
                        bible_version,
                    } => (
                        Some(reference.to_string()),
                        Some(bible_version.to_string()),
                        None,
                    ),
                    ScriptureRequest::PrefixExcerpt {
                        display_reference,
                        bible_version,
                        ..
                    } => (
                        Some(display_reference.to_string()),
                        Some(bible_version.to_string()),
                        None,
                    ),
                    ScriptureRequest::Combined(references) => {
                        (None, None, Some(references.to_vec()))
                    }
                });
        let starts_with_leader = parsed_content.as_ref().is_some_and(|content| {
            content
                .segments()
                .iter()
                .find(|segment| !segment.text.is_empty())
                .is_some_and(|segment| {
                    segment.speaker == crate::workflow::description_parser::SpeakerRole::Leader
                })
        });
        let style = plan.render_style();
        let first_cue_macro = style.and_then(|style| {
            style.title().map_or_else(
                || {
                    style
                        .content()
                        .cue_macro()
                        .map(|binding| binding.select(starts_with_leader).to_string())
                },
                |title| {
                    title
                        .cue_macro()
                        .map(|binding| binding.select(false).to_string())
                },
            )
        });
        let first_content_cue_macro = style.and_then(|style| {
            style.title()?;
            style
                .content()
                .cue_macro()
                .map(|binding| binding.select(starts_with_leader).to_string())
        });
        let file_path = plan.file_path().map(|path| path.display().to_string());
        let status = PreviewStatus::from(plan.disposition());
        let background = plan.background().map(|background| background.id().clone());
        let arrangement = plan.arrangement().map(str::to_string);
        let content_slide = style.map(|style| style.content().slide().to_string());
        let title_slide =
            style.and_then(|style| style.title().map(|title| title.slide().to_string()));
        let item_type = plan.item_type().map(str::to_string);
        let classification_rule = plan.classification_rule().map(str::to_string);

        Self {
            output_key: plan.output_key.to_string(),
            position: plan.position,
            pco_title: plan.pco_title,
            playlist_name: plan.playlist_name,
            file_path,
            status,
            reason: plan.reason,
            classification_rule,
            item_type,
            parsed_content,
            background,
            arrangement,
            scripture_reference,
            bible_version,
            scripture_refs,
            content_slide,
            title_slide,
            first_cue_macro,
            first_content_cue_macro,
        }
    }
}

/// Render typed plans back into preview rows for MCP output.
pub fn render_preview(plans: &[ResolvedItemPlan]) -> Vec<PreviewEntry> {
    plans.iter().cloned().map(PreviewEntry::from).collect()
}
