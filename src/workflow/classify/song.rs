//! Classification for song presentation types and arrangement selection.

use super::file_stem;
use crate::planning_center::types::Item;
use crate::propresenter::library::{LibraryArrangement, LibraryCatalog};
use crate::workflow::classify_matching::strip_title_prefix;
use crate::workflow::library_search::{
    resolve_exact_library_file, resolve_song_library_match, strip_hymn_number,
    ExactLibraryFileMatch, SongLibraryMatch,
};
use crate::workflow::plan::{
    ExistingTransform, ItemKind, OutputKey, PlanDisposition, ReadyAction, ResolvedItemPlan,
    ReviewContext,
};

pub(super) enum SongPolicy {
    Review,
    PreserveExisting {
        arrangement: Option<String>,
    },
    RestyleExisting {
        arrangement: Option<String>,
        transform: ExistingTransform,
    },
}

#[derive(Debug)]
enum RequestedArrangement {
    Configured(String),
    PlanningCenter(String),
}

impl RequestedArrangement {
    fn name(&self) -> &str {
        match self {
            Self::Configured(name) | Self::PlanningCenter(name) => name,
        }
    }

    const fn permits_native_default_fallback(&self) -> bool {
        matches!(self, Self::PlanningCenter(_))
    }
}

pub(super) fn build_song_plan(
    output_key: OutputKey,
    type_key: &str,
    policy: &SongPolicy,
    item: &Item,
    target_library_file: Option<&str>,
    file_index: Option<&LibraryCatalog>,
) -> ResolvedItemPlan {
    let song_title = item
        .song
        .as_ref()
        .map_or(item.title.as_str(), |s| s.title.as_str());
    let stripped = strip_title_prefix(&item.title);
    let bare_title = strip_hymn_number(song_title);
    let configured_arrangement = match policy {
        SongPolicy::Review => None,
        SongPolicy::PreserveExisting { arrangement }
        | SongPolicy::RestyleExisting { arrangement, .. } => arrangement.as_ref(),
    };
    let arrangement = configured_arrangement.map_or_else(
        || {
            item.song.as_ref().and_then(|song| {
                let arrangement = song.arrangement.as_deref()?.trim();
                (!arrangement.is_empty())
                    .then(|| RequestedArrangement::PlanningCenter(arrangement.to_string()))
            })
        },
        |arrangement| Some(RequestedArrangement::Configured(arrangement.clone())),
    );
    let restyle = match policy {
        SongPolicy::RestyleExisting { transform, .. } => Some(transform),
        SongPolicy::Review | SongPolicy::PreserveExisting { .. } => None,
    };

    let explicit_target_match =
        target_library_file.map(|name| (name, resolve_exact_library_file(file_index, name)));
    let song_match = match &explicit_target_match {
        Some((_, ExactLibraryFileMatch::Unique(path))) => SongLibraryMatch::Resolved(path.clone()),
        Some((_, ExactLibraryFileMatch::Missing | ExactLibraryFileMatch::Ambiguous)) => {
            SongLibraryMatch::Missing
        }
        None => {
            resolve_song_library_match(file_index, song_title, &item.title, &stripped, &bare_title)
        }
    };
    let (disposition, reason) = match policy {
        SongPolicy::Review => (
            PlanDisposition::NeedsReview(ReviewContext::new(proposed_existing_action(
                &song_match,
                None,
                restyle,
            ))),
            "Configured to require review".to_string(),
        ),
        SongPolicy::PreserveExisting { .. } | SongPolicy::RestyleExisting { .. } => {
            use_existing_song_action(
                explicit_target_match.as_ref(),
                &song_match,
                arrangement.as_ref(),
                restyle,
                file_index,
                target_library_file,
            )
        }
    };

    let playlist_name = match &song_match {
        SongLibraryMatch::Resolved(path) | SongLibraryMatch::Candidate(path) => file_stem(path),
        SongLibraryMatch::Missing => song_title.to_string(),
    };

    ResolvedItemPlan::new(
        output_key,
        item.position,
        item.title.clone(),
        playlist_name,
        reason,
        ItemKind::Song,
        Some(type_key.to_string()),
        disposition,
    )
}

fn use_existing_song_action(
    explicit_target_match: Option<&(&str, ExactLibraryFileMatch)>,
    song_match: &SongLibraryMatch,
    arrangement: Option<&RequestedArrangement>,
    restyle: Option<&ExistingTransform>,
    file_index: Option<&LibraryCatalog>,
    target_library_file: Option<&str>,
) -> (PlanDisposition, String) {
    if let Some((target, ExactLibraryFileMatch::Ambiguous)) = explicit_target_match {
        return (
            PlanDisposition::NeedsReview(ReviewContext::new(None)),
            format!("Configured existing song target is ambiguous: {target}"),
        );
    }

    match song_match {
        SongLibraryMatch::Resolved(path) => {
            resolved_song_action(path, arrangement, restyle, file_index)
        }
        SongLibraryMatch::Candidate(_) => (
            PlanDisposition::NeedsReview(ReviewContext::new(proposed_existing_action(
                song_match,
                arrangement,
                restyle,
            ))),
            "Possible library match".to_string(),
        ),
        SongLibraryMatch::Missing => (
            PlanDisposition::NeedsReview(ReviewContext::new(None)),
            target_library_file.map_or_else(
                || "No song library match".to_string(),
                |target| format!("Configured existing song not found: {target}"),
            ),
        ),
    }
}

fn resolved_song_action(
    path: &str,
    arrangement: Option<&RequestedArrangement>,
    restyle: Option<&ExistingTransform>,
    file_index: Option<&LibraryCatalog>,
) -> (PlanDisposition, String) {
    let Some(requested) = arrangement else {
        return match resolve_unrequested_arrangement(file_index, path) {
            Ok(Some(default)) => (
                PlanDisposition::Ready(song_action(path, Some(default.clone()), restyle)),
                format!(
                    "{}; using native arrangement '{default}'",
                    action_reason(restyle, "Library match")
                ),
            ),
            Ok(None) => (
                PlanDisposition::Ready(song_action(path, None, restyle)),
                action_reason(restyle, "Library match"),
            ),
            Err(reason) => (
                PlanDisposition::NeedsReview(ReviewContext::new(Some(song_action(
                    path, None, restyle,
                )))),
                reason,
            ),
        };
    };

    match resolve_song_arrangement(file_index, path, requested) {
        Ok(SongArrangementResolution::Selected {
            canonical_name,
            used_default_fallback,
        }) => (
            PlanDisposition::Ready(song_action(path, Some(canonical_name.clone()), restyle)),
            if used_default_fallback {
                format!(
                    "{}; requested Planning Center arrangement '{}' is unavailable, using native arrangement '{canonical_name}'",
                    action_reason(restyle, "Library match"),
                    requested.name()
                )
            } else {
                action_reason(restyle, "Library match")
            },
        ),
        Ok(SongArrangementResolution::NoSelection) => (
            PlanDisposition::Ready(song_action(path, None, restyle)),
            action_reason(restyle, "Library match"),
        ),
        Err(review_reason) => (
            PlanDisposition::NeedsReview(ReviewContext::new(Some(song_action(
                path,
                Some(requested.name().to_string()),
                restyle,
            )))),
            review_reason,
        ),
    }
}

fn proposed_existing_action(
    song_match: &SongLibraryMatch,
    arrangement: Option<&RequestedArrangement>,
    restyle: Option<&ExistingTransform>,
) -> Option<ReadyAction> {
    match song_match {
        SongLibraryMatch::Resolved(path) | SongLibraryMatch::Candidate(path) => Some(song_action(
            path,
            arrangement.map(|requested| requested.name().to_string()),
            restyle,
        )),
        SongLibraryMatch::Missing => None,
    }
}

fn song_action(
    path: &str,
    arrangement: Option<String>,
    restyle: Option<&ExistingTransform>,
) -> ReadyAction {
    match restyle {
        None => ReadyAction::UseExisting {
            file_path: path.into(),
            arrangement,
        },
        Some(transform) => ReadyAction::RestyleExisting {
            file_path: path.into(),
            arrangement,
            transform: transform.clone(),
        },
    }
}

fn action_reason(restyle: Option<&ExistingTransform>, base: &str) -> String {
    restyle.map_or_else(
        || base.to_string(),
        |transform| {
            transform.replacement_background().map_or_else(
                || format!("{base}; applying existing presentation transform"),
                |background| format!("{base}; managed background '{}'", background.id()),
            )
        },
    )
}

const PCO_DEFAULT_ARRANGEMENT: &str = "Default Arrangement";
const NATIVE_DEFAULT_ARRANGEMENT: &str = "Default";

#[derive(Debug, PartialEq, Eq)]
enum SongArrangementResolution {
    Selected {
        canonical_name: String,
        used_default_fallback: bool,
    },
    NoSelection,
}

fn resolve_song_arrangement(
    file_index: Option<&LibraryCatalog>,
    presentation_path: &str,
    requested: &RequestedArrangement,
) -> Result<SongArrangementResolution, String> {
    let requested_name = requested.name();
    let Some(entry) = file_index.and_then(|index| {
        index
            .entries()
            .iter()
            .find(|entry| entry.full_path().to_string_lossy() == presentation_path)
    }) else {
        return Err(format!(
            "Could not verify arrangement '{requested_name}' because the resolved library file is not indexed"
        ));
    };

    let exact_matches = entry
        .arrangements()
        .iter()
        .filter(|arrangement| arrangement.name().eq_ignore_ascii_case(requested_name))
        .collect::<Vec<_>>();
    if exact_matches.is_empty()
        && requested_name.eq_ignore_ascii_case(PCO_DEFAULT_ARRANGEMENT)
        && requested.permits_native_default_fallback()
        && entry.arrangements().is_empty()
    {
        return Ok(SongArrangementResolution::NoSelection);
    }
    let used_default_fallback =
        exact_matches.is_empty() && requested.permits_native_default_fallback();
    let matches = if used_default_fallback {
        entry
            .arrangements()
            .iter()
            .filter(|arrangement| {
                arrangement
                    .name()
                    .eq_ignore_ascii_case(NATIVE_DEFAULT_ARRANGEMENT)
            })
            .collect::<Vec<_>>()
    } else {
        exact_matches
    };
    let available = arrangement_names(entry.arrangements());

    match matches.as_slice() {
        [LibraryArrangement::Complete { name }] => Ok(SongArrangementResolution::Selected {
            canonical_name: name.clone(),
            used_default_fallback,
        }),
        [LibraryArrangement::Incomplete { .. }] => Err(format!(
            "{} in '{}' has a missing or invalid UUID; available arrangements: {available}",
            arrangement_candidate_description(requested_name, used_default_fallback),
            entry.file_name()
        )),
        [] => Err(format!(
            "Arrangement '{requested_name}' is unavailable in '{}'; available arrangements: {available}",
            entry.file_name()
        )),
        _ => Err(format!(
            "{} is ambiguous in '{}'; available arrangements: {available}",
            arrangement_candidate_description(requested_name, used_default_fallback),
            entry.file_name()
        )),
    }
}

fn arrangement_candidate_description(requested: &str, used_default_fallback: bool) -> String {
    if used_default_fallback {
        format!(
            "Arrangement '{requested}' is unavailable and fallback arrangement '{NATIVE_DEFAULT_ARRANGEMENT}'"
        )
    } else {
        format!("Arrangement '{requested}'")
    }
}

fn arrangement_names(arrangements: &[LibraryArrangement]) -> String {
    let mut names = arrangements
        .iter()
        .map(LibraryArrangement::name)
        .filter(|name| !name.trim().is_empty())
        .collect::<Vec<_>>();
    names.sort_unstable_by(|left, right| {
        left.to_ascii_lowercase()
            .cmp(&right.to_ascii_lowercase())
            .then_with(|| left.cmp(right))
    });
    if names.is_empty() {
        "none".to_string()
    } else {
        names.join(", ")
    }
}

fn resolve_unrequested_arrangement(
    file_index: Option<&LibraryCatalog>,
    presentation_path: &str,
) -> Result<Option<String>, String> {
    let Some(entry) = file_index.and_then(|index| {
        index
            .entries()
            .iter()
            .find(|entry| entry.full_path().to_string_lossy() == presentation_path)
    }) else {
        return Ok(None);
    };
    if entry.arrangements().is_empty() {
        return Ok(None);
    }
    let defaults = entry
        .arrangements()
        .iter()
        .filter(|arrangement| {
            arrangement
                .name()
                .eq_ignore_ascii_case(NATIVE_DEFAULT_ARRANGEMENT)
        })
        .collect::<Vec<_>>();
    match defaults.as_slice() {
        [LibraryArrangement::Complete { name }] => Ok(Some(name.clone())),
        [LibraryArrangement::Incomplete { .. }] => Err(format!(
            "Native arrangement '{NATIVE_DEFAULT_ARRANGEMENT}' in '{}' has a missing or invalid UUID",
            entry.file_name()
        )),
        [] => Err(format!(
            "No arrangement was supplied and '{}' has no native '{NATIVE_DEFAULT_ARRANGEMENT}' arrangement; available arrangements: {}",
            entry.file_name(),
            arrangement_names(entry.arrangements())
        )),
        _ => Err(format!(
            "Native arrangement '{NATIVE_DEFAULT_ARRANGEMENT}' is ambiguous in '{}'; available arrangements: {}",
            entry.file_name(),
            arrangement_names(entry.arrangements())
        )),
    }
}
