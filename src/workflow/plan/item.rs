//! Scripture sources and executable service-item decisions.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{RenderStyle, ResolvedBackground};
use crate::project_config::{BackgroundAssetPath, BackgroundId, PresentationTypeKey};
use crate::workflow::description_parser::ParsedContent;
use propresenter::SlideType;

use super::action::{ReadyAction, ReviewContext};
use super::output_key::OutputKey;
use super::scripture::ScriptureContent;
use crate::project_config::ItemKind;

/// Readiness state for one planned playlist output.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", content = "details", rename_all = "snake_case")]
pub enum PlanDisposition {
    /// The action is complete and may cross the reviewed execution boundary.
    Ready(ReadyAction),
    /// The output is intentionally excluded from the playlist.
    Skip,
    /// The output cannot execute until a human resolves the stated reason.
    NeedsReview(ReviewContext),
}

/// Incompatible output semantics rejected before a plan can be reviewed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PlanSemanticsError {
    /// An operator-selected native slide type contradicts the content carried
    /// by the executable action.
    #[error("{action} content requires the {required} slide type, not {requested}")]
    IncompatibleSlideType {
        /// Content operation whose native representation is fixed.
        action: &'static str,
        /// Native slide type required by that operation.
        required: &'static str,
        /// Conflicting operator-selected native slide type.
        requested: &'static str,
    },
}

/// Single owner of the semantic properties that determine native output.
///
/// Classification provenance (`item_kind` and `item_type`), an optional
/// operator-selected native slide type, and executable disposition move
/// together through checked transitions. Callers cannot update one while
/// leaving the others in a contradictory state.
#[derive(Debug, Clone, Serialize)]
struct PlannedOutput {
    item_kind: ItemKind,
    #[serde(rename = "item_type")]
    #[serde(skip_serializing_if = "Option::is_none")]
    configured_type: Option<PresentationTypeKey>,
    #[serde(skip_serializing_if = "Option::is_none")]
    slide_type_override: Option<SlideType>,
    disposition: PlanDisposition,
}

impl PlannedOutput {
    fn new(
        item_kind: ItemKind,
        configured_type: Option<PresentationTypeKey>,
        disposition: PlanDisposition,
    ) -> Self {
        let mut output = Self {
            item_kind,
            configured_type,
            slide_type_override: None,
            disposition,
        };
        output.normalize_action_defined_kind();
        output
    }

    const fn action(&self) -> Option<&ReadyAction> {
        match &self.disposition {
            PlanDisposition::Ready(action) => Some(action),
            PlanDisposition::NeedsReview(context) => context.proposed_action(),
            PlanDisposition::Skip => None,
        }
    }

    fn slide_type(&self) -> SlideType {
        if let Some(slide_type) = self.slide_type_override {
            return slide_type;
        }
        if let Some(required) = self.action().and_then(ReadyAction::required_slide_type) {
            return required;
        }
        default_slide_type(self.item_kind)
    }

    fn set_slide_type(&mut self, slide_type: SlideType) -> Result<(), PlanSemanticsError> {
        if let Some(action) = self.action() {
            if let Some(required) = action.required_slide_type() {
                if slide_type != required {
                    return Err(PlanSemanticsError::IncompatibleSlideType {
                        action: action.operation_name(),
                        required: required.name(),
                        requested: slide_type.name(),
                    });
                }
            }
        }
        self.item_kind = kind_for_slide_type(self.item_kind, slide_type);
        self.slide_type_override = Some(slide_type);
        Ok(())
    }

    fn replace_disposition(
        &mut self,
        disposition: PlanDisposition,
    ) -> Result<(), PlanSemanticsError> {
        if let (Some(selected), Some(action)) =
            (self.slide_type_override, disposition_action(&disposition))
        {
            if let Some(required) = action.required_slide_type() {
                if selected != required {
                    return Err(PlanSemanticsError::IncompatibleSlideType {
                        action: action.operation_name(),
                        required: required.name(),
                        requested: selected.name(),
                    });
                }
            }
        }
        self.disposition = disposition;
        self.normalize_action_defined_kind();
        Ok(())
    }

    fn normalize_action_defined_kind(&mut self) {
        match self.action().and_then(ReadyAction::required_slide_type) {
            Some(SlideType::Scripture) => self.item_kind = ItemKind::Scripture,
            Some(SlideType::Title) => self.item_kind = ItemKind::Nametag,
            Some(SlideType::Text | SlideType::Lyrics | SlideType::Graphic) | None => {}
        }
    }
}

const fn disposition_action(disposition: &PlanDisposition) -> Option<&ReadyAction> {
    match disposition {
        PlanDisposition::Ready(action) => Some(action),
        PlanDisposition::NeedsReview(context) => context.proposed_action(),
        PlanDisposition::Skip => None,
    }
}

const fn default_slide_type(item_kind: ItemKind) -> SlideType {
    match item_kind {
        ItemKind::Song => SlideType::Lyrics,
        ItemKind::Scripture => SlideType::Scripture,
        ItemKind::Nametag => SlideType::Title,
        ItemKind::Announcement | ItemKind::Graphic => SlideType::Graphic,
        ItemKind::Liturgy | ItemKind::Other => SlideType::Text,
    }
}

const fn kind_for_slide_type(current: ItemKind, slide_type: SlideType) -> ItemKind {
    match slide_type {
        SlideType::Lyrics => ItemKind::Song,
        SlideType::Scripture => ItemKind::Scripture,
        SlideType::Title => ItemKind::Nametag,
        SlideType::Graphic => match current {
            ItemKind::Announcement | ItemKind::Graphic => current,
            ItemKind::Song
            | ItemKind::Scripture
            | ItemKind::Nametag
            | ItemKind::Liturgy
            | ItemKind::Other => ItemKind::Graphic,
        },
        SlideType::Text => match current {
            ItemKind::Liturgy | ItemKind::Other => current,
            ItemKind::Song
            | ItemKind::Scripture
            | ItemKind::Nametag
            | ItemKind::Announcement
            | ItemKind::Graphic => ItemKind::Other,
        },
    }
}

/// Shared typed plan used by preview/build workflow stages.
#[derive(Debug, Clone, Serialize)]
pub struct ResolvedItemPlan {
    /// Stable workflow key for a single planned output.
    pub(crate) output_key: OutputKey,
    pub position: usize,
    pub pco_title: String,
    pub playlist_name: String,
    pub reason: String,
    /// Configured classification rule that produced this output, when any.
    #[serde(skip_serializing_if = "Option::is_none")]
    classification_rule: Option<String>,
    #[serde(flatten)]
    output: PlannedOutput,
}

impl ResolvedItemPlan {
    /// Construct one plan while normalizing content-defined output semantics.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        output_key: OutputKey,
        position: usize,
        pco_title: String,
        playlist_name: String,
        reason: String,
        item_kind: ItemKind,
        configured_type: Option<PresentationTypeKey>,
        disposition: PlanDisposition,
    ) -> Self {
        Self {
            output_key,
            position,
            pco_title,
            playlist_name,
            reason,
            classification_rule: None,
            output: PlannedOutput::new(item_kind, configured_type, disposition),
        }
    }

    /// Return the stable key for this planned output.
    pub fn output_key(&self) -> &str {
        self.output_key.as_str()
    }

    /// Stable key for the primary output generated from a Planning Center item.
    pub fn primary_output_key(pco_item_id: &str) -> String {
        OutputKey::primary(pco_item_id).to_string()
    }

    /// Stable key for an expanded output generated from a rule step.
    pub fn expanded_output_key(pco_item_id: &str, step_index: usize, use_type: &str) -> String {
        OutputKey::expanded(pco_item_id, step_index, use_type).to_string()
    }

    /// Stable key for a project-required presentation inserted when absent.
    pub fn required_output_key(required_item_id: &str) -> String {
        OutputKey::required(required_item_id).to_string()
    }

    /// Return the action that can execute without further review.
    pub const fn ready_action(&self) -> Option<&ReadyAction> {
        match &self.output.disposition {
            PlanDisposition::Ready(action) => Some(action),
            PlanDisposition::Skip | PlanDisposition::NeedsReview(_) => None,
        }
    }

    /// Return the action used to populate preview details.
    ///
    /// Review items retain a complete proposal after collision and size audits,
    /// so previews can keep showing the exact file/content/style being reviewed.
    pub const fn preview_action(&self) -> Option<&ReadyAction> {
        match &self.output.disposition {
            PlanDisposition::Ready(action) => Some(action),
            PlanDisposition::NeedsReview(context) => context.proposed_action(),
            PlanDisposition::Skip => None,
        }
    }

    /// Return whether this plan is intentionally excluded from the playlist.
    pub const fn is_skipped(&self) -> bool {
        matches!(self.output.disposition, PlanDisposition::Skip)
    }

    /// Return whether this plan still requires an explicit human decision.
    pub const fn needs_review(&self) -> bool {
        matches!(self.output.disposition, PlanDisposition::NeedsReview(_))
    }

    /// Library files a review found and could not choose between.
    pub fn review_candidates(&self) -> &[PathBuf] {
        match &self.output.disposition {
            PlanDisposition::NeedsReview(context) => context.candidates(),
            PlanDisposition::Ready(_) | PlanDisposition::Skip => &[],
        }
    }

    /// Whether a suggested song still needs an explicit source-file decision.
    pub(crate) const fn requires_song_selection(&self) -> bool {
        match &self.output.disposition {
            PlanDisposition::NeedsReview(context) => context.requires_song_selection(),
            PlanDisposition::Ready(_) | PlanDisposition::Skip => false,
        }
    }

    /// Whether an identified song still needs an explicit arrangement decision.
    pub(crate) const fn requires_song_arrangement(&self) -> bool {
        match &self.output.disposition {
            PlanDisposition::NeedsReview(context) => context.requires_song_arrangement(),
            PlanDisposition::Ready(_) | PlanDisposition::Skip => false,
        }
    }

    /// Domain classification used for reporting and policy diagnostics.
    pub const fn item_kind(&self) -> ItemKind {
        self.output.item_kind
    }

    /// Configured presentation type that selected this plan, when any.
    pub const fn item_type(&self) -> Option<&PresentationTypeKey> {
        self.output.configured_type.as_ref()
    }

    /// Configured classification rule that produced this output, when any.
    pub fn classification_rule(&self) -> Option<&str> {
        self.classification_rule.as_deref()
    }

    /// Retain the selected rule on every direct or expanded output it produces.
    pub(crate) fn set_classification_rule(&mut self, rule_id: &str) {
        self.classification_rule = Some(rule_id.to_string());
    }

    /// Current readiness state for preview and execution.
    pub const fn disposition(&self) -> &PlanDisposition {
        &self.output.disposition
    }

    /// Apply an operator-selected native slide type after checking it against
    /// action-defined content such as scripture and title generation.
    pub(crate) fn set_slide_type(
        &mut self,
        slide_type: SlideType,
    ) -> Result<(), PlanSemanticsError> {
        self.output.set_slide_type(slide_type)
    }

    /// Use the given image files for this plan's background in this build,
    /// whether the plan is ready or still a proposal under review.
    pub(crate) fn use_background_files(
        &mut self,
        files: &BTreeMap<BackgroundId, BackgroundAssetPath>,
    ) {
        let action = match &mut self.output.disposition {
            PlanDisposition::Ready(action) => Some(action),
            PlanDisposition::NeedsReview(context) => context.proposed_action_mut(),
            PlanDisposition::Skip => None,
        };
        if let Some(action) = action {
            action.use_background_file(files);
        }
    }

    /// Replace this plan with one complete executable action.
    pub(crate) fn set_ready_action(
        &mut self,
        action: ReadyAction,
    ) -> Result<(), PlanSemanticsError> {
        self.output
            .replace_disposition(PlanDisposition::Ready(action))
    }

    /// Mark this output as deliberately omitted.
    pub(crate) fn skip(&mut self) {
        self.output.disposition = PlanDisposition::Skip;
    }

    /// Promote a complete reviewed proposal into executable state.
    pub(crate) fn approve_proposed_action(&mut self) -> Result<bool, PlanSemanticsError> {
        if !self.needs_review()
            || self.requires_song_selection()
            || self.requires_song_arrangement()
        {
            return Ok(false);
        }
        let Some(action) = self.preview_action().cloned() else {
            return Ok(false);
        };
        self.output
            .replace_disposition(PlanDisposition::Ready(action))?;
        Ok(true)
    }

    /// Return the existing presentation shown by this plan, when any.
    pub fn file_path(&self) -> Option<&Path> {
        self.preview_action().and_then(ReadyAction::file_path)
    }

    /// Return rendering metadata shown by this plan, when any.
    pub const fn render_style(&self) -> Option<&RenderStyle> {
        match self.preview_action() {
            Some(action) => action.render_style(),
            None => None,
        }
    }

    /// Return the reviewed presentation background, including transforms that
    /// deliberately preserve slide content rather than carry a render style.
    pub const fn background(&self) -> Option<&ResolvedBackground> {
        match self.preview_action() {
            Some(action) => action.background(),
            None => None,
        }
    }

    /// Return the read-only arrangement shown by this plan, when any.
    pub fn arrangement(&self) -> Option<&str> {
        self.preview_action().and_then(ReadyAction::arrangement)
    }

    /// Return parsed description content when this plan is description-driven.
    pub const fn parsed_content(&self) -> Option<&ParsedContent> {
        match self.preview_action() {
            Some(action) => action.parsed_content(),
            None => None,
        }
    }

    /// Return scripture generation data when this plan is scripture-driven.
    pub const fn scripture_content(&self) -> Option<&ScriptureContent> {
        match self.preview_action() {
            Some(action) => action.scripture_content(),
            None => None,
        }
    }

    /// Move this plan behind review while retaining any complete action as the
    /// proposed resolution.
    pub fn require_review(&mut self, reason: String) {
        let previous = std::mem::replace(
            &mut self.output.disposition,
            PlanDisposition::NeedsReview(ReviewContext::new(None)),
        );
        let context = match previous {
            PlanDisposition::Ready(action) => ReviewContext::new(Some(action)),
            PlanDisposition::NeedsReview(context) => context,
            PlanDisposition::Skip => ReviewContext::new(None),
        };
        self.output.disposition = PlanDisposition::NeedsReview(context);
        self.reason = reason;
    }

    /// Resolve the `ProPresenter` slide type for the plan.
    pub fn slide_type(&self) -> SlideType {
        self.output.slide_type()
    }
}
