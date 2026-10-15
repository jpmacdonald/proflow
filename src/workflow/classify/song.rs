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
use propresenter::library::{LibraryArrangement, LibraryCatalog, LibraryEntry};

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
        Some((_, ExactLibraryFileMatch::Missing | ExactLibraryFileMatch::Ambiguous(_))) => {
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
        | SongLibraryMatch::Ambiguous(_)
        | SongLibraryMatch::ConflictingTitles(_) => song_title.to_string(),
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
    if let Some((target, ExactLibraryFileMatch::Ambiguous(paths))) = explicit_target_match {
        return (
            PlanDisposition::NeedsReview(ReviewContext::file_choices(paths)),
            format!("Configured existing song target is ambiguous: {target}"),
        );
    }

    match song_match {
        SongLibraryMatch::Resolved(path) => {
            resolved_song_action(path, arrangement, restyle, file_index)
        }
        // The candidate carries the arrangement the file resolves to, so
        // confirming it never names an arrangement the file lacks.
        SongLibraryMatch::Candidate(path) => {
            match resolved_song_action(path, arrangement, restyle, file_index) {
                (PlanDisposition::Ready(action), _) => (
                    PlanDisposition::NeedsReview(ReviewContext::song_candidate(action)),
                    format!("Possible library match requires an explicit file selection: {path}"),
                ),
                // Choosing one of the file's arrangements confirms the file too.
                (unresolved, reason) => (
                    unresolved,
                    format!("Possible library match {path}: {reason}"),
                ),
            }
        }
        SongLibraryMatch::Ambiguous(paths) => (
            PlanDisposition::NeedsReview(ReviewContext::file_choices(paths)),
            "Song title matches multiple library files; select the intended file explicitly".to_string(),
        ),
        SongLibraryMatch::ConflictingTitles(paths) => (
            PlanDisposition::NeedsReview(ReviewContext::file_choices(paths)),
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
                (SongArrangementResolution::OnlyArrangement(name), requested) => {
                    format!(
                        "{}; Planning Center arrangement '{}' is not in the file, using its only arrangement '{name}'",
                        action_reason(restyle, "Library match"),
                        requested.name().unwrap_or_default()
                    )
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
    /// The file's one arrangement, used for a Planning Center name it lacks.
    OnlyArrangement(String),
    Unarranged,
}

impl SongArrangementResolution {
    fn into_name(self) -> Option<String> {
        match self {
            Self::Exact(name)
            | Self::PlanningCenterDefaultAlias(name)
            | Self::NativeDefault(name)
            | Self::OnlyArrangement(name) => Some(name),
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

    let resolution = match requested {
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
    };
    // Planning Center names its own arrangements, often after an artist. When
    // the file defines exactly one arrangement there is nothing to choose
    // between, so that one is used and the plan says so. A configured name is
    // a project decision and is never substituted.
    match (resolution, requested, arrangements) {
        (
            Err(ArrangementIssue::Unavailable),
            RequestedArrangement::PlanningCenter(_),
            [LibraryArrangement::Complete { name }],
        ) => Ok(SongArrangementResolution::OnlyArrangement(name.clone())),
        (resolution, _, _) => resolution,
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
        fn property_unavailable_arrangement_selects_only_a_planning_center_sole_arrangement(
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
            // Only a Planning Center name, and only when the file has nothing
            // else to choose, falls back; never a configured name, and never
            // to `Default` when another arrangement exists.
            let expected = if configured || include_default {
                Err(ArrangementIssue::Unavailable)
            } else {
                Ok(SongArrangementResolution::OnlyArrangement("Youth".to_string()))
            };
            prop_assert_eq!(resolve_song_arrangement(&arrangements, &requested), expected);
        }
    }
}
