//! Reviewed-preview request normalization and one-time revision state.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::paths::expand_user_path;
use crate::project_config::{BackgroundId, ProjectConfig};
use crate::propresenter::playlist::PlaylistMediaAsset;
use crate::workflow::execute::{
    EntryOverride as WorkflowEntryOverride, OverrideAction, PreparedBuildRequest,
};
use crate::workflow::plan::ResolvedBackground;

use super::mcp_err;
use super::schema::{EntryOverride, EntryOverrideAction, PlaylistMediaAssetArg};

pub(super) const DEFAULT_DAYS_AHEAD: i64 = 30;
const MAX_DAYS_AHEAD: i64 = 365;

pub(super) struct PreparedPlanSnapshot {
    pub(super) revision: String,
    pub(super) prepared: PreparedBuildRequest,
}

/// Replace the executable snapshot for one plan with its latest successful
/// preview state. An unresolved preview invalidates any older prepared state.
pub(super) fn replace_prepared_snapshot(
    snapshots: &mut HashMap<String, PreparedPlanSnapshot>,
    plan_id: String,
    prepared: Option<PreparedBuildRequest>,
) -> Option<String> {
    if let Some(prepared) = prepared {
        let revision = uuid::Uuid::new_v4().to_string();
        snapshots.insert(
            plan_id,
            PreparedPlanSnapshot {
                revision: revision.clone(),
                prepared,
            },
        );
        Some(revision)
    } else {
        snapshots.remove(&plan_id);
        None
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub(super) enum ReviewedPlanError {
    #[error("plan '{plan_id}' has no unconsumed reviewed preview in this server process")]
    Missing { plan_id: String },
    #[error("preview_revision for plan '{plan_id}' is stale or invalid")]
    RevisionMismatch { plan_id: String },
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

pub(super) fn bounded_days(value: Option<i64>, default: i64) -> Result<i64, rmcp::ErrorData> {
    let value = value.unwrap_or(default);
    if (1..=MAX_DAYS_AHEAD).contains(&value) {
        Ok(value)
    } else {
        Err(mcp_err(format!(
            "days_ahead must be between 1 and {MAX_DAYS_AHEAD}, got {value}"
        )))
    }
}

pub(super) fn consume_reviewed_plan(
    snapshots: &mut HashMap<String, PreparedPlanSnapshot>,
    plan_id: &str,
    preview_revision: &str,
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
