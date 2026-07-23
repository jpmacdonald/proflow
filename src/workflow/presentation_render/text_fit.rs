//! Native text measurement and macro-selected destination postconditions.
//!
//! This module owns the physical render proof. The parent module owns semantic
//! source-to-cue planning; this module receives the exact checked spec, rendered
//! document, and immutable asset snapshot and proves every final destination.

use std::collections::BTreeMap;

use super::{
    bind_role_to_audience_slide, PresentationRenderError, RenderRole, RenderStyle,
    LEADER_ROLE_SUFFIX,
};
use crate::propresenter::audience::PresentationDestination;
use crate::propresenter::presentation_spec::{
    CueContent, CueRoleId, GroupSpec, PresentationSpec, TextField,
};
use crate::propresenter::render::{RenderAssets, RenderedPresentation, ResolvedCueRole};
use crate::propresenter::rtf::StyledSegment;
#[cfg(test)]
use crate::propresenter::text_fit::NativeTextRequestError;
use crate::propresenter::text_fit::{
    AudienceTextRendering, CueTextFitSummary, NativeTextFitOracle, TextFitDestinationIdentity,
    TextFitEvidence, TextFitRequest,
};
use crate::workflow::execute::RenderAssetSnapshot;

#[cfg(test)]
use crate::propresenter::rtf::rtf_to_text;
#[cfg(test)]
use crate::propresenter::text_flow::TextLayout;
#[cfg(test)]
use crate::propresenter::theme::{extract_role_metrics, DEFAULT_MAX_LINES_PER_SLIDE};

#[cfg(test)]
const DEFAULT_WRAP_COLUMN: usize = 45;

pub(super) trait RenderTextFit {
    fn measure_segments(
        &mut self,
        role: &ResolvedCueRole<'_>,
        field: &TextField,
        segments: &[StyledSegment],
        max_lines: usize,
    ) -> Result<TextFitEvidence, PresentationRenderError>;

    fn measure_rendered(
        &mut self,
        role: &ResolvedCueRole<'_>,
        field: &TextField,
        slide: &crate::propresenter::generated::rv_data::PresentationSlide,
        max_lines: usize,
    ) -> Result<TextFitEvidence, PresentationRenderError>;
}

pub(super) struct NativeRenderTextFit<'a> {
    oracle: &'a mut NativeTextFitOracle,
}

impl<'a> NativeRenderTextFit<'a> {
    pub(super) const fn new(oracle: &'a mut NativeTextFitOracle) -> Self {
        Self { oracle }
    }
}

impl RenderTextFit for NativeRenderTextFit<'_> {
    fn measure_segments(
        &mut self,
        role: &ResolvedCueRole<'_>,
        field: &TextField,
        segments: &[StyledSegment],
        _max_lines: usize,
    ) -> Result<TextFitEvidence, PresentationRenderError> {
        let request = TextFitRequest::from_resolved_segments(role, field, segments)?;
        self.oracle
            .measure(&request)
            .map_err(PresentationRenderError::from)
    }

    fn measure_rendered(
        &mut self,
        role: &ResolvedCueRole<'_>,
        field: &TextField,
        slide: &crate::propresenter::generated::rv_data::PresentationSlide,
        _max_lines: usize,
    ) -> Result<TextFitEvidence, PresentationRenderError> {
        let request = TextFitRequest::from_rendered_field(role, field, slide)?;
        self.oracle
            .measure(&request)
            .map_err(PresentationRenderError::from)
    }
}

#[cfg(test)]
pub(super) struct DiagnosticRenderTextFit;

#[cfg(test)]
impl RenderTextFit for DiagnosticRenderTextFit {
    fn measure_segments(
        &mut self,
        role: &ResolvedCueRole<'_>,
        _field: &TextField,
        segments: &[StyledSegment],
        max_lines: usize,
    ) -> Result<TextFitEvidence, PresentationRenderError> {
        let layout = layout_for(role, Some(max_lines))?;
        let line_count = estimated_segment_lines(segments, layout);
        Ok(TextFitEvidence::diagnostic(
            line_count,
            line_count <= max_lines,
        ))
    }

    fn measure_rendered(
        &mut self,
        role: &ResolvedCueRole<'_>,
        field: &TextField,
        slide: &crate::propresenter::generated::rv_data::PresentationSlide,
        max_lines: usize,
    ) -> Result<TextFitEvidence, PresentationRenderError> {
        let index = role.field_index(field)?;
        let rtf = slide
            .base_slide
            .as_ref()
            .and_then(|base| base.elements.get(index))
            .and_then(|element| element.element.as_ref())
            .and_then(|element| element.text.as_ref())
            .ok_or(NativeTextRequestError::InvalidNativeSlot { index })?
            .rtf_data
            .as_slice();
        let visible = std::str::from_utf8(rtf)
            .ok()
            .and_then(rtf_to_text)
            .unwrap_or_default();
        let layout = layout_for(role, Some(max_lines))?;
        let line_count = visible
            .split('\n')
            .map(|paragraph| layout.estimated_lines(paragraph))
            .sum();
        Ok(TextFitEvidence::diagnostic(
            line_count,
            line_count <= max_lines,
        ))
    }
}

pub(super) const fn evidence_satisfies_policy(
    evidence: &TextFitEvidence,
    max_lines: usize,
) -> bool {
    evidence.fits_bounds() && evidence.line_count() <= max_lines
}

/// Physical destinations against which every candidate partition must fit.
#[derive(Clone, Copy)]
pub(super) struct CandidateTextFit<'a, 'theme> {
    source_role: &'a ResolvedCueRole<'theme>,
    audience_assets: Option<&'a RenderAssetSnapshot>,
    max_lines: usize,
}

impl<'a, 'theme> CandidateTextFit<'a, 'theme> {
    pub(super) const fn new(
        source_role: &'a ResolvedCueRole<'theme>,
        audience_assets: Option<&'a RenderAssetSnapshot>,
        max_lines: usize,
    ) -> Self {
        Self {
            source_role,
            audience_assets,
            max_lines,
        }
    }

    /// Test one candidate against its source theme and all macro destinations.
    pub(super) fn fits(
        self,
        cue_role: &CueRoleId,
        configured_role: &RenderRole,
        starts_with_leader: bool,
        segments: &[StyledSegment],
        text_fit: &mut dyn RenderTextFit,
    ) -> Result<bool, PresentationRenderError> {
        let field = TextField::body();
        let source_evidence =
            text_fit.measure_segments(self.source_role, &field, segments, self.max_lines)?;
        if !evidence_satisfies_policy(&source_evidence, self.max_lines) {
            return Ok(false);
        }
        let (Some(audience_assets), Some(binding)) =
            (self.audience_assets, configured_role.cue_macro())
        else {
            return Ok(true);
        };
        let measurements = measure_audience_destinations(
            AudienceTextCandidate {
                cue_role,
                configured_role,
                macro_name: binding.select(starts_with_leader),
                field: &field,
                segments,
                source_evidence: &source_evidence,
                max_lines: self.max_lines,
            },
            audience_assets,
            text_fit,
        )?;
        Ok(measurements
            .iter()
            .all(|measurement| evidence_satisfies_policy(&measurement.evidence, self.max_lines)))
    }
}

pub(super) fn retain_final_text_fit(
    spec: &PresentationSpec,
    assets: &RenderAssets<'_>,
    rendered: &mut RenderedPresentation,
    style: &RenderStyle,
    audience_assets: Option<&RenderAssetSnapshot>,
    max_lines: usize,
    text_fit: &mut dyn RenderTextFit,
) -> Result<(), PresentationRenderError> {
    let mut evidence_by_cue =
        BTreeMap::<usize, Vec<crate::propresenter::text_fit::TextFitDestinationSummary>>::new();
    let mut audience_evidence_by_cue =
        BTreeMap::<usize, BTreeMap<String, (TextFitDestinationIdentity, TextFitEvidence)>>::new();
    for (cue_index, cue_spec) in spec.groups().flat_map(GroupSpec::cues).enumerate() {
        let CueContent::Text(bindings) = cue_spec.content() else {
            continue;
        };
        let cue = rendered
            .presentation()
            .cues
            .get(cue_index)
            .ok_or(PresentationRenderError::RenderedCueUnavailable { cue_index })?;
        let slide = rendered_presentation_slide(cue)
            .ok_or(PresentationRenderError::RenderedPresentationSlideUnavailable { cue_index })?;
        let role = assets.role(cue_spec.role())?;
        let theme_slide_uuid = role
            .slide()
            .base_slide
            .as_ref()
            .and_then(|slide| slide.uuid.as_ref())
            .map(|uuid| uuid.string.clone());
        for (field, segments) in bindings.iter() {
            let evidence = text_fit.measure_rendered(role, field, slide, max_lines)?;
            validate_source_evidence(cue_index, cue_spec.role(), &evidence, max_lines)?;
            evidence_by_cue
                .entry(cue_index)
                .or_default()
                .push(evidence.summarize(TextFitDestinationIdentity::SourceTheme {
                    cue_role: cue_spec.role().as_str().to_string(),
                    field: field.as_str().to_string(),
                    theme_slide_uuid: theme_slide_uuid.clone(),
                }));

            let (Some(audience_assets), Some((configured_role, macro_name))) =
                (audience_assets, cue_audience_macro(cue_spec.role(), style))
            else {
                continue;
            };
            let measurements = measure_audience_destinations(
                AudienceTextCandidate {
                    cue_role: cue_spec.role(),
                    configured_role,
                    macro_name,
                    field,
                    segments,
                    source_evidence: &evidence,
                    max_lines,
                },
                audience_assets,
                text_fit,
            )?;
            for measurement in measurements {
                retain_audience_measurement(
                    &mut audience_evidence_by_cue,
                    cue_index,
                    cue_spec.role(),
                    field,
                    max_lines,
                    measurement,
                )?;
            }
        }
    }
    let summaries = evidence_by_cue
        .into_iter()
        .map(|(cue_index, mut destinations)| {
            if let Some(audience) = audience_evidence_by_cue.remove(&cue_index) {
                destinations.extend(
                    audience
                        .into_values()
                        .map(|(identity, evidence)| evidence.summarize(identity)),
                );
            }
            Ok(CueTextFitSummary::new(cue_index, destinations))
        })
        .collect::<Result<Vec<_>, PresentationRenderError>>()?;
    rendered.retain_text_fit_summary(summaries)?;
    Ok(())
}

fn validate_source_evidence(
    cue_index: usize,
    cue_role: &CueRoleId,
    evidence: &TextFitEvidence,
    max_lines: usize,
) -> Result<(), PresentationRenderError> {
    if evidence_satisfies_policy(evidence, max_lines) {
        return Ok(());
    }
    let used = evidence.used_rect();
    Err(PresentationRenderError::FinalTextOverflow {
        cue_index,
        role: cue_role.as_str().to_string(),
        fits_bounds: evidence.fits_bounds(),
        line_count: evidence.line_count(),
        max_lines,
        used_width: used.width(),
        used_height: used.height(),
    })
}

type AudienceEvidenceByCue =
    BTreeMap<usize, BTreeMap<String, (TextFitDestinationIdentity, TextFitEvidence)>>;

fn retain_audience_measurement(
    evidence_by_cue: &mut AudienceEvidenceByCue,
    cue_index: usize,
    cue_role: &CueRoleId,
    field: &TextField,
    max_lines: usize,
    measurement: AudienceMeasurement,
) -> Result<(), PresentationRenderError> {
    if !evidence_satisfies_policy(&measurement.evidence, max_lines) {
        let used = measurement.evidence.used_rect();
        return Err(PresentationRenderError::AudienceTextOverflow {
            cue_index,
            role: cue_role.as_str().to_string(),
            screen_name: measurement.screen_name,
            fits_bounds: measurement.evidence.fits_bounds(),
            line_count: measurement.evidence.line_count(),
            max_lines,
            used_width: used.width(),
            used_height: used.height(),
        });
    }
    let destination_key = format!("{}\0{}", measurement.screen_uuid, field.as_str());
    if evidence_by_cue
        .entry(cue_index)
        .or_default()
        .insert(
            destination_key,
            (measurement.identity, measurement.evidence),
        )
        .is_some()
    {
        return Err(PresentationRenderError::DuplicateAudienceScreenEvidence {
            cue_index,
            screen_uuid: measurement.screen_uuid,
        });
    }
    Ok(())
}

fn cue_audience_macro<'a>(
    cue_role: &CueRoleId,
    style: &'a RenderStyle,
) -> Option<(&'a RenderRole, &'a str)> {
    if let Some(title) = style.title() {
        if cue_role.as_str() == title.id() {
            return title
                .cue_macro()
                .map(|binding| (title, binding.select(false)));
        }
    }

    let content = style.content();
    let leader_role = format!("{}{LEADER_ROLE_SUFFIX}", content.id());
    let starts_with_leader = cue_role.as_str() == leader_role;
    content
        .cue_macro()
        .map(|binding| (content, binding.select(starts_with_leader)))
}

struct AudienceMeasurement {
    screen_uuid: String,
    screen_name: String,
    identity: TextFitDestinationIdentity,
    evidence: TextFitEvidence,
}

#[derive(Clone, Copy)]
struct AudienceTextCandidate<'a> {
    cue_role: &'a CueRoleId,
    configured_role: &'a RenderRole,
    macro_name: &'a str,
    field: &'a TextField,
    segments: &'a [StyledSegment],
    source_evidence: &'a TextFitEvidence,
    max_lines: usize,
}

fn measure_audience_destinations(
    candidate: AudienceTextCandidate<'_>,
    audience_assets: &RenderAssetSnapshot,
    text_fit: &mut dyn RenderTextFit,
) -> Result<Vec<AudienceMeasurement>, PresentationRenderError> {
    let AudienceTextCandidate {
        cue_role,
        configured_role,
        macro_name,
        field,
        segments,
        source_evidence,
        max_lines,
    } = candidate;
    let destinations = audience_assets
        .audience_destinations_for_macro(macro_name)
        .ok_or_else(|| PresentationRenderError::MissingAudienceDestinations {
            macro_name: macro_name.to_string(),
        })?;
    destinations
        .screens()
        .iter()
        .map(|screen| {
            let (rendering, evidence) = match screen.presentation() {
                PresentationDestination::SourcePresentation => (
                    AudienceTextRendering::SourcePresentation,
                    source_evidence.clone(),
                ),
                PresentationDestination::ThemeOverride(destination) => {
                    let destination_slide =
                        crate::propresenter::generated::rv_data::PresentationSlide {
                            base_slide: Some(destination.base_slide().clone()),
                            ..crate::propresenter::generated::rv_data::PresentationSlide::default()
                        };
                    let destination_role = bind_role_to_audience_slide(
                        configured_role,
                        cue_role.clone(),
                        &destination_slide,
                    )?;
                    (
                        AudienceTextRendering::ThemeOverride {
                            theme_document_sha256: digest_hex(destination.document_sha256()),
                            theme_slide_uuid: destination.slide_uuid().to_string(),
                        },
                        text_fit.measure_segments(&destination_role, field, segments, max_lines)?,
                    )
                }
            };
            let screen_uuid = screen.screen_uuid().to_string();
            let screen_name = screen.screen_name().to_string();
            Ok(AudienceMeasurement {
                screen_uuid: screen_uuid.clone(),
                screen_name: screen_name.clone(),
                identity: TextFitDestinationIdentity::AudienceScreen {
                    field: field.as_str().to_string(),
                    screen_uuid,
                    screen_name,
                    macro_name: macro_name.to_string(),
                    audience_look_uuid: destinations.uuid().to_string(),
                    audience_look_name: destinations.name().to_string(),
                    rendering,
                },
                evidence,
            })
        })
        .collect()
}

fn rendered_presentation_slide(
    cue: &crate::propresenter::generated::rv_data::Cue,
) -> Option<&crate::propresenter::generated::rv_data::PresentationSlide> {
    let mut slides = cue.actions.iter().filter_map(|action| {
        let crate::propresenter::generated::rv_data::action::ActionTypeData::Slide(slide) =
            action.action_type_data.as_ref()?
        else {
            return None;
        };
        let crate::propresenter::generated::rv_data::action::slide_type::Slide::Presentation(
            presentation,
        ) = slide.slide.as_ref()?
        else {
            return None;
        };
        Some(presentation)
    });
    let first = slides.next()?;
    slides.next().is_none().then_some(first)
}

fn digest_hex(digest: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

#[cfg(test)]
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

#[cfg(test)]
fn estimated_segment_lines(segments: &[StyledSegment], layout: TextLayout) -> usize {
    segments
        .iter()
        .map(|segment| layout.estimated_lines(&segment.text))
        .sum()
}
