//! The choices a review offers for each open decision.
//!
//! Every option is derived from what the review already found: the files it
//! could not choose between, the action it proposed, and the arrangements the
//! proposed file defines. An option is offered only if applying it to the plan
//! is accepted, so none of them is a dead end the next build would refuse.

use std::path::Path;

use crate::workflow::plan::{ReadyAction, ResolvedItemPlan};
use proflow_propresenter::library::{LibraryArrangement, LibraryCatalog};

use super::decisions::{DecisionChoice, DecisionKind, DecisionOption, EntryOverrideAction};
use super::overrides::{apply_override, EntryOverride};
use super::{BuildServiceError, ServiceBuildExecutor};

impl ServiceBuildExecutor<'_> {
    /// The kind of question and the options for every plan that needs
    /// review, in plan order.
    pub(super) fn decision_options(
        &self,
        plans: &[ResolvedItemPlan],
    ) -> Result<Vec<(DecisionKind, Vec<DecisionOption>)>, BuildServiceError> {
        let catalog = self.lock_library()?;
        Ok(plans
            .iter()
            .filter(|plan| plan.needs_review())
            .map(|plan| options_for(plan, &catalog))
            .collect())
    }
}

fn options_for(
    plan: &ResolvedItemPlan,
    catalog: &LibraryCatalog,
) -> (DecisionKind, Vec<DecisionOption>) {
    let action = |action| DecisionOption::new(DecisionChoice::Action(action));
    let (kind, mut offered) = if !plan.review_candidates().is_empty() {
        let files = plan.review_candidates().iter().map(|file| {
            action(EntryOverrideAction::UseExisting {
                file_path: file.display().to_string(),
                arrangement: None,
            })
        });
        (DecisionKind::WhichFile, files.collect())
    } else if plan.requires_song_arrangement() {
        let arrangements = plan
            .preview_action()
            .and_then(existing_file)
            .and_then(|file| catalog.entry_at(file))
            .map_or(&[][..], |entry| entry.arrangements());
        let complete = arrangements
            .iter()
            .filter_map(|arrangement| match arrangement {
                LibraryArrangement::Complete { name } => {
                    Some(action(EntryOverrideAction::SelectArrangement {
                        arrangement: name.clone(),
                    }))
                }
                LibraryArrangement::Incomplete { .. } => None,
            });
        (DecisionKind::WhichArrangement, complete.collect())
    } else if plan.requires_song_selection() {
        // A possible match is a file to confirm, like any other candidate.
        let candidate = plan.preview_action().and_then(as_proposed).map(action);
        (DecisionKind::WhichFile, candidate.into_iter().collect())
    } else {
        plan.preview_action().and_then(as_proposed).map_or_else(
            || (DecisionKind::Unresolved, Vec::new()),
            |proposed| (DecisionKind::ConfirmProposal, vec![action(proposed)]),
        )
    };
    offered.retain(|option| is_accepted(plan, option));
    offered.push(DecisionOption::new(DecisionChoice::Skip));
    (kind, offered)
}

/// The decision that takes the review's own proposal as it stands.
fn as_proposed(action: &ReadyAction) -> Option<EntryOverrideAction> {
    match action {
        ReadyAction::UseExisting {
            file_path,
            arrangement,
        } => Some(EntryOverrideAction::UseExisting {
            file_path: file_path.display().to_string(),
            arrangement: arrangement.clone(),
        }),
        // Selecting the proposed arrangement keeps the restyle; a restyle
        // with no arrangement has no decision that restates it.
        ReadyAction::RestyleExisting { arrangement, .. } => {
            arrangement
                .as_ref()
                .map(|arrangement| EntryOverrideAction::SelectArrangement {
                    arrangement: arrangement.clone(),
                })
        }
        ReadyAction::EditDescription { file_path, .. } => {
            Some(EntryOverrideAction::EditDescription {
                file_path: file_path.display().to_string(),
                background: None,
            })
        }
        ReadyAction::GenerateDescription { .. }
        | ReadyAction::GenerateScripture { .. }
        | ReadyAction::GenerateTitle { .. } => {
            Some(EntryOverrideAction::GenerateNew { background: None })
        }
    }
}

fn existing_file(action: &ReadyAction) -> Option<&Path> {
    match action {
        ReadyAction::UseExisting { file_path, .. }
        | ReadyAction::RestyleExisting { file_path, .. }
        | ReadyAction::EditDescription { file_path, .. } => Some(file_path),
        ReadyAction::GenerateDescription { .. }
        | ReadyAction::GenerateScripture { .. }
        | ReadyAction::GenerateTitle { .. } => None,
    }
}

/// Whether the plan takes this option as a decision.
fn is_accepted(plan: &ResolvedItemPlan, option: &DecisionOption) -> bool {
    let DecisionChoice::Action(action) = &option.choice else {
        return true;
    };
    // Options carry no background, so resolving one needs no configuration.
    let Ok(action) = super::decisions::resolve_without_background(action.clone()) else {
        return false;
    };
    apply_override(
        plan,
        Some(&EntryOverride {
            output_key: plan.output_key.to_string(),
            playlist_name: None,
            slide_type: None,
            action: Some(action),
        }),
    )
    .is_ok()
}
