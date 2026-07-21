//! Checked final boundary for presentations created by the semantic renderer.

use std::collections::HashMap;

use prost::Message;

use super::arrangement::{checked_operator_cue_indices, OperatorTraversalError};
use super::generated::rv_data::{self, action};
use super::inspection::{summarize_presentation_structure, PresentationReferenceDiagnostic};
use super::render::slide_instance::validate_slide_identity_graph;
use super::render::SlideInstantiationError;

/// A rendered native presentation whose final identity and reference graphs
/// were checked after every workflow transform.
///
/// The immutable borrow prevents the protobuf from changing between validation
/// and encoding. Raw native codecs remain available for reverse engineering;
/// product-generated files cross this stricter boundary.
pub struct GeneratedPresentation<'a> {
    presentation: &'a rv_data::Presentation,
}

impl<'a> GeneratedPresentation<'a> {
    /// Validate the complete generated document graph.
    pub fn new(
        presentation: &'a rv_data::Presentation,
    ) -> Result<Self, GeneratedPresentationError> {
        validate_document_identity(presentation)?;
        validate_cues(presentation)?;
        validate_groups(presentation)?;
        validate_arrangements(presentation)?;

        let diagnostics = summarize_presentation_structure(presentation).reference_diagnostics;
        if !diagnostics.is_empty() {
            return Err(GeneratedPresentationError::ReferenceGraph(diagnostics));
        }
        checked_operator_cue_indices(presentation)?;
        Ok(Self { presentation })
    }

    /// Encode the exact presentation proven by [`Self::new`].
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        self.presentation.encode_to_vec()
    }
}

/// Structural failures that may never reach a generated `.pro` file.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum GeneratedPresentationError {
    /// The document has no operator-visible name or UUID.
    #[error("generated presentation requires a non-empty name and document UUID")]
    MissingDocumentIdentity,
    /// A generated presentation cannot be empty.
    #[error("generated presentation contains no cues")]
    Empty,
    /// A cue has no canonical UUID.
    #[error("generated cue {index} has missing or invalid UUID {value:?}")]
    InvalidCueIdentity {
        /// Zero-based cue index.
        index: usize,
        /// Native identity text, when one was present.
        value: Option<String>,
    },
    /// Two generated cues share one identity.
    #[error("generated cues {first_index} and {duplicate_index} share UUID '{uuid}'")]
    DuplicateCueIdentity {
        /// Reused native cue UUID.
        uuid: String,
        /// Index where the UUID first appeared.
        first_index: usize,
        /// Index where the UUID was reused.
        duplicate_index: usize,
    },
    /// A cue action has no canonical UUID.
    #[error(
        "generated cue {cue_index} action {action_index} has missing or invalid UUID {value:?}"
    )]
    InvalidActionIdentity {
        /// Zero-based parent cue index.
        cue_index: usize,
        /// Zero-based action index within the cue.
        action_index: usize,
        /// Native identity text, when one was present.
        value: Option<String>,
    },
    /// Two actions in one generated document share one identity.
    #[error(
        "generated cue {cue_index} action {action_index} reuses action UUID '{uuid}' first seen at cue {first_cue_index} action {first_action_index}"
    )]
    DuplicateActionIdentity {
        /// Reused native action UUID.
        uuid: String,
        /// Parent cue index where the UUID first appeared.
        first_cue_index: usize,
        /// Action index where the UUID first appeared.
        first_action_index: usize,
        /// Parent cue index where the UUID was reused.
        cue_index: usize,
        /// Action index where the UUID was reused.
        action_index: usize,
    },
    /// A cue-local presentation slide contains an invalid native identity graph.
    #[error("generated cue {cue_index} action {action_index} has invalid slide graph: {source}")]
    SlideGraph {
        /// Zero-based parent cue index.
        cue_index: usize,
        /// Zero-based action index within the cue.
        action_index: usize,
        /// Exact local graph violation.
        #[source]
        source: SlideInstantiationError,
    },
    /// A cue group has no canonical UUID.
    #[error("generated cue group {index} has missing or invalid UUID {value:?}")]
    InvalidGroupIdentity {
        /// Zero-based cue-group index.
        index: usize,
        /// Native identity text, when one was present.
        value: Option<String>,
    },
    /// Two generated cue groups share one identity.
    #[error("generated cue groups {first_index} and {duplicate_index} share UUID '{uuid}'")]
    DuplicateGroupIdentity {
        /// Reused native cue-group UUID.
        uuid: String,
        /// Index where the UUID first appeared.
        first_index: usize,
        /// Index where the UUID was reused.
        duplicate_index: usize,
    },
    /// An arrangement has no canonical UUID.
    #[error("generated arrangement {index} has missing or invalid UUID {value:?}")]
    InvalidArrangementIdentity {
        /// Zero-based arrangement index.
        index: usize,
        /// Native identity text, when one was present.
        value: Option<String>,
    },
    /// Two generated arrangements share one identity.
    #[error("generated arrangements {first_index} and {duplicate_index} share UUID '{uuid}'")]
    DuplicateArrangementIdentity {
        /// Reused native arrangement UUID.
        uuid: String,
        /// Index where the UUID first appeared.
        first_index: usize,
        /// Index where the UUID was reused.
        duplicate_index: usize,
    },
    /// A group, arrangement, or selected-arrangement reference is dangling or ambiguous.
    #[error("generated presentation has invalid native references: {0:?}")]
    ReferenceGraph(Vec<PresentationReferenceDiagnostic>),
    /// The selected/default operator traversal cannot be resolved exactly.
    #[error(transparent)]
    OperatorTraversal(#[from] OperatorTraversalError),
}

fn validate_document_identity(
    presentation: &rv_data::Presentation,
) -> Result<(), GeneratedPresentationError> {
    if presentation.name.trim().is_empty()
        || presentation.name.trim() != presentation.name
        || native_uuid(presentation.uuid.as_ref()).is_none()
    {
        Err(GeneratedPresentationError::MissingDocumentIdentity)
    } else {
        Ok(())
    }
}

fn validate_cues(presentation: &rv_data::Presentation) -> Result<(), GeneratedPresentationError> {
    if presentation.cues.is_empty() {
        return Err(GeneratedPresentationError::Empty);
    }
    let mut cue_ids = HashMap::new();
    let mut action_ids = HashMap::new();
    for (cue_index, cue) in presentation.cues.iter().enumerate() {
        let cue_uuid = native_uuid(cue.uuid.as_ref()).ok_or_else(|| {
            GeneratedPresentationError::InvalidCueIdentity {
                index: cue_index,
                value: cue.uuid.as_ref().map(|uuid| uuid.string.clone()),
            }
        })?;
        if let Some(first_index) = cue_ids.insert(cue_uuid, cue_index) {
            return Err(GeneratedPresentationError::DuplicateCueIdentity {
                uuid: cue_uuid.to_string(),
                first_index,
                duplicate_index: cue_index,
            });
        }
        for (action_index, cue_action) in cue.actions.iter().enumerate() {
            let action_uuid = native_uuid(cue_action.uuid.as_ref()).ok_or_else(|| {
                GeneratedPresentationError::InvalidActionIdentity {
                    cue_index,
                    action_index,
                    value: cue_action.uuid.as_ref().map(|uuid| uuid.string.clone()),
                }
            })?;
            if let Some((first_cue_index, first_action_index)) =
                action_ids.insert(action_uuid, (cue_index, action_index))
            {
                return Err(GeneratedPresentationError::DuplicateActionIdentity {
                    uuid: action_uuid.to_string(),
                    first_cue_index,
                    first_action_index,
                    cue_index,
                    action_index,
                });
            }
            if let Some(action::ActionTypeData::Slide(slide_action)) = &cue_action.action_type_data
            {
                if let Some(action::slide_type::Slide::Presentation(slide)) = &slide_action.slide {
                    validate_slide_identity_graph(slide).map_err(|source| {
                        GeneratedPresentationError::SlideGraph {
                            cue_index,
                            action_index,
                            source,
                        }
                    })?;
                }
            }
        }
    }
    Ok(())
}

fn validate_groups(presentation: &rv_data::Presentation) -> Result<(), GeneratedPresentationError> {
    let mut group_ids = HashMap::new();
    for (index, cue_group) in presentation.cue_groups.iter().enumerate() {
        let identity = cue_group
            .group
            .as_ref()
            .and_then(|group| native_uuid(group.uuid.as_ref()));
        let Some(identity) = identity else {
            return Err(GeneratedPresentationError::InvalidGroupIdentity {
                index,
                value: cue_group
                    .group
                    .as_ref()
                    .and_then(|group| group.uuid.as_ref())
                    .map(|uuid| uuid.string.clone()),
            });
        };
        if let Some(first_index) = group_ids.insert(identity, index) {
            return Err(GeneratedPresentationError::DuplicateGroupIdentity {
                uuid: identity.to_string(),
                first_index,
                duplicate_index: index,
            });
        }
    }
    Ok(())
}

fn validate_arrangements(
    presentation: &rv_data::Presentation,
) -> Result<(), GeneratedPresentationError> {
    let mut arrangement_ids = HashMap::new();
    for (index, arrangement) in presentation.arrangements.iter().enumerate() {
        let identity = native_uuid(arrangement.uuid.as_ref()).ok_or_else(|| {
            GeneratedPresentationError::InvalidArrangementIdentity {
                index,
                value: arrangement.uuid.as_ref().map(|uuid| uuid.string.clone()),
            }
        })?;
        if let Some(first_index) = arrangement_ids.insert(identity, index) {
            return Err(GeneratedPresentationError::DuplicateArrangementIdentity {
                uuid: identity.to_string(),
                first_index,
                duplicate_index: index,
            });
        }
    }
    Ok(())
}

fn native_uuid(identity: Option<&rv_data::Uuid>) -> Option<uuid::Uuid> {
    let identity = identity?;
    let value = identity.string.trim();
    if value != identity.string {
        return None;
    }
    uuid::Uuid::parse_str(value).ok()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic)]

    use super::*;

    fn id() -> rv_data::Uuid {
        rv_data::Uuid {
            string: uuid::Uuid::new_v4().to_string(),
        }
    }

    fn generated_fixture() -> rv_data::Presentation {
        let cue_id = id();
        let group_id = id();
        let arrangement_id = id();
        rv_data::Presentation {
            uuid: Some(id()),
            name: "Generated".to_string(),
            cues: vec![rv_data::Cue {
                uuid: Some(cue_id.clone()),
                actions: vec![rv_data::Action {
                    uuid: Some(id()),
                    action_type_data: Some(action::ActionTypeData::Slide(action::SlideType {
                        slide: Some(action::slide_type::Slide::Presentation(
                            rv_data::PresentationSlide {
                                base_slide: Some(rv_data::Slide {
                                    uuid: Some(id()),
                                    ..rv_data::Slide::default()
                                }),
                                ..rv_data::PresentationSlide::default()
                            },
                        )),
                    })),
                    ..rv_data::Action::default()
                }],
                ..rv_data::Cue::default()
            }],
            cue_groups: vec![rv_data::presentation::CueGroup {
                group: Some(rv_data::Group {
                    uuid: Some(group_id.clone()),
                    ..rv_data::Group::default()
                }),
                cue_identifiers: vec![cue_id],
            }],
            arrangements: vec![rv_data::presentation::Arrangement {
                uuid: Some(arrangement_id.clone()),
                name: "Default".to_string(),
                group_identifiers: vec![group_id],
            }],
            selected_arrangement: Some(arrangement_id),
            ..rv_data::Presentation::default()
        }
    }

    #[test]
    fn validated_borrow_encodes_the_exact_generated_document() {
        let presentation = generated_fixture();
        let checked = GeneratedPresentation::new(&presentation).expect("valid generated graph");

        assert_eq!(checked.encode(), presentation.encode_to_vec());
    }

    #[test]
    fn rejects_dangling_native_references() {
        let mut presentation = generated_fixture();
        presentation.arrangements[0].group_identifiers[0] = id();

        assert!(matches!(
            GeneratedPresentation::new(&presentation),
            Err(GeneratedPresentationError::ReferenceGraph(_))
        ));
    }

    #[test]
    fn rejects_duplicate_action_identity() {
        let mut presentation = generated_fixture();
        let duplicate = presentation.cues[0].actions[0].clone();
        presentation.cues[0].actions.push(duplicate);

        assert!(matches!(
            GeneratedPresentation::new(&presentation),
            Err(GeneratedPresentationError::DuplicateActionIdentity {
                first_cue_index: 0,
                first_action_index: 0,
                cue_index: 0,
                action_index: 1,
                ..
            })
        ));
    }

    #[test]
    fn rejects_invalid_cue_local_slide_graph() {
        let mut presentation = generated_fixture();
        let Some(action::ActionTypeData::Slide(slide)) =
            &mut presentation.cues[0].actions[0].action_type_data
        else {
            panic!("slide action fixture");
        };
        let Some(action::slide_type::Slide::Presentation(slide)) = &mut slide.slide else {
            panic!("presentation slide fixture");
        };
        slide.base_slide = None;

        assert!(matches!(
            GeneratedPresentation::new(&presentation),
            Err(GeneratedPresentationError::SlideGraph {
                source: SlideInstantiationError::MissingBaseSlide,
                ..
            })
        ));
    }

    #[test]
    fn rejects_duplicate_group_and_arrangement_identities() {
        let mut duplicate_group = generated_fixture();
        duplicate_group
            .cue_groups
            .push(duplicate_group.cue_groups[0].clone());
        assert!(matches!(
            GeneratedPresentation::new(&duplicate_group),
            Err(GeneratedPresentationError::DuplicateGroupIdentity {
                first_index: 0,
                duplicate_index: 1,
                ..
            })
        ));

        let mut duplicate_arrangement = generated_fixture();
        duplicate_arrangement
            .arrangements
            .push(duplicate_arrangement.arrangements[0].clone());
        assert!(matches!(
            GeneratedPresentation::new(&duplicate_arrangement),
            Err(GeneratedPresentationError::DuplicateArrangementIdentity {
                first_index: 0,
                duplicate_index: 1,
                ..
            })
        ));
    }
}
