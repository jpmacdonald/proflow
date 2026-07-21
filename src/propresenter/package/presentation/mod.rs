//! Package-specific adapters for embedded native presentations.

use prost::Message;

use super::model::{EmbeddedPresentationStructure, EmbeddedPresentationSummary, PlaylistPackage};
use crate::propresenter::generated::rv_data;
use crate::propresenter::inspection::summarize_presentation_structure;

/// Return compact summaries for embedded files that decode as presentations.
#[must_use]
pub fn embedded_presentation_summaries(
    package: &PlaylistPackage,
) -> Vec<EmbeddedPresentationSummary> {
    let mut summaries = Vec::new();
    for file in package
        .embedded_file_details()
        .filter(|file| file.is_presentation)
    {
        let Some(data) = package.embedded_file(&file.name) else {
            continue;
        };
        let Ok(presentation) = rv_data::Presentation::decode(data) else {
            continue;
        };
        summaries.push(EmbeddedPresentationSummary {
            archive_path: file.name.clone(),
            basename: file.basename.clone(),
            presentation_uuid: presentation.uuid.as_ref().map(|uuid| uuid.string.clone()),
            presentation_name: presentation.name,
            cue_count: presentation.cues.len(),
            cue_group_count: presentation.cue_groups.len(),
            arrangement_names: presentation
                .arrangements
                .iter()
                .map(|arrangement| arrangement.name.clone())
                .collect(),
        });
    }
    summaries
}

/// Return semantic structures for embedded presentation files that decode cleanly.
#[must_use]
pub fn embedded_presentation_structures(
    package: &PlaylistPackage,
) -> Vec<EmbeddedPresentationStructure> {
    let mut structures = Vec::new();
    for file in package
        .embedded_file_details()
        .filter(|file| file.is_presentation)
    {
        let Some(data) = package.embedded_file(&file.name) else {
            continue;
        };
        let Ok(presentation) = rv_data::Presentation::decode(data) else {
            continue;
        };
        structures.push(EmbeddedPresentationStructure {
            archive_path: file.name.clone(),
            basename: file.basename.clone(),
            structure: summarize_presentation_structure(&presentation),
        });
    }
    structures
}
