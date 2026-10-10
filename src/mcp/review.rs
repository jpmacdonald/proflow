//! Reviewed-preview request normalization and one-time revision state.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::paths::expand_user_path;
use crate::planning_center::PlanLookaheadDays;
use crate::project_config::{BackgroundId, ProjectConfig};
use crate::workflow::execute::{
    EntryOverride as WorkflowEntryOverride, OverrideAction, PreparedBuildRequest,
};
use crate::workflow::plan::ResolvedBackground;
use proflow_propresenter::playlist::PlaylistMediaAsset;

use super::mcp_err;
use super::schema::{EntryOverride, EntryOverrideAction, PlaylistMediaAssetArg};

/// One-time token naming a sealed preview held by this server process.
///
/// A revision is minted when a fully resolved preview is stored and is
/// consumed by exactly one build attempt. It has no meaning to another
/// process or after a restart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(transparent)]
pub(super) struct PreviewRevision(uuid::Uuid);

impl PreviewRevision {
    fn fresh() -> Self {
        Self(uuid::Uuid::new_v4())
    }

    /// Parse a revision the caller echoed back from `preview_playlist`.
    pub(super) fn parse(value: &str) -> Result<Self, ReviewedPlanError> {
        uuid::Uuid::parse_str(value)
            .map(Self)
            .map_err(|_| ReviewedPlanError::MalformedRevision {
                value: value.to_string(),
            })
    }
}

pub(super) struct PreparedPlanSnapshot {
    pub(super) revision: PreviewRevision,
    pub(super) prepared: PreparedBuildRequest,
}

/// Outcome of storing one preview for a plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PreviewReplacement {
    /// Revision of the newly stored executable preview, if it was executable.
    pub(super) revision: Option<PreviewRevision>,
    /// Revision of the earlier executable preview this one invalidated.
    pub(super) superseded: Option<PreviewRevision>,
}

/// Replace the executable snapshot for one plan with its latest successful
/// preview state. An unresolved preview invalidates any older prepared state.
pub(super) fn replace_prepared_snapshot(
    snapshots: &mut HashMap<String, PreparedPlanSnapshot>,
    plan_id: String,
    prepared: Option<PreparedBuildRequest>,
) -> PreviewReplacement {
    let superseded = snapshots.remove(&plan_id).map(|old| old.revision);
    let revision = prepared.map(|prepared| {
        let revision = PreviewRevision::fresh();
        snapshots.insert(plan_id, PreparedPlanSnapshot { revision, prepared });
        revision
    });
    PreviewReplacement {
        revision,
        superseded,
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub(super) enum ReviewedPlanError {
    #[error("plan '{plan_id}' has no unconsumed reviewed preview in this server process")]
    Missing { plan_id: String },
    #[error("preview_revision for plan '{plan_id}' is stale or invalid")]
    RevisionMismatch { plan_id: String },
    #[error("preview_revision '{value}' is not a revision issued by preview_playlist")]
    MalformedRevision { value: String },
    #[error(
        "service_name must match Planning Center metadata '{actual}' for the reviewed preview"
    )]
    ServiceNameMismatch { actual: String },
}

pub(super) fn parse_media_assets(
    args: Option<Vec<PlaylistMediaAssetArg>>,
) -> Vec<PlaylistMediaAsset> {
    args.unwrap_or_default()
        .into_iter()
        .map(|asset| PlaylistMediaAsset {
            source_path: PathBuf::from(asset.path),
            archive_path: asset.archive_path,
        })
        .collect()
}

pub(super) fn resolve_entry_override(
    mappings: &ProjectConfig,
    entry: EntryOverride,
) -> Result<WorkflowEntryOverride, rmcp::ErrorData> {
    let action = entry
        .action
        .map(|action| resolve_override_action(mappings, action))
        .transpose()?;

    Ok(WorkflowEntryOverride {
        output_key: entry.output_key,
        playlist_name: entry.playlist_name,
        slide_type: entry.slide_type,
        action,
    })
}

fn resolve_override_action(
    mappings: &ProjectConfig,
    action: EntryOverrideAction,
) -> Result<OverrideAction, rmcp::ErrorData> {
    match action {
        EntryOverrideAction::UseExisting {
            file_path,
            arrangement,
        } => Ok(OverrideAction::UseExisting {
            file_path: operator_path(file_path)?,
            arrangement: optional_operator_identity("arrangement", arrangement)?,
        }),
        EntryOverrideAction::GenerateNew { background } => Ok(OverrideAction::GenerateNew {
            background: background
                .map(|id| resolve_background_override(mappings, id))
                .transpose()?,
        }),
        EntryOverrideAction::EditDescription {
            file_path,
            background,
        } => Ok(OverrideAction::EditDescription {
            file_path: operator_path(file_path)?,
            background: background
                .map(|id| resolve_background_override(mappings, id))
                .transpose()?,
        }),
        EntryOverrideAction::SetBackground { background } => Ok(OverrideAction::SetBackground {
            background: resolve_background_override(mappings, background)?,
        }),
        EntryOverrideAction::SelectArrangement { arrangement } => {
            Ok(OverrideAction::SelectArrangement {
                arrangement: required_operator_identity("arrangement", arrangement)?,
            })
        }
    }
}

fn resolve_background_override(
    mappings: &ProjectConfig,
    id: BackgroundId,
) -> Result<ResolvedBackground, rmcp::ErrorData> {
    let file = mappings.backgrounds().get(&id).cloned().ok_or_else(|| {
        let mut available: Vec<_> = mappings
            .backgrounds()
            .keys()
            .map(std::string::ToString::to_string)
            .collect();
        available.sort();
        mcp_err(format!(
            "unknown background id '{id}'; configured backgrounds: {}",
            available.join(", ")
        ))
    })?;
    Ok(ResolvedBackground::new(id, file))
}

fn operator_path(value: String) -> Result<PathBuf, rmcp::ErrorData> {
    let value = required_operator_identity("file_path", value)?;
    Ok(expand_user_path(value))
}

fn optional_operator_identity(
    field: &'static str,
    value: Option<String>,
) -> Result<Option<String>, rmcp::ErrorData> {
    value
        .map(|value| required_operator_identity(field, value))
        .transpose()
}

fn required_operator_identity(
    field: &'static str,
    value: String,
) -> Result<String, rmcp::ErrorData> {
    if value.is_empty() || value.trim() != value || value.chars().any(char::is_control) {
        Err(mcp_err(format!(
            "override {field} must be non-empty, unpadded, and contain no control characters"
        )))
    } else {
        Ok(value)
    }
}

pub(super) fn bounded_usize(
    name: &str,
    value: Option<usize>,
    default: usize,
    maximum: usize,
) -> Result<usize, rmcp::ErrorData> {
    let value = value.unwrap_or(default);
    if (1..=maximum).contains(&value) {
        Ok(value)
    } else {
        Err(mcp_err(format!(
            "{name} must be between 1 and {maximum}, got {value}"
        )))
    }
}

pub(super) fn bounded_days(
    value: Option<i64>,
    default: PlanLookaheadDays,
) -> Result<PlanLookaheadDays, rmcp::ErrorData> {
    value
        .map_or(Ok(default), PlanLookaheadDays::new)
        .map_err(|error| mcp_err(error.to_string()))
}

pub(super) fn consume_reviewed_plan(
    snapshots: &mut HashMap<String, PreparedPlanSnapshot>,
    plan_id: &str,
    preview_revision: PreviewRevision,
    supplied_service_name: Option<&str>,
) -> Result<PreparedPlanSnapshot, ReviewedPlanError> {
    let reviewed = snapshots
        .get(plan_id)
        .ok_or_else(|| ReviewedPlanError::Missing {
            plan_id: plan_id.to_string(),
        })?;
    if reviewed.revision != preview_revision {
        return Err(ReviewedPlanError::RevisionMismatch {
            plan_id: plan_id.to_string(),
        });
    }
    if let Some(supplied) = supplied_service_name {
        if supplied != reviewed.prepared.service_name() {
            return Err(ReviewedPlanError::ServiceNameMismatch {
                actual: reviewed.prepared.service_name().to_string(),
            });
        }
    }

    snapshots
        .remove(plan_id)
        .ok_or_else(|| ReviewedPlanError::Missing {
            plan_id: plan_id.to_string(),
        })
}
