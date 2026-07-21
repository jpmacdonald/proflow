//! Semantic cue planning for presentation sources.
//!
//! This module translates checked workflow content into cue specifications. It
//! deliberately stops before native rendering, macro mutation, and final output
//! proof, which remain owned by the parent render phase.

use super::text_fit::{CandidateTextFit, RenderTextFit};
use super::{PresentationRenderError, PresentationSource};
use crate::bible::Verse;
use crate::propresenter::presentation_spec::{
    CueLabel, CueRoleId, CueSpec, PresentationSpecError, TextBindings, TextField,
};
use crate::propresenter::rtf::StyledSegment;
use crate::propresenter::scripture_layout::split_verses_with_fit;
use crate::propresenter::text_flow::{pack_segments_with_fit, FitPartitionError};
use crate::workflow::description_parser::{
    DescriptionFlow, ParsedContent, ParsedSegment, SpeakerRole,
};
use crate::workflow::plan::RenderRole;

#[cfg(test)]
use crate::propresenter::text_flow::TextLayout;

/// Immutable semantic roles and physical destinations used while planning cues.
pub(super) struct CueCompiler<'a, 'theme> {
    content_id: &'a CueRoleId,
    leader_content_id: Option<&'a CueRoleId>,
    title_id: &'a CueRoleId,
    divider_id: Option<&'a CueRoleId>,
    content_role: &'a RenderRole,
    candidate_text_fit: CandidateTextFit<'a, 'theme>,
}

impl<'a, 'theme> CueCompiler<'a, 'theme> {
    pub(super) const fn new(
        content_id: &'a CueRoleId,
        leader_content_id: Option<&'a CueRoleId>,
        title_id: &'a CueRoleId,
        divider_id: Option<&'a CueRoleId>,
        content_role: &'a RenderRole,
        candidate_text_fit: CandidateTextFit<'a, 'theme>,
    ) -> Self {
        Self {
            content_id,
            leader_content_id,
            title_id,
            divider_id,
            content_role,
            candidate_text_fit,
        }
    }

    pub(super) fn compile(
        &self,
        source: PresentationSource<'_>,
        text_fit: &mut dyn RenderTextFit,
    ) -> Result<Vec<CueSpec>, PresentationRenderError> {
        match source {
            PresentationSource::Description(content) => self.compile_description(content, text_fit),
            PresentationSource::Title { text } => self.compile_title(text),
            PresentationSource::Scripture {
                title,
                label_prefix,
                verses,
            } => self.compile_scripture(title, label_prefix, verses, text_fit),
            PresentationSource::CombinedScripture { passages } => {
                self.compile_combined_scripture(passages, text_fit)
            }
        }
    }

    fn compile_description(
        &self,
        content: &ParsedContent,
        text_fit: &mut dyn RenderTextFit,
    ) -> Result<Vec<CueSpec>, PresentationRenderError> {
        let mut cues = Vec::new();
        if let Some(title) = content.title_text().filter(|title| !title.is_empty()) {
            cues.push(text_cue(self.title_id.clone(), title, None)?);
        }
        let slides = {
            let mut fits = |segments: &[ParsedSegment]| {
                let starts_with_leader = starts_with_leader(segments);
                self.candidate_text_fit.fits(
                    self.content_role_id(starts_with_leader),
                    self.content_role,
                    starts_with_leader,
                    &styled_segments(segments, self.content_role)?,
                    text_fit,
                )
            };
            pack_description_for_slides(content, &mut fits)
                .map_err(|error| map_partition_error(error, self.content_role.id()))?
        };
        for segments in slides {
            let role = self.content_role_id(starts_with_leader(&segments));
            cues.push(CueSpec::text(
                role.clone(),
                TextBindings::single(
                    TextField::body(),
                    styled_segments(&segments, self.content_role)?,
                ),
            ));
        }
        Ok(cues)
    }

    fn compile_title(&self, text: &str) -> Result<Vec<CueSpec>, PresentationRenderError> {
        if text.is_empty() {
            Ok(Vec::new())
        } else {
            Ok(vec![text_cue(self.title_id.clone(), text, None)?])
        }
    }

    fn compile_scripture(
        &self,
        title: &str,
        label_prefix: &str,
        verses: &[Verse],
        text_fit: &mut dyn RenderTextFit,
    ) -> Result<Vec<CueSpec>, PresentationRenderError> {
        validate_scripture(title, label_prefix, verses)?;
        let mut cues = vec![text_cue(self.title_id.clone(), title, None)?];
        cues.extend(self.scripture_cues(label_prefix, verses, text_fit)?);
        Ok(cues)
    }

    fn compile_combined_scripture(
        &self,
        passages: &[super::CombinedScripturePassage],
        text_fit: &mut dyn RenderTextFit,
    ) -> Result<Vec<CueSpec>, PresentationRenderError> {
        if passages.is_empty() {
            return Err(PresentationRenderError::EmptyScripturePassage);
        }
        let mut cues = Vec::new();
        for (index, passage) in passages.iter().enumerate() {
            cues.push(text_cue(self.title_id.clone(), &passage.title, None)?);
            cues.extend(self.scripture_cues(&passage.label_prefix, &passage.verses, text_fit)?);
            if index + 1 < passages.len() {
                let divider_id = self
                    .divider_id
                    .ok_or(PresentationRenderError::MissingDividerRole)?;
                cues.push(text_cue(divider_id.clone(), "", None)?);
            }
        }
        Ok(cues)
    }

    fn scripture_cues(
        &self,
        label_prefix: &str,
        verses: &[Verse],
        text_fit: &mut dyn RenderTextFit,
    ) -> Result<Vec<CueSpec>, PresentationRenderError> {
        let mut fits = |text: &str| {
            self.candidate_text_fit.fits(
                self.content_id,
                self.content_role,
                false,
                &[StyledSegment::unstyled(text)],
                text_fit,
            )
        };
        split_verses_with_fit(verses, &mut fits)
            .map_err(|error| map_partition_error(error, self.content_role.id()))?
            .into_iter()
            .map(|slide| {
                let label = format!("{label_prefix}{}", slide.label());
                text_cue(self.content_id.clone(), slide.text(), Some(&label))
                    .map_err(PresentationRenderError::from)
            })
            .collect()
    }

    fn content_role_id(&self, starts_with_leader: bool) -> &CueRoleId {
        if starts_with_leader {
            self.leader_content_id.unwrap_or(self.content_id)
        } else {
            self.content_id
        }
    }
}

fn validate_scripture(
    title: &str,
    label_prefix: &str,
    verses: &[Verse],
) -> Result<(), PresentationRenderError> {
    if title.trim().is_empty() {
        return Err(PresentationRenderError::EmptyScriptureTitle);
    }
    if label_prefix.trim().is_empty() {
        return Err(PresentationRenderError::EmptyScriptureLabelPrefix);
    }
    if verses.is_empty() {
        return Err(PresentationRenderError::EmptyScripturePassage);
    }
    Ok(())
}

fn starts_with_leader(segments: &[ParsedSegment]) -> bool {
    segments
        .iter()
        .find(|segment| !segment.text.is_empty())
        .is_some_and(|segment| segment.speaker == SpeakerRole::Leader)
}

fn pack_description_for_slides<E, F>(
    content: &ParsedContent,
    fits: &mut F,
) -> Result<Vec<Vec<ParsedSegment>>, FitPartitionError<E>>
where
    F: FnMut(&[ParsedSegment]) -> Result<bool, E>,
{
    let ordinary = pack_segments_with_fit(content.segments(), fits)?;
    if content.flow() != DescriptionFlow::QuestionAnswer || ordinary.len() <= 1 {
        return Ok(ordinary);
    }

    let mut slides = Vec::new();
    let mut cursor = 0;
    for pair in content.question_answer_pairs() {
        if cursor < pair.question_start() {
            slides.extend(pack_segments_with_fit(
                &content.segments()[cursor..pair.question_start()],
                fits,
            )?);
        }

        let pair_segments = &content.segments()[pair.question_start()..pair.end()];
        let together = pack_segments_with_fit(pair_segments, fits)?;
        if together.len() <= 1 {
            slides.extend(together);
        } else {
            slides.extend(pack_segments_with_fit(
                &content.segments()[pair.question_start()..pair.answer_start()],
                fits,
            )?);
            slides.extend(pack_segments_with_fit(
                &content.segments()[pair.answer_start()..pair.end()],
                fits,
            )?);
        }
        cursor = pair.end();
    }
    if cursor < content.segments().len() {
        slides.extend(pack_segments_with_fit(&content.segments()[cursor..], fits)?);
    }
    Ok(slides)
}

#[cfg(test)]
pub(super) fn pack_description_for_slides_estimated(
    content: &ParsedContent,
    layout: TextLayout,
) -> Vec<Vec<ParsedSegment>> {
    let mut fits = |segments: &[ParsedSegment]| {
        Ok::<_, std::convert::Infallible>(
            segments
                .iter()
                .map(|segment| layout.estimated_lines(&segment.text))
                .sum::<usize>()
                <= layout.max_lines(),
        )
    };
    match pack_description_for_slides(content, &mut fits) {
        Ok(slides) => slides,
        Err(FitPartitionError::NoFittingPartition) => Vec::new(),
        Err(FitPartitionError::Measurement(unreachable)) => match unreachable {},
    }
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

fn map_partition_error(
    error: FitPartitionError<PresentationRenderError>,
    role: &str,
) -> PresentationRenderError {
    match error {
        FitPartitionError::Measurement(error) => error,
        FitPartitionError::NoFittingPartition => PresentationRenderError::NoFittingTextPartition {
            role: role.to_string(),
        },
    }
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
