//! Insertion and resolution of presentations required by project configuration.

use super::file_stem;
use crate::project_config::{
    CompiledRequiredPlaylistItem, ProjectConfig, RequiredPlaylistPlacement,
    ResolvedRequiredPresentation,
};
use crate::propresenter::library::LibraryCatalog;
use crate::workflow::library_search::{resolve_exact_library_file, ExactLibraryFileMatch};
use crate::workflow::plan::{
    OutputKey, PlanDisposition, ReadyAction, ResolvedItemPlan, ReviewContext,
};

pub(super) fn ensure_required_playlist_items(
    entries: &mut Vec<ResolvedItemPlan>,
    mappings: &ProjectConfig,
    file_index: Option<&LibraryCatalog>,
    service_name: Option<&str>,
) {
    let mut start = Vec::new();
    let mut end = Vec::new();
    for required in mappings.compiled_required_playlist_items() {
        if !required.applies_to(service_name) {
            continue;
        }
        let target = resolve_exact_library_file(file_index, required.library_file());
        if let ExactLibraryFileMatch::Unique(path) = &target {
            entries.retain(|entry| {
                entry.is_skipped()
                    || entry.file_path().and_then(std::path::Path::to_str) != Some(path)
            });
        }
        let plan = build_required_playlist_item(required, &target, service_name, entries);
        match required.placement() {
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

fn build_required_playlist_item(
    required: &CompiledRequiredPlaylistItem,
    target: &ExactLibraryFileMatch,
    service_name: Option<&str>,
    existing: &[ResolvedItemPlan],
) -> ResolvedItemPlan {
    let position = required_position(required.placement(), existing);
    let presentation = required.presentation_for_service(service_name);
    let kind = match &presentation {
        ResolvedRequiredPresentation::Preserve { kind, .. }
        | ResolvedRequiredPresentation::Restyle { kind, .. } => *kind,
    };
    let (disposition, reason, playlist_name) = match target {
        ExactLibraryFileMatch::Unique(path) => {
            let action = match presentation {
                ResolvedRequiredPresentation::Preserve { arrangement, .. } => {
                    ReadyAction::UseExisting {
                        file_path: path.into(),
                        arrangement,
                    }
                }
                ResolvedRequiredPresentation::Restyle {
                    arrangement,
                    transform,
                    ..
                } => ReadyAction::RestyleExisting {
                    file_path: path.into(),
                    arrangement,
                    transform,
                },
            };
            (
                PlanDisposition::Ready(action),
                format!(
                    "Required playlist item inserted at {}",
                    required_placement_name(required.placement())
                ),
                file_stem(path),
            )
        }
        ExactLibraryFileMatch::Missing => (
            PlanDisposition::NeedsReview(ReviewContext::new(None)),
            format!(
                "Required playlist file not found: {}",
                required.library_file()
            ),
            file_stem(required.library_file()),
        ),
        ExactLibraryFileMatch::Ambiguous => (
            PlanDisposition::NeedsReview(ReviewContext::new(None)),
            format!(
                "Required playlist file is ambiguous: {}",
                required.library_file()
            ),
            file_stem(required.library_file()),
        ),
    };
    ResolvedItemPlan::new(
        OutputKey::required(required.id()),
        position,
        file_stem(required.library_file()),
        playlist_name,
        reason,
        kind,
        Some(required.type_key().to_string()),
        disposition,
    )
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
