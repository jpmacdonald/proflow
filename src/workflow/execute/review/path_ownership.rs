//! Physical path ownership checks for reviewed build inputs and outputs.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::workflow::approval::{plan_source_paths, OutputReviewError, PhysicalPath};
use crate::workflow::plan::{ReadyAction, ResolvedItemPlan};

use super::super::BuildServiceError;

#[derive(Debug)]
pub(super) enum ReviewedOutputOwner {
    Playlist,
    Plan(String),
}

impl ReviewedOutputOwner {
    fn label(&self) -> String {
        match self {
            Self::Playlist => "playlist".to_string(),
            Self::Plan(output_key) => format!("plan '{output_key}'"),
        }
    }

    fn is_plan(&self, output_key: &str) -> bool {
        matches!(self, Self::Plan(candidate) if candidate == output_key)
    }
}

#[derive(Debug)]
pub(super) struct PlannedOutputTarget {
    owner: ReviewedOutputOwner,
    pub(super) physical: PhysicalPath,
}

impl PlannedOutputTarget {
    pub(super) fn resolve(
        owner: ReviewedOutputOwner,
        path: &Path,
    ) -> Result<Self, OutputReviewError> {
        Ok(Self {
            owner,
            physical: PhysicalPath::resolve_output(path)?,
        })
    }
}

pub(super) fn validate_reviewed_path_ownership(
    plans: &[ResolvedItemPlan],
    project_data_root: &Path,
    additional_sources: &[PathBuf],
    outputs: &[PlannedOutputTarget],
) -> Result<(), BuildServiceError> {
    let mut unique_outputs = BTreeMap::new();
    for output in outputs {
        if let Some(first) = unique_outputs.insert(output.physical.clone(), output) {
            return Err(OutputReviewError::DuplicateTarget {
                path: output.physical.as_path().to_path_buf(),
                first: first.owner.label(),
                second: output.owner.label(),
            }
            .into());
        }
    }

    let mut presentation_sources = BTreeSet::new();
    for plan in plans {
        let (path, allows_own_write) = match plan.ready_action() {
            Some(ReadyAction::UseExisting { file_path, .. }) => (file_path, false),
            Some(
                ReadyAction::RestyleExisting { file_path, .. }
                | ReadyAction::EditDescription { file_path, .. },
            ) => (file_path, true),
            Some(
                ReadyAction::GenerateDescription { .. }
                | ReadyAction::GenerateScripture { .. }
                | ReadyAction::GenerateTitle { .. },
            )
            | None => continue,
        };
        let source = PhysicalPath::resolve(path)?;
        presentation_sources.insert(source.clone());
        for output in outputs.iter().filter(|output| output.physical == source) {
            if allows_own_write && output.owner.is_plan(plan.output_key.as_str()) {
                continue;
            }
            return Err(OutputReviewError::SourceOutputOverlap {
                path: source.as_path().to_path_buf(),
                input: format!("plan '{}'", plan.output_key),
                output: output.owner.label(),
            }
            .into());
        }
    }

    for source_path in plan_source_paths(plans, project_data_root)? {
        let source = PhysicalPath::resolve(&source_path)?;
        if presentation_sources.contains(&source) {
            continue;
        }
        reject_source_output_overlap("plan data", &source, outputs)?;
    }
    for source_path in additional_sources {
        let source = PhysicalPath::resolve(source_path)?;
        reject_source_output_overlap("additional reviewed input", &source, outputs)?;
    }
    Ok(())
}

fn reject_source_output_overlap(
    source_label: &str,
    source: &PhysicalPath,
    outputs: &[PlannedOutputTarget],
) -> Result<(), OutputReviewError> {
    if let Some(output) = outputs.iter().find(|output| output.physical == *source) {
        return Err(OutputReviewError::SourceOutputOverlap {
            path: source.as_path().to_path_buf(),
            input: source_label.to_string(),
            output: output.owner.label(),
        });
    }
    Ok(())
}
