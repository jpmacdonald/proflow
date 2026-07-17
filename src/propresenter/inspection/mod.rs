//! Semantic inspection of standalone native `ProPresenter` presentations.
//!
//! This boundary understands presentation content and operator traversal but
//! has no knowledge of playlist archives or embedded package paths.

mod cue;
mod model;

use std::collections::BTreeMap;

use crate::propresenter::generated::rv_data;
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
    let cue_indexes_by_uuid = cue_indexes_by_uuid(presentation);
    let cue_group_indexes_by_uuid = cue_group_indexes_by_uuid(presentation);
    let arrangement_indexes_by_uuid = arrangement_indexes_by_uuid(presentation);
    let mut reference_diagnostics = Vec::new();
    for (uuid, indexes) in &cue_indexes_by_uuid {
        if indexes.len() > 1 {
            reference_diagnostics.push(PresentationReferenceDiagnostic::DuplicateCueUuid {
                uuid: (*uuid).to_string(),
                cue_indexes: indexes.clone(),
            });
        }
    }
    for (uuid, indexes) in &cue_group_indexes_by_uuid {
        if indexes.len() > 1 {
            reference_diagnostics.push(PresentationReferenceDiagnostic::DuplicateCueGroupUuid {
                uuid: (*uuid).to_string(),
                cue_group_indexes: indexes.clone(),
            });
        }
    }
    for (uuid, indexes) in &arrangement_indexes_by_uuid {
        if indexes.len() > 1 {
            reference_diagnostics.push(PresentationReferenceDiagnostic::DuplicateArrangementUuid {
                uuid: (*uuid).to_string(),
                arrangement_indexes: indexes.clone(),
            });
        }
    }
    if let Some(selected) = presentation.selected_arrangement.as_ref() {
        match resolve_reference(&arrangement_indexes_by_uuid, &selected.string) {
            ReferenceResolution::Missing => reference_diagnostics.push(
                PresentationReferenceDiagnostic::DanglingSelectedArrangement {
                    uuid: selected.string.clone(),
                },
            ),
            ReferenceResolution::Ambiguous(indexes) => reference_diagnostics.push(
                PresentationReferenceDiagnostic::AmbiguousSelectedArrangement {
                    uuid: selected.string.clone(),
                    arrangement_indexes: indexes.to_vec(),
                },
            ),
            ReferenceResolution::Unique(_) => {}
        }
    }

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
            for (reference_index, cue_id) in cue_group.cue_identifiers.iter().enumerate() {
                match resolve_reference(&cue_indexes_by_uuid, &cue_id.string) {
                    ReferenceResolution::Unique(cue_index) => {
                        cue_indexes.push(cue_index);
                        cue_group_names_by_cue_index[cue_index].push(name.clone());
                    }
                    ReferenceResolution::Missing => reference_diagnostics.push(
                        PresentationReferenceDiagnostic::DanglingCueReference {
                            cue_group_index: index,
                            reference_index,
                            uuid: cue_id.string.clone(),
                        },
                    ),
                    ReferenceResolution::Ambiguous(indexes) => reference_diagnostics.push(
                        PresentationReferenceDiagnostic::AmbiguousCueReference {
                            cue_group_index: index,
                            reference_index,
                            uuid: cue_id.string.clone(),
                            cue_indexes: indexes.to_vec(),
                        },
                    ),
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
            for (reference_index, group_id) in arrangement.group_identifiers.iter().enumerate() {
                match resolve_reference(&cue_group_indexes_by_uuid, &group_id.string) {
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
                            match resolve_reference(&cue_indexes_by_uuid, &cue_id.string) {
                                ReferenceResolution::Unique(index) => Some(index),
                                ReferenceResolution::Missing
                                | ReferenceResolution::Ambiguous(_) => None,
                            }
                        }));
                    }
                    ReferenceResolution::Missing => reference_diagnostics.push(
                        PresentationReferenceDiagnostic::DanglingGroupReference {
                            arrangement_index: index,
                            reference_index,
                            uuid: group_id.string.clone(),
                        },
                    ),
                    ReferenceResolution::Ambiguous(indexes) => reference_diagnostics.push(
                        PresentationReferenceDiagnostic::AmbiguousGroupReference {
                            arrangement_index: index,
                            reference_index,
                            uuid: group_id.string.clone(),
                            cue_group_indexes: indexes.to_vec(),
                        },
                    ),
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

pub(crate) fn percent_decode_lossy(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
            {
                decoded.push((hi << 4) | lo);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(decoded).unwrap_or_else(|_| value.to_string())
}

const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

type ReferenceIndex<'a> = BTreeMap<&'a str, Vec<usize>>;

enum ReferenceResolution<'a> {
    Missing,
    Unique(usize),
    Ambiguous(&'a [usize]),
}

fn resolve_reference<'a>(index: &'a ReferenceIndex<'_>, uuid: &str) -> ReferenceResolution<'a> {
    match index.get(uuid).map(Vec::as_slice) {
        None | Some([]) => ReferenceResolution::Missing,
        Some([index]) => ReferenceResolution::Unique(*index),
        Some(indexes) => ReferenceResolution::Ambiguous(indexes),
    }
}

fn cue_indexes_by_uuid(presentation: &rv_data::Presentation) -> ReferenceIndex<'_> {
    let mut indexes = BTreeMap::new();
    for (index, cue) in presentation.cues.iter().enumerate() {
        if let Some(uuid) = &cue.uuid {
            indexes
                .entry(uuid.string.as_str())
                .or_insert_with(Vec::new)
                .push(index);
        }
    }
    indexes
}

fn cue_group_indexes_by_uuid(presentation: &rv_data::Presentation) -> ReferenceIndex<'_> {
    let mut indexes = BTreeMap::new();
    for (index, cue_group) in presentation.cue_groups.iter().enumerate() {
        if let Some(uuid) = cue_group
            .group
            .as_ref()
            .and_then(|group| group.uuid.as_ref())
        {
            indexes
                .entry(uuid.string.as_str())
                .or_insert_with(Vec::new)
                .push(index);
        }
    }
    indexes
}

fn arrangement_indexes_by_uuid(presentation: &rv_data::Presentation) -> ReferenceIndex<'_> {
    let mut index = BTreeMap::new();
    for (arrangement_index, arrangement) in presentation.arrangements.iter().enumerate() {
        if let Some(uuid) = arrangement.uuid.as_ref() {
            index
                .entry(uuid.string.as_str())
                .or_insert_with(Vec::new)
                .push(arrangement_index);
        }
    }
    index
}

#[cfg(test)]
mod tests;
