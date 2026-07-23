//! Validation of exact final native bytes against reviewed semantic promises.

use std::collections::{BTreeMap, BTreeSet};

use crate::propresenter::generated::rv_data;
use crate::propresenter::inspection::PresentationStructureSummary;
use crate::propresenter::resolution::inspect_presentation_size;
use crate::propresenter::PresentationSize;
use crate::workflow::{
    ExpectedCueCount, ExpectedMacroPolicy, ExpectedMacroSelector, ExpectedPresentationContract,
};

/// A deterministic mismatch between reviewed intent and exact final bytes.
#[derive(Debug, thiserror::Error)]
pub enum PresentationContractError {
    /// Playlist arrangement metadata differs from the reviewed plan.
    #[error("expected playlist arrangement {expected:?}, found {actual:?}")]
    Arrangement {
        /// Reviewed arrangement name.
        expected: Option<String>,
        /// Arrangement name carried by the playlist item.
        actual: Option<String>,
    },
    /// The selected native arrangement graph could not be traversed safely.
    #[error(transparent)]
    Traversal(#[from] crate::propresenter::arrangement::OperatorTraversalError),
    /// Operator traversal was empty or had the wrong exact cardinality.
    #[error("expected operator cue count {expected:?}, found {actual}")]
    CueCount {
        /// Reviewed cardinality contract.
        expected: ExpectedCueCount,
        /// Effective operator traversal length.
        actual: usize,
    },
    /// Native presentation slides do not share the required canvas.
    #[error("expected canvas {expected}, found {actual}")]
    Canvas {
        /// Reviewed project canvas.
        expected: PresentationSize,
        /// Native canvas state description.
        actual: String,
    },
    /// A promised macro selector did not resolve in the effective graph.
    #[error("macro selector {selector:?} does not resolve in the effective presentation")]
    MacroSelector {
        /// Selector that did not resolve.
        selector: ExpectedMacroSelector,
    },
    /// A selected arrangement group did not have an allowed semantic name.
    #[error("macro group {index} is '{actual}', expected one of {allowed:?}")]
    MacroGroupName {
        /// Selected-arrangement group occurrence.
        index: usize,
        /// Native group name at that occurrence.
        actual: String,
        /// Reviewed accepted group names.
        allowed: Vec<String>,
    },
    /// Two macro regions resolved to one native cue.
    #[error("multiple macro regions resolve to native cue {cue_index}")]
    DuplicateMacroTarget {
        /// Native cue claimed by multiple regions.
        cue_index: usize,
    },
    /// Final macro actions differ from the exact owned policy.
    #[error("native cue {cue_index} expected macros {expected:?}, found {actual:?}")]
    Macros {
        /// Native raw cue index.
        cue_index: usize,
        /// Exact reviewed macro actions.
        expected: Vec<String>,
        /// Macro actions found in final bytes.
        actual: Vec<String>,
    },
    /// The first operator cue does not carry the required background.
    #[error("native cue {cue_index} expected background '{expected}', found {actual:?}")]
    Background {
        /// Native raw cue index.
        cue_index: usize,
        /// Required media basename.
        expected: String,
        /// Media basenames found on the cue.
        actual: Vec<String>,
    },
    /// Scripture output omitted native Bible-reference metadata.
    #[error("scripture presentation has no native Bible-reference metadata")]
    MissingBibleReference,
    /// Scripture output omitted operator-visible verse-range labels.
    #[error("scripture presentation has no labeled content cue")]
    MissingScriptureLabel,
}

pub(super) fn validate_final_presentation(
    presentation: &rv_data::Presentation,
    summary: &PresentationStructureSummary,
    selected_arrangement: Option<&str>,
    expected: &ExpectedPresentationContract,
) -> Result<(), PresentationContractError> {
    let actual_arrangement = selected_arrangement.map(str::to_string);
    if actual_arrangement != expected.arrangement {
        return Err(PresentationContractError::Arrangement {
            expected: expected.arrangement.clone(),
            actual: actual_arrangement,
        });
    }

    if presentation.cues.is_empty() {
        return Err(PresentationContractError::CueCount {
            expected: expected.operator_cues,
            actual: 0,
        });
    }
    let traversal = crate::propresenter::arrangement::checked_operator_cue_indices(presentation)?;
    let count_matches = match expected.operator_cues {
        ExpectedCueCount::NonEmpty => !traversal.is_empty(),
        ExpectedCueCount::Exact(count) => traversal.len() == count,
    };
    if !count_matches {
        return Err(PresentationContractError::CueCount {
            expected: expected.operator_cues,
            actual: traversal.len(),
        });
    }

    let size = inspect_presentation_size(presentation);
    if !size.matches(expected.canvas) {
        return Err(PresentationContractError::Canvas {
            expected: expected.canvas,
            actual: size.describe(),
        });
    }

    validate_macros(presentation, &traversal, &expected.macros)?;
    validate_background_and_scripture(summary, &traversal, expected)?;

    Ok(())
}

fn validate_macros(
    presentation: &rv_data::Presentation,
    traversal: &[usize],
    policy: &ExpectedMacroPolicy,
) -> Result<(), PresentationContractError> {
    let ExpectedMacroPolicy::Exact(regions) = policy else {
        return Ok(());
    };
    let groups = crate::propresenter::arrangement::checked_selected_group_entries(presentation)?;
    let mut expected_by_cue = BTreeMap::<usize, String>::new();
    for region in regions {
        let cue_index = match &region.selector {
            ExpectedMacroSelector::OperatorCue { index } => traversal
                .get(*index)
                .copied()
                .ok_or_else(|| PresentationContractError::MacroSelector {
                    selector: region.selector.clone(),
                })?,
            ExpectedMacroSelector::ArrangementGroup {
                index,
                allowed_names,
            } => {
                let group = groups
                    .as_ref()
                    .and_then(|groups| groups.get(*index))
                    .ok_or_else(|| PresentationContractError::MacroSelector {
                        selector: region.selector.clone(),
                    })?;
                if !allowed_names.iter().any(|name| name == group.name) {
                    return Err(PresentationContractError::MacroGroupName {
                        index: *index,
                        actual: group.name.to_string(),
                        allowed: allowed_names.clone(),
                    });
                }
                group.cue_index
            }
        };
        if expected_by_cue
            .insert(cue_index, region.macro_name.clone())
            .is_some()
        {
            return Err(PresentationContractError::DuplicateMacroTarget { cue_index });
        }
    }

    let mut visited = BTreeSet::new();
    for &cue_index in traversal {
        if !visited.insert(cue_index) {
            continue;
        }
        let actual = presentation.cues[cue_index]
            .actions
            .iter()
            .filter_map(crate::propresenter::macros::macro_action_name)
            .map(str::to_string)
            .collect::<Vec<_>>();
        let expected = expected_by_cue
            .get(&cue_index)
            .cloned()
            .into_iter()
            .collect::<Vec<_>>();
        if actual != expected {
            return Err(PresentationContractError::Macros {
                cue_index,
                expected,
                actual,
            });
        }
    }
    Ok(())
}

fn validate_background_and_scripture(
    summary: &PresentationStructureSummary,
    traversal: &[usize],
    expected: &ExpectedPresentationContract,
) -> Result<(), PresentationContractError> {
    if let Some(expected_background) = &expected.first_background {
        let cue_index = traversal[0];
        let actual = summary.cues[cue_index].background_media.clone();
        if !actual.iter().any(|name| name == expected_background) {
            return Err(PresentationContractError::Background {
                cue_index,
                expected: expected_background.clone(),
                actual,
            });
        }
    }
    if expected.requires_scripture_metadata && summary.bible_reference.is_none() {
        return Err(PresentationContractError::MissingBibleReference);
    }
    if expected.requires_scripture_labels
        && !traversal
            .iter()
            .any(|index| !summary.cues[*index].slide_labels.is_empty())
    {
        return Err(PresentationContractError::MissingScriptureLabel);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::propresenter::generated::rv_data::action;
    use crate::propresenter::inspection::summarize_presentation_structure;
    use crate::workflow::ExpectedMacroPolicy;

    fn one_cue_presentation() -> rv_data::Presentation {
        rv_data::Presentation {
            cues: vec![rv_data::Cue {
                uuid: Some(rv_data::Uuid {
                    string: "CUE-1".to_string(),
                }),
                actions: vec![rv_data::Action {
                    action_type_data: Some(action::ActionTypeData::Slide(action::SlideType {
                        slide: Some(action::slide_type::Slide::Presentation(
                            rv_data::PresentationSlide {
                                base_slide: Some(rv_data::Slide {
                                    size: Some(rv_data::graphics::Size {
                                        width: 1920.0,
                                        height: 1080.0,
                                    }),
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
            ..rv_data::Presentation::default()
        }
    }

    fn contract() -> ExpectedPresentationContract {
        ExpectedPresentationContract {
            canvas: PresentationSize::FULL_HD,
            operator_cues: ExpectedCueCount::NonEmpty,
            arrangement: None,
            first_background: None,
            macros: ExpectedMacroPolicy::Preserve,
            requires_scripture_metadata: false,
            requires_scripture_labels: false,
        }
    }

    #[test]
    fn exact_final_bytes_must_have_operator_cues_and_the_reviewed_canvas() {
        let presentation = one_cue_presentation();
        let summary = summarize_presentation_structure(&presentation);
        validate_final_presentation(&presentation, &summary, None, &contract())
            .expect("valid final presentation");

        let empty = rv_data::Presentation::default();
        let error = validate_final_presentation(
            &empty,
            &summarize_presentation_structure(&empty),
            None,
            &contract(),
        )
        .expect_err("zero-cue artifact must fail");
        assert!(matches!(
            error,
            PresentationContractError::CueCount { actual: 0, .. }
        ));

        let mut wrong_size = presentation;
        let action::ActionTypeData::Slide(slide) = wrong_size.cues[0].actions[0]
            .action_type_data
            .as_mut()
            .expect("slide action")
        else {
            panic!("expected slide action")
        };
        let action::slide_type::Slide::Presentation(slide) =
            slide.slide.as_mut().expect("presentation slide")
        else {
            panic!("expected presentation slide")
        };
        slide.base_slide.as_mut().expect("base slide").size = Some(rv_data::graphics::Size {
            width: 1280.0,
            height: 720.0,
        });
        let error = validate_final_presentation(
            &wrong_size,
            &summarize_presentation_structure(&wrong_size),
            None,
            &contract(),
        )
        .expect_err("wrong canvas must fail");
        assert!(matches!(error, PresentationContractError::Canvas { .. }));
    }
}
