//! Reviewed-preview request normalization and one-time revision state.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::planning_center::PlanLookaheadDays;
use crate::project_config::ProjectConfig;
use crate::workflow::execute::{
    resolve_override_action, EntryOverride as WorkflowEntryOverride, PreparedBuildRequest,
};
use proflow_propresenter::playlist::{
    PlaylistExportIntent, PlaylistExportMode, PlaylistMediaAsset,
};

use super::mcp_err;
use super::schema::{EntryOverride, PlaylistMediaAssetArg};

/// One-time token naming a prepared build this server process previewed.
///
/// A revision is minted when a fully resolved preview is stored and is
/// consumed by exactly one build attempt. The prepared build itself is in its
/// build directory on disk; the revision, and the record of which rendered
/// stage it showed, exist only in this process and are gone after a restart.
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

/// Build the playlist export intent a preview request asks for.
pub(super) fn playlist_export_intent(
    mode: PlaylistExportMode,
    media_assets: Option<Vec<PlaylistMediaAssetArg>>,
) -> Result<PlaylistExportIntent, rmcp::ErrorData> {
    crate::workflow::execute::playlist_export_intent(
        mode,
        media_assets
            .unwrap_or_default()
            .into_iter()
            .map(|asset| PlaylistMediaAsset {
                source_path: PathBuf::from(asset.path),
                archive_path: asset.archive_path,
            })
            .collect(),
    )
    .map_err(|error| rmcp::ErrorData::invalid_params(error.to_string(), None))
}

pub(super) fn resolve_entry_override(
    mappings: &ProjectConfig,
    entry: EntryOverride,
) -> Result<WorkflowEntryOverride, rmcp::ErrorData> {
    Ok(WorkflowEntryOverride {
        output_key: entry.output_key,
        playlist_name: entry.playlist_name,
        slide_type: entry.slide_type,
        action: entry
            .action
            .map(|action| resolve_override_action(mappings, action))
            .transpose()
            .map_err(|error| mcp_err(error.to_string()))?,
    })
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
