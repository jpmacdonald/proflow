//! Classification for song presentation types and arrangement selection.

use super::file_stem;
use crate::planning_center::types::Item;
use crate::project_config::PresentationTypeKey;
use crate::workflow::classify_matching::strip_title_prefix;
use crate::workflow::library_search::{
    resolve_exact_library_file, resolve_song_library_match, strip_hymn_number,
    ExactLibraryFileMatch, SongLibraryMatch,
};
use crate::workflow::plan::{
    ExistingTransform, ItemKind, OutputKey, PlanDisposition, ReadyAction, ResolvedItemPlan,
    ReviewContext,
};
use proflow_propresenter::library::{LibraryArrangement, LibraryCatalog, LibraryEntry};

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
    Absent,
}

impl RequestedArrangement {
    fn name(&self) -> Option<&str> {
        match self {
            Self::Configured(name) | Self::PlanningCenter(name) => Some(name),
            Self::Absent => None,
        }
    }
}

pub(super) fn build_song_plan(
    output_key: OutputKey,
    type_key: &PresentationTypeKey,
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
    // Compiled service/type configuration is an explicit override, not a
    // fallback. An unavailable override therefore cannot revive the PCO choice.
    let arrangement = configured_arrangement.map_or_else(
        || {
            item.song
                .as_ref()
                .and_then(|song| {
                    let arrangement = song.arrangement.as_deref()?.trim();
                    (!arrangement.is_empty()).then_some(arrangement)
                })
                .map_or(RequestedArrangement::Absent, |name| {
                    RequestedArrangement::PlanningCenter(name.to_string())
                })
        },
        |arrangement| RequestedArrangement::Configured(arrangement.clone()),
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
    let (disposition, reason) = use_existing_song_action(
        explicit_target_match.as_ref(),
        &song_match,
        &arrangement,
        restyle,
        file_index,
        target_library_file,
    );

    let playlist_name = match &song_match {
        SongLibraryMatch::Resolved(path) | SongLibraryMatch::Candidate(path) => file_stem(path),
        SongLibraryMatch::Missing
        | SongLibraryMatch::Ambiguous
        | SongLibraryMatch::ConflictingTitles => song_title.to_string(),
    };

    let mut plan = ResolvedItemPlan::new(
        output_key,
        item.position,
        item.title.clone(),
        playlist_name,
        reason,
        ItemKind::Song,
        Some(type_key.clone()),
        disposition,
    );
    if matches!(policy, SongPolicy::Review) {
        plan.require_review(format!("Configured to require review; {}", plan.reason));
    }
    plan
}

fn use_existing_song_action(
    explicit_target_match: Option<&(&str, ExactLibraryFileMatch)>,
    song_match: &SongLibraryMatch,
    arrangement: &RequestedArrangement,
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
        SongLibraryMatch::Candidate(path) => (
            PlanDisposition::NeedsReview(ReviewContext::song_candidate(song_action(
                path, arrangement.name().map(str::to_string), restyle,
            ))),
            format!("Possible library match requires an explicit file selection: {path}"),
        ),
        SongLibraryMatch::Ambiguous => (
            PlanDisposition::NeedsReview(ReviewContext::new(None)),
            "Song title matches multiple library files; select the intended file explicitly".to_string(),
        ),
        SongLibraryMatch::ConflictingTitles => (
            PlanDisposition::NeedsReview(ReviewContext::new(None)),
            "Planning Center song and item titles identify different library files; select the intended file explicitly".to_string(),
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
    arrangement: &RequestedArrangement,
    restyle: Option<&ExistingTransform>,
    file_index: Option<&LibraryCatalog>,
) -> (PlanDisposition, String) {
    let entry = file_index.and_then(|index| {
        index
            .entries()
            .iter()
            .find(|entry| entry.full_path().to_string_lossy() == path)
    });
    let resolution = entry
        .ok_or(ArrangementIssue::Unindexed)
        .and_then(|entry| resolve_song_arrangement(entry.arrangements(), arrangement));
    match resolution {
        Ok(resolution) => {
            let reason = match (&resolution, arrangement) {
                (SongArrangementResolution::Exact(name), RequestedArrangement::Configured(_)) => {
                    format!("{}; explicit configured arrangement '{name}' overrides Planning Center arrangement selection", action_reason(restyle, "Library match"))
                }
                (SongArrangementResolution::PlanningCenterDefaultAlias(name), _) => {
                    format!("{}; Planning Center 'Default Arrangement' alias using native arrangement '{name}'", action_reason(restyle, "Library match"))
                }
                (SongArrangementResolution::NativeDefault(name), _) => {
                    format!(
                        "{}; using native arrangement '{name}'",
                        action_reason(restyle, "Library match")
                    )
                }
                _ => action_reason(restyle, "Library match"),
            };
            (
                PlanDisposition::Ready(song_action(path, resolution.into_name(), restyle)),
                reason,
            )
        }
        Err(issue) => (
            PlanDisposition::NeedsReview(ReviewContext::song_arrangement(song_action(
                path,
                arrangement.name().map(str::to_string),
                restyle,
            ))),
            issue.reason(entry, arrangement),
        ),
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

/// Selection authority is finite: exact explicit request, the one documented
/// PCO alias, or a native default for an omitted request. Search never substitutes
/// the sole available arrangement for an unavailable name.
#[derive(Debug, PartialEq, Eq)]
enum SongArrangementResolution {
    Exact(String),
    PlanningCenterDefaultAlias(String),
    NativeDefault(String),
    Unarranged,
}

impl SongArrangementResolution {
    fn into_name(self) -> Option<String> {
        match self {
            Self::Exact(name)
            | Self::PlanningCenterDefaultAlias(name)
            | Self::NativeDefault(name) => Some(name),
            Self::Unarranged => None,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ArrangementIssue {
    Unindexed,
    Unavailable,
    Incomplete,
    Ambiguous,
}

impl ArrangementIssue {
    fn reason(&self, entry: Option<&LibraryEntry>, requested: &RequestedArrangement) -> String {
        let selection = requested.name().map_or_else(
            || format!("No arrangement was supplied; native arrangement '{NATIVE_DEFAULT_ARRANGEMENT}'"),
            |name| format!("Arrangement '{name}'"),
        );
        let Some(entry) = entry else {
            return format!(
                "Could not verify {selection} because the resolved library file is not indexed"
            );
        };
        let issue = match self {
            Self::Unindexed => "could not be verified",
            Self::Unavailable => "is unavailable",
            Self::Incomplete => "has a missing or invalid UUID",
            Self::Ambiguous => "is ambiguous",
        };
        format!(
            "{selection} {issue} in '{}'; available arrangements: {}",
            entry.file_name(),
            arrangement_names(entry.arrangements())
        )
    }
}

fn resolve_song_arrangement(
    arrangements: &[LibraryArrangement],
    requested: &RequestedArrangement,
) -> Result<SongArrangementResolution, ArrangementIssue> {
    let requested_name = requested.name().unwrap_or(NATIVE_DEFAULT_ARRANGEMENT);
    let exact_matches = arrangements
        .iter()
        .filter(|arrangement| arrangement.name().eq_ignore_ascii_case(requested_name))
        .collect::<Vec<_>>();
    match exact_matches.as_slice() {
        [LibraryArrangement::Complete { name }] => {
            return Ok(match requested {
                RequestedArrangement::Absent => {
                    SongArrangementResolution::NativeDefault(name.clone())
                }
                _ => SongArrangementResolution::Exact(name.clone()),
            })
        }
        [LibraryArrangement::Incomplete { .. }] => return Err(ArrangementIssue::Incomplete),
        [] => {}
        _ => return Err(ArrangementIssue::Ambiguous),
    }

    match requested {
        RequestedArrangement::Absent if arrangements.is_empty() => {
            Ok(SongArrangementResolution::Unarranged)
        }
        RequestedArrangement::PlanningCenter(name)
            if name.eq_ignore_ascii_case(PCO_DEFAULT_ARRANGEMENT) =>
        {
            if arrangements.is_empty() {
                return Ok(SongArrangementResolution::Unarranged);
            }
            let defaults = arrangements
                .iter()
                .filter(|arrangement| {
                    arrangement
                        .name()
                        .eq_ignore_ascii_case(NATIVE_DEFAULT_ARRANGEMENT)
                })
                .collect::<Vec<_>>();
            match defaults.as_slice() {
                [LibraryArrangement::Complete { name }] => Ok(
                    SongArrangementResolution::PlanningCenterDefaultAlias(name.clone()),
                ),
                [LibraryArrangement::Incomplete { .. }] => Err(ArrangementIssue::Incomplete),
                [] => Err(ArrangementIssue::Unavailable),
                _ => Err(ArrangementIssue::Ambiguous),
            }
        }
        _ => Err(ArrangementIssue::Unavailable),
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

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn property_unavailable_explicit_arrangement_never_selects_default_or_sole_name(
            name in "Requested [A-Za-z]{1,32}",
            configured in any::<bool>(),
            include_default in any::<bool>(),
        ) {
            let mut arrangements = vec![LibraryArrangement::Complete { name: "Youth".to_string() }];
            if include_default {
                arrangements.push(LibraryArrangement::Complete { name: "Default".to_string() });
            }
            let requested = if configured {
                RequestedArrangement::Configured(name)
            } else {
                RequestedArrangement::PlanningCenter(name)
            };
            prop_assert_eq!(resolve_song_arrangement(&arrangements, &requested), Err(ArrangementIssue::Unavailable));
        }
    }
}
