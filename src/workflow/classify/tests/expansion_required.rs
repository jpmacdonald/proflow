use std::path::Path;

use super::super::expansion::resolve_speaker;
use super::support::*;
use crate::planning_center::types::{Category, Item};
use crate::project_config::parse_project_config_str;
use crate::propresenter::library::LibraryCatalog;
use crate::workflow::classify::build_plan;
use crate::workflow::plan::{
    CueTransform, ItemKind, MacroTransform, ReadyAction, ResolvedItemPlan,
};
use tempfile::tempdir;

fn assert_managed_speaker_nametag(plan: &ResolvedItemPlan) {
    assert_eq!(plan.output_key, "pco:1:expand:0:title_static");
    assert_eq!(plan.item_type.as_deref(), Some("title_static"));
    assert!(plan
        .file_path()
        .and_then(Path::to_str)
        .is_some_and(|path| path.ends_with("Hope Nametag.pro")));
    assert_eq!(plan.playlist_name, "Hope Nametag");
    let Some(ReadyAction::RestyleExisting { transform, .. }) = plan.ready_action() else {
        panic!("speaker nametag should use the configured generic restyle policy");
    };
    assert_eq!(
        transform
            .replacement_background()
            .map(|background| background.id().as_str()),
        Some("default")
    );
    assert!(matches!(
        transform.cues(),
        CueTransform::RetainOperatorPrefix(limit) if limit.get() == 1
    ));
    let MacroTransform::Enforce(macros) = transform.macros() else {
        panic!("speaker nametag should enforce the configured macro policy");
    };
    assert_eq!(macros.regions()[0].enter_macro(), "Name Tag/Title");
}

#[test]
fn expansion_outputs_have_stable_keys_and_respect_declared_type() {
    let config = parse_project_config_str(
        r#"
            {
              "version": 4,
              "defaults": { "background": "default" },
              "backgrounds": { "default": "backgrounds/default.png" },
              "cue_roles": {
                "content": { "slide": "Content" }
              },
              "presentation_types": {
                "title_static": {
                  "kind": "nametag",
                  "content_source": "static",
                  "output_strategy": "restyle_existing",
                  "background": "default",
                  "operator_cue_limit": 1,
                  "macro_transitions": {
                    "regions": [{
                      "selector": { "kind": "operator_cue", "index": 0 },
                      "enter_macro": "Name Tag/Title"
                    }]
                  }
                },
                "liturgical_edited": {
                  "kind": "liturgy",
                  "content_source": "description",
                  "description_parser": "liturgical",
                  "output_strategy": "edit_in_place",
                  "display": { "kind": "single", "role": "content" }
                }
              },
              "people": {
                "Hope": {
                  "nametag": "Hope Nametag"
                }
              },
              "item_rules": [
                {
                  "id": "call_to_worship_bundle",
                  "match": { "title_prefix": ["call to worship"] },
                  "expand": [
                    { "use_type": "title_static", "speaker": "resolved" },
                    {
                      "use_type": "liturgical_edited",
                      "speaker": "resolved",
                      "target": { "library_file": "Call to Worship.pro" }
                    }
                  ]
                }
              ]
            }
            "#,
    )
    .expect("config should parse");

    let library_dir = tempdir().expect("tempdir");
    write_library_presentation(&library_dir.path().join("Hope Nametag.pro"));
    write_library_presentation(&library_dir.path().join("Call to Worship.pro"));
    let index = LibraryCatalog::build(library_dir.path()).expect("fixture library should index");

    let items = vec![Item {
        id: "1".to_string(),
        position: 1,
        title: "Call to Worship (Hope)".to_string(),
        description: Some("Leader: Grace and peace".to_string()),
        category: Category::Text,
        note: None,
        song: None,
        scripture: None,
    }];

    let plans = build_plan(&items, &config, Some(&index), None);
    assert_eq!(plans.len(), 2);
    assert_managed_speaker_nametag(&plans[0]);

    assert_eq!(plans[1].output_key, "pco:1:expand:1:liturgical_edited");
    assert_eq!(plans[1].item_type.as_deref(), Some("liturgical_edited"));
    assert!(is_edit_description(&plans[1]));
    assert!(plans[1]
        .file_path()
        .and_then(Path::to_str)
        .is_some_and(|path| path.ends_with("Call to Worship.pro")));

    let unresolved = build_plan(
        &[Item {
            id: "2".to_string(),
            position: 2,
            title: "Call to Worship (Unknown Person)".to_string(),
            description: Some("Leader: Grace and peace".to_string()),
            category: Category::Text,
            note: None,
            song: None,
            scripture: None,
        }],
        &config,
        Some(&index),
        None,
    );
    assert_eq!(unresolved.len(), 2);
    assert!(unresolved.iter().all(ResolvedItemPlan::needs_review));
    assert_eq!(unresolved[0].item_kind, ItemKind::Nametag);
    assert_eq!(unresolved[1].item_kind, ItemKind::Liturgy);
}

#[test]
fn speaker_resolution_requires_a_configured_person() {
    let config = parse_project_config_str(
        r#"
            {
              "version": 4,
              "people": {
                "Hope": { "last": "Lee", "nametag": "Hope Nametag" }
              }
            }
            "#,
    )
    .expect("config should parse");

    assert_eq!(
        resolve_speaker("Call to Worship (Hope)", None, &config).as_deref(),
        Some("Hope")
    );
    assert_eq!(
        resolve_speaker("Call to Worship (Jordan)", None, &config),
        None
    );
    assert_eq!(
        resolve_speaker("Call to Worship", Some("Liturgist: Jordan"), &config),
        None
    );
    assert_eq!(
        resolve_speaker("Call to Worship", Some("Liturgist: Hope Lee"), &config).as_deref(),
        Some("Hope Lee")
    );
    assert_eq!(
        resolve_speaker("Call to Worship (Hope & Robert)", None, &config),
        None
    );
    assert_eq!(
        resolve_speaker("Call to Worship", Some("Liturgist: Hope leads"), &config),
        None
    );
}

#[test]
fn expanded_outputs_preserve_distinct_keys_even_when_file_matches_repeat() {
    let config = parse_project_config_str(
        r#"
            {
              "version": 4,
              "presentation_types": {
                "static_slide": {
                  "kind": "graphic",
                  "content_source": "static",
                  "output_strategy": "preserve_existing"
                }
              },
              "item_rules": [
                {
                  "id": "bundle",
                  "match": { "title_prefix": ["welcome"] },
                  "expand": [
                    {
                      "use_type": "static_slide",
                      "target": { "library_file": "Announcements.pro" }
                    },
                    {
                      "use_type": "static_slide",
                      "target": { "library_file": "Announcements.pro" }
                    }
                  ]
                }
              ]
            }
            "#,
    )
    .expect("config should parse");

    let library_dir = tempdir().expect("tempdir");
    write_library_presentation(&library_dir.path().join("Announcements.pro"));
    let index = LibraryCatalog::build(library_dir.path()).expect("fixture library should index");

    let items = vec![Item {
        id: "1".to_string(),
        position: 1,
        title: "Welcome".to_string(),
        description: None,
        category: Category::Graphic,
        note: None,
        song: None,
        scripture: None,
    }];

    let plans = build_plan(&items, &config, Some(&index), None);
    assert_eq!(plans.len(), 2);
    assert_eq!(plans[0].output_key, "pco:1:expand:0:static_slide");
    assert_eq!(plans[1].output_key, "pco:1:expand:1:static_slide");
    assert_eq!(plans[0].file_path(), plans[1].file_path());
}

#[test]
fn required_playlist_items_insert_at_edges_without_duplicating_pco_matches() {
    let config = parse_project_config_str(
        r#"
            {
              "version": 4,
              "defaults": {
                "presentation_size": { "width": 1920, "height": 1080 }
              },
              "service_groups": {
                "weekly": { "service_types": ["Sunday Morning"] }
              },
              "presentation_types": {
                "static_graphic": {
                  "kind": "graphic",
                  "content_source": "static",
                  "output_strategy": "preserve_existing"
                }
              },
              "required_playlist_items": [
                {
                  "id": "pre_service",
                  "use_type": "static_graphic",
                  "library_file": "Pre-Service.pro",
                  "placement": "start",
                  "service_group": "weekly"
                },
                {
                  "id": "closing",
                  "use_type": "static_graphic",
                  "library_file": "Closing.pro",
                  "placement": "end",
                  "service_group": "weekly"
                }
              ],
              "item_rules": [{
                "id": "pre_service",
                "match": { "title_prefix": ["pre-service"] },
                "use_type": "static_graphic",
                "target": { "library_file": "Pre-Service.pro" }
              }]
            }
            "#,
    )
    .expect("required playlist config should parse");
    let library = tempdir().expect("temporary library");
    for name in ["Pre-Service.pro", "Closing.pro"] {
        write_library_presentation_with_size(&library.path().join(name), 1920.0, 1080.0);
    }
    let index = LibraryCatalog::build(library.path()).expect("fixture library should index");
    let items = vec![
        test_text_item("ordinary-before", 5, "Ordinary Before", None),
        test_text_item("pre", 10, "Pre-Service Slides", None),
        test_text_item("ordinary-after", 15, "Ordinary After", None),
    ];

    let plans = build_plan(&items, &config, Some(&index), Some("Sunday Morning"));

    assert_eq!(
        plans.len(),
        4,
        "the PCO pre-service item must be replaced by one required item"
    );
    assert_eq!(plans[0].output_key, "required:pre_service");
    assert_eq!(plans[1].output_key, "pco:ordinary-before:main");
    assert_eq!(plans[2].output_key, "pco:ordinary-after:main");
    assert_eq!(plans[3].output_key, "required:closing");
    assert!(!plans.iter().any(|plan| plan.output_key == "pco:pre:main"));
    assert!(is_use_existing(&plans[3]));
    assert!(plans[3]
        .file_path()
        .and_then(Path::to_str)
        .is_some_and(|path| path.ends_with("Closing.pro")));

    let inserted = build_plan(&[], &config, Some(&index), Some("Sunday Morning"));
    assert_eq!(
        inserted
            .iter()
            .map(|plan| plan.output_key.as_str())
            .collect::<Vec<_>>(),
        vec!["required:pre_service", "required:closing"]
    );

    let other_service = build_plan(&items, &config, Some(&index), Some("Christmas Eve"));
    assert_eq!(other_service.len(), 3);
    assert_eq!(other_service[1].output_key, "pco:pre:main");
}
