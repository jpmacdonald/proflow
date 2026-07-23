//! Translation from checked workflow content into native presentations.
//!
//! This module is the render phase of the service-build pipeline. It owns the
//! one translation from workflow roles and content into the renderer's
//! [`PresentationSpec`]. Filesystem review, target writes, and document-envelope
//! preservation remain at the execution boundary.

mod spec;
mod text_fit;

use thiserror::Error;

#[cfg(test)]
use self::spec::pack_description_for_slides_estimated;
use self::spec::CueCompiler;
#[cfg(test)]
use self::text_fit::DiagnosticRenderTextFit;
use self::text_fit::{retain_final_text_fit, CandidateTextFit, NativeRenderTextFit, RenderTextFit};
use super::description_parser::ParsedContent;
#[cfg(test)]
use super::description_parser::{ParsedSegment, SpeakerRole};
use super::execute::RenderAssetSnapshot;
use super::plan::{RenderRole, RenderStyle};
use super::{ExpectedMacroRegion, ExpectedMacroSelector};
use crate::bible::Verse;
use crate::propresenter::arrangement::OperatorTraversalError;
use crate::propresenter::macros::{replace_entry_macro, MacroApplyError, MacroCache};
use crate::propresenter::presentation_spec::{
    CueRoleId, GroupSpec, PresentationSpec, PresentationSpecError, TextField,
};
use crate::propresenter::render::{
    render_presentation, RenderAssets, RenderError, RenderedCueRoles, RenderedPresentation,
    ResolvedCueRole, TemplateSlotError,
};
use crate::propresenter::text_fit::{NativeTextFitOracle, NativeTextRequestError, TextFitError};
use crate::propresenter::theme::{ThemeCache, ThemeSlideError, DEFAULT_MAX_LINES_PER_SLIDE};

#[cfg(test)]
use crate::propresenter::text_flow::{TextLayout, TextLayoutError};

const SCRIPTURE_DIVIDER_ROLE: &str = "__proflow_scripture_divider";
pub(super) const LEADER_ROLE_SUFFIX: &str = "::leader_first";

/// Content accepted by the presentation render phase.
#[derive(Clone, Copy)]
pub(crate) enum PresentationSource<'a> {
    /// Styled description text with an optional leading title.
    Description(&'a ParsedContent),
    /// One title cue with no body cues.
    Title { text: &'a str },
    /// One scripture passage with native verse-range labels.
    Scripture {
        title: &'a str,
        label_prefix: &'a str,
        verses: &'a [Verse],
    },
    /// Several scripture passages separated by blank content-template cues.
    CombinedScripture {
        passages: &'a [CombinedScripturePassage],
    },
}

impl PresentationSource<'_> {
    const fn needs_divider_role(&self) -> bool {
        matches!(
            self,
            Self::CombinedScripture { passages } if passages.len() > 1
        )
    }
}

/// One fully resolved passage in a combined scripture presentation.
#[derive(Debug)]
pub(crate) struct CombinedScripturePassage {
    title: String,
    label_prefix: String,
    verses: Vec<Verse>,
}

impl CombinedScripturePassage {
    /// Build a non-empty passage whose slide labels have an explicit prefix.
    pub(crate) fn new(
        title: String,
        label_prefix: String,
        verses: Vec<Verse>,
    ) -> Result<Self, PresentationRenderError> {
        if title.trim().is_empty() {
            return Err(PresentationRenderError::EmptyScriptureTitle);
        }
        if label_prefix.trim().is_empty() {
            return Err(PresentationRenderError::EmptyScriptureLabelPrefix);
        }
        if verses.is_empty() {
            return Err(PresentationRenderError::EmptyScripturePassage);
        }
        Ok(Self {
            title,
            label_prefix,
            verses,
        })
    }
}

/// Failure in the checked workflow-to-native render phase.
#[derive(Debug, Error)]
pub enum PresentationRenderError {
    /// A configured theme slide could not be resolved safely.
    #[error(transparent)]
    Theme(#[from] ThemeSlideError),
    /// A semantic role or cue specification was invalid.
    #[error(transparent)]
    Spec(#[from] PresentationSpecError),
    /// A semantic specification could not be rendered from its native assets.
    #[error(transparent)]
    Render(#[from] RenderError),
    /// The rendered document's effective operator traversal was invalid.
    #[error(transparent)]
    Traversal(#[from] OperatorTraversalError),
    /// A native text field could not be bound to its semantic name.
    #[error(transparent)]
    Template(#[from] TemplateSlotError),
    /// Checked text layout could not be constructed.
    #[cfg(test)]
    #[error(transparent)]
    TextLayout(#[from] TextLayoutError),
    /// A resolved native text field cannot be measured faithfully.
    #[error(transparent)]
    NativeTextRequest(#[from] NativeTextRequestError),
    /// The persistent `TextKit` helper rejected or could not measure a request.
    #[error(transparent)]
    NativeTextFit(#[from] TextFitError),
    /// A configured cue macro was not available or could not be applied.
    #[error(transparent)]
    Macro(#[from] MacroApplyError),
    /// A role declared explicit fields without declaring the required body.
    #[error("render role '{role}' must map semantic field 'body'")]
    MissingBodyBinding {
        /// Configured semantic role identifier.
        role: String,
    },
    /// Liturgical speaker content was assigned to a role without configured
    /// editor colors.
    #[error("render role '{role}' requires configured speaker colors")]
    MissingSpeakerPalette {
        /// Configured semantic role identifier.
        role: String,
    },
    /// Two independently configured roles claim the same semantic identity.
    #[error("render style contains duplicate cue role id '{role}'")]
    DuplicateRole {
        /// Reused semantic role identifier.
        role: String,
    },
    /// Source content did not produce any cues.
    #[error("presentation source produced no cues")]
    EmptyPresentation,
    /// Combined scripture content was built without its required divider role.
    #[error("combined scripture render is missing its divider role")]
    MissingDividerRole,
    /// A scripture source had no verses to render.
    #[error("scripture passage contains no verses")]
    EmptyScripturePassage,
    /// A scripture source had no operator-visible title.
    #[error("scripture passage title cannot be blank")]
    EmptyScriptureTitle,
    /// A scripture source had no native slide-label prefix.
    #[error("scripture slide-label prefix cannot be blank")]
    EmptyScriptureLabelPrefix,
    /// No nonempty grammatical partition fits the resolved native text box.
    #[error("render role '{role}' cannot fit even one indivisible text fragment")]
    NoFittingTextPartition {
        /// Semantic role whose text box cannot contain the source.
        role: String,
    },
    /// Final native shaping violates the physical box or configured line bound.
    #[error(
        "rendered cue {cue_index} for role '{role}' does not fit: fits_bounds={fits_bounds}, lines={line_count}/{max_lines}, used={used_width:.2}x{used_height:.2}pt"
    )]
    FinalTextOverflow {
        /// Rendered cue index.
        cue_index: usize,
        /// Semantic role used by the cue.
        role: String,
        /// Native helper's physical-bounds result.
        fits_bounds: bool,
        /// Native `TextKit` visual line count.
        line_count: usize,
        /// Configured visual-line maximum.
        max_lines: usize,
        /// Native laid-out width in points.
        used_width: f64,
        /// Native laid-out height in points.
        used_height: f64,
    },
    /// A rendered cue expected from the checked specification is unavailable.
    #[error("rendered cue {cue_index} is unavailable during final text measurement")]
    RenderedCueUnavailable {
        /// Missing zero-based cue index.
        cue_index: usize,
    },
    /// A rendered text cue does not contain exactly one presentation slide.
    #[error("rendered cue {cue_index} does not contain exactly one presentation slide")]
    RenderedPresentationSlideUnavailable {
        /// Invalid zero-based cue index.
        cue_index: usize,
    },
    /// A configured cue macro has no compiled Audience Look destinations.
    #[error("configured macro '{macro_name}' has no resolved audience destinations")]
    MissingAudienceDestinations {
        /// Exact installed macro name.
        macro_name: String,
    },
    /// A macro-selected output theme violates its physical or line bound.
    #[error(
        "rendered cue {cue_index} for role '{role}' overflows audience screen '{screen_name}': fits_bounds={fits_bounds}, lines={line_count}/{max_lines}, used={used_width:.2}x{used_height:.2}pt"
    )]
    AudienceTextOverflow {
        /// Rendered cue index.
        cue_index: usize,
        /// Semantic role used by the cue.
        role: String,
        /// Operator-visible configured audience screen.
        screen_name: String,
        /// Native helper's physical-bounds result.
        fits_bounds: bool,
        /// Native `TextKit` visual line count.
        line_count: usize,
        /// Configured visual-line maximum.
        max_lines: usize,
        /// Native laid-out width in points.
        used_width: f64,
        /// Native laid-out height in points.
        used_height: f64,
    },
    /// One audience screen was measured more than once for a cue.
    #[error(
        "rendered cue {cue_index} produced duplicate evidence for audience screen {screen_uuid}"
    )]
    DuplicateAudienceScreenEvidence {
        /// Rendered cue index.
        cue_index: usize,
        /// Stable native audience-screen UUID.
        screen_uuid: String,
    },
}

/// Render one workflow source using a persistent native `TextKit` session.
pub(crate) fn render_source_with_native_fit(
    name: &str,
    source: PresentationSource<'_>,
    style: &RenderStyle,
    assets: &RenderAssetSnapshot,
    oracle: &mut NativeTextFitOracle,
) -> Result<RenderedPresentation, PresentationRenderError> {
    let mut text_fit = NativeRenderTextFit::new(oracle);
    render_source_with_fit(
        name,
        source,
        style,
        assets.themes(),
        Some(assets),
        &mut text_fit,
    )
}

/// Keep approximation available only to pure unit tests. Production callers
/// cannot accidentally render without native physical evidence.
#[cfg(test)]
pub(crate) fn render_source(
    name: &str,
    source: PresentationSource<'_>,
    style: &RenderStyle,
    themes: &ThemeCache,
) -> Result<RenderedPresentation, PresentationRenderError> {
    render_source_with_fit(
        name,
        source,
        style,
        themes,
        None,
        &mut DiagnosticRenderTextFit,
    )
}

fn render_source_with_fit(
    name: &str,
    source: PresentationSource<'_>,
    style: &RenderStyle,
    themes: &ThemeCache,
    audience_assets: Option<&RenderAssetSnapshot>,
    text_fit: &mut dyn RenderTextFit,
) -> Result<RenderedPresentation, PresentationRenderError> {
    let content_id = CueRoleId::new(style.content().id())?;
    let leader_content_id = style
        .content()
        .cue_macro()
        .and_then(|binding| binding.leader_enter())
        .map(|_| CueRoleId::new(format!("{}{LEADER_ROLE_SUFFIX}", style.content().id())))
        .transpose()?;
    let title_id = match style.title() {
        Some(title) => {
            let id = CueRoleId::new(title.id())?;
            if id == content_id {
                return Err(PresentationRenderError::DuplicateRole {
                    role: id.as_str().to_string(),
                });
            }
            id
        }
        None => content_id.clone(),
    };
    if leader_content_id.as_ref() == Some(&title_id) {
        return Err(PresentationRenderError::DuplicateRole {
            role: title_id.as_str().to_string(),
        });
    }
    let divider_id = source
        .needs_divider_role()
        .then(|| CueRoleId::new(SCRIPTURE_DIVIDER_ROLE))
        .transpose()?;
    if divider_id.as_ref().is_some_and(|id| {
        id == &content_id || id == &title_id || leader_content_id.as_ref() == Some(id)
    }) {
        return Err(PresentationRenderError::DuplicateRole {
            role: SCRIPTURE_DIVIDER_ROLE.to_string(),
        });
    }

    let content_role = resolve_role(style.content(), content_id.clone(), themes)?;
    let max_lines = style
        .max_lines_per_slide()
        .unwrap_or(DEFAULT_MAX_LINES_PER_SLIDE);
    let candidate_text_fit = CandidateTextFit::new(&content_role, audience_assets, max_lines);
    let cues = CueCompiler::new(
        &content_id,
        leader_content_id.as_ref(),
        &title_id,
        divider_id.as_ref(),
        style.content(),
        candidate_text_fit,
    )
    .compile(source, text_fit)?;
    let mut remaining_roles = Vec::new();
    if let Some(leader_content_id) = leader_content_id.as_ref() {
        remaining_roles.push(resolve_role(
            style.content(),
            leader_content_id.clone(),
            themes,
        )?);
    }
    if let Some(title) = style.title() {
        remaining_roles.push(resolve_role(title, title_id.clone(), themes)?);
    }
    if let Some(divider_id) = divider_id.as_ref() {
        remaining_roles.push(resolve_role(style.content(), divider_id.clone(), themes)?);
    }
    let assets = RenderAssets::new(content_role, remaining_roles)?;
    let mut cues = cues.into_iter();
    let first = cues
        .next()
        .ok_or(PresentationRenderError::EmptyPresentation)?;
    let spec = PresentationSpec::new(
        name,
        GroupSpec::anonymous(first, cues.collect()),
        Vec::new(),
    )?;
    let mut rendered = render_presentation(&spec, &assets)?;
    if let Some(audience_assets) = audience_assets {
        apply_role_macros(&mut rendered, style, audience_assets.macros())?;
    }
    retain_final_text_fit(
        &spec,
        &assets,
        &mut rendered,
        style,
        audience_assets,
        max_lines,
        text_fit,
    )?;
    Ok(rendered)
}

/// Apply configured macros to the actual semantic role transitions emitted by
/// the renderer.
pub(crate) fn apply_role_macros(
    rendered: &mut RenderedPresentation,
    style: &RenderStyle,
    macros: &MacroCache,
) -> Result<(), PresentationRenderError> {
    // Work on a detached document so a missing macro or stale cue index cannot
    // leave a partially transformed render behind.
    let mut presentation = rendered.presentation().clone();
    if let Some(title) = style.title() {
        apply_macro_for_role(
            &mut presentation,
            rendered.cue_roles(),
            title.id(),
            false,
            title,
            macros,
        )?;
    }
    apply_macro_for_role(
        &mut presentation,
        rendered.cue_roles(),
        style.content().id(),
        false,
        style.content(),
        macros,
    )?;
    if style
        .content()
        .cue_macro()
        .and_then(|binding| binding.leader_enter())
        .is_some()
    {
        let leader_id = format!("{}{LEADER_ROLE_SUFFIX}", style.content().id());
        apply_macro_for_role(
            &mut presentation,
            rendered.cue_roles(),
            &leader_id,
            true,
            style.content(),
            macros,
        )?;
    }
    rendered.replace_preserving_role_mapping(presentation)?;
    Ok(())
}

/// Lower semantic role transitions into the exact operator-cue macro contract
/// produced by this render. Text fitting determines cue boundaries, so this
/// contract can only become exact after rendering.
pub(crate) fn resolved_macro_regions(
    rendered: &RenderedPresentation,
    style: &RenderStyle,
) -> Result<Vec<ExpectedMacroRegion>, PresentationRenderError> {
    let traversal =
        crate::propresenter::arrangement::checked_operator_cue_indices(rendered.presentation())?;
    let content_id = style.content().id();
    let leader_id = format!("{content_id}{LEADER_ROLE_SUFFIX}");
    let title = style.title();
    let mut regions = Vec::new();

    for transition in rendered.cue_roles().transitions() {
        let role_id = transition.role().as_str();
        let binding = if role_id == leader_id {
            style
                .content()
                .cue_macro()
                .map(|binding| binding.select(true))
        } else if role_id == content_id {
            style
                .content()
                .cue_macro()
                .map(|binding| binding.select(false))
        } else if let Some(title) = title.filter(|title| title.id() == role_id) {
            title.cue_macro().map(|binding| binding.select(false))
        } else {
            None
        };
        let Some(macro_name) = binding else {
            continue;
        };
        let operator_index = traversal
            .iter()
            .position(|&cue_index| cue_index == transition.cue_index())
            .ok_or(PresentationRenderError::Render(
                RenderError::RoleOperatorTraversalChanged,
            ))?;
        regions.push(ExpectedMacroRegion {
            selector: ExpectedMacroSelector::OperatorCue {
                index: operator_index,
            },
            macro_name: macro_name.to_string(),
        });
    }
    Ok(regions)
}

fn apply_macro_for_role(
    presentation: &mut crate::propresenter::generated::rv_data::Presentation,
    cue_roles: &RenderedCueRoles,
    role_id: &str,
    starts_with_leader: bool,
    role: &RenderRole,
    macros: &MacroCache,
) -> Result<(), PresentationRenderError> {
    let Some(binding) = role.cue_macro() else {
        return Ok(());
    };
    let role_id = CueRoleId::new(role_id)?;
    for &index in cue_roles.entries(&role_id) {
        let cue_count = presentation.cues.len();
        let cue = presentation
            .cues
            .get_mut(index)
            .ok_or(MacroApplyError::CueUnavailable { index, cue_count })?;
        replace_entry_macro(cue, binding.select(starts_with_leader), macros)?;
    }
    Ok(())
}

fn resolve_role<'a>(
    role: &RenderRole,
    id: CueRoleId,
    themes: &'a ThemeCache,
) -> Result<ResolvedCueRole<'a>, PresentationRenderError> {
    if role.text_slots().is_empty() {
        let slide = themes.text_template(role.slide())?;
        return bind_role_to_slide(role, id, slide);
    }
    let template = themes.slide_template(role.slide())?;
    bind_role_to_slide(role, id, template.slide())
}

pub(super) fn bind_role_to_slide<'a>(
    role: &RenderRole,
    id: CueRoleId,
    slide: &'a crate::propresenter::generated::rv_data::PresentationSlide,
) -> Result<ResolvedCueRole<'a>, PresentationRenderError> {
    if role.text_slots().is_empty() {
        return ResolvedCueRole::body(id, slide).map_err(PresentationRenderError::from);
    }
    if !role.text_slots().contains_key("body") {
        return Err(PresentationRenderError::MissingBodyBinding {
            role: role.id().to_string(),
        });
    }

    let bindings = role
        .text_slots()
        .iter()
        .map(|(semantic, native)| Ok((TextField::new(semantic)?, native.as_str())))
        .collect::<Result<Vec<_>, PresentationRenderError>>()?;
    let Some((first, rest)) = bindings.split_first() else {
        return Err(PresentationRenderError::MissingBodyBinding {
            role: role.id().to_string(),
        });
    };
    ResolvedCueRole::with_slots(
        id,
        slide,
        (first.0.clone(), first.1),
        rest.iter().map(|(field, native)| (field.clone(), *native)),
    )
    .map_err(PresentationRenderError::from)
}

pub(super) fn bind_role_to_audience_slide<'a>(
    role: &RenderRole,
    id: CueRoleId,
    slide: &'a crate::propresenter::generated::rv_data::PresentationSlide,
) -> Result<ResolvedCueRole<'a>, PresentationRenderError> {
    if role.text_slots().len() <= 1 {
        return ResolvedCueRole::body(id, slide).map_err(PresentationRenderError::from);
    }
    bind_role_to_slide(role, id, slide)
}

#[cfg(test)]
mod tests;
