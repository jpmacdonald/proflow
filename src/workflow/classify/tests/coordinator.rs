use std::{num::NonZeroUsize, path::Path};

use super::support::*;
use crate::planning_center::types::{Category, Item};
use crate::project_config::{
    parse_project_config_str, validate_project_config, DecisionChoiceConfig, DescriptionParserKind,
};
use crate::propresenter::library::LibraryCatalog;
use crate::workflow::classify::{
    build_plan, build_preview, render_preview, PreviewEntry, PreviewStatus, PreviewSummary,
};
use crate::workflow::classify_matching::decision_choice_matches;
use crate::workflow::description_parser::parse_description;
use crate::workflow::plan::{
    BackgroundTransform, CueTransform, ExistingTransform, MacroTransform, OutputKey,
    PlanDisposition, ReadyAction, ResolvedItemPlan,
};
use tempfile::tempdir;

#[test]
fn preview_summary_counts_each_status_once() {
    let entries = [
        PreviewStatus::Used,
        PreviewStatus::Created,
        PreviewStatus::Edited,
        PreviewStatus::Skipped,
        PreviewStatus::Uncertain,
    ]
    .into_iter()
    .map(|status| PreviewEntry {
        status,
        ..PreviewEntry::default()
    })
    .collect::<Vec<_>>();

    let summary = PreviewSummary::from_entries(&entries);

    assert_eq!(summary.used_count, 1);
    assert_eq!(summary.created_count, 1);
    assert_eq!(summary.edited_count, 1);
    assert_eq!(summary.skip_count, 1);
    assert_eq!(summary.uncertain_count, 1);
    assert_eq!(summary.total_playlist_items, 3);
}

#[test]
fn generated_name_that_normalizes_to_empty_requires_review() {
    let style = test_render_style(test_render_role("content", None), None);
    let mut plan = test_plan(PlanDisposition::Ready(ReadyAction::GenerateTitle {
        text: "Prayer".to_string(),
        style,
    }));
    plan.playlist_name = "(Prayer)".to_string();
    let mut plans = vec![plan];

    super::super::audit_mutable_presentation_target_collisions(&mut plans);

    assert!(matches!(
        plans[0].disposition,
        PlanDisposition::NeedsReview(_)
    ));
    assert!(plans[0]
        .reason
        .contains("has no safe filename characters after normalization"));
}

#[test]
fn build_preview_uses_fixture_rules_for_library_scripture_and_skip() {
    let config = load_config();
    let items = load_items();
    let (_library_dir, index) = fixture_library();

    let entries = build_preview(&items, &config, Some(&index), Some("Sunday Morning"));

    assert!(validate_project_config(config.as_raw()).is_empty());
    assert_eq!(entries.len(), 4);

    let call_to_worship = entries
        .iter()
        .find(|entry| entry.pco_title == "Call to Worship")
        .expect("call to worship entry");
    assert!(matches!(call_to_worship.status, PreviewStatus::Edited));
    assert!(call_to_worship
        .file_path
        .as_deref()
        .is_some_and(|path| path.ends_with("Call to Worship.pro")));
    assert_eq!(
        call_to_worship
            .parsed_content
            .as_ref()
            .map(|c| c.segments().len()),
        Some(3)
    );
    assert_eq!(
        call_to_worship.content_slide.as_deref(),
        Some("Call to Worship")
    );

    let song = entries
        .iter()
        .find(|entry| entry.pco_title == "Amazing Grace")
        .expect("song entry");
    assert!(matches!(song.status, PreviewStatus::Used));
    assert!(song
        .file_path
        .as_deref()
        .is_some_and(|path| path.ends_with("Amazing Grace.pro")));
    assert_eq!(song.item_type.as_deref(), Some("song"));
    assert_eq!(song.content_slide, None);

    let scripture = entries
        .iter()
        .find(|entry| entry.pco_title == "Scripture: John 3:16-17 NRSVue")
        .expect("scripture entry");
    assert!(matches!(scripture.status, PreviewStatus::Created));
    assert_eq!(
        scripture.scripture_reference.as_deref(),
        Some("John 3:16-17")
    );
    assert_eq!(scripture.bible_version.as_deref(), Some("NRSVue"));
    assert_eq!(scripture.content_slide.as_deref(), Some("Scripture"));

    let sermon = entries
        .iter()
        .find(|entry| entry.pco_title == "Sermon")
        .expect("sermon entry");
    assert!(matches!(sermon.status, PreviewStatus::Skipped));
    assert_eq!(sermon.reason, "Sermon is added day-of");
}

#[test]
fn output_keys_follow_pco_item_ids_when_plan_positions_change() {
    let config = load_config();
    let items = load_items();
    let (_library_dir, index) = fixture_library();
    let original = build_plan(&items, &config, Some(&index), Some("Sunday Morning"));

    let mut reordered = items;
    reordered.reverse();
    for (index, item) in reordered.iter_mut().enumerate() {
        item.position = index + 1;
    }
    let reordered = build_plan(&reordered, &config, Some(&index), Some("Sunday Morning"));

    let keys_by_title = |plans: &[ResolvedItemPlan]| {
        plans
            .iter()
            .map(|plan| (plan.pco_title.clone(), plan.output_key.clone()))
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    assert_eq!(keys_by_title(&original), keys_by_title(&reordered));
    assert_eq!(
        keys_by_title(&original)
            .get("Call to Worship")
            .map(crate::workflow::plan::OutputKey::as_str),
        Some("pco:item-1:main")
    );
}

#[test]
fn ordered_skip_rule_precedes_a_broader_fallback() {
    let config = parse_project_config_str(
        r#"
            {
              "version": 4,
              "presentation_types": {
                "song": {
                  "kind": "song",
                  "content_source": "song",
                  "output_strategy": "preserve_existing"
                }
              },
              "item_rules": [
                {
                  "id": "sermon_manual_only",
                  "match": { "title_prefix": ["sermon"] },
                  "action": {
                    "kind": "skip",
                    "reason": "Sermon slides are added manually after ProFlow builds"
                  }
                },
                {
                  "id": "wrong_sermon_fallback",
                  "match": { "title_prefix": ["sermon"] },
                  "use_type": "song"
                }
              ]
            }
            "#,
    )
    .expect("config should parse");
    let item = Item {
        id: "1".to_string(),
        position: 1,
        title: "Sermon".to_string(),
        description: None,
        category: Category::Text,
        note: None,
        song: None,
        scripture: None,
    };

    let entries = build_preview(&[item], &config, None, Some("Sunday Morning"));

    assert_eq!(entries.len(), 1);
    assert!(matches!(entries[0].status, PreviewStatus::Skipped));
    assert_eq!(
        entries[0].reason,
        "Sermon slides are added manually after ProFlow builds"
    );
    assert!(entries[0].item_type.is_none());
}

#[test]
fn v4_contextual_baptism_decision_selects_allowed_file() {
    let config = parse_project_config_str(
        r#"
            {
              "version": 4,
              "presentation_types": {
                "liturgical_static": {
                  "kind": "liturgy",
                  "content_source": "static",
                  "output_strategy": "preserve_existing"
                }
              },
              "item_rules": [
                {
                  "id": "baptism_contextual",
                  "match": { "title_prefix": ["baptism"] },
                  "decision": {
                    "kind": "choose_existing_file",
                    "context_fields": ["title", "description"],
                    "instructions": "Use Him for a boy, Her for a girl, Them for multiple candidates.",
                    "on_ambiguous": "ask",
                    "choices": {
                      "him": {
                        "use_type": "liturgical_static",
                        "file": "Baptism Him.pro",
                        "match": { "any": ["son of"] }
                      },
                      "her": {
                        "use_type": "liturgical_static",
                        "file": "Baptism Her.pro",
                        "match": { "any": ["daughter of"] }
                      },
                      "them": {
                        "use_type": "liturgical_static",
                        "file": "Baptism Them.pro",
                        "match": { "any": ["children of"] }
                      }
                    }
                  }
                }
              ]
            }
            "#,
    )
    .expect("config should parse");

    let library_dir = tempdir().expect("tempdir");
    write_library_presentation(&library_dir.path().join("Baptism Him.pro"));
    let index = LibraryCatalog::build(library_dir.path()).expect("fixture library should index");

    let plans = build_plan(
        &[Item {
            id: "1".to_string(),
            position: 1,
            title: "Baptism".to_string(),
            description: Some("James, son of Jane and John".to_string()),
            category: Category::Text,
            note: None,
            song: None,
            scripture: None,
        }],
        &config,
        Some(&index),
        None,
    );

    assert_eq!(plans.len(), 1);
    assert!(is_use_existing(&plans[0]));
    assert_eq!(plans[0].playlist_name, "Baptism Him");
    assert!(plans[0]
        .file_path()
        .and_then(Path::to_str)
        .is_some_and(|path| path.ends_with("Baptism Him.pro")));
    assert!(plans[0].render_style().is_none());
}

#[test]
fn contextual_choice_supports_all_only_and_none_only_matchers() {
    let all_only = DecisionChoiceConfig {
        match_spec: crate::project_config::DecisionChoiceMatch {
            all: vec!["child".to_string(), "baptism".to_string()],
            ..Default::default()
        },
        ..Default::default()
    };
    let none_only = DecisionChoiceConfig {
        match_spec: crate::project_config::DecisionChoiceMatch {
            none: vec!["private".to_string()],
            ..Default::default()
        },
        ..Default::default()
    };

    assert!(decision_choice_matches(
        &all_only,
        "child baptism during worship"
    ));
    assert!(!decision_choice_matches(&all_only, "child dedication"));
    assert!(decision_choice_matches(&none_only, "public baptism"));
    assert!(!decision_choice_matches(&none_only, "private baptism"));
}

#[test]
fn v4_contextual_decision_requires_review_when_ambiguous() {
    let config = parse_project_config_str(
        r#"
            {
              "version": 4,
              "presentation_types": {
                "liturgical_static": {
                  "kind": "liturgy",
                  "content_source": "static",
                  "output_strategy": "preserve_existing"
                }
              },
              "item_rules": [
                {
                  "id": "baptism_contextual",
                  "match": { "title_prefix": ["baptism"] },
                  "decision": {
                    "kind": "choose_existing_file",
                    "instructions": "Ask if unclear.",
                    "choices": {
                      "him": {
                        "use_type": "liturgical_static",
                        "file": "Baptism Him.pro",
                        "match": { "any": ["son of"] }
                      },
                      "her": {
                        "use_type": "liturgical_static",
                        "file": "Baptism Her.pro",
                        "match": { "any": ["daughter of"] }
                      }
                    }
                  }
                }
              ]
            }
            "#,
    )
    .expect("config should parse");

    let plans = build_plan(
        &[Item {
            id: "1".to_string(),
            position: 1,
            title: "Baptism".to_string(),
            description: Some("Baptism during worship".to_string()),
            category: Category::Text,
            note: None,
            song: None,
            scripture: None,
        }],
        &config,
        None,
        None,
    );

    assert_eq!(plans.len(), 1);
    assert!(plans[0].needs_review());
    assert!(plans[0].reason.contains("no choice matched"));
    assert!(plans[0].reason.contains("Ask if unclear."));
}

#[test]
fn duplicate_edit_in_place_targets_require_review_during_classification() {
    let config = mutable_target_collision_config();
    let library = tempdir().expect("temporary library");
    write_library_presentation(&library.path().join("Weekly Slot.pro"));
    let index = LibraryCatalog::build(library.path()).expect("fixture library should index");
    let items = vec![
        test_text_item("first", 1, "Edited First", Some("Leader: First text")),
        test_text_item("second", 2, "Edited Second", Some("Leader: Second text")),
    ];

    let plans = build_plan(&items, &config, Some(&index), None);

    assert_eq!(plans.len(), 2);
    assert!(plans.iter().all(ResolvedItemPlan::needs_review));
    assert_eq!(plans[0].reason, plans[1].reason);
    assert!(plans[0].reason.contains("mutable native file"));
    assert!(plans[0].reason.contains("Weekly Slot.pro"));
    assert!(plans[0].reason.contains("pco:first:main"));
    assert!(plans[0].reason.contains("pco:second:main"));
}

#[test]
fn canonical_generated_filename_collisions_require_review_in_preview() {
    let config = mutable_target_collision_config();
    let items = vec![
        test_text_item(
            "first",
            1,
            "Generated: Weekly (Hope)",
            Some("Leader: First text"),
        ),
        test_text_item(
            "second",
            2,
            "Generated - Weekly (Robert)",
            Some("Leader: Second text"),
        ),
    ];

    let plans = build_plan(&items, &config, None, None);
    let preview = render_preview(&plans);

    assert!(plans.iter().all(ResolvedItemPlan::needs_review));
    assert!(preview
        .iter()
        .all(|entry| matches!(entry.status, PreviewStatus::Uncertain)));
    assert_eq!(plans[0].reason, plans[1].reason);
    assert!(plans[0].reason.contains("generated file"));
    assert!(plans[0].reason.contains("Generated - Weekly.pro"));
    assert!(plans[0].reason.contains("pco:first:main"));
    assert!(plans[0].reason.contains("pco:second:main"));
}

fn restyle_plan(output_key: &str, playlist_name: &str, file_path: &str) -> ResolvedItemPlan {
    let transform = ExistingTransform::new(
        BackgroundTransform::Preserve,
        MacroTransform::Preserve,
        CueTransform::RetainOperatorPrefix(
            NonZeroUsize::new(1).expect("one is a non-zero cue limit"),
        ),
    )
    .expect("cue retention is a non-empty transform");
    let mut plan = test_plan(PlanDisposition::Ready(ReadyAction::RestyleExisting {
        file_path: file_path.into(),
        arrangement: None,
        transform,
    }));
    plan.output_key = OutputKey::new(output_key.to_string()).expect("valid output key");
    plan.playlist_name = playlist_name.to_string();
    plan
}

#[test]
fn restyles_of_one_native_file_with_different_playlist_names_require_review() {
    let mut plans = vec![
        restyle_plan("test:first", "First Display Name", "/library/Shared.pro"),
        restyle_plan("test:second", "Second Display Name", "/library/Shared.pro"),
    ];

    super::super::audit_mutable_presentation_target_collisions(&mut plans);

    assert!(plans.iter().all(ResolvedItemPlan::needs_review));
    assert_eq!(plans[0].reason, plans[1].reason);
    assert!(plans[0].reason.contains("mutable native file"));
    assert!(plans[0].reason.contains("/library/Shared.pro"));
    assert!(plans[0].reason.contains("test:first"));
    assert!(plans[0].reason.contains("test:second"));
}

#[test]
fn restyles_of_distinct_native_files_may_share_one_playlist_name() {
    let mut plans = vec![
        restyle_plan("test:first", "Shared Display Name", "/library/First.pro"),
        restyle_plan("test:second", "Shared Display Name", "/library/Second.pro"),
    ];

    super::super::audit_mutable_presentation_target_collisions(&mut plans);

    assert!(plans.iter().all(|plan| !plan.needs_review()));
    assert!(plans.iter().all(|plan| matches!(
        plan.ready_action(),
        Some(ReadyAction::RestyleExisting { .. })
    )));
}

#[test]
fn edit_and_restyle_of_one_native_file_share_one_ownership_key() {
    let mut edit = test_plan(PlanDisposition::Ready(ReadyAction::EditDescription {
        file_path: "/library/Shared.pro".into(),
        parsed_content: parse_description(
            "Edited native content",
            "Edited",
            DescriptionParserKind::Liturgical,
        )
        .expect("valid description")
        .expect("non-empty description"),
        style: test_render_style(test_render_role("content", None), None),
    }));
    edit.output_key = OutputKey::new("test:edit".to_string()).expect("valid output key");
    edit.playlist_name = "Edited Display Name".to_string();
    let restyle = restyle_plan(
        "test:restyle",
        "Restyled Display Name",
        "/library/Shared.pro",
    );
    let mut plans = vec![edit, restyle];

    super::super::audit_mutable_presentation_target_collisions(&mut plans);

    assert!(plans.iter().all(ResolvedItemPlan::needs_review));
    assert_eq!(plans[0].reason, plans[1].reason);
    assert!(plans[0].reason.contains("mutable native file"));
    assert!(plans[0].reason.contains("/library/Shared.pro"));
    assert!(plans[0].reason.contains("test:edit"));
    assert!(plans[0].reason.contains("test:restyle"));
}

#[test]
fn repeated_use_existing_targets_remain_valid_playlist_references() {
    let config = mutable_target_collision_config();
    let library = tempdir().expect("temporary library");
    write_library_presentation(&library.path().join("Reusable.pro"));
    let index = LibraryCatalog::build(library.path()).expect("fixture library should index");
    let items = vec![
        test_text_item("first", 1, "Existing First", None),
        test_text_item("second", 2, "Existing Second", None),
    ];

    let plans = build_plan(&items, &config, Some(&index), None);

    assert_eq!(plans.len(), 2);
    assert!(plans.iter().all(is_use_existing));
    assert_eq!(plans[0].file_path(), plans[1].file_path());
}

#[test]
fn selected_existing_presentation_with_wrong_size_requires_review() {
    let config = parse_project_config_str(
        r#"
            {
              "version": 4,
              "defaults": {
                "presentation_size": { "width": 1920, "height": 1080 }
              },
              "presentation_types": {
                "static_graphic": {
                  "kind": "graphic",
                  "content_source": "static",
                  "output_strategy": "preserve_existing"
                }
              },
              "item_rules": [{
                "id": "graphic",
                "match": { "title_prefix": ["graphic"] },
                "use_type": "static_graphic",
                "target": { "library_file": "Legacy.pro" }
              }]
            }
            "#,
    )
    .expect("size-audited config should parse");
    let library = tempdir().expect("temporary library");
    write_library_presentation_with_size(&library.path().join("Legacy.pro"), 1280.0, 720.0);
    let index = LibraryCatalog::build(library.path()).expect("fixture library should index");
    let items = vec![Item {
        id: "legacy".to_string(),
        position: 1,
        title: "Graphic".to_string(),
        description: None,
        category: Category::Graphic,
        note: None,
        song: None,
        scripture: None,
    }];

    let plans = build_plan(&items, &config, Some(&index), None);

    assert!(plans[0].needs_review());
    assert!(plans[0].reason.contains("1280x720"));
    assert!(plans[0].reason.contains("1920x1080"));
    assert!(plans[0].reason.contains("then reapply the theme"));
}

#[test]
fn restyled_same_aspect_presentation_is_normalized_without_review() {
    let config = parse_project_config_str(
        r#"
            {
              "version": 4,
              "defaults": {
                "presentation_size": { "width": 1920, "height": 1080 }
              },
              "presentation_types": {
                "managed": {
                  "kind": "graphic",
                  "content_source": "static",
                  "output_strategy": "restyle_existing",
                  "operator_cue_limit": 1
                }
              },
              "item_rules": [{
                "id": "managed",
                "match": { "title_prefix": ["managed"] },
                "use_type": "managed",
                "target": { "library_file": "Legacy.pro" }
              }]
            }
            "#,
    )
    .expect("managed resize config should parse");
    let library = tempdir().expect("temporary library");
    write_library_presentation_with_size(&library.path().join("Legacy.pro"), 1280.0, 720.0);
    let index = LibraryCatalog::build(library.path()).expect("fixture library should index");
    let items = vec![Item {
        id: "legacy".to_string(),
        position: 1,
        title: "Managed".to_string(),
        description: None,
        category: Category::Graphic,
        note: None,
        song: None,
        scripture: None,
    }];

    let plans = build_plan(&items, &config, Some(&index), None);

    assert!(matches!(
        plans[0].ready_action(),
        Some(ReadyAction::RestyleExisting { .. })
    ));
}
