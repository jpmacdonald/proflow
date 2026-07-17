//! Translation from checked workflow content into native presentations.
//!
//! This module is the render phase of the service-build pipeline. It owns the
//! one translation from workflow roles and content into the renderer's
//! [`PresentationSpec`]. Filesystem review, target writes, and document-envelope
//! preservation remain at the execution boundary.

use thiserror::Error;

use super::description_parser::{DescriptionFlow, ParsedContent, ParsedSegment, SpeakerRole};
use super::plan::{RenderRole, RenderStyle};
use crate::bible::Verse;
use crate::propresenter::macros::{replace_entry_macro, MacroApplyError, MacroCache};
use crate::propresenter::presentation_spec::{
    CueLabel, CueRoleId, CueSpec, GroupSpec, PresentationSpec, PresentationSpecError, TextBindings,
    TextField,
};
use crate::propresenter::render::{
    render_presentation, RenderAssets, RenderError, RenderedPresentation, ResolvedCueRole,
    TemplateSlotError,
};
use crate::propresenter::rtf::StyledSegment;
use crate::propresenter::scripture_layout::split_verses_for_slides;
use crate::propresenter::text_flow::{pack_segments_for_slides, TextLayout, TextLayoutError};
use crate::propresenter::theme::{
    extract_role_metrics, ThemeCache, ThemeSlideError, DEFAULT_MAX_LINES_PER_SLIDE,
};

const DEFAULT_WRAP_COLUMN: usize = 45;
const SCRIPTURE_DIVIDER_ROLE: &str = "__proflow_scripture_divider";
const LEADER_ROLE_SUFFIX: &str = "::leader_first";

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
pub(crate) enum PresentationRenderError {
    /// A configured theme slide could not be resolved safely.
    #[error(transparent)]
    Theme(#[from] ThemeSlideError),
    /// A semantic role or cue specification was invalid.
    #[error(transparent)]
    Spec(#[from] PresentationSpecError),
    /// A semantic specification could not be rendered from its native assets.
    #[error(transparent)]
    Render(#[from] RenderError),
    /// A native text field could not be bound to its semantic name.
    #[error(transparent)]
    Template(#[from] TemplateSlotError),
    /// Checked text layout could not be constructed.
    #[error(transparent)]
    TextLayout(#[from] TextLayoutError),
    /// A configured cue macro was not available or could not be applied.
    #[error(transparent)]
    Macro(#[from] MacroApplyError),
    /// A role declared explicit fields without declaring the required body.
    #[error("render role '{role}' must map semantic field 'body'")]
    MissingBodyBinding { role: String },
    /// Liturgical speaker content was assigned to a role without configured
    /// editor colors.
    #[error("render role '{role}' requires configured speaker colors")]
    MissingSpeakerPalette { role: String },
    /// Two independently configured roles claim the same semantic identity.
    #[error("render style contains duplicate cue role id '{role}'")]
    DuplicateRole { role: String },
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
}

/// Render one workflow content source through the generic presentation model.
pub(crate) fn render_source(
    name: &str,
    source: PresentationSource<'_>,
    style: &RenderStyle,
    themes: &ThemeCache,
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
    let layout = layout_for(&content_role, style.max_lines_per_slide())?;
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
    let cues = cue_specs(
        source,
        &content_id,
        leader_content_id.as_ref(),
        &title_id,
        divider_id.as_ref(),
        style.content(),
        layout,
    )?;
    let mut cues = cues.into_iter();
    let first = cues
        .next()
        .ok_or(PresentationRenderError::EmptyPresentation)?;
    let spec = PresentationSpec::new(
        name,
        GroupSpec::anonymous(first, cues.collect()),
        Vec::new(),
    )?;
    render_presentation(&spec, &assets).map_err(PresentationRenderError::from)
}

/// Apply configured macros to the actual semantic role transitions emitted by
/// the renderer.
pub(crate) fn apply_role_macros(
    rendered: &mut RenderedPresentation,
    style: &RenderStyle,
    macros: &MacroCache,
) -> Result<(), PresentationRenderError> {
    if let Some(title) = style.title() {
        apply_macro_for_role(rendered, title.id(), false, title, macros)?;
    }
    apply_macro_for_role(
        rendered,
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
        apply_macro_for_role(rendered, &leader_id, true, style.content(), macros)?;
    }
    Ok(())
}

fn apply_macro_for_role(
    rendered: &mut RenderedPresentation,
    role_id: &str,
    starts_with_leader: bool,
    role: &RenderRole,
    macros: &MacroCache,
) -> Result<(), PresentationRenderError> {
    let Some(binding) = role.cue_macro() else {
        return Ok(());
    };
    let role_id = CueRoleId::new(role_id)?;
    for &index in rendered.cue_roles.entries(&role_id) {
        let cue_count = rendered.presentation.cues.len();
        let cue = rendered
            .presentation
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
        return ResolvedCueRole::body(id, slide).map_err(PresentationRenderError::from);
    }
    if !role.text_slots().contains_key("body") {
        return Err(PresentationRenderError::MissingBodyBinding {
            role: role.id().to_string(),
        });
    }

    let template = themes.slide_template(role.slide())?;
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
        template.slide(),
        (first.0.clone(), first.1),
        rest.iter().map(|(field, native)| (field.clone(), *native)),
    )
    .map_err(PresentationRenderError::from)
}

fn layout_for(
    content_role: &ResolvedCueRole<'_>,
    max_lines_override: Option<usize>,
) -> Result<TextLayout, PresentationRenderError> {
    let (wrap_column, max_lines) = extract_role_metrics(content_role, &TextField::body())?
        .map_or((DEFAULT_WRAP_COLUMN, DEFAULT_MAX_LINES_PER_SLIDE), |m| {
            (m.chars_per_line, m.max_lines)
        });
    Ok(TextLayout::new(
        wrap_column,
        max_lines_override.map_or(max_lines, |configured| configured.min(max_lines)),
    )?)
}

fn cue_specs(
    source: PresentationSource<'_>,
    content_id: &CueRoleId,
    leader_content_id: Option<&CueRoleId>,
    title_id: &CueRoleId,
    divider_id: Option<&CueRoleId>,
    content_role: &RenderRole,
    layout: TextLayout,
) -> Result<Vec<CueSpec>, PresentationRenderError> {
    match source {
        PresentationSource::Description(content) => {
            let mut cues = Vec::new();
            if let Some(title) = content.title_text().filter(|title| !title.is_empty()) {
                cues.push(text_cue(title_id.clone(), title, None)?);
            }
            for segments in pack_description_for_slides(content, layout) {
                let starts_with_leader = segments
                    .iter()
                    .find(|segment| !segment.text.is_empty())
                    .is_some_and(|segment| segment.speaker == SpeakerRole::Leader);
                let role = if starts_with_leader {
                    leader_content_id.unwrap_or(content_id)
                } else {
                    content_id
                };
                cues.push(CueSpec::text(
                    role.clone(),
                    TextBindings::single(
                        TextField::body(),
                        styled_segments(&segments, content_role)?,
                    ),
                ));
            }
            Ok(cues)
        }
        PresentationSource::Title { text } => {
            if text.is_empty() {
                Ok(Vec::new())
            } else {
                Ok(vec![text_cue(title_id.clone(), text, None)?])
            }
        }
        PresentationSource::Scripture {
            title,
            label_prefix,
            verses,
        } => {
            if title.trim().is_empty() {
                return Err(PresentationRenderError::EmptyScriptureTitle);
            }
            if label_prefix.trim().is_empty() {
                return Err(PresentationRenderError::EmptyScriptureLabelPrefix);
            }
            if verses.is_empty() {
                return Err(PresentationRenderError::EmptyScripturePassage);
            }
            let mut cues = vec![text_cue(title_id.clone(), title, None)?];
            cues.extend(scripture_cues(content_id, label_prefix, verses, layout)?);
            Ok(cues)
        }
        PresentationSource::CombinedScripture { passages } => {
            if passages.is_empty() {
                return Err(PresentationRenderError::EmptyScripturePassage);
            }
            let mut cues = Vec::new();
            for (index, passage) in passages.iter().enumerate() {
                cues.push(text_cue(title_id.clone(), &passage.title, None)?);
                cues.extend(scripture_cues(
                    content_id,
                    &passage.label_prefix,
                    &passage.verses,
                    layout,
                )?);
                if index + 1 < passages.len() {
                    let divider_id =
                        divider_id.ok_or(PresentationRenderError::MissingDividerRole)?;
                    cues.push(text_cue(divider_id.clone(), "", None)?);
                }
            }
            Ok(cues)
        }
    }
}

fn pack_description_for_slides(
    content: &ParsedContent,
    layout: TextLayout,
) -> Vec<Vec<ParsedSegment>> {
    let ordinary = pack_segments_for_slides(content.segments(), layout);
    if content.flow() != DescriptionFlow::QuestionAnswer || ordinary.len() <= 1 {
        return ordinary;
    }

    let mut slides = Vec::new();
    let mut cursor = 0;
    for pair in content.question_answer_pairs() {
        if cursor < pair.question_start() {
            slides.extend(pack_segments_for_slides(
                &content.segments()[cursor..pair.question_start()],
                layout,
            ));
        }

        let pair_segments = &content.segments()[pair.question_start()..pair.end()];
        let together = pack_segments_for_slides(pair_segments, layout);
        if together.len() <= 1 {
            slides.extend(together);
        } else {
            slides.extend(pack_segments_for_slides(
                &content.segments()[pair.question_start()..pair.answer_start()],
                layout,
            ));
            slides.extend(pack_segments_for_slides(
                &content.segments()[pair.answer_start()..pair.end()],
                layout,
            ));
        }
        cursor = pair.end();
    }
    if cursor < content.segments().len() {
        slides.extend(pack_segments_for_slides(
            &content.segments()[cursor..],
            layout,
        ));
    }
    slides
}

fn styled_segments(
    segments: &[ParsedSegment],
    role: &RenderRole,
) -> Result<Vec<StyledSegment>, PresentationRenderError> {
    segments
        .iter()
        .map(|segment| {
            let color = match segment.speaker {
                SpeakerRole::Neutral => None,
                SpeakerRole::Leader => Some(
                    role.speaker_palette()
                        .ok_or_else(|| PresentationRenderError::MissingSpeakerPalette {
                            role: role.id().to_string(),
                        })?
                        .leader(),
                ),
                SpeakerRole::Audience => Some(
                    role.speaker_palette()
                        .ok_or_else(|| PresentationRenderError::MissingSpeakerPalette {
                            role: role.id().to_string(),
                        })?
                        .audience(),
                ),
            };
            Ok(StyledSegment {
                text: segment.text.clone(),
                color,
                bold: segment.bold,
                italic: segment.italic,
            })
        })
        .collect()
}

fn scripture_cues(
    role: &CueRoleId,
    label_prefix: &str,
    verses: &[Verse],
    layout: TextLayout,
) -> Result<Vec<CueSpec>, PresentationRenderError> {
    split_verses_for_slides(verses, layout)
        .into_iter()
        .map(|slide| {
            let label = format!("{label_prefix}{}", slide.label());
            text_cue(role.clone(), slide.text(), Some(&label))
                .map_err(PresentationRenderError::from)
        })
        .collect()
}

fn text_cue(
    role: CueRoleId,
    text: &str,
    label: Option<&str>,
) -> Result<CueSpec, PresentationSpecError> {
    let cue = CueSpec::text(
        role,
        TextBindings::single(TextField::body(), vec![StyledSegment::unstyled(text)]),
    );
    match label {
        Some(label) => Ok(cue.with_label(CueLabel::new(label)?)),
        None => Ok(cue),
    }
}

#[cfg(test)]
mod tests;
