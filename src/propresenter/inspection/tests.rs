#![allow(clippy::expect_used, clippy::panic)]

use super::*;
use crate::propresenter::generated::rv_data::{self, action};
use prost::Message;

fn real_fixture_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/propresenter/native/corpus")
}

#[test]
fn summarizes_style_signatures_from_a_standalone_presentation() {
    let fixture = real_fixture_dir().join("presentations/heidelberg-catechism-question-1.pro");
    let data = std::fs::read(fixture).expect("read fixture");
    let mut presentation =
        rv_data::Presentation::decode(data.as_slice()).expect("decode presentation");

    let summary = summarize_presentation_structure(&presentation);
    let title_cue = &summary.cues[0];
    assert_eq!(title_cue.macros, vec!["Name Tag/Title"]);
    let style = title_cue
        .text_styles
        .first()
        .expect("title cue should expose text style");
    assert_eq!(style.slide_size.as_deref(), Some("1920.0x1080.0"));
    assert!(style.bounds.is_some());
    assert!(style.font_size.is_some());
    assert!(style.color.is_some());

    mutate_first_text_font_size(&mut presentation, 12.0);
    let changed = summarize_presentation_structure(&presentation);
    assert_ne!(summary.cues[0].text_styles, changed.cues[0].text_styles);
}

#[test]
fn summarizes_scripture_labels_and_installed_group_bindings() {
    let cue_uuid = rv_data::Uuid {
        string: "CUE".to_string(),
    };
    let presentation = rv_data::Presentation {
        bible_reference: Some(rv_data::presentation::BibleReference {
            book_index: 42,
            book_name: "John".to_string(),
            chapter_range: Some(rv_data::IntRange { start: 3, end: 3 }),
            verse_range: Some(rv_data::IntRange { start: 16, end: 17 }),
            translation_name: "New Revised Standard Version Updated Edition".to_string(),
            translation_display_abbreviation: "NRSVue".to_string(),
            translation_internal_abbreviation: "NRSVUE".to_string(),
            book_key: "JHN".to_string(),
        }),
        cues: vec![rv_data::Cue {
            uuid: Some(cue_uuid.clone()),
            actions: vec![rv_data::Action {
                label: Some(action::Label {
                    text: "John 3:16-17".to_string(),
                    color: Some(rv_data::Color {
                        red: 1.0,
                        green: 0.0,
                        blue: 0.0,
                        alpha: 1.0,
                    }),
                }),
                action_type_data: Some(action::ActionTypeData::Slide(action::SlideType::default())),
                ..rv_data::Action::default()
            }],
            ..rv_data::Cue::default()
        }],
        cue_groups: vec![rv_data::presentation::CueGroup {
            group: Some(rv_data::Group {
                uuid: Some(rv_data::Uuid {
                    string: "GROUP".to_string(),
                }),
                name: "Verse".to_string(),
                color: Some(rv_data::Color {
                    red: 0.25,
                    green: 0.5,
                    blue: 0.75,
                    alpha: 1.0,
                }),
                hot_key: Some(rv_data::HotKey {
                    code: rv_data::KeyCode::AnsiV as i32,
                    control_identifier: "verse".to_string(),
                }),
                application_group_identifier: Some(rv_data::Uuid {
                    string: "APPLICATION-GROUP".to_string(),
                }),
                application_group_name: "Verse".to_string(),
            }),
            cue_identifiers: vec![cue_uuid],
        }],
        ..rv_data::Presentation::default()
    };

    let summary = summarize_presentation_structure(&presentation);

    assert_eq!(
        summary.bible_reference,
        Some(BibleReferenceSummary {
            book_index: 42,
            book_name: "John".to_string(),
            chapter_range: Some(IntRangeSummary { start: 3, end: 3 }),
            verse_range: Some(IntRangeSummary { start: 16, end: 17 }),
            translation_name: "New Revised Standard Version Updated Edition".to_string(),
            translation_display_abbreviation: "NRSVue".to_string(),
            translation_internal_abbreviation: "NRSVUE".to_string(),
            book_key: "JHN".to_string(),
        })
    );
    assert_eq!(
        summary.cues[0].slide_labels,
        vec![ActionLabelSignature {
            text: "John 3:16-17".to_string(),
            color: Some("#FF0000FF".to_string()),
        }]
    );
    assert_eq!(summary.cue_groups[0].color.as_deref(), Some("#4080BFFF"));
    assert_eq!(
        summary.cue_groups[0].hot_key,
        Some(HotKeySignature {
            code: rv_data::KeyCode::AnsiV as i32,
            control_identifier: "verse".to_string(),
        })
    );
    assert_eq!(
        summary.cue_groups[0]
            .application_group_identifier
            .as_deref(),
        Some("APPLICATION-GROUP")
    );
    assert_eq!(summary.cue_groups[0].application_group_name, "Verse");
}

#[test]
fn operator_indexes_use_the_canonical_repeated_arrangement_traversal() {
    let cue = |id: &str| rv_data::Cue {
        uuid: Some(rv_data::Uuid {
            string: id.to_string(),
        }),
        ..rv_data::Cue::default()
    };
    let group = |id: &str, cue_id: &str| rv_data::presentation::CueGroup {
        group: Some(rv_data::Group {
            uuid: Some(rv_data::Uuid {
                string: id.to_string(),
            }),
            ..rv_data::Group::default()
        }),
        cue_identifiers: vec![rv_data::Uuid {
            string: cue_id.to_string(),
        }],
    };
    let presentation = rv_data::Presentation {
        selected_arrangement: Some(rv_data::Uuid {
            string: "default".to_string(),
        }),
        arrangements: vec![rv_data::presentation::Arrangement {
            uuid: Some(rv_data::Uuid {
                string: "default".to_string(),
            }),
            name: "Default".to_string(),
            group_identifiers: ["verse", "chorus", "chorus"]
                .into_iter()
                .map(|id| rv_data::Uuid {
                    string: id.to_string(),
                })
                .collect(),
        }],
        cue_groups: vec![group("verse", "verse-cue"), group("chorus", "chorus-cue")],
        cues: vec![cue("verse-cue"), cue("chorus-cue")],
        ..rv_data::Presentation::default()
    };

    let summary = summarize_presentation_structure(&presentation);
    let canonical = crate::propresenter::arrangement::operator_cue_indices(&presentation);

    assert_eq!(summary.operator_cue_indexes, canonical);
    assert_eq!(summary.operator_cue_indexes, vec![0, 1, 1]);
}

#[test]
fn dangling_references_are_explicit_and_change_the_semantic_summary() {
    let clean = presentation_with_reference_graph();
    let mut malformed = clean.clone();
    malformed.cue_groups[0]
        .cue_identifiers
        .push(native_uuid("missing-cue"));
    malformed.arrangements[0]
        .group_identifiers
        .push(native_uuid("missing-group"));

    let clean_summary = summarize_presentation_structure(&clean);
    let malformed_summary = summarize_presentation_structure(&malformed);

    assert!(clean_summary.reference_diagnostics.is_empty());
    assert!(malformed_summary.reference_diagnostics.iter().any(|issue| {
        matches!(
            issue,
            PresentationReferenceDiagnostic::DanglingCueReference {
                cue_group_index: 0,
                reference_index: 1,
                uuid,
            } if uuid == "missing-cue"
        )
    }));
    assert!(malformed_summary.reference_diagnostics.iter().any(|issue| {
        matches!(
            issue,
            PresentationReferenceDiagnostic::DanglingGroupReference {
                arrangement_index: 0,
                reference_index: 2,
                uuid,
            } if uuid == "missing-group"
        )
    }));
    assert_ne!(clean_summary, malformed_summary);
}

#[test]
fn duplicate_uuids_are_not_collapsed_into_arbitrary_reference_targets() {
    let clean = presentation_with_reference_graph();
    let mut malformed = clean.clone();
    malformed.cues[1].uuid = Some(native_uuid("cue-a"));
    malformed.cue_groups[1]
        .group
        .as_mut()
        .expect("second group")
        .uuid = Some(native_uuid("group-a"));

    let clean_summary = summarize_presentation_structure(&clean);
    let malformed_summary = summarize_presentation_structure(&malformed);

    assert!(malformed_summary.reference_diagnostics.iter().any(|issue| {
        matches!(
            issue,
            PresentationReferenceDiagnostic::DuplicateCueUuid { uuid, cue_indexes }
                if uuid == "cue-a" && cue_indexes == &[0, 1]
        )
    }));
    assert!(malformed_summary.reference_diagnostics.iter().any(|issue| {
        matches!(
            issue,
            PresentationReferenceDiagnostic::DuplicateCueGroupUuid {
                uuid,
                cue_group_indexes,
            } if uuid == "group-a" && cue_group_indexes == &[0, 1]
        )
    }));
    assert!(malformed_summary.reference_diagnostics.iter().any(|issue| {
        matches!(
            issue,
            PresentationReferenceDiagnostic::AmbiguousCueReference {
                uuid,
                cue_indexes,
                ..
            } if uuid == "cue-a" && cue_indexes == &[0, 1]
        )
    }));
    assert!(malformed_summary.reference_diagnostics.iter().any(|issue| {
        matches!(
            issue,
            PresentationReferenceDiagnostic::AmbiguousGroupReference {
                uuid,
                cue_group_indexes,
                ..
            } if uuid == "group-a" && cue_group_indexes == &[0, 1]
        )
    }));
    assert!(malformed_summary.cue_groups[0].cue_indexes.is_empty());
    assert!(malformed_summary.arrangements[0].group_names.is_empty());
    assert_ne!(clean_summary, malformed_summary);
}

#[test]
fn duplicate_and_dangling_arrangement_selection_is_diagnosed() {
    let mut duplicate = presentation_with_reference_graph();
    duplicate
        .arrangements
        .push(duplicate.arrangements[0].clone());

    let duplicate_summary = summarize_presentation_structure(&duplicate);
    assert!(duplicate_summary.reference_diagnostics.iter().any(|issue| {
        matches!(
            issue,
            PresentationReferenceDiagnostic::DuplicateArrangementUuid {
                uuid,
                arrangement_indexes,
            } if uuid == "arrangement" && arrangement_indexes == &[0, 1]
        )
    }));
    assert!(duplicate_summary.reference_diagnostics.iter().any(|issue| {
        matches!(
            issue,
            PresentationReferenceDiagnostic::AmbiguousSelectedArrangement {
                uuid,
                arrangement_indexes,
            } if uuid == "arrangement" && arrangement_indexes == &[0, 1]
        )
    }));

    let mut dangling = presentation_with_reference_graph();
    dangling.selected_arrangement = Some(native_uuid("missing-arrangement"));
    let dangling_summary = summarize_presentation_structure(&dangling);
    assert!(dangling_summary.reference_diagnostics.iter().any(|issue| {
        matches!(
            issue,
            PresentationReferenceDiagnostic::DanglingSelectedArrangement { uuid }
                if uuid == "missing-arrangement"
        )
    }));
}

fn presentation_with_reference_graph() -> rv_data::Presentation {
    rv_data::Presentation {
        selected_arrangement: Some(native_uuid("arrangement")),
        arrangements: vec![rv_data::presentation::Arrangement {
            uuid: Some(native_uuid("arrangement")),
            name: "Default".to_string(),
            group_identifiers: vec![native_uuid("group-a"), native_uuid("group-b")],
        }],
        cue_groups: vec![
            native_group("group-a", "First", &["cue-a"]),
            native_group("group-b", "Second", &["cue-b"]),
        ],
        cues: vec![native_cue("cue-a"), native_cue("cue-b")],
        ..rv_data::Presentation::default()
    }
}

fn native_uuid(value: &str) -> rv_data::Uuid {
    rv_data::Uuid {
        string: value.to_string(),
    }
}

fn native_cue(uuid: &str) -> rv_data::Cue {
    rv_data::Cue {
        uuid: Some(native_uuid(uuid)),
        ..rv_data::Cue::default()
    }
}

fn native_group(uuid: &str, name: &str, cue_uuids: &[&str]) -> rv_data::presentation::CueGroup {
    rv_data::presentation::CueGroup {
        group: Some(rv_data::Group {
            uuid: Some(native_uuid(uuid)),
            name: name.to_string(),
            ..rv_data::Group::default()
        }),
        cue_identifiers: cue_uuids.iter().map(|uuid| native_uuid(uuid)).collect(),
    }
}

fn mutate_first_text_font_size(presentation: &mut rv_data::Presentation, size: f64) {
    for cue in &mut presentation.cues {
        for action in &mut cue.actions {
            let Some(action::ActionTypeData::Slide(slide_type)) = &mut action.action_type_data
            else {
                continue;
            };
            let Some(action::slide_type::Slide::Presentation(slide)) = &mut slide_type.slide else {
                continue;
            };
            let Some(base_slide) = &mut slide.base_slide else {
                continue;
            };
            for element in &mut base_slide.elements {
                let Some(graphics) = &mut element.element else {
                    continue;
                };
                let Some(text) = &mut graphics.text else {
                    continue;
                };
                let attributes = text.attributes.get_or_insert_with(Default::default);
                let font = attributes.font.get_or_insert_with(Default::default);
                font.size = size;
                return;
            }
        }
    }
    panic!("fixture should contain at least one text element");
}
