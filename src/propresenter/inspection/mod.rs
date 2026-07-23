//! Semantic inspection of standalone native `ProPresenter` presentations.
//!
//! This boundary understands presentation content and operator traversal but
//! has no knowledge of playlist archives or embedded package paths.

mod cue;
mod model;

use crate::propresenter::generated::rv_data;
use crate::propresenter::presentation_graph::{ReferenceResolution, ResolvedPresentationGraph};
use cue::{color_signature, summarize_bible_reference, summarize_cue};

pub use model::{
    ActionLabelSignature, ArrangementStructureSummary, BibleReferenceSummary,
    CueGroupStructureSummary, CueStructureSummary, HotKeySignature, IntRangeSummary,
    PresentationReferenceDiagnostic, PresentationStructureSummary, TextStyleSignature,
};

/// Return a semantic summary for a presentation.
#[must_use]
#[allow(
    clippy::too_many_lines,
    reason = "keeping every presentation field translation together makes the semantic boundary auditable"
)]
pub fn summarize_presentation_structure(
    presentation: &rv_data::Presentation,
) -> PresentationStructureSummary {
    let graph = ResolvedPresentationGraph::new(presentation);
    let reference_diagnostics = graph.reference_diagnostics();

    let mut cue_group_names_by_cue_index = vec![Vec::new(); presentation.cues.len()];
    let cue_groups = presentation
        .cue_groups
        .iter()
        .enumerate()
        .map(|(index, cue_group)| {
            let (uuid, name) = cue_group
                .group
                .as_ref()
                .map(|group| {
                    (
                        group.uuid.as_ref().map(|uuid| uuid.string.clone()),
                        group.name.clone(),
                    )
                })
                .unwrap_or_default();
            let mut cue_indexes = Vec::new();
            for cue_id in &cue_group.cue_identifiers {
                match graph.cue(&cue_id.string) {
                    ReferenceResolution::Unique(cue_index) => {
                        cue_indexes.push(cue_index);
                        cue_group_names_by_cue_index[cue_index].push(name.clone());
                    }
                    ReferenceResolution::Missing | ReferenceResolution::Ambiguous(_) => {}
                }
            }
            CueGroupStructureSummary {
                index,
                uuid,
                name,
                color: cue_group
                    .group
                    .as_ref()
                    .and_then(|group| group.color.as_ref())
                    .map(color_signature),
                hot_key: cue_group
                    .group
                    .as_ref()
                    .and_then(|group| group.hot_key.as_ref())
                    .map(|hot_key| HotKeySignature {
                        code: hot_key.code,
                        control_identifier: hot_key.control_identifier.clone(),
                    }),
                application_group_identifier: cue_group
                    .group
                    .as_ref()
                    .and_then(|group| group.application_group_identifier.as_ref())
                    .map(|uuid| uuid.string.clone()),
                application_group_name: cue_group
                    .group
                    .as_ref()
                    .map(|group| group.application_group_name.clone())
                    .unwrap_or_default(),
                cue_indexes,
            }
        })
        .collect::<Vec<_>>();

    let cues = presentation
        .cues
        .iter()
        .enumerate()
        .map(|(index, cue)| summarize_cue(index, cue, cue_group_names_by_cue_index[index].clone()))
        .collect::<Vec<_>>();

    let arrangements = presentation
        .arrangements
        .iter()
        .enumerate()
        .map(|(index, arrangement)| {
            let mut group_names = Vec::new();
            let mut cue_indexes = Vec::new();
            for group_id in &arrangement.group_identifiers {
                match graph.group(&group_id.string) {
                    ReferenceResolution::Unique(group_index) => {
                        let group = &presentation.cue_groups[group_index];
                        group_names.push(
                            group
                                .group
                                .as_ref()
                                .map(|group| group.name.clone())
                                .unwrap_or_default(),
                        );
                        cue_indexes.extend(group.cue_identifiers.iter().filter_map(|cue_id| {
                            match graph.cue(&cue_id.string) {
                                ReferenceResolution::Unique(index) => Some(index),
                                ReferenceResolution::Missing
                                | ReferenceResolution::Ambiguous(_) => None,
                            }
                        }));
                    }
                    ReferenceResolution::Missing | ReferenceResolution::Ambiguous(_) => {}
                }
            }
            ArrangementStructureSummary {
                index,
                uuid: arrangement.uuid.as_ref().map(|uuid| uuid.string.clone()),
                name: arrangement.name.clone(),
                group_names,
                cue_indexes,
            }
        })
        .collect::<Vec<_>>();

    PresentationStructureSummary {
        uuid: presentation.uuid.as_ref().map(|uuid| uuid.string.clone()),
        name: presentation.name.clone(),
        bible_reference: presentation
            .bible_reference
            .as_ref()
            .map(summarize_bible_reference),
        cues,
        cue_groups,
        arrangements,
        operator_cue_indexes: crate::propresenter::arrangement::operator_cue_indices(presentation),
        reference_diagnostics,
    }
}

#[cfg(test)]
mod tests;
