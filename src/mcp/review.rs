//! Preview-request normalization.

use crate::planning_center::PlanLookaheadDays;
use crate::workflow::execute::{ItemDecision, MediaDecision, ReviewDecisions};

use super::mcp_err;
use super::schema::PreviewPlaylistArgs;

/// The decisions a preview request carries, as the record written to the
/// plan's `review.toml`. A preview replaces every earlier decision.
pub(super) fn recorded_decisions(args: PreviewPlaylistArgs) -> ReviewDecisions {
    let skipped = args
        .skip_output_keys
        .unwrap_or_default()
        .into_iter()
        .map(|item| ItemDecision {
            skip: true,
            ..ItemDecision::for_item(item)
        });
    let changed = args
        .overrides
        .unwrap_or_default()
        .into_iter()
        .map(|entry| ItemDecision {
            playlist_name: entry.playlist_name,
            slide_type: entry.slide_type,
            action: entry.action,
            ..ItemDecision::for_item(entry.output_key)
        });
    ReviewDecisions {
        playlist_name: args.playlist_name,
        package_mode: args.package_mode.unwrap_or_default(),
        media: args
            .media_assets
            .unwrap_or_default()
            .into_iter()
            .map(|asset| MediaDecision {
                path: asset.path,
                archive_path: asset.archive_path,
            })
            .collect(),
        decisions: skipped.chain(changed).collect(),
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
