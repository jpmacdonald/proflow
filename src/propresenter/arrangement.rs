//! Operator-visible cue ordering for `ProPresenter` presentations.

use std::{
    collections::{HashMap, HashSet},
    num::NonZeroUsize,
};

use super::generated::rv_data;

/// Failure to retain a checked prefix of the operator-visible cue traversal.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RetainOperatorCuesError {
    /// The presentation has no cue that can be shown in operator order.
    #[error("presentation has no operator-visible cues")]
    EmptyTraversal,
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

/// Return cue indices in the order visible to a `ProPresenter` operator.
///
/// The selected arrangement owns the order when it resolves to presentation
/// groups. Without a usable arrangement, cue-group order is used. Presentations
/// without either structure fall back to their stored cue order.
#[must_use]
pub fn operator_cue_indices(presentation: &rv_data::Presentation) -> Vec<usize> {
    if let Some(arrangement) = selected_or_default_arrangement(presentation) {
        if let Some(arranged_indices) = resolved_arrangement_cue_indices(presentation, arrangement)
        {
            return arranged_indices;
        }
    }

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
    let traversal = operator_cue_indices(presentation);
    if traversal.is_empty() {
        return Err(RetainOperatorCuesError::EmptyTraversal);
    }
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
    let name = arrangement.name.as_str();
    if name.trim().is_empty() || name.trim() != name || name.chars().any(char::is_control) {
        return None;
    }
    let uuid = uuid::Uuid::parse_str(&arrangement.uuid.as_ref()?.string).ok()?;
    let matching_uuid_count = presentation
        .arrangements
        .iter()
        .filter_map(|candidate| candidate.uuid.as_ref())
        .filter_map(|candidate| uuid::Uuid::parse_str(&candidate.string).ok())
        .filter(|candidate| *candidate == uuid)
        .count();
    if matching_uuid_count != 1 {
        return None;
    }
    resolved_arrangement_cue_indices(presentation, arrangement)?;
    Some(uuid)
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
    presentation.arrangements.iter().any(|arrangement| {
        arrangement.name == name
            && selectable_arrangement_uuid(presentation, arrangement).as_ref() == Some(uuid)
    })
}

pub(crate) fn resolved_arrangement_cue_indices(
    presentation: &rv_data::Presentation,
    arrangement: &rv_data::presentation::Arrangement,
) -> Option<Vec<usize>> {
    if arrangement.group_identifiers.is_empty() {
        return None;
    }
    let mut indices = Vec::new();
    for group_id in &arrangement.group_identifiers {
        let mut groups = presentation.cue_groups.iter().filter(|group| {
            group
                .group
                .as_ref()
                .and_then(|group| group.uuid.as_ref())
                .is_some_and(|uuid| uuid.string == group_id.string)
        });
        let group = groups.next()?;
        if groups.next().is_some() || !append_group_cue_indices(presentation, group, &mut indices) {
            return None;
        }
    }
    (!indices.is_empty()).then_some(indices)
}

/// One ordered selected-arrangement group occurrence and its entry cue.
pub(crate) struct SelectedGroupEntry<'a> {
    pub(crate) name: &'a str,
    pub(crate) cue_index: usize,
}

/// Resolve every group occurrence in the selected/default arrangement.
///
/// Repeated group identifiers remain repeated occurrences. The complete
/// arrangement and each entry cue must resolve uniquely; otherwise no partial
/// region list escapes.
pub(crate) fn selected_group_entries(
    presentation: &rv_data::Presentation,
) -> Option<Vec<SelectedGroupEntry<'_>>> {
    let arrangement = selected_or_default_arrangement(presentation)?;
    // Region selection and operator traversal must describe the same native
    // structure. Reject a partially resolvable arrangement instead of pairing
    // its entry cues with the raw/group fallback traversal.
    resolved_arrangement_cue_indices(presentation, arrangement)?;
    let mut entries = Vec::with_capacity(arrangement.group_identifiers.len());
    for group_id in &arrangement.group_identifiers {
        let mut groups = presentation.cue_groups.iter().filter(|candidate| {
            candidate
                .group
                .as_ref()
                .and_then(|group| group.uuid.as_ref())
                .is_some_and(|uuid| uuid.string == group_id.string)
        });
        let group = groups.next()?;
        if groups.next().is_some() {
            return None;
        }
        let group_name = group.group.as_ref()?.name.as_str();
        let first_cue = group.cue_identifiers.first()?;
        let mut cues = presentation.cues.iter().enumerate().filter(|(_, cue)| {
            cue.uuid
                .as_ref()
                .is_some_and(|uuid| uuid.string == first_cue.string)
        });
        let (cue_index, _) = cues.next()?;
        if cues.next().is_some() {
            return None;
        }
        entries.push(SelectedGroupEntry {
            name: group_name,
            cue_index,
        });
    }
    (!entries.is_empty()).then_some(entries)
}

fn selected_or_default_arrangement(
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
                string: "service".to_string(),
            }),
            arrangements: vec![arrangement(
                "service",
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
            Some("service")
        );
        assert_eq!(operator_cue_indices(&presentation), vec![0]);
    }

    #[test]
    fn retaining_operator_prefix_prunes_removed_cues_from_a_surviving_group() {
        let mut content_group = group("content-group", &["content-1", "content-2"]);
        content_group.group.as_mut().expect("content group").name = "Content metadata".to_string();
        let mut presentation = rv_data::Presentation {
            selected_arrangement: Some(rv_data::Uuid {
                string: "service".to_string(),
            }),
            arrangements: vec![arrangement(
                "service",
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
    fn retaining_operator_prefix_removes_empty_arrangements_and_dangling_selection() {
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

        retain_first_operator_cues(
            &mut presentation,
            NonZeroUsize::new(1).expect("nonzero count"),
        )
        .expect("retain raw fallback prefix");

        assert!(presentation.selected_arrangement.is_none());
        assert_eq!(presentation.arrangements.len(), 1);
        assert_eq!(presentation.arrangements[0].name, "Alternate");
        assert_eq!(presentation.cue_groups.len(), 1);
        assert_eq!(presentation.cues.len(), 1);
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

        assert_eq!(error, RetainOperatorCuesError::EmptyTraversal);
        assert_eq!(presentation, original);
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

        assert!(selected_group_entries(&presentation).is_none());
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
    }
}
