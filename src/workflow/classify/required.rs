//! Insertion and resolution of presentations required by project configuration.

use super::file_stem;
use crate::project_config::{
    ExistingSource, PresentationPolicy, ProjectConfig, RequiredPlaylistItemConfig,
    RequiredPlaylistPlacement,
};
use crate::propresenter::library::LibraryCatalog;
use crate::workflow::library_search::{resolve_exact_library_file, ExactLibraryFileMatch};
use crate::workflow::plan::{
    ItemKind, OutputKey, PlanDisposition, ReadyAction, ResolvedItemPlan, ReviewContext,
};

pub(super) fn ensure_required_playlist_items(
    entries: &mut Vec<ResolvedItemPlan>,
    mappings: &ProjectConfig,
    file_index: Option<&LibraryCatalog>,
    service_name: Option<&str>,
) {
    let mut start = Vec::new();
    let mut end = Vec::new();
    for required in mappings.required_playlist_items() {
        if !required_playlist_item_applies(required, mappings, service_name) {
            continue;
        }
        let target = resolve_exact_library_file(file_index, &required.library_file);
        if let ExactLibraryFileMatch::Unique(path) = &target {
            entries.retain(|entry| {
                entry.is_skipped()
                    || entry.file_path().and_then(std::path::Path::to_str) != Some(path)
            });
        }
        let plan = build_required_playlist_item(required, &target, mappings, service_name, entries);
        match required.placement {
            RequiredPlaylistPlacement::Start => start.push(plan),
            RequiredPlaylistPlacement::End => end.push(plan),
        }
    }

    if start.is_empty() && end.is_empty() {
        return;
    }
    let mut combined = Vec::with_capacity(start.len() + entries.len() + end.len());
    combined.extend(start);
    combined.append(entries);
    combined.extend(end);
    *entries = combined;
}

fn required_playlist_item_applies(
    required: &RequiredPlaylistItemConfig,
    mappings: &ProjectConfig,
    service_name: Option<&str>,
) -> bool {
    let Some(group_name) = required.service_group.as_deref() else {
        return true;
    };
    let Some(service_name) = service_name else {
        return false;
    };
    mappings
        .service_groups()
        .get(group_name)
        .is_some_and(|group| {
            group
                .service_types
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(service_name))
        })
}

fn build_required_playlist_item(
    required: &RequiredPlaylistItemConfig,
    target: &ExactLibraryFileMatch,
    mappings: &ProjectConfig,
    service_name: Option<&str>,
    existing: &[ResolvedItemPlan],
) -> ResolvedItemPlan {
    let position = required_position(required.placement, existing);
    let Some(policy) = mappings.presentation_policy(&required.use_type) else {
        return ResolvedItemPlan {
            output_key: OutputKey::required(&required.id),
            position,
            pco_title: file_stem(&required.library_file),
            playlist_name: file_stem(&required.library_file),
            reason: format!("Unknown presentation type '{}'", required.use_type),
            item_kind: ItemKind::Other,
            item_type: Some(required.use_type.clone()),
            disposition: PlanDisposition::NeedsReview(ReviewContext::new(None)),
        };
    };

    let (kind, arrangement, transform) = match policy {
        PresentationPolicy::PreserveExisting {
            kind,
            source: ExistingSource::Static,
            arrangement,
        } => (*kind, arrangement.for_service(service_name), None),
        PresentationPolicy::RestyleExisting {
            kind,
            source: ExistingSource::Static,
            arrangement,
            transform,
        } => (
            *kind,
            arrangement.for_service(service_name),
            Some(transform.for_service(service_name)),
        ),
        _ => {
            return ResolvedItemPlan {
                output_key: OutputKey::required(&required.id),
                position,
                pco_title: file_stem(&required.library_file),
                playlist_name: file_stem(&required.library_file),
                reason: format!(
                    "Required playlist type '{}' is not a checked static existing-presentation policy",
                    required.use_type
                ),
                item_kind: policy.kind(),
                item_type: Some(required.use_type.clone()),
                disposition: PlanDisposition::NeedsReview(ReviewContext::new(None)),
            };
        }
    };
    let (disposition, reason, playlist_name) = match target {
        ExactLibraryFileMatch::Unique(path) => {
            let action = match transform {
                None => ReadyAction::UseExisting {
                    file_path: path.into(),
                    arrangement,
                },
                Some(transform) => ReadyAction::RestyleExisting {
                    file_path: path.into(),
                    arrangement,
                    transform,
                },
            };
            (
                PlanDisposition::Ready(action),
                format!(
                    "Required playlist item inserted at {}",
                    required_placement_name(required.placement)
                ),
                file_stem(path),
            )
        }
        ExactLibraryFileMatch::Missing => (
            PlanDisposition::NeedsReview(ReviewContext::new(None)),
            format!(
                "Required playlist file not found: {}",
                required.library_file
            ),
            file_stem(&required.library_file),
        ),
        ExactLibraryFileMatch::Ambiguous => (
            PlanDisposition::NeedsReview(ReviewContext::new(None)),
            format!(
                "Required playlist file is ambiguous: {}",
                required.library_file
            ),
            file_stem(&required.library_file),
        ),
    };
    ResolvedItemPlan {
        output_key: OutputKey::required(&required.id),
        position,
        pco_title: file_stem(&required.library_file),
        playlist_name,
        reason,
        item_kind: kind,
        item_type: Some(required.use_type.clone()),
        disposition,
    }
}

fn required_position(placement: RequiredPlaylistPlacement, existing: &[ResolvedItemPlan]) -> usize {
    match placement {
        RequiredPlaylistPlacement::Start => existing
            .first()
            .map_or(0, |entry| entry.position.saturating_sub(1)),
        RequiredPlaylistPlacement::End => existing
            .last()
            .map_or(0, |entry| entry.position.saturating_add(1)),
    }
}

const fn required_placement_name(placement: RequiredPlaylistPlacement) -> &'static str {
    match placement {
        RequiredPlaylistPlacement::Start => "start",
        RequiredPlaylistPlacement::End => "end",
    }
}
