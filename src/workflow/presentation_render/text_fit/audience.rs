//! Measure and retain proof for macro-selected audience destinations.

use super::{
    bind_role_to_audience_slide, digest_hex, evidence_satisfies_policy, AudienceMeasurement,
    AudienceTextCandidate, AudienceTextRendering, CueRoleId, MacroName, PresentationDestination,
    PresentationRenderError, RenderAssetSnapshot, RenderRole, RenderStyle, RenderTextFit,
    TextField, TextFitDestinationIdentity, TextFitEvidence, LEADER_ROLE_SUFFIX,
};
use std::collections::BTreeMap;

type AudienceEvidenceByCue =
    BTreeMap<usize, BTreeMap<String, (TextFitDestinationIdentity, TextFitEvidence)>>;

pub(super) fn retain_audience_measurement(
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

pub(super) fn cue_audience_macro<'a>(
    cue_role: &CueRoleId,
    style: &'a RenderStyle,
) -> Option<(&'a RenderRole, &'a MacroName)> {
    if let Some(title) = style.title() {
        if cue_role == title.id() {
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

pub(super) fn measure_audience_destinations(
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
        .audience_destinations_for_macro(macro_name.as_str())
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
                        propresenter::unstable_native::rv_data::PresentationSlide {
                            base_slide: Some(destination.base_slide().clone()),
                            ..propresenter::unstable_native::rv_data::PresentationSlide::default()
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
