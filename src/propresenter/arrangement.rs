//! Operator-visible cue ordering for `ProPresenter` presentations.

use super::generated::rv_data;

/// Return cue indices in the order visible to a `ProPresenter` operator.
///
/// The selected arrangement owns the order when it resolves to presentation
/// groups. Without a usable arrangement, cue-group order is used. Presentations
/// without either structure fall back to their stored cue order.
#[must_use]
pub fn operator_cue_indices(presentation: &rv_data::Presentation) -> Vec<usize> {
    if let Some(arrangement) = selected_or_default_arrangement(presentation) {
        let mut arranged_indices = Vec::new();
        let complete = !arrangement.group_identifiers.is_empty()
            && arrangement.group_identifiers.iter().all(|group_id| {
                let Some(group) = presentation.cue_groups.iter().find(|group| {
                    group
                        .group
                        .as_ref()
                        .and_then(|group| group.uuid.as_ref())
                        .is_some_and(|uuid| uuid.string == group_id.string)
                }) else {
                    return false;
                };
                append_group_cue_indices(presentation, group, &mut arranged_indices)
            });
        if complete && !arranged_indices.is_empty() {
            return arranged_indices;
        }
    }

    let mut indices = Vec::new();
    for group in &presentation.cue_groups {
        append_group_cue_indices(presentation, group, &mut indices);
    }

    if indices.is_empty() {
        indices.extend(0..presentation.cues.len());
    }

    indices
}

fn selected_or_default_arrangement(
    presentation: &rv_data::Presentation,
) -> Option<&rv_data::presentation::Arrangement> {
    if let Some(selected) = &presentation.selected_arrangement {
        if let Some(arrangement) = presentation.arrangements.iter().find(|arrangement| {
            arrangement
                .uuid
                .as_ref()
                .is_some_and(|uuid| uuid.string == selected.string)
        }) {
            return Some(arrangement);
        }
    }

    presentation
        .arrangements
        .iter()
        .find(|arrangement| arrangement.name.eq_ignore_ascii_case("Default"))
}

fn append_group_cue_indices(
    presentation: &rv_data::Presentation,
    group: &rv_data::presentation::CueGroup,
    indices: &mut Vec<usize>,
) -> bool {
    for cue_id in &group.cue_identifiers {
        let Some(index) = presentation.cues.iter().position(|cue| {
            cue.uuid
                .as_ref()
                .is_some_and(|uuid| uuid.string == cue_id.string)
        }) else {
            return false;
        };
        if !indices.contains(&index) {
            indices.push(index);
        }
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
}
