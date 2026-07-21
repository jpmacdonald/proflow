//! Strict native text-fit proof for presentations restyled in place.
//!
//! Existing presentations do not carry `ProFlow` semantic cue roles. This
//! boundary therefore accepts only the shape we can map without guessing: one
//! nonempty source text element and one uniquely addressable audience-theme
//! text slot. Any richer shape returns a typed review error.

use std::collections::BTreeSet;

use crate::propresenter::audience::{
    AudienceLookDestinations, AudienceScreenDestination, PresentationDestination,
};
use crate::propresenter::generated::rv_data;
use crate::propresenter::macros::macro_action_name;
use crate::propresenter::presentation_spec::{CueRoleId, PresentationSpecError, TextField};
use crate::propresenter::render::{ResolvedCueRole, TemplateSlotError};
use crate::propresenter::rtf::{rtf_to_text, StyledSegment};
use crate::propresenter::text_fit::{
    AudienceTextRendering, CueTextFitSummary, NativeTextFitOracle, NativeTextRequestError,
    TextFitDestinationIdentity, TextFitDestinationSummary, TextFitError, TextFitEvidence,
    TextFitRequest,
};

use super::RenderAssetSnapshot;

pub(super) fn prove_restyled_text_fit(
    presentation: &rv_data::Presentation,
    assets: &RenderAssetSnapshot,
    oracle: &mut NativeTextFitOracle,
) -> Result<Vec<CueTextFitSummary>, RestyleTextFitError> {
    let destination_role_id = CueRoleId::new("restyled_existing")?;
    let operator_cues =
        crate::propresenter::arrangement::checked_operator_cue_indices(presentation)?;
    let mut active_macro = None::<String>;
    let mut summaries = Vec::new();

    for cue_index in operator_cues {
        let cue = presentation
            .cues
            .get(cue_index)
            .ok_or(RestyleTextFitError::CueUnavailable { cue_index })?;
        if let Some(macro_name) = cue_macro(cue, cue_index)? {
            active_macro = Some(macro_name.to_string());
        }
        if let Some(summary) = prove_restyled_cue(
            cue_index,
            cue,
            active_macro.as_deref(),
            &destination_role_id,
            assets,
            oracle,
        )? {
            summaries.push(summary);
        }
    }
    Ok(summaries)
}

fn cue_macro(cue: &rv_data::Cue, cue_index: usize) -> Result<Option<&str>, RestyleTextFitError> {
    let cue_macros = cue
        .actions
        .iter()
        .filter_map(macro_action_name)
        .collect::<Vec<_>>();
    match cue_macros.as_slice() {
        [] => Ok(None),
        [macro_name] => Ok(Some(*macro_name)),
        _ => Err(RestyleTextFitError::AmbiguousCueMacro {
            cue_index,
            count: cue_macros.len(),
        }),
    }
}

fn prove_restyled_cue(
    cue_index: usize,
    cue: &rv_data::Cue,
    active_macro: Option<&str>,
    destination_role_id: &CueRoleId,
    assets: &RenderAssetSnapshot,
    oracle: &mut NativeTextFitOracle,
) -> Result<Option<CueTextFitSummary>, RestyleTextFitError> {
    let Some(slide) = presentation_slide(cue, cue_index)? else {
        return Ok(None);
    };
    let Some(source) = one_visible_text(slide, cue_index)? else {
        return Ok(None);
    };
    let macro_name = active_macro.ok_or(RestyleTextFitError::MissingActiveMacro { cue_index })?;
    let (source_evidence, source_summary) = measure_source(cue, cue_index, &source, oracle)?;
    let destinations = assets
        .audience_destinations_for_macro(macro_name)
        .ok_or_else(|| RestyleTextFitError::MissingAudienceDestinations {
            cue_index,
            macro_name: macro_name.to_string(),
        })?;
    let proof = AudienceProof {
        cue_index,
        macro_name,
        destinations,
        destination_role_id,
        source: &source,
        source_evidence: &source_evidence,
    };
    let mut measured_screens = BTreeSet::new();
    let mut evidence = vec![source_summary];
    for screen in destinations.screens() {
        let screen_uuid = screen.screen_uuid().to_string();
        if !measured_screens.insert(screen_uuid.clone()) {
            return Err(RestyleTextFitError::DuplicateAudienceScreen {
                cue_index,
                screen_uuid,
            });
        }
        evidence.push(measure_audience_screen(&proof, screen, oracle)?);
    }
    Ok(Some(CueTextFitSummary::new(cue_index, evidence)))
}

fn measure_source(
    cue: &rv_data::Cue,
    cue_index: usize,
    source: &VisibleText<'_>,
    oracle: &mut NativeTextFitOracle,
) -> Result<(TextFitEvidence, TextFitDestinationSummary), RestyleTextFitError> {
    let cue_uuid = nonempty_uuid(cue.uuid.as_ref())
        .ok_or(RestyleTextFitError::MissingCueIdentity { cue_index })?;
    let text_element_uuid = nonempty_uuid(source.graphics.uuid.as_ref())
        .ok_or(RestyleTextFitError::MissingTextElementIdentity { cue_index })?;
    let request =
        TextFitRequest::from_native_slide_element(source.slide, source.graphics, source.text)?;
    let source_evidence = oracle.measure(&request)?;
    if !source_evidence.fits_bounds() {
        return Err(overflow_error(
            cue_index,
            "source presentation",
            &source_evidence,
        ));
    }
    let summary = source_evidence.summarize(TextFitDestinationIdentity::ExistingPresentation {
        cue_uuid,
        text_element_uuid,
    });
    Ok((source_evidence, summary))
}

fn nonempty_uuid(uuid: Option<&rv_data::Uuid>) -> Option<String> {
    uuid.map(|uuid| uuid.string.clone())
        .filter(|uuid| !uuid.is_empty())
}

struct AudienceProof<'a, 'source> {
    cue_index: usize,
    macro_name: &'a str,
    destinations: &'a AudienceLookDestinations,
    destination_role_id: &'a CueRoleId,
    source: &'a VisibleText<'source>,
    source_evidence: &'a TextFitEvidence,
}

fn measure_audience_screen(
    proof: &AudienceProof<'_, '_>,
    screen: &AudienceScreenDestination,
    oracle: &mut NativeTextFitOracle,
) -> Result<TextFitDestinationSummary, RestyleTextFitError> {
    let (rendering, evidence) = match screen.presentation() {
        PresentationDestination::SourcePresentation => (
            AudienceTextRendering::SourcePresentation,
            proof.source_evidence.clone(),
        ),
        PresentationDestination::ThemeOverride(destination) => {
            if proof.source_evidence.metric_style_run_count() > 1 {
                return Err(RestyleTextFitError::UnsupportedAudienceMetricStyles {
                    cue_index: proof.cue_index,
                    screen_name: screen.screen_name().to_string(),
                    metric_style_run_count: proof.source_evidence.metric_style_run_count(),
                });
            }
            let destination_slide = rv_data::PresentationSlide {
                base_slide: Some(destination.base_slide().clone()),
                ..rv_data::PresentationSlide::default()
            };
            let role = ResolvedCueRole::body(proof.destination_role_id.clone(), &destination_slide)
                .map_err(|source| RestyleTextFitError::AmbiguousAudienceTextMapping {
                    cue_index: proof.cue_index,
                    screen_name: screen.screen_name().to_string(),
                    source,
                })?;
            let request = TextFitRequest::from_resolved_segments(
                &role,
                &TextField::body(),
                &[StyledSegment::unstyled(proof.source.content.clone())],
            )?;
            (
                AudienceTextRendering::ThemeOverride {
                    theme_document_sha256: digest_hex(destination.document_sha256()),
                    theme_slide_uuid: destination.slide_uuid().to_string(),
                },
                oracle.measure(&request)?,
            )
        }
    };
    if !evidence.fits_bounds() {
        return Err(overflow_error(
            proof.cue_index,
            screen.screen_name(),
            &evidence,
        ));
    }
    Ok(
        evidence.summarize(TextFitDestinationIdentity::AudienceScreen {
            screen_uuid: screen.screen_uuid().to_string(),
            screen_name: screen.screen_name().to_string(),
            macro_name: proof.macro_name.to_string(),
            audience_look_uuid: proof.destinations.uuid().to_string(),
            audience_look_name: proof.destinations.name().to_string(),
            rendering,
        }),
    )
}

#[cfg(test)]
pub(crate) fn prove_restyled_text_fit_for_test(
    presentation: &rv_data::Presentation,
    assets: &RenderAssetSnapshot,
    oracle: &mut NativeTextFitOracle,
) -> Result<Vec<CueTextFitSummary>, String> {
    prove_restyled_text_fit(presentation, assets, oracle).map_err(|error| error.to_string())
}

struct VisibleText<'a> {
    slide: &'a rv_data::PresentationSlide,
    graphics: &'a rv_data::graphics::Element,
    text: &'a rv_data::graphics::Text,
    content: String,
}

fn presentation_slide(
    cue: &rv_data::Cue,
    cue_index: usize,
) -> Result<Option<&rv_data::PresentationSlide>, RestyleTextFitError> {
    let slides = cue
        .actions
        .iter()
        .filter_map(|action| {
            let rv_data::action::ActionTypeData::Slide(slide) = action.action_type_data.as_ref()?
            else {
                return None;
            };
            let rv_data::action::slide_type::Slide::Presentation(slide) = slide.slide.as_ref()?
            else {
                return None;
            };
            Some(slide)
        })
        .collect::<Vec<_>>();
    match slides.as_slice() {
        [] => Ok(None),
        [slide] => Ok(Some(*slide)),
        _ => Err(RestyleTextFitError::AmbiguousPresentationSlides {
            cue_index,
            count: slides.len(),
        }),
    }
}

fn one_visible_text(
    slide: &rv_data::PresentationSlide,
    cue_index: usize,
) -> Result<Option<VisibleText<'_>>, RestyleTextFitError> {
    let base = slide
        .base_slide
        .as_ref()
        .ok_or(RestyleTextFitError::MissingBaseSlide { cue_index })?;
    let mut visible = Vec::new();
    for graphics in base
        .elements
        .iter()
        .filter_map(|element| element.element.as_ref())
    {
        let Some(text) = graphics.text.as_ref() else {
            continue;
        };
        let rtf = std::str::from_utf8(&text.rtf_data)
            .map_err(|source| RestyleTextFitError::InvalidTextEncoding { cue_index, source })?;
        match rtf_to_text(rtf) {
            Some(content) => visible.push(VisibleText {
                slide,
                graphics,
                text,
                content,
            }),
            None if rtf.trim_start().starts_with("{\\rtf") => {}
            None => return Err(RestyleTextFitError::InvalidTextRtf { cue_index }),
        }
    }
    match visible.len() {
        0 => Ok(None),
        1 => Ok(visible.pop()),
        count => Err(RestyleTextFitError::AmbiguousSourceText { cue_index, count }),
    }
}

fn overflow_error(
    cue_index: usize,
    destination: &str,
    evidence: &TextFitEvidence,
) -> RestyleTextFitError {
    let used = evidence.used_rect();
    RestyleTextFitError::Overflow {
        cue_index,
        destination: destination.to_string(),
        used_width: used.width(),
        used_height: used.height(),
        line_count: evidence.line_count(),
    }
}

fn digest_hex(digest: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

#[derive(Debug, thiserror::Error)]
pub(super) enum RestyleTextFitError {
    #[error(transparent)]
    OperatorTraversal(#[from] crate::propresenter::arrangement::OperatorTraversalError),
    #[error(transparent)]
    Spec(#[from] PresentationSpecError),
    #[error(transparent)]
    NativeRequest(#[from] NativeTextRequestError),
    #[error(transparent)]
    NativeFit(#[from] TextFitError),
    #[error("restyled cue {cue_index} is unavailable")]
    CueUnavailable { cue_index: usize },
    #[error(
        "restyled cue {cue_index} contains {count} macro actions; one active macro is required"
    )]
    AmbiguousCueMacro { cue_index: usize, count: usize },
    #[error("restyled cue {cue_index} contains {count} presentation slides")]
    AmbiguousPresentationSlides { cue_index: usize, count: usize },
    #[error("restyled cue {cue_index} has no base slide")]
    MissingBaseSlide { cue_index: usize },
    #[error("restyled cue {cue_index} text RTF is not UTF-8: {source}")]
    InvalidTextEncoding {
        cue_index: usize,
        source: std::str::Utf8Error,
    },
    #[error("restyled cue {cue_index} contains malformed non-RTF text data")]
    InvalidTextRtf { cue_index: usize },
    #[error(
        "restyled cue {cue_index} has {count} nonempty text elements; mapping would be ambiguous"
    )]
    AmbiguousSourceText { cue_index: usize, count: usize },
    #[error("restyled text cue {cue_index} has no active macro")]
    MissingActiveMacro { cue_index: usize },
    #[error("restyled cue {cue_index} has no native cue UUID")]
    MissingCueIdentity { cue_index: usize },
    #[error("restyled cue {cue_index} has no native text-element UUID")]
    MissingTextElementIdentity { cue_index: usize },
    #[error("restyled cue {cue_index} macro '{macro_name}' has no resolved audience destinations")]
    MissingAudienceDestinations {
        cue_index: usize,
        macro_name: String,
    },
    #[error(
        "restyled cue {cue_index} cannot map text uniquely to audience screen '{screen_name}': {source}"
    )]
    AmbiguousAudienceTextMapping {
        cue_index: usize,
        screen_name: String,
        source: TemplateSlotError,
    },
    #[error("restyled cue {cue_index} repeats audience screen {screen_uuid}")]
    DuplicateAudienceScreen {
        cue_index: usize,
        screen_uuid: String,
    },
    #[error(
        "restyled cue {cue_index} has {metric_style_run_count} metric-affecting text runs; audience screen '{screen_name}' cannot be proved without ProPresenter's private inline-style mapping"
    )]
    UnsupportedAudienceMetricStyles {
        cue_index: usize,
        screen_name: String,
        metric_style_run_count: usize,
    },
    #[error(
        "restyled cue {cue_index} overflows {destination}: lines={line_count}, used={used_width:.2}x{used_height:.2}pt"
    )]
    Overflow {
        cue_index: usize,
        destination: String,
        used_width: f64,
        used_height: f64,
        line_count: usize,
    },
}
