//! Reviewed presentation targets and the native file export boundary.

use std::path::Path;

use crate::propresenter::generated::rv_data;
use crate::propresenter::generated_document::GeneratedPresentation;
use crate::propresenter::playlist::{PlaylistEntry, PlaylistError, SelectedArrangement};
use crate::propresenter::render::RenderedPresentation;
use crate::propresenter::serialize::{encode_existing_presentation, write_presentation_bytes};
use crate::propresenter::PresentationSize;
use crate::workflow::plan::ResolvedItemPlan;
use crate::workflow::presentation_render::PresentationRenderError;

use super::super::BuildServiceError;

#[derive(Clone, Copy)]
pub(in crate::workflow::execute) struct ReviewedBackgroundAsset<'a> {
    pub(in crate::workflow::execute) path: &'a Path,
    pub(in crate::workflow::execute) data: &'a [u8],
}

#[derive(Clone, Copy)]
pub(in crate::workflow::execute) struct ReviewedRenderTarget<'a> {
    pub(in crate::workflow::execute) write_path: &'a Path,
    pub(in crate::workflow::execute) final_path: &'a Path,
    pub(in crate::workflow::execute) existing_bytes: Option<&'a [u8]>,
    pub(in crate::workflow::execute) presentation_size: PresentationSize,
    pub(in crate::workflow::execute) background: Option<ReviewedBackgroundAsset<'a>>,
}

/// Apply one fallible native transform to a detached document and retain it
/// only when the rendered role mapping is still exact.
pub(super) fn update_rendered_document(
    rendered: &mut RenderedPresentation,
    update: impl FnOnce(&mut rv_data::Presentation) -> Result<(), BuildServiceError>,
) -> Result<(), BuildServiceError> {
    let mut candidate = rendered.presentation().clone();
    update(&mut candidate)?;
    rendered
        .replace_preserving_role_mapping(candidate)
        .map_err(PresentationRenderError::from)?;
    Ok(())
}

pub(super) fn write_existing_playlist_presentation(
    entry: &ResolvedItemPlan,
    presentation: &rv_data::Presentation,
    target: ReviewedRenderTarget<'_>,
    selected_arrangement: Option<SelectedArrangement>,
) -> Result<(PlaylistEntry, usize), BuildServiceError> {
    validate_rendered_presentation_size(
        presentation,
        target.presentation_size,
        entry.output_key.as_str(),
    )?;
    let encoded = encode_existing_presentation(presentation)?;
    write_encoded_playlist_presentation(
        entry,
        presentation.cues.len(),
        target,
        encoded,
        selected_arrangement,
    )
}

pub(super) fn write_generated_playlist_presentation(
    entry: &ResolvedItemPlan,
    presentation: &rv_data::Presentation,
    target: ReviewedRenderTarget<'_>,
) -> Result<(PlaylistEntry, usize), BuildServiceError> {
    validate_rendered_presentation_size(
        presentation,
        target.presentation_size,
        entry.output_key.as_str(),
    )?;
    let encoded = GeneratedPresentation::new(presentation)?.encode();
    write_encoded_playlist_presentation(entry, presentation.cues.len(), target, encoded, None)
}

pub(super) fn validate_rendered_presentation_size(
    presentation: &rv_data::Presentation,
    expected: PresentationSize,
    output_key: &str,
) -> Result<(), BuildServiceError> {
    let actual = crate::propresenter::resolution::inspect_presentation_size(presentation);
    if actual.matches(expected) {
        Ok(())
    } else {
        Err(BuildServiceError::PresentationSizeInvariant {
            output_key: output_key.to_string(),
            expected,
            actual: actual.describe(),
        })
    }
}

fn write_encoded_playlist_presentation(
    entry: &ResolvedItemPlan,
    cue_count: usize,
    target: ReviewedRenderTarget<'_>,
    encoded: Vec<u8>,
    selected_arrangement: Option<SelectedArrangement>,
) -> Result<(PlaylistEntry, usize), BuildServiceError> {
    write_presentation_bytes(target.write_path, &encoded)?;
    let playlist_entry = PlaylistEntry::embedded(
        entry.playlist_name.clone(),
        target.final_path.display().to_string(),
        encoded,
    )
    .map_err(PlaylistError::from)?
    .with_selected_arrangement(selected_arrangement)
    .map_err(PlaylistError::from)?;
    Ok((playlist_entry, cue_count))
}
