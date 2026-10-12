//! The choices a review offers for each open decision.
//!
//! Every option is derived from what the review already found: the files it
//! could not choose between, the action it proposed, and the arrangements the
//! proposed file defines. An option is offered only if applying it to the plan
//! is accepted, so none of them is a dead end the next build would refuse.

use std::path::Path;

use crate::workflow::plan::{ReadyAction, ResolvedItemPlan};
use proflow_propresenter::library::{LibraryArrangement, LibraryCatalog};

use super::decisions::{DecisionChoice, DecisionOption, EntryOverrideAction};
use super::overrides::{apply_override, EntryOverride};
use super::{BuildServiceError, ServiceBuildExecutor};

impl ServiceBuildExecutor<'_> {
    /// Options for every plan that needs review, in plan order.
    pub(super) fn decision_options(
        &self,
        plans: &[ResolvedItemPlan],
    ) -> Result<Vec<Vec<DecisionOption>>, BuildServiceError> {
        let catalog = self.lock_library()?;
        Ok(plans
            .iter()
            .filter(|plan| plan.needs_review())
            .map(|plan| options_for(plan, &catalog))
            .collect())
    }
}

fn options_for(plan: &ResolvedItemPlan, catalog: &LibraryCatalog) -> Vec<DecisionOption> {
    let mut offered = Vec::new();
    for candidate in plan.review_candidates() {
        offered.push(use_file(candidate, None));
    }
    if plan.requires_song_arrangement() {
        if let Some(file) = plan.preview_action().and_then(existing_file) {
            let arrangements = catalog
                .entry_at(file)
                .map_or(&[][..], |entry| entry.arrangements());
            for arrangement in arrangements {
                if let LibraryArrangement::Complete { name } = arrangement {
                    offered.push(DecisionOption {
                        summary: format!("use arrangement '{name}'"),
                        choice: DecisionChoice::Action(EntryOverrideAction::SelectArrangement {
                            arrangement: name.clone(),
                        }),
                    });
                }
            }
        }
    } else if let Some(proposed) = plan.preview_action().and_then(as_proposed) {
        offered.push(proposed);
    }
    offered.retain(|option| is_accepted(plan, option));
    offered.push(DecisionOption {
        summary: "leave it out of the playlist".to_string(),
        choice: DecisionChoice::Skip,
    });
    offered
}

/// The decision that takes the review's own proposal as it stands.
fn as_proposed(action: &ReadyAction) -> Option<DecisionOption> {
    match action {
        ReadyAction::UseExisting {
            file_path,
            arrangement,
        } => Some(use_file(file_path, arrangement.as_deref())),
        ReadyAction::RestyleExisting {
            file_path,
            arrangement,
            ..
        } => arrangement.as_ref().map(|arrangement| DecisionOption {
            summary: format!(
                "restyle {} with arrangement '{arrangement}'",
                file_name(file_path)
            ),
            choice: DecisionChoice::Action(EntryOverrideAction::SelectArrangement {
                arrangement: arrangement.clone(),
            }),
        }),
        ReadyAction::EditDescription { file_path, .. } => Some(DecisionOption {
            summary: format!("render the description into {}", file_name(file_path)),
            choice: DecisionChoice::Action(EntryOverrideAction::EditDescription {
                file_path: file_path.display().to_string(),
                background: None,
            }),
        }),
        ReadyAction::GenerateDescription { .. }
        | ReadyAction::GenerateScripture { .. }
        | ReadyAction::GenerateTitle { .. } => Some(DecisionOption {
            summary: "generate a new presentation as proposed".to_string(),
            choice: DecisionChoice::Action(EntryOverrideAction::GenerateNew { background: None }),
        }),
    }
}

fn use_file(file: &Path, arrangement: Option<&str>) -> DecisionOption {
    DecisionOption {
        summary: arrangement.map_or_else(
            || format!("use {}", file.display()),
            |arrangement| format!("use {} with arrangement '{arrangement}'", file.display()),
        ),
        choice: DecisionChoice::Action(EntryOverrideAction::UseExisting {
            file_path: file.display().to_string(),
            arrangement: arrangement.map(ToString::to_string),
        }),
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

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
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
