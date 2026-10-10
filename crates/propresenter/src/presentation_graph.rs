//! Canonical resolution of native presentation identities and references.

use std::collections::BTreeMap;

use super::{generated::rv_data, inspection::PresentationReferenceDiagnostic};

type ReferenceIndex<'a> = BTreeMap<&'a str, Vec<usize>>;

/// The exact result of resolving one native UUID reference.
pub enum ReferenceResolution<'a> {
    Missing,
    Unique(usize),
    Ambiguous(&'a [usize]),
}

/// A cue/action location in raw protobuf order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionIndex {
    pub(crate) cue_index: usize,
    pub(crate) action_index: usize,
}

/// One immutable owner for all document-level native identity indexes.
///
/// Inspection, operator traversal, and validation query this graph instead of
/// independently interpreting UUID strings. The graph intentionally retains
/// ambiguous identities: inspection can report them while mutation rejects
/// them through the same resolution result.
pub struct ResolvedPresentationGraph<'a> {
    presentation: &'a rv_data::Presentation,
    cues: ReferenceIndex<'a>,
    groups: ReferenceIndex<'a>,
    arrangements: ReferenceIndex<'a>,
    actions: BTreeMap<&'a str, Vec<ActionIndex>>,
}

impl<'a> ResolvedPresentationGraph<'a> {
    pub(crate) fn new(presentation: &'a rv_data::Presentation) -> Self {
        let mut graph = Self {
            presentation,
            cues: BTreeMap::new(),
            groups: BTreeMap::new(),
            arrangements: BTreeMap::new(),
            actions: BTreeMap::new(),
        };
        for (cue_index, cue) in presentation.cues.iter().enumerate() {
            if let Some(uuid) = &cue.uuid {
                graph.cues.entry(&uuid.string).or_default().push(cue_index);
            }
            for (action_index, action) in cue.actions.iter().enumerate() {
                if let Some(uuid) = &action.uuid {
                    graph
                        .actions
                        .entry(&uuid.string)
                        .or_default()
                        .push(ActionIndex {
                            cue_index,
                            action_index,
                        });
                }
            }
        }
        for (index, cue_group) in presentation.cue_groups.iter().enumerate() {
            if let Some(uuid) = cue_group
                .group
                .as_ref()
                .and_then(|group| group.uuid.as_ref())
            {
                graph.groups.entry(&uuid.string).or_default().push(index);
            }
        }
        for (index, arrangement) in presentation.arrangements.iter().enumerate() {
            if let Some(uuid) = &arrangement.uuid {
                graph
                    .arrangements
                    .entry(&uuid.string)
                    .or_default()
                    .push(index);
            }
        }
        graph
    }

    pub(crate) const fn presentation(&self) -> &'a rv_data::Presentation {
        self.presentation
    }

    pub(crate) fn cue(&self, uuid: &str) -> ReferenceResolution<'_> {
        resolve(&self.cues, uuid)
    }

    pub(crate) fn group(&self, uuid: &str) -> ReferenceResolution<'_> {
        resolve(&self.groups, uuid)
    }

    pub(crate) fn arrangement(&self, uuid: &str) -> ReferenceResolution<'_> {
        resolve(&self.arrangements, uuid)
    }

    pub(crate) fn action(&self, uuid: &str) -> ActionReferenceResolution {
        match self.actions.get(uuid).map(Vec::as_slice) {
            None | Some([]) => ActionReferenceResolution::Missing,
            Some([location]) => ActionReferenceResolution::Unique(*location),
            Some([first, ..]) => ActionReferenceResolution::Ambiguous { first: *first },
        }
    }

    pub(crate) fn reference_diagnostics(&self) -> Vec<PresentationReferenceDiagnostic> {
        let mut diagnostics = Vec::new();
        for (uuid, indexes) in &self.cues {
            if indexes.len() > 1 {
                diagnostics.push(PresentationReferenceDiagnostic::DuplicateCueUuid {
                    uuid: (*uuid).to_string(),
                    cue_indexes: indexes.clone(),
                });
            }
        }
        for (uuid, indexes) in &self.groups {
            if indexes.len() > 1 {
                diagnostics.push(PresentationReferenceDiagnostic::DuplicateCueGroupUuid {
                    uuid: (*uuid).to_string(),
                    cue_group_indexes: indexes.clone(),
                });
            }
        }
        for (uuid, indexes) in &self.arrangements {
            if indexes.len() > 1 {
                diagnostics.push(PresentationReferenceDiagnostic::DuplicateArrangementUuid {
                    uuid: (*uuid).to_string(),
                    arrangement_indexes: indexes.clone(),
                });
            }
        }
        if let Some(selected) = &self.presentation.selected_arrangement {
            match self.arrangement(&selected.string) {
                ReferenceResolution::Missing => diagnostics.push(
                    PresentationReferenceDiagnostic::DanglingSelectedArrangement {
                        uuid: selected.string.clone(),
                    },
                ),
                ReferenceResolution::Ambiguous(indexes) => diagnostics.push(
                    PresentationReferenceDiagnostic::AmbiguousSelectedArrangement {
                        uuid: selected.string.clone(),
                        arrangement_indexes: indexes.to_vec(),
                    },
                ),
                ReferenceResolution::Unique(_) => {}
            }
        }
        for (group_index, group) in self.presentation.cue_groups.iter().enumerate() {
            for (reference_index, cue) in group.cue_identifiers.iter().enumerate() {
                match self.cue(&cue.string) {
                    ReferenceResolution::Missing => {
                        diagnostics.push(PresentationReferenceDiagnostic::DanglingCueReference {
                            cue_group_index: group_index,
                            reference_index,
                            uuid: cue.string.clone(),
                        });
                    }
                    ReferenceResolution::Ambiguous(indexes) => {
                        diagnostics.push(PresentationReferenceDiagnostic::AmbiguousCueReference {
                            cue_group_index: group_index,
                            reference_index,
                            uuid: cue.string.clone(),
                            cue_indexes: indexes.to_vec(),
                        });
                    }
                    ReferenceResolution::Unique(_) => {}
                }
            }
        }
        for (arrangement_index, arrangement) in self.presentation.arrangements.iter().enumerate() {
            for (reference_index, group) in arrangement.group_identifiers.iter().enumerate() {
                match self.group(&group.string) {
                    ReferenceResolution::Missing => {
                        diagnostics.push(PresentationReferenceDiagnostic::DanglingGroupReference {
                            arrangement_index,
                            reference_index,
                            uuid: group.string.clone(),
                        });
                    }
                    ReferenceResolution::Ambiguous(indexes) => {
                        diagnostics.push(
                            PresentationReferenceDiagnostic::AmbiguousGroupReference {
                                arrangement_index,
                                reference_index,
                                uuid: group.string.clone(),
                                cue_group_indexes: indexes.to_vec(),
                            },
                        );
                    }
                    ReferenceResolution::Unique(_) => {}
                }
            }
        }
        diagnostics
    }
}

pub enum ActionReferenceResolution {
    Missing,
    Unique(ActionIndex),
    Ambiguous { first: ActionIndex },
}

fn resolve<'a>(index: &'a ReferenceIndex<'_>, uuid: &str) -> ReferenceResolution<'a> {
    match index.get(uuid).map(Vec::as_slice) {
        None | Some([]) => ReferenceResolution::Missing,
        Some([index]) => ReferenceResolution::Unique(*index),
        Some(indexes) => ReferenceResolution::Ambiguous(indexes),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_all_document_identity_kinds_without_guessing_duplicates() {
        let id = |value: &str| rv_data::Uuid {
            string: value.to_string(),
        };
        let presentation = rv_data::Presentation {
            cues: vec![
                rv_data::Cue {
                    uuid: Some(id("cue")),
                    actions: vec![rv_data::Action {
                        uuid: Some(id("action")),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                rv_data::Cue {
                    uuid: Some(id("cue")),
                    ..Default::default()
                },
            ],
            cue_groups: vec![rv_data::presentation::CueGroup {
                group: Some(rv_data::Group {
                    uuid: Some(id("group")),
                    ..Default::default()
                }),
                cue_identifiers: vec![id("cue")],
            }],
            arrangements: vec![rv_data::presentation::Arrangement {
                uuid: Some(id("arrangement")),
                group_identifiers: vec![id("group")],
                ..Default::default()
            }],
            ..Default::default()
        };
        let graph = ResolvedPresentationGraph::new(&presentation);
        assert!(matches!(
            graph.cue("cue"),
            ReferenceResolution::Ambiguous([0, 1])
        ));
        assert!(matches!(
            graph.group("group"),
            ReferenceResolution::Unique(0)
        ));
        assert!(matches!(
            graph.arrangement("arrangement"),
            ReferenceResolution::Unique(0)
        ));
        assert!(matches!(
            graph.action("action"),
            ActionReferenceResolution::Unique(ActionIndex {
                cue_index: 0,
                action_index: 0
            })
        ));
    }
}
