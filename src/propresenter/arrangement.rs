//! Operator-visible cue ordering for `ProPresenter` presentations.

use std::{
    collections::{HashMap, HashSet},
    num::NonZeroUsize,
};

use super::generated::rv_data;

/// Failure to retain a checked prefix of the operator-visible cue traversal.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RetainOperatorCuesError {
    /// The presentation's selected/default operator traversal was not safe to mutate.
    #[error(transparent)]
    Traversal(#[from] OperatorTraversalError),
    /// The requested prefix is longer than the complete operator traversal.
    #[error(
        "cannot retain {requested} operator-visible cue occurrences; presentation has {available}"
    )]
    CountExceedsTraversal {
        /// Number of operator-visible occurrences requested by the caller.
        requested: usize,
        /// Number of operator-visible occurrences available in the presentation.
        available: usize,
    },
}

/// Why operator-visible cue order cannot safely drive a native mutation.
///
/// Inspection deliberately remains best-effort, but mutations must never fall
/// through a malformed selected/default arrangement to a different cue order.
#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum OperatorTraversalError {
    /// The selected native identifier did not resolve to an arrangement.
    #[error("selected arrangement {identifier:?} is unavailable")]
    SelectedArrangementUnavailable {
        /// Exact native identifier stored by the presentation.
        identifier: String,
    },
    /// More than one arrangement used the selected native identifier.
    #[error("selected arrangement {identifier:?} is ambiguous")]
    SelectedArrangementAmbiguous {
        /// Duplicated native identifier.
        identifier: String,
    },
    /// The selected arrangement's complete group/cue graph did not resolve.
    #[error("selected arrangement {name:?} has an incomplete group/cue traversal")]
    SelectedArrangementIncomplete {
        /// Native arrangement display name.
        name: String,
    },
    /// The selected arrangement did not have one exact selectable identity.
    #[error("selected arrangement {name:?} has an invalid or ambiguous native identity")]
    SelectedArrangementIdentityInvalid {
        /// Native arrangement display name.
        name: String,
    },
    /// More than one unselected arrangement was named `Default`.
    #[error("presentation has more than one default arrangement")]
    DefaultArrangementAmbiguous,
    /// The only default arrangement's complete group/cue graph did not resolve.
    #[error("default arrangement {name:?} has an incomplete group/cue traversal")]
    DefaultArrangementIncomplete {
        /// Native arrangement display name.
        name: String,
    },
    /// The default arrangement did not have one exact selectable identity.
    #[error("default arrangement {name:?} has an invalid or ambiguous native identity")]
    DefaultArrangementIdentityInvalid {
        /// Native arrangement display name.
        name: String,
    },
    /// Arrangement-less cue groups contained a missing or ambiguous cue reference.
    #[error("presentation cue-group traversal is incomplete")]
    CueGroupTraversalIncomplete,
    /// The presentation has no cue that can be shown in operator order.
    #[error("presentation has no operator-visible cues")]
    EmptyTraversal,
}

/// Return cue indices in the order visible to a `ProPresenter` operator.
///
/// The selected arrangement owns the order when it resolves to presentation
/// groups. Without a usable arrangement, cue-group order is used. Presentations
/// without either structure fall back to their stored cue order.
#[must_use]
pub fn operator_cue_indices(presentation: &rv_data::Presentation) -> Vec<usize> {
    if let Some(arrangement) = selected_or_default_resolved_arrangement(presentation) {
        return arrangement.cue_indices().to_vec();
    }
    fallback_operator_cue_indices(presentation)
}

/// Return the exact operator traversal permitted to drive native mutation.
///
/// Unlike [`operator_cue_indices`], this never substitutes cue-group or raw
/// order for a malformed selected/default arrangement.
pub(crate) fn checked_operator_cue_indices(
    presentation: &rv_data::Presentation,
) -> Result<Vec<usize>, OperatorTraversalError> {
    if let Some(arrangement) = checked_selected_or_default_arrangement(presentation)? {
        return Ok(arrangement.cue_indices().to_vec());
    }

    if presentation.cue_groups.is_empty() {
        return if presentation.cues.is_empty() {
            Err(OperatorTraversalError::EmptyTraversal)
        } else {
            Ok((0..presentation.cues.len()).collect())
        };
    }
    let mut indices = Vec::new();
    if presentation
        .cue_groups
        .iter()
        .all(|group| append_group_cue_indices(presentation, group, &mut indices))
        && !indices.is_empty()
    {
        Ok(indices)
    } else {
        Err(OperatorTraversalError::CueGroupTraversalIncomplete)
    }
}

fn fallback_operator_cue_indices(presentation: &rv_data::Presentation) -> Vec<usize> {
    let mut indices = Vec::new();
    let complete = presentation
        .cue_groups
        .iter()
        .all(|group| append_group_cue_indices(presentation, group, &mut indices));
    if complete && !indices.is_empty() {
        indices
    } else {
        (0..presentation.cues.len()).collect()
    }
}

/// Retain only the native cues represented by the first `count` occurrences in
/// operator order.
///
/// Cue-group and arrangement references are pruned after cue selection. Empty
/// groups and arrangements are removed, and a selected arrangement is cleared
/// when its arrangement no longer exists. All retained protobuf values keep
/// their original metadata. The transform is applied atomically only after the
/// complete traversal and requested count have been checked.
pub fn retain_first_operator_cues(
    presentation: &mut rv_data::Presentation,
    count: NonZeroUsize,
) -> Result<bool, RetainOperatorCuesError> {
    let traversal = checked_operator_cue_indices(presentation)?;
    if count.get() > traversal.len() {
        return Err(RetainOperatorCuesError::CountExceedsTraversal {
            requested: count.get(),
            available: traversal.len(),
        });
    }

    let retained_indices = traversal
        .into_iter()
        .take(count.get())
        .collect::<HashSet<_>>();
    let mut transformed = presentation.clone();
    transformed.cues = std::mem::take(&mut transformed.cues)
        .into_iter()
        .enumerate()
        .filter_map(|(index, cue)| retained_indices.contains(&index).then_some(cue))
        .collect();

    let retained_cue_ids = unique_cue_ids(&transformed.cues);
    for group in &mut transformed.cue_groups {
        group
            .cue_identifiers
            .retain(|identifier| retained_cue_ids.contains(&identifier.string));
    }
    transformed
        .cue_groups
        .retain(|group| !group.cue_identifiers.is_empty());

    let retained_group_ids = unique_group_ids(&transformed.cue_groups);
    for arrangement in &mut transformed.arrangements {
        arrangement
            .group_identifiers
            .retain(|identifier| retained_group_ids.contains(&identifier.string));
    }
    transformed
        .arrangements
        .retain(|arrangement| !arrangement.group_identifiers.is_empty());

    if transformed
        .selected_arrangement
        .as_ref()
        .is_some_and(|selected| {
            !transformed.arrangements.iter().any(|arrangement| {
                arrangement
                    .uuid
                    .as_ref()
                    .is_some_and(|identifier| identifier.string == selected.string)
            })
        })
    {
        transformed.selected_arrangement = None;
    }

    let changed = transformed != *presentation;
    if changed {
        *presentation = transformed;
    }
    Ok(changed)
}

fn unique_cue_ids(cues: &[rv_data::Cue]) -> HashSet<String> {
    unique_ids(cues.iter().filter_map(|cue| cue.uuid.as_ref()))
}

fn unique_group_ids(groups: &[rv_data::presentation::CueGroup]) -> HashSet<String> {
    unique_ids(
        groups
            .iter()
            .filter_map(|group| group.group.as_ref()?.uuid.as_ref()),
    )
}

fn unique_ids<'a>(identifiers: impl Iterator<Item = &'a rv_data::Uuid>) -> HashSet<String> {
    let mut counts = HashMap::<&str, usize>::new();
    for identifier in identifiers {
        *counts.entry(&identifier.string).or_default() += 1;
    }
    counts
        .into_iter()
        .filter(|(_, count)| *count == 1)
        .map(|(identifier, _)| identifier.to_string())
        .collect()
}

/// Return the checked UUID for an arrangement that can be selected safely.
///
/// Selection requires exact operator identity and a nonempty traversal whose
/// every group and cue reference resolves uniquely inside the presentation.
pub(crate) fn selectable_arrangement_uuid(
    presentation: &rv_data::Presentation,
    arrangement: &rv_data::presentation::Arrangement,
) -> Option<uuid::Uuid> {
    selectable_arrangement(presentation, arrangement)
        .ok()
        .map(|arrangement| arrangement.uuid())
}

/// Whether an exact UUID/name pair identifies one selectable arrangement.
///
/// [`selectable_arrangement_uuid`] owns the structural predicate, including
/// UUID uniqueness and complete group/cue traversal. This wrapper binds the
/// caller-supplied native display name without restating those checks.
pub(crate) fn has_selectable_arrangement(
    presentation: &rv_data::Presentation,
    uuid: &uuid::Uuid,
    name: &str,
) -> bool {
    selectable_arrangement_by_identity(presentation, uuid, name).is_ok()
}

/// Why an operator-supplied arrangement selection cannot be bound exactly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ArrangementSelectionError {
    /// No native arrangement has the supplied exact UUID/name identity.
    #[error("arrangement is unavailable")]
    Unavailable,
    /// More than one native arrangement has the supplied identity.
    #[error("arrangement identity is ambiguous across {matches} matches")]
    Ambiguous {
        /// Number of native arrangements sharing the identity.
        matches: usize,
    },
    /// The arrangement references a missing or ambiguous group/cue.
    #[error("arrangement has an incomplete group/cue traversal")]
    Incomplete,
}

/// Resolve one exact native UUID/name pair through the canonical arrangement
/// identity and graph predicate.
pub(crate) fn selectable_arrangement_by_identity<'a>(
    presentation: &'a rv_data::Presentation,
    uuid: &uuid::Uuid,
    name: &str,
) -> Result<SelectableArrangement<'a>, ArrangementSelectionError> {
    let matches = presentation
        .arrangements
        .iter()
        .filter(|arrangement| {
            arrangement
                .uuid
                .as_ref()
                .and_then(|native| uuid::Uuid::parse_str(&native.string).ok())
                .as_ref()
                == Some(uuid)
        })
        .collect::<Vec<_>>();
    let arrangement = match matches.as_slice() {
        [arrangement] if arrangement.name == name => *arrangement,
        [_] | [] => return Err(ArrangementSelectionError::Unavailable),
        _ => {
            return Err(ArrangementSelectionError::Ambiguous {
                matches: matches.len(),
            });
        }
    };
    selectable_arrangement(presentation, arrangement)
}

/// Resolve one case-insensitive operator arrangement name through the same
/// identity and graph predicate used by playlist and background boundaries.
pub(crate) fn selectable_arrangement_by_name<'a>(
    presentation: &'a rv_data::Presentation,
    name: &str,
) -> Result<SelectableArrangement<'a>, ArrangementSelectionError> {
    let matches = presentation
        .arrangements
        .iter()
        .filter(|arrangement| arrangement.name.eq_ignore_ascii_case(name))
        .collect::<Vec<_>>();
    let arrangement = match matches.as_slice() {
        [arrangement] => *arrangement,
        [] => return Err(ArrangementSelectionError::Unavailable),
        _ => {
            return Err(ArrangementSelectionError::Ambiguous {
                matches: matches.len(),
            });
        }
    };
    selectable_arrangement(presentation, arrangement)
}

/// Check one native arrangement's exact identity and complete group/cue graph.
pub(crate) fn selectable_arrangement<'a>(
    presentation: &'a rv_data::Presentation,
    arrangement: &'a rv_data::presentation::Arrangement,
) -> Result<SelectableArrangement<'a>, ArrangementSelectionError> {
    let resolved = resolve_arrangement(presentation, arrangement)
        .ok_or(ArrangementSelectionError::Incomplete)?;
    let uuid = resolved.checked_uuid().map_err(|error| match error {
        ArrangementIdentityError::AmbiguousUuid(uuid) => ArrangementSelectionError::Ambiguous {
            matches: presentation
                .arrangements
                .iter()
                .filter_map(|arrangement| arrangement.uuid.as_ref())
                .filter_map(|native| uuid::Uuid::parse_str(&native.string).ok())
                .filter(|candidate| *candidate == uuid)
                .count(),
        },
        ArrangementIdentityError::InvalidName | ArrangementIdentityError::MissingOrInvalidUuid => {
            ArrangementSelectionError::Incomplete
        }
    })?;
    Ok(SelectableArrangement { resolved, uuid })
}

/// An arrangement whose native identity and complete traversal are proven.
pub(crate) struct SelectableArrangement<'a> {
    resolved: ResolvedArrangement<'a>,
    uuid: uuid::Uuid,
}

impl<'a> SelectableArrangement<'a> {
    pub(crate) const fn name(&self) -> &'a str {
        self.resolved.name()
    }

    pub(crate) const fn native_uuid(&self) -> Option<&'a rv_data::Uuid> {
        self.resolved.native_uuid()
    }

    pub(crate) const fn uuid(&self) -> uuid::Uuid {
        self.uuid
    }

    pub(crate) const fn entry_cue_index(&self) -> usize {
        self.resolved.entry_cue_index()
    }
}

/// Why a structurally resolved arrangement cannot be used as an exact native
/// selection identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ArrangementIdentityError {
    InvalidName,
    MissingOrInvalidUuid,
    AmbiguousUuid(uuid::Uuid),
}

/// One checked group occurrence in an arrangement traversal.
pub(crate) struct ResolvedArrangementGroup<'a> {
    name: &'a str,
    entry_cue_index: Option<usize>,
}

impl<'a> ResolvedArrangementGroup<'a> {
    pub(crate) const fn name(&self) -> &'a str {
        self.name
    }

    pub(crate) const fn entry_cue_index(&self) -> Option<usize> {
        self.entry_cue_index
    }
}

/// A native arrangement whose complete group/cue graph resolves uniquely.
///
/// The view preserves repeated group occurrences and their native cue order.
/// Exact selectable identity is checked separately because operator traversal
/// can still use legacy documents whose UUID strings are not canonical UUIDs.
pub(crate) struct ResolvedArrangement<'a> {
    arrangement: &'a rv_data::presentation::Arrangement,
    identity: Result<uuid::Uuid, ArrangementIdentityError>,
    groups: Vec<ResolvedArrangementGroup<'a>>,
    entry_cue_index: usize,
    cue_indices: Vec<usize>,
}

impl<'a> ResolvedArrangement<'a> {
    pub(crate) const fn name(&self) -> &'a str {
        self.arrangement.name.as_str()
    }

    pub(crate) const fn native_uuid(&self) -> Option<&'a rv_data::Uuid> {
        self.arrangement.uuid.as_ref()
    }

    pub(crate) fn groups(&self) -> &[ResolvedArrangementGroup<'a>] {
        &self.groups
    }

    pub(crate) fn cue_indices(&self) -> &[usize] {
        &self.cue_indices
    }

    pub(crate) const fn entry_cue_index(&self) -> usize {
        self.entry_cue_index
    }

    pub(crate) const fn checked_uuid(&self) -> Result<uuid::Uuid, ArrangementIdentityError> {
        self.identity
    }
}

/// Resolve one complete arrangement graph without guessing through ambiguous
/// group or cue identifiers.
pub(crate) fn resolve_arrangement<'a>(
    presentation: &'a rv_data::Presentation,
    arrangement: &'a rv_data::presentation::Arrangement,
) -> Option<ResolvedArrangement<'a>> {
    if arrangement.group_identifiers.is_empty() {
        return None;
    }
    let mut groups = Vec::with_capacity(arrangement.group_identifiers.len());
    let mut cue_indices = Vec::new();
    for group_id in &arrangement.group_identifiers {
        let mut matches = presentation.cue_groups.iter().filter(|candidate| {
            candidate
                .group
                .as_ref()
                .and_then(|group| group.uuid.as_ref())
                .is_some_and(|uuid| uuid.string == group_id.string)
        });
        let group = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        let name = group.group.as_ref()?.name.as_str();
        let start = cue_indices.len();
        if !append_group_cue_indices(presentation, group, &mut cue_indices) {
            return None;
        }
        groups.push(ResolvedArrangementGroup {
            name,
            entry_cue_index: cue_indices.get(start).copied(),
        });
    }
    let entry_cue_index = cue_indices.first().copied()?;
    Some(ResolvedArrangement {
        arrangement,
        identity: arrangement_identity(presentation, arrangement),
        groups,
        entry_cue_index,
        cue_indices,
    })
}

fn arrangement_identity(
    presentation: &rv_data::Presentation,
    arrangement: &rv_data::presentation::Arrangement,
) -> Result<uuid::Uuid, ArrangementIdentityError> {
    let name = arrangement.name.as_str();
    if name.trim().is_empty() || name.trim() != name || name.chars().any(char::is_control) {
        return Err(ArrangementIdentityError::InvalidName);
    }
    let uuid = arrangement
        .uuid
        .as_ref()
        .and_then(|native| uuid::Uuid::parse_str(&native.string).ok())
        .ok_or(ArrangementIdentityError::MissingOrInvalidUuid)?;
    let matching_uuid_count = presentation
        .arrangements
        .iter()
        .filter_map(|candidate| candidate.uuid.as_ref())
        .filter_map(|candidate| uuid::Uuid::parse_str(&candidate.string).ok())
        .filter(|candidate| *candidate == uuid)
        .count();
    if matching_uuid_count != 1 {
        return Err(ArrangementIdentityError::AmbiguousUuid(uuid));
    }
    Ok(uuid)
}

/// One ordered selected-arrangement group occurrence and its entry cue.
pub(crate) struct SelectedGroupEntry<'a> {
    pub(crate) name: &'a str,
    pub(crate) cue_index: usize,
}

/// Resolve every group occurrence in the checked selected/default arrangement.
///
/// Repeated group identifiers remain repeated occurrences. The complete
/// arrangement identity and each entry cue must resolve uniquely; otherwise no
/// partial region list escapes.
pub(crate) fn checked_selected_group_entries(
    presentation: &rv_data::Presentation,
) -> Result<Option<Vec<SelectedGroupEntry<'_>>>, OperatorTraversalError> {
    let Some(resolved) = checked_selected_or_default_arrangement(presentation)? else {
        return Ok(None);
    };
    Ok(resolved
        .groups()
        .iter()
        .map(|group| {
            Some(SelectedGroupEntry {
                name: group.name(),
                cue_index: group.entry_cue_index()?,
            })
        })
        .collect())
}

fn selected_or_default_resolved_arrangement(
    presentation: &rv_data::Presentation,
) -> Option<ResolvedArrangement<'_>> {
    let arrangement = selected_or_default_arrangement_for_inspection(presentation)?;
    resolve_arrangement(presentation, arrangement)
}

fn selected_or_default_arrangement_for_inspection(
    presentation: &rv_data::Presentation,
) -> Option<&rv_data::presentation::Arrangement> {
    if let Some(selected) = &presentation.selected_arrangement {
        let mut matches = presentation.arrangements.iter().filter(|arrangement| {
            arrangement
                .uuid
                .as_ref()
                .is_some_and(|uuid| uuid.string == selected.string)
        });
        return match (matches.next(), matches.next()) {
            (Some(arrangement), None) => Some(arrangement),
            _ => None,
        };
    }

    let mut defaults = presentation
        .arrangements
        .iter()
        .filter(|arrangement| arrangement.name.eq_ignore_ascii_case("Default"));
    match (defaults.next(), defaults.next()) {
        (Some(arrangement), None) => Some(arrangement),
        _ => None,
    }
}

fn checked_selected_or_default_arrangement(
    presentation: &rv_data::Presentation,
) -> Result<Option<ResolvedArrangement<'_>>, OperatorTraversalError> {
    if let Some(selected) = &presentation.selected_arrangement {
        let mut matches = presentation.arrangements.iter().filter(|arrangement| {
            arrangement
                .uuid
                .as_ref()
                .is_some_and(|uuid| uuid.string == selected.string)
        });
        let arrangement = match (matches.next(), matches.next()) {
            (None, _) => {
                return Err(OperatorTraversalError::SelectedArrangementUnavailable {
                    identifier: selected.string.clone(),
                });
            }
            (Some(arrangement), None) => arrangement,
            (Some(_), Some(_)) => {
                return Err(OperatorTraversalError::SelectedArrangementAmbiguous {
                    identifier: selected.string.clone(),
                });
            }
        };
        let resolved = resolve_arrangement(presentation, arrangement).ok_or_else(|| {
            OperatorTraversalError::SelectedArrangementIncomplete {
                name: arrangement.name.clone(),
            }
        })?;
        resolved.checked_uuid().map_err(|_| {
            OperatorTraversalError::SelectedArrangementIdentityInvalid {
                name: arrangement.name.clone(),
            }
        })?;
        return Ok(Some(resolved));
    }

    let mut defaults = presentation
        .arrangements
        .iter()
        .filter(|arrangement| arrangement.name.eq_ignore_ascii_case("Default"));
    match (defaults.next(), defaults.next()) {
        (None, _) => Ok(None),
        (Some(arrangement), None) => {
            let resolved = resolve_arrangement(presentation, arrangement).ok_or_else(|| {
                OperatorTraversalError::DefaultArrangementIncomplete {
                    name: arrangement.name.clone(),
                }
            })?;
            resolved.checked_uuid().map_err(|_| {
                OperatorTraversalError::DefaultArrangementIdentityInvalid {
                    name: arrangement.name.clone(),
                }
            })?;
            Ok(Some(resolved))
        }
        (Some(_), Some(_)) => Err(OperatorTraversalError::DefaultArrangementAmbiguous),
    }
}

fn append_group_cue_indices(
    presentation: &rv_data::Presentation,
    group: &rv_data::presentation::CueGroup,
    indices: &mut Vec<usize>,
) -> bool {
    for cue_id in &group.cue_identifiers {
        let mut matches = presentation.cues.iter().enumerate().filter(|(_, cue)| {
            cue.uuid
                .as_ref()
                .is_some_and(|uuid| uuid.string == cue_id.string)
        });
        let Some((index, _)) = matches.next() else {
            return false;
        };
        if matches.next().is_some() {
            return false;
        }
        indices.push(index);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(id: &str) -> rv_data::Cue {
        rv_data::Cue {
            uuid: Some(rv_data::Uuid {
                string: id.to_string(),
            }),
            ..rv_data::Cue::default()
        }
    }

    fn group(id: &str, cue_ids: &[&str]) -> rv_data::presentation::CueGroup {
        rv_data::presentation::CueGroup {
            group: Some(rv_data::Group {
                uuid: Some(rv_data::Uuid {
                    string: id.to_string(),
                }),
                ..rv_data::Group::default()
            }),
            cue_identifiers: cue_ids
                .iter()
                .map(|id| rv_data::Uuid {
                    string: (*id).to_string(),
                })
                .collect(),
        }
    }

    fn arrangement(id: &str, name: &str, group_ids: &[&str]) -> rv_data::presentation::Arrangement {
        rv_data::presentation::Arrangement {
            uuid: Some(rv_data::Uuid {
                string: id.to_string(),
            }),
            name: name.to_string(),
            group_identifiers: group_ids
                .iter()
                .map(|id| rv_data::Uuid {
                    string: (*id).to_string(),
                })
                .collect(),
        }
    }

    #[test]
    fn retaining_operator_prefix_prunes_references_and_preserves_retained_metadata() {
        let mut title = cue("title");
        title.name = "Retained title metadata".to_string();
        title.is_enabled = true;
        let mut title_group = group("title-group", &["title"]);
        title_group.group.as_mut().expect("title group").name =
            "Retained group metadata".to_string();
        let presentation_uuid = rv_data::Uuid {
            string: "presentation-id".to_string(),
        };
        let mut presentation = rv_data::Presentation {
            uuid: Some(presentation_uuid.clone()),
            name: "Retained presentation metadata".to_string(),
            notes: "Notes survive cue selection".to_string(),
            selected_arrangement: Some(rv_data::Uuid {
                string: "11111111-1111-4111-8111-111111111111".to_string(),
            }),
            arrangements: vec![arrangement(
                "11111111-1111-4111-8111-111111111111",
                "Retained arrangement metadata",
                &["title-group", "content-group"],
            )],
            cues: vec![cue("content-2"), title, cue("content-1")],
            cue_groups: vec![
                group("content-group", &["content-1", "content-2"]),
                title_group,
            ],
            ..rv_data::Presentation::default()
        };

        let changed = retain_first_operator_cues(
            &mut presentation,
            NonZeroUsize::new(1).expect("nonzero count"),
        )
        .expect("retain title cue");

        assert!(changed);
        assert_eq!(presentation.uuid, Some(presentation_uuid));
        assert_eq!(presentation.name, "Retained presentation metadata");
        assert_eq!(presentation.notes, "Notes survive cue selection");
        assert_eq!(presentation.cues.len(), 1);
        assert_eq!(
            presentation.cues[0]
                .uuid
                .as_ref()
                .map(|id| id.string.as_str()),
            Some("title")
        );
        assert_eq!(presentation.cues[0].name, "Retained title metadata");
        assert!(presentation.cues[0].is_enabled);
        assert_eq!(presentation.cue_groups.len(), 1);
        assert_eq!(
            presentation.cue_groups[0]
                .group
                .as_ref()
                .map(|group| group.name.as_str()),
            Some("Retained group metadata")
        );
        assert_eq!(presentation.cue_groups[0].cue_identifiers.len(), 1);
        assert_eq!(presentation.arrangements.len(), 1);
        assert_eq!(
            presentation.arrangements[0].name,
            "Retained arrangement metadata"
        );
        assert_eq!(presentation.arrangements[0].group_identifiers.len(), 1);
        assert_eq!(
            presentation
                .selected_arrangement
                .as_ref()
                .map(|id| id.string.as_str()),
            Some("11111111-1111-4111-8111-111111111111")
        );
        assert_eq!(operator_cue_indices(&presentation), vec![0]);
    }

    #[test]
    fn retaining_operator_prefix_prunes_removed_cues_from_a_surviving_group() {
        let mut content_group = group("content-group", &["content-1", "content-2"]);
        content_group.group.as_mut().expect("content group").name = "Content metadata".to_string();
        let mut presentation = rv_data::Presentation {
            selected_arrangement: Some(rv_data::Uuid {
                string: "11111111-1111-4111-8111-111111111111".to_string(),
            }),
            arrangements: vec![arrangement(
                "11111111-1111-4111-8111-111111111111",
                "Service",
                &["title-group", "content-group"],
            )],
            cues: vec![cue("content-2"), cue("title"), cue("content-1")],
            cue_groups: vec![content_group, group("title-group", &["title"])],
            ..rv_data::Presentation::default()
        };

        retain_first_operator_cues(
            &mut presentation,
            NonZeroUsize::new(2).expect("nonzero count"),
        )
        .expect("retain title and first content cue");

        let content_group = presentation
            .cue_groups
            .iter()
            .find(|group| {
                group
                    .group
                    .as_ref()
                    .and_then(|group| group.uuid.as_ref())
                    .is_some_and(|id| id.string == "content-group")
            })
            .expect("surviving content group");
        assert_eq!(
            content_group
                .group
                .as_ref()
                .map(|group| group.name.as_str()),
            Some("Content metadata")
        );
        assert_eq!(content_group.cue_identifiers.len(), 1);
        assert_eq!(content_group.cue_identifiers[0].string, "content-1");
        assert_eq!(operator_cue_indices(&presentation), vec![0, 1]);
    }

    #[test]
    fn retaining_operator_prefix_rejects_a_dangling_selected_arrangement_atomically() {
        let mut presentation = rv_data::Presentation {
            selected_arrangement: Some(rv_data::Uuid {
                string: "broken-selected".to_string(),
            }),
            arrangements: vec![
                arrangement("broken-selected", "Broken", &["missing-group"]),
                arrangement("retained", "Alternate", &["first-group"]),
                arrangement("removed", "Removed", &["second-group"]),
            ],
            cues: vec![cue("first"), cue("second")],
            cue_groups: vec![
                group("first-group", &["first"]),
                group("second-group", &["second"]),
            ],
            ..rv_data::Presentation::default()
        };
        let original = presentation.clone();

        let error = retain_first_operator_cues(
            &mut presentation,
            NonZeroUsize::new(1).expect("nonzero count"),
        )
        .expect_err("mutation cannot use the raw inspection fallback");

        assert_eq!(
            error,
            RetainOperatorCuesError::Traversal(
                OperatorTraversalError::SelectedArrangementIncomplete {
                    name: "Broken".to_string(),
                }
            )
        );
        assert_eq!(presentation, original);
    }

    #[test]
    fn retaining_more_than_the_operator_traversal_is_atomic() {
        let mut presentation = rv_data::Presentation {
            name: "Unchanged".to_string(),
            cues: vec![cue("one"), cue("two")],
            cue_groups: vec![group("all", &["one", "two"])],
            ..rv_data::Presentation::default()
        };
        let original = presentation.clone();

        let error = retain_first_operator_cues(
            &mut presentation,
            NonZeroUsize::new(3).expect("nonzero count"),
        )
        .expect_err("oversized prefix must fail");

        assert_eq!(
            error,
            RetainOperatorCuesError::CountExceedsTraversal {
                requested: 3,
                available: 2,
            }
        );
        assert_eq!(presentation, original);
    }

    #[test]
    fn retaining_from_an_empty_traversal_is_atomic() {
        let mut presentation = rv_data::Presentation {
            name: "Empty".to_string(),
            ..rv_data::Presentation::default()
        };
        let original = presentation.clone();

        let error = retain_first_operator_cues(
            &mut presentation,
            NonZeroUsize::new(1).expect("nonzero count"),
        )
        .expect_err("empty traversal must fail");

        assert_eq!(
            error,
            RetainOperatorCuesError::Traversal(OperatorTraversalError::EmptyTraversal)
        );
        assert_eq!(presentation, original);
    }

    #[test]
    fn retaining_rejects_an_invalid_default_arrangement_identity_atomically() {
        let mut presentation = rv_data::Presentation {
            arrangements: vec![arrangement("not-a-uuid", "Default", &["group"])],
            cues: vec![cue("cue")],
            cue_groups: vec![group("group", &["cue"])],
            ..rv_data::Presentation::default()
        };
        let original = presentation.clone();

        let error = retain_first_operator_cues(
            &mut presentation,
            NonZeroUsize::new(1).expect("nonzero count"),
        )
        .expect_err("mutation requires selectable default identity");

        assert_eq!(
            error,
            RetainOperatorCuesError::Traversal(
                OperatorTraversalError::DefaultArrangementIdentityInvalid {
                    name: "Default".to_string(),
                }
            )
        );
        assert_eq!(presentation, original);
        assert_eq!(operator_cue_indices(&presentation), vec![0]);
    }

    #[test]
    fn operator_order_uses_selected_arrangement_groups_and_cues() {
        let presentation = rv_data::Presentation {
            selected_arrangement: Some(rv_data::Uuid {
                string: "alternate".to_string(),
            }),
            arrangements: vec![rv_data::presentation::Arrangement {
                uuid: Some(rv_data::Uuid {
                    string: "alternate".to_string(),
                }),
                name: "Alternate".to_string(),
                group_identifiers: vec![
                    rv_data::Uuid {
                        string: "title-group".to_string(),
                    },
                    rv_data::Uuid {
                        string: "content-group".to_string(),
                    },
                ],
            }],
            cues: vec![cue("content-2"), cue("title"), cue("content-1")],
            cue_groups: vec![
                group("content-group", &["content-1", "content-2"]),
                group("title-group", &["title"]),
            ],
            ..rv_data::Presentation::default()
        };

        assert_eq!(operator_cue_indices(&presentation), vec![1, 2, 0]);
        assert_eq!(
            checked_operator_cue_indices(&presentation),
            Err(OperatorTraversalError::SelectedArrangementIdentityInvalid {
                name: "Alternate".to_string(),
            })
        );
    }

    #[test]
    fn operator_order_falls_back_to_raw_cues_without_groups() {
        let presentation = rv_data::Presentation {
            cues: vec![cue("one"), cue("two")],
            ..rv_data::Presentation::default()
        };

        assert_eq!(operator_cue_indices(&presentation), vec![0, 1]);
    }

    #[test]
    fn operator_order_does_not_guess_an_unnamed_first_arrangement() {
        let presentation = rv_data::Presentation {
            arrangements: vec![rv_data::presentation::Arrangement {
                name: "Seasonal".to_string(),
                group_identifiers: vec![rv_data::Uuid {
                    string: "title-group".to_string(),
                }],
                ..rv_data::presentation::Arrangement::default()
            }],
            cues: vec![cue("content"), cue("title")],
            cue_groups: vec![
                group("content-group", &["content"]),
                group("title-group", &["title"]),
            ],
            ..rv_data::Presentation::default()
        };

        assert_eq!(operator_cue_indices(&presentation), vec![0, 1]);
    }

    #[test]
    fn incomplete_arrangement_falls_back_without_using_partial_order() {
        let presentation = rv_data::Presentation {
            selected_arrangement: Some(rv_data::Uuid {
                string: "broken".to_string(),
            }),
            arrangements: vec![rv_data::presentation::Arrangement {
                uuid: Some(rv_data::Uuid {
                    string: "broken".to_string(),
                }),
                name: "Broken".to_string(),
                group_identifiers: vec![
                    rv_data::Uuid {
                        string: "title-group".to_string(),
                    },
                    rv_data::Uuid {
                        string: "missing-group".to_string(),
                    },
                ],
            }],
            cues: vec![cue("content"), cue("title")],
            cue_groups: vec![
                group("content-group", &["content"]),
                group("title-group", &["title"]),
            ],
            ..rv_data::Presentation::default()
        };

        assert_eq!(operator_cue_indices(&presentation), vec![0, 1]);
        assert_eq!(
            checked_operator_cue_indices(&presentation),
            Err(OperatorTraversalError::SelectedArrangementIncomplete {
                name: "Broken".to_string(),
            })
        );
    }

    #[test]
    fn selected_group_entries_reject_a_partially_resolved_arrangement() {
        let presentation = rv_data::Presentation {
            selected_arrangement: Some(rv_data::Uuid {
                string: "broken".to_string(),
            }),
            arrangements: vec![rv_data::presentation::Arrangement {
                uuid: Some(rv_data::Uuid {
                    string: "broken".to_string(),
                }),
                name: "Broken".to_string(),
                group_identifiers: vec![
                    rv_data::Uuid {
                        string: "title-group".to_string(),
                    },
                    rv_data::Uuid {
                        string: "content-group".to_string(),
                    },
                ],
            }],
            cues: vec![cue("title")],
            cue_groups: vec![
                group("title-group", &["title"]),
                group("content-group", &["missing-content"]),
            ],
            ..rv_data::Presentation::default()
        };

        assert!(matches!(
            checked_selected_group_entries(&presentation),
            Err(OperatorTraversalError::SelectedArrangementIncomplete { .. })
        ));
    }

    #[test]
    fn repeated_arrangement_groups_repeat_their_cues_in_operator_order() {
        let presentation = rv_data::Presentation {
            selected_arrangement: Some(rv_data::Uuid {
                string: "default".to_string(),
            }),
            arrangements: vec![rv_data::presentation::Arrangement {
                uuid: Some(rv_data::Uuid {
                    string: "default".to_string(),
                }),
                name: "Default".to_string(),
                group_identifiers: vec![
                    rv_data::Uuid {
                        string: "verse".to_string(),
                    },
                    rv_data::Uuid {
                        string: "chorus".to_string(),
                    },
                    rv_data::Uuid {
                        string: "chorus".to_string(),
                    },
                ],
            }],
            cues: vec![cue("verse-1"), cue("verse-2"), cue("chorus")],
            cue_groups: vec![
                group("verse", &["verse-1", "verse-2"]),
                group("chorus", &["chorus"]),
            ],
            ..rv_data::Presentation::default()
        };

        assert_eq!(operator_cue_indices(&presentation), vec![0, 1, 2, 2]);
    }

    #[test]
    fn selectable_arrangement_requires_exact_identity_and_resolved_traversal() {
        let arrangement_uuid = uuid::Uuid::new_v4();
        let mut presentation = rv_data::Presentation {
            arrangements: vec![rv_data::presentation::Arrangement {
                uuid: Some(rv_data::Uuid {
                    string: arrangement_uuid.to_string(),
                }),
                name: "Default".to_string(),
                group_identifiers: vec![rv_data::Uuid {
                    string: "verse".to_string(),
                }],
            }],
            cues: vec![cue("verse-1")],
            cue_groups: vec![group("verse", &["verse-1"])],
            ..rv_data::Presentation::default()
        };

        assert_eq!(
            selectable_arrangement_uuid(&presentation, &presentation.arrangements[0]),
            Some(arrangement_uuid)
        );
        let selected = selectable_arrangement_by_name(&presentation, "default")
            .expect("case-insensitive checked selection");
        assert_eq!(selected.uuid(), arrangement_uuid);

        presentation.arrangements[0].group_identifiers[0].string = "missing".to_string();
        assert!(
            selectable_arrangement_uuid(&presentation, &presentation.arrangements[0]).is_none()
        );
    }

    #[test]
    fn duplicate_arrangement_uuid_is_neither_selectable_nor_traversed_arbitrarily() {
        let duplicate_uuid = uuid::Uuid::new_v4().to_string();
        let arrangement = |name: &str, group_id: &str| rv_data::presentation::Arrangement {
            uuid: Some(rv_data::Uuid {
                string: duplicate_uuid.clone(),
            }),
            name: name.to_string(),
            group_identifiers: vec![rv_data::Uuid {
                string: group_id.to_string(),
            }],
        };
        let presentation = rv_data::Presentation {
            selected_arrangement: Some(rv_data::Uuid {
                string: duplicate_uuid.clone(),
            }),
            arrangements: vec![
                arrangement("First", "first-group"),
                arrangement("Second", "second-group"),
            ],
            cues: vec![cue("first"), cue("second")],
            cue_groups: vec![
                group("first-group", &["first"]),
                group("second-group", &["second"]),
            ],
            ..rv_data::Presentation::default()
        };

        assert!(presentation.arrangements.iter().all(|arrangement| {
            selectable_arrangement_uuid(&presentation, arrangement).is_none()
        }));
        assert_eq!(operator_cue_indices(&presentation), vec![0, 1]);
    }

    #[test]
    fn dangling_selected_arrangement_does_not_fall_back_to_a_named_default() {
        let presentation = rv_data::Presentation {
            selected_arrangement: Some(rv_data::Uuid {
                string: "missing".to_string(),
            }),
            arrangements: vec![rv_data::presentation::Arrangement {
                uuid: Some(rv_data::Uuid {
                    string: "default".to_string(),
                }),
                name: "Default".to_string(),
                group_identifiers: vec![rv_data::Uuid {
                    string: "second-group".to_string(),
                }],
            }],
            cues: vec![cue("first"), cue("second")],
            cue_groups: vec![
                group("first-group", &["first"]),
                group("second-group", &["second"]),
            ],
            ..rv_data::Presentation::default()
        };

        assert_eq!(operator_cue_indices(&presentation), vec![0, 1]);
        assert_eq!(
            checked_operator_cue_indices(&presentation),
            Err(OperatorTraversalError::SelectedArrangementUnavailable {
                identifier: "missing".to_string(),
            })
        );
    }
}
