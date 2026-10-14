//! Checked background, cue-role, macro, and render-style policy.

mod error;
pub(super) use error::identifier_problem;
pub use error::{IdentifierProblem, RenderPlanError};

use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroUsize,
};

use serde::Serialize;

use super::identity::{MacroName, NameRejection, ThemeSlideName};
use crate::project_config::{BackgroundAssetPath, BackgroundId};
use propresenter::presentation_spec::{CueRoleId, PresentationSpecError};

/// A background identifier resolved to the exact project asset used by a plan.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ResolvedBackground {
    id: BackgroundId,
    file: BackgroundAssetPath,
}

impl ResolvedBackground {
    /// Resolve a configured identifier/path pair into reviewed plan state.
    pub const fn new(id: BackgroundId, file: BackgroundAssetPath) -> Self {
        Self { id, file }
    }

    /// Return the project background identifier shown in previews.
    pub const fn id(&self) -> &BackgroundId {
        &self.id
    }

    /// Return the validated project-relative image path used during execution.
    pub const fn file(&self) -> &BackgroundAssetPath {
        &self.file
    }
}

/// Macro triggered when the operator enters a cue region.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CueMacro {
    enter: MacroName,
    #[serde(skip_serializing_if = "Option::is_none")]
    leader_enter: Option<MacroName>,
}

impl CueMacro {
    /// Build an ordinary entry macro and its optional leader-first variant.
    pub fn new(enter: String, leader_enter: Option<String>) -> Result<Self, RenderPlanError> {
        let enter = MacroName::new(enter).map_err(|rejection| match rejection {
            NameRejection::Blank => RenderPlanError::BlankCueMacro,
            NameRejection::Malformed { name, problem } => {
                RenderPlanError::InvalidCueMacro { name, problem }
            }
        })?;
        let leader_enter = leader_enter
            .map(MacroName::new)
            .transpose()
            .map_err(|rejection| match rejection {
                NameRejection::Blank => RenderPlanError::BlankLeaderCueMacro,
                NameRejection::Malformed { name, problem } => {
                    RenderPlanError::InvalidLeaderCueMacro { name, problem }
                }
            })?;
        Ok(Self {
            enter,
            leader_enter,
        })
    }

    /// Return the ordinary region-entry macro.
    pub const fn enter(&self) -> &MacroName {
        &self.enter
    }

    /// Select the explicit leader-first variant when applicable.
    pub fn select(&self, starts_with_leader: bool) -> &MacroName {
        if starts_with_leader {
            self.leader_enter.as_ref().unwrap_or(&self.enter)
        } else {
            &self.enter
        }
    }

    /// Return the optional leader-first entry macro.
    pub const fn leader_enter(&self) -> Option<&MacroName> {
        self.leader_enter.as_ref()
    }
}

/// Editor colors used to preserve liturgical speaker distinctions in native
/// text runs. Macro selection is semantic and never inferred from these RGB
/// values.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct SpeakerPalette {
    leader: (u8, u8, u8),
    audience: (u8, u8, u8),
}

impl SpeakerPalette {
    /// Build one explicit leader/audience editor palette.
    pub const fn new(leader: (u8, u8, u8), audience: (u8, u8, u8)) -> Self {
        Self { leader, audience }
    }

    /// Leader/liturgist editor color.
    pub const fn leader(self) -> (u8, u8, u8) {
        self.leader
    }

    /// Congregational editor color.
    pub const fn audience(self) -> (u8, u8, u8) {
        self.audience
    }
}

/// Ordered macro regions for one structure-preserving native presentation.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RestyleMacroPolicy {
    regions: Box<[RestyleMacroRegion]>,
}

impl RestyleMacroPolicy {
    /// Build a nonempty sequence of exact operator-region transitions.
    pub fn new(regions: Vec<RestyleMacroRegion>) -> Result<Self, RenderPlanError> {
        if regions.is_empty() {
            return Err(RenderPlanError::EmptyRestyleMacroPolicy);
        }
        let mut operator_cues = BTreeSet::new();
        let mut arrangement_groups = BTreeSet::new();
        for region in &regions {
            match region.selector() {
                RestyleMacroSelector::OperatorCue { index } if !operator_cues.insert(*index) => {
                    return Err(RenderPlanError::DuplicateOperatorCueSelector { index: *index });
                }
                RestyleMacroSelector::ArrangementGroup { index, .. }
                    if !arrangement_groups.insert(*index) =>
                {
                    return Err(RenderPlanError::DuplicateArrangementGroupSelector {
                        index: *index,
                    });
                }
                RestyleMacroSelector::OperatorCue { .. }
                | RestyleMacroSelector::ArrangementGroup { .. } => {}
            }
        }
        Ok(Self {
            regions: regions.into_boxed_slice(),
        })
    }

    /// Return configured regions in deterministic application order.
    pub fn regions(&self) -> &[RestyleMacroRegion] {
        &self.regions
    }
}

/// One exact macro target inside the selected operator traversal.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RestyleMacroRegion {
    selector: RestyleMacroSelector,
    enter_macro: CueMacro,
}

impl RestyleMacroRegion {
    /// Bind one checked selector to one exact installed macro name.
    pub fn new(
        selector: RestyleMacroSelector,
        enter_macro: String,
    ) -> Result<Self, RenderPlanError> {
        Ok(Self {
            selector,
            enter_macro: CueMacro::new(enter_macro, None)?,
        })
    }

    /// Native region selector.
    pub const fn selector(&self) -> &RestyleMacroSelector {
        &self.selector
    }

    /// Exact macro name applied to the region entry.
    pub const fn enter_macro(&self) -> &MacroName {
        self.enter_macro.enter()
    }
}

/// Deterministic native evidence used to locate a macro region.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RestyleMacroSelector {
    /// Zero-based cue occurrence in selected operator order.
    OperatorCue { index: usize },
    /// Zero-based group occurrence in the selected arrangement.
    ArrangementGroup {
        index: usize,
        allowed_names: BTreeSet<String>,
    },
}

impl RestyleMacroSelector {
    /// Build an arrangement-group selector with at least one exact allowed name.
    pub fn arrangement_group(
        index: usize,
        allowed_names: BTreeSet<String>,
    ) -> Result<Self, RenderPlanError> {
        if allowed_names.is_empty() {
            return Err(RenderPlanError::EmptyArrangementGroupNames { index });
        }
        if let Some((name, problem)) = allowed_names
            .iter()
            .find_map(|name| identifier_problem(name).map(|problem| (name.clone(), problem)))
        {
            return Err(RenderPlanError::InvalidArrangementGroupName {
                index,
                name,
                problem,
            });
        }
        Ok(Self::ArrangementGroup {
            index,
            allowed_names,
        })
    }
}

/// One configured cue role resolved to the exact theme slide and macro binding
/// needed during rendering.
#[derive(Debug, Clone, Serialize)]
pub struct RenderRole {
    /// Stable cue-role identifier from project config.
    id: CueRoleId,
    /// Exact theme slide used for this cue region.
    slide: ThemeSlideName,
    /// Semantic text field to exact named native graphics element.
    text_slots: BTreeMap<String, String>,
    /// Macro triggered when the operator enters this cue region.
    #[serde(skip_serializing_if = "Option::is_none")]
    cue_macro: Option<CueMacro>,
    /// Optional semantic speaker palette for liturgical content roles.
    #[serde(skip_serializing_if = "Option::is_none")]
    speaker_palette: Option<SpeakerPalette>,
}

impl RenderRole {
    /// Resolve one complete render role from validated project configuration.
    pub fn new(
        id: String,
        slide: String,
        text_slots: BTreeMap<String, String>,
        cue_macro: Option<CueMacro>,
        speaker_palette: Option<SpeakerPalette>,
    ) -> Result<Self, RenderPlanError> {
        if id.trim().is_empty() {
            return Err(RenderPlanError::BlankRoleId);
        }
        if let Some(problem) = identifier_problem(&id) {
            return Err(RenderPlanError::InvalidRoleId {
                role_id: id,
                problem,
            });
        }
        let slide = match ThemeSlideName::new(slide) {
            Ok(slide) => slide,
            Err(NameRejection::Blank) => {
                return Err(RenderPlanError::BlankRoleSlide { role_id: id });
            }
            Err(NameRejection::Malformed { name, problem }) => {
                return Err(RenderPlanError::InvalidRoleSlide {
                    role_id: id,
                    slide: name,
                    problem,
                });
            }
        };
        if let Some((slot, _)) = text_slots.iter().find(|(slot, _)| slot.trim().is_empty()) {
            return Err(RenderPlanError::BlankTextSlotName {
                role_id: id,
                slot: slot.clone(),
            });
        }
        if let Some((slot, _)) = text_slots
            .iter()
            .find(|(_, element)| element.trim().is_empty())
        {
            return Err(RenderPlanError::BlankTextSlotElement {
                role_id: id,
                slot: slot.clone(),
            });
        }
        if let Some((slot, problem)) = text_slots
            .keys()
            .find_map(|slot| identifier_problem(slot).map(|problem| (slot, problem)))
        {
            return Err(RenderPlanError::InvalidTextSlotName {
                role_id: id,
                slot: slot.clone(),
                problem,
            });
        }
        if let Some((slot, element, problem)) = text_slots.iter().find_map(|(slot, element)| {
            identifier_problem(element).map(|problem| (slot, element, problem))
        }) {
            return Err(RenderPlanError::InvalidTextSlotElement {
                role_id: id,
                slot: slot.clone(),
                element: element.clone(),
                problem,
            });
        }

        let mut native_elements = BTreeMap::<&str, &str>::new();
        for (slot, element) in &text_slots {
            if let Some(first_slot) = native_elements.insert(element, slot) {
                return Err(RenderPlanError::DuplicateTextSlotElement {
                    role_id: id,
                    first_slot: first_slot.to_string(),
                    duplicate_slot: slot.clone(),
                    element: element.clone(),
                });
            }
        }
        if !text_slots.is_empty() && !text_slots.contains_key("body") {
            return Err(RenderPlanError::MissingBodyTextSlot { role_id: id });
        }
        let has_leader_macro = cue_macro
            .as_ref()
            .and_then(CueMacro::leader_enter)
            .is_some();
        if has_leader_macro != speaker_palette.is_some() {
            return Err(RenderPlanError::IncompleteResponsiveRole { role_id: id });
        }
        if speaker_palette.is_some_and(|palette| palette.leader == palette.audience) {
            return Err(RenderPlanError::IndistinguishableSpeakerColors { role_id: id });
        }
        let id = CueRoleId::new(id.clone()).map_err(|source| RenderPlanError::RoleIdRejected {
            role_id: id,
            source,
        })?;
        Ok(Self {
            id,
            slide,
            text_slots,
            cue_macro,
            speaker_palette,
        })
    }

    /// Return the stable cue-role identifier from project config.
    pub const fn id(&self) -> &CueRoleId {
        &self.id
    }

    /// Return the exact theme slide used for this cue region.
    pub const fn slide(&self) -> &ThemeSlideName {
        &self.slide
    }

    /// Return semantic-to-native text element bindings for this role.
    pub const fn text_slots(&self) -> &BTreeMap<String, String> {
        &self.text_slots
    }

    /// Return the macro triggered when entering this cue region.
    pub const fn cue_macro(&self) -> Option<&CueMacro> {
        self.cue_macro.as_ref()
    }

    /// Return the configured liturgical speaker palette, when this role is
    /// speaker-aware.
    pub const fn speaker_palette(&self) -> Option<SpeakerPalette> {
        self.speaker_palette
    }
}

/// Complete styling metadata for a rendered presentation.
///
/// Every rendered action requires one content role. A separate title role is
/// explicit, so slide and macro bindings cannot become misaligned parallel
/// options.
#[derive(Debug, Clone, Serialize)]
pub struct RenderStyle {
    #[serde(skip_serializing_if = "Option::is_none")]
    background: Option<ResolvedBackground>,
    /// Required role for generated body/content cues.
    content: RenderRole,
    /// Optional separate role for a leading title cue.
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<RenderRole>,
    /// Max logical lines per generated content slide.
    #[serde(skip_serializing_if = "Option::is_none")]
    max_lines_per_slide: Option<NonZeroUsize>,
}

impl RenderStyle {
    /// Build one complete render policy, rejecting a zero line bound.
    pub fn new(
        background: Option<ResolvedBackground>,
        content: RenderRole,
        title: Option<RenderRole>,
        max_lines_per_slide: Option<usize>,
    ) -> Result<Self, RenderPlanError> {
        if title
            .as_ref()
            .is_some_and(|title| title.id() == content.id())
        {
            return Err(RenderPlanError::DuplicateRoleId {
                role_id: content.id().as_str().to_string(),
            });
        }
        let max_lines_per_slide = max_lines_per_slide
            .map(|value| NonZeroUsize::new(value).ok_or(RenderPlanError::ZeroMaxLines))
            .transpose()?;
        Ok(Self {
            background,
            content,
            title,
            max_lines_per_slide,
        })
    }

    /// Return this checked style with only its resolved background replaced.
    ///
    /// Replacing a background cannot invalidate cue-role or line-bound
    /// invariants, so callers do not need to reconstruct and revalidate the
    /// complete style.
    #[must_use]
    pub fn with_background(mut self, background: ResolvedBackground) -> Self {
        self.background = Some(background);
        self
    }

    /// Return the resolved background asset, when configured.
    pub const fn background(&self) -> Option<&ResolvedBackground> {
        self.background.as_ref()
    }

    /// Return the required role used for body/content cues.
    pub const fn content(&self) -> &RenderRole {
        &self.content
    }

    /// Return the separate leading title role, when configured.
    pub const fn title(&self) -> Option<&RenderRole> {
        self.title.as_ref()
    }

    /// Return the non-zero logical-line bound, when configured.
    pub const fn max_lines_per_slide(&self) -> Option<usize> {
        match self.max_lines_per_slide {
            Some(value) => Some(value.get()),
            None => None,
        }
    }
}
