use super::support::*;
use crate::planning_center::types::{Category, Item};
use crate::project_config::{parse_project_config_str, ExistingSource, PresentationPolicy};
use crate::propresenter::library::LibraryCatalog;
use crate::workflow::classify::{build_plan, PreviewEntry};
use crate::workflow::description_parser::{ParsedContent, ParsedSegment, SpeakerRole};
use crate::workflow::plan::{CueMacro, PlanDisposition, ReadyAction, RenderRole};
use tempfile::tempdir;

#[test]
fn edit_in_place_without_parsed_content_requires_review() {
    let config = parse_project_config_str(
        r#"
            {
              "version": 4,
              "cue_roles": {
                "content": {
                  "slide": "Content",
                  "enter_macro": "Scripture/Prayer",
                  "leader_enter_macro": "Scripture/Prayer (Highlighted)",
                  "speaker_colors": {
                    "leader": "\u0023FEDB4F",
                    "audience": "\u0023FFFFFF"
                  }
                }
              },
              "presentation_types": {
                "weekly": {
                  "kind": "liturgy",
                  "content_source": "description",
                  "description_parser": "liturgical",
                  "output_strategy": "edit_in_place",
                  "display": { "kind": "single", "role": "content" }
                }
              },
              "item_rules": [{
                "id": "weekly",
                "match": { "title_prefix": ["weekly"] },
                "use_type": "weekly",
                "target": { "library_file": "Weekly.pro" }
              }]
            }
            "#,
    )
    .expect("edit config should parse");
    let library_dir = tempdir().expect("tempdir");
    write_library_presentation(&library_dir.path().join("Weekly.pro"));
    let index = LibraryCatalog::build(library_dir.path()).expect("fixture library should index");
    let item = Item {
        id: "weekly".to_string(),
        position: 1,
        title: "Weekly Text".to_string(),
        description: None,
        category: Category::Text,
        note: None,
        song: None,
        scripture: None,
    };

    let plans = build_plan(&[item], &config, Some(&index), None);

    assert!(plans[0].needs_review());
    assert_eq!(plans[0].reason, "No description content to edit");
}

#[test]
fn content_nametag_without_description_uses_the_item_title() {
    let config = parse_project_config_str(
        r#"
            {
              "version": 4,
              "cue_roles": {
                "title": { "slide": "Title" }
              },
              "presentation_types": {
                "sermon_title": {
                  "kind": "nametag",
                  "content_source": "description",
                  "description_parser": "content_nametag",
                  "output_strategy": "generate_new",
                  "display": { "kind": "single", "role": "title" }
                }
              },
              "item_rules": [{
                "id": "sermon_title",
                "match": { "title_prefix": ["sermon"] },
                "use_type": "sermon_title"
              }]
            }
            "#,
    )
    .expect("sermon-title config should parse");
    let item = Item {
        id: "sermon".to_string(),
        position: 1,
        title: "Sermon - Daily Bread (Hope)".to_string(),
        description: None,
        category: Category::Title,
        note: None,
        song: None,
        scripture: None,
    };

    let plans = build_plan(&[item], &config, None, None);

    let Some(ReadyAction::GenerateDescription { parsed_content, .. }) = plans[0].ready_action()
    else {
        panic!("title-only content nametag should be renderable");
    };
    assert_eq!(parsed_content.segments().len(), 1);
    assert_eq!(parsed_content.segments()[0].text, "Sermon - Daily Bread");
    assert_eq!(parsed_content.segments()[0].speaker, SpeakerRole::Neutral);
    assert!(parsed_content.segments()[0].bold.is_none());
    assert!(parsed_content.segments()[0].italic.is_none());
    assert!(parsed_content.title_text().is_none());
}

#[test]
fn description_placeholders_block_edit_and_generation_plans() {
    let config = parse_project_config_str(
        r#"
            {
              "version": 4,
              "cue_roles": {
                "content": {
                  "slide": "Content",
                  "enter_macro": "Scripture/Prayer",
                  "leader_enter_macro": "Scripture/Prayer (Highlighted)",
                  "speaker_colors": {
                    "leader": "\u0023FEDB4F",
                    "audience": "\u0023FFFFFF"
                  }
                }
              },
              "presentation_types": {
                "weekly_edit": {
                  "kind": "liturgy",
                  "content_source": "description",
                  "description_parser": "liturgical",
                  "output_strategy": "edit_in_place",
                  "display": { "kind": "single", "role": "content" }
                },
                "weekly_generate": {
                  "kind": "liturgy",
                  "content_source": "description",
                  "description_parser": "liturgical",
                  "output_strategy": "generate_new",
                  "display": { "kind": "single", "role": "content" }
                }
              },
              "item_rules": [
                {
                  "id": "weekly_edit",
                  "match": { "title_prefix": ["weekly edit"] },
                  "use_type": "weekly_edit",
                  "target": { "library_file": "Weekly.pro" }
                },
                {
                  "id": "weekly_generate",
                  "match": { "title_prefix": ["weekly generate"] },
                  "use_type": "weekly_generate"
                }
              ]
            }
            "#,
    )
    .expect("placeholder config should parse");
    let library_dir = tempdir().expect("tempdir");
    write_library_presentation(&library_dir.path().join("Weekly.pro"));
    let index = LibraryCatalog::build(library_dir.path()).expect("fixture library should index");
    let description =
        "[CONFESSION no slide] - introduction\n[SLIDE/ALL] - [insert prayer]\n[SILENT CONFESSION]";
    let items = [
        Item {
            id: "weekly-edit".to_string(),
            position: 1,
            title: "Weekly Edit".to_string(),
            description: Some(description.to_string()),
            category: Category::Text,
            note: None,
            song: None,
            scripture: None,
        },
        Item {
            id: "weekly-generate".to_string(),
            position: 2,
            title: "Weekly Generate".to_string(),
            description: Some(description.to_string()),
            category: Category::Text,
            note: None,
            song: None,
            scripture: None,
        },
    ];

    let plans = build_plan(&items, &config, Some(&index), None);

    assert_eq!(plans.len(), 2);
    for plan in plans {
        assert!(plan.needs_review());
        assert_eq!(
            plan.reason,
            "Unresolved description placeholder 'insert prayer'"
        );
        assert!(plan.parsed_content().is_none());
    }
}

#[test]
fn preview_selects_leader_macro_only_for_the_leader_content_region() {
    let parsed_content = ParsedContent::new(
        vec![ParsedSegment {
            text: "Leader response".to_string(),
            speaker: SpeakerRole::Leader,
            bold: None,
            italic: None,
        }],
        Some("Title".to_string()),
    );
    let split = PreviewEntry::from(test_plan(PlanDisposition::Ready(
        ReadyAction::GenerateDescription {
            parsed_content: parsed_content.clone(),
            style: test_render_style(
                test_render_role(
                    "Content",
                    Some(
                        CueMacro::new(
                            "Content".to_string(),
                            Some("Content Highlighted".to_string()),
                        )
                        .expect("test cue macro should be valid"),
                    ),
                ),
                Some(test_render_role(
                    "Title",
                    Some(
                        CueMacro::new("Title".to_string(), Some("Title Highlighted".to_string()))
                            .expect("test cue macro should be valid"),
                    ),
                )),
            ),
        },
    )));
    let single = PreviewEntry::from(test_plan(PlanDisposition::Ready(
        ReadyAction::GenerateDescription {
            parsed_content,
            style: test_render_style(
                test_render_role(
                    "Content",
                    Some(
                        CueMacro::new(
                            "Content".to_string(),
                            Some("Content Highlighted".to_string()),
                        )
                        .expect("test cue macro should be valid"),
                    ),
                ),
                None,
            ),
        },
    )));

    assert_eq!(split.first_cue_macro.as_deref(), Some("Title"));
    assert_eq!(
        split.first_content_cue_macro.as_deref(),
        Some("Content Highlighted")
    );
    assert_eq!(
        single.first_cue_macro.as_deref(),
        Some("Content Highlighted")
    );
    assert_eq!(single.first_content_cue_macro, None);
}

#[test]
fn broad_arrangement_override_only_changes_use_existing_style() {
    let config = parse_project_config_str(
        r#"
            {
              "version": 4,
              "cue_roles": {
                "content": {
                  "slide": "Content",
                  "enter_macro": "Scripture/Prayer",
                  "leader_enter_macro": "Scripture/Prayer (Highlighted)",
                  "speaker_colors": {
                    "leader": "\u0023FEDB4F",
                    "audience": "\u0023FFFFFF"
                  }
                }
              },
              "presentation_types": {
                "existing": {
                  "kind": "graphic",
                  "content_source": "static",
                  "output_strategy": "preserve_existing",
                  "arrangement": "Default"
                },
                "rendered": {
                  "kind": "liturgy",
                  "content_source": "description",
                  "description_parser": "liturgical",
                  "output_strategy": "generate_new",
                  "display": { "kind": "single", "role": "content" }
                }
              },
              "overrides": [{
                "when": { "service_type": "Christmas Eve" },
                "arrangement": "Seasonal"
              }]
            }
            "#,
    )
    .expect("arrangement config should parse");

    let Some(PresentationPolicy::PreserveExisting {
        source: ExistingSource::Static,
        arrangement,
        ..
    }) = config.presentation_policy("existing")
    else {
        panic!("existing should compile as a static preserve-existing policy");
    };
    let existing_arrangement = arrangement.for_service(Some("Christmas Eve"));
    let Some(PresentationPolicy::GenerateDescription { render, .. }) =
        config.presentation_policy("rendered")
    else {
        panic!("rendered should compile as generated description");
    };
    let rendered = render.for_service(Some("Christmas Eve"));

    assert_eq!(existing_arrangement.as_deref(), Some("Seasonal"));
    assert_eq!(rendered.content().slide(), "Content");
}

#[test]
fn static_restyle_does_not_inherit_the_generated_default_background() {
    let config = parse_project_config_str(
        r#"
            {
              "version": 4,
              "defaults": { "background": "default" },
              "backgrounds": { "default": "backgrounds/default.png" },
              "presentation_types": {
                "liturgy": {
                  "kind": "liturgy",
                  "content_source": "static",
                  "output_strategy": "restyle_existing",
                  "macro_transitions": {
                    "regions": [{
                      "selector": { "kind": "operator_cue", "index": 0 },
                      "enter_macro": "Scripture/Prayer"
                    }]
                  }
                },
                "graphic": {
                  "kind": "graphic",
                  "content_source": "static",
                  "output_strategy": "preserve_existing"
                }
              },
              "item_rules": [
                {
                  "id": "liturgy",
                  "match": { "title_prefix": ["liturgy"] },
                  "use_type": "liturgy",
                  "target": { "library_file": "Liturgy.pro" }
                },
                {
                  "id": "graphic",
                  "match": { "title_prefix": ["graphic"] },
                  "use_type": "graphic",
                  "target": { "library_file": "Graphic.pro" }
                }
              ]
            }
            "#,
    )
    .expect("static background policy should parse");
    let library_dir = tempdir().expect("tempdir");
    write_library_presentation(&library_dir.path().join("Liturgy.pro"));
    write_library_presentation(&library_dir.path().join("Graphic.pro"));
    let index = LibraryCatalog::build(library_dir.path()).expect("fixture library should index");
    let items = [
        Item {
            id: "liturgy".to_string(),
            position: 1,
            title: "Liturgy".to_string(),
            description: None,
            category: Category::Text,
            note: None,
            song: None,
            scripture: None,
        },
        Item {
            id: "graphic".to_string(),
            position: 2,
            title: "Graphic".to_string(),
            description: None,
            category: Category::Graphic,
            note: None,
            song: None,
            scripture: None,
        },
    ];

    let plans = build_plan(&items, &config, Some(&index), None);

    let Some(ReadyAction::RestyleExisting { transform, .. }) = plans[0].ready_action() else {
        panic!("reusable liturgy should be restyled");
    };
    assert!(transform.replacement_background().is_none());
    let crate::workflow::plan::MacroTransform::Enforce(macro_transitions) = transform.macros()
    else {
        panic!("restyle should enforce macros");
    };
    assert_eq!(macro_transitions.regions().len(), 1);
    assert_eq!(
        macro_transitions.regions()[0].enter_macro(),
        "Scripture/Prayer"
    );
    assert!(matches!(
        plans[1].ready_action(),
        Some(ReadyAction::UseExisting { .. })
    ));
    assert!(plans[1].background().is_none());
}

#[test]
fn description_generate_new_uses_strategy_not_edited_fallback() {
    let config = parse_project_config_str(
        r#"
            {
              "version": 4,
              "cue_roles": {
                "information": {
                  "slide": "Information (Projectors)",
                  "enter_macro": "Scripture/Prayer",
                  "leader_enter_macro": "Scripture/Prayer (Highlighted)",
                  "speaker_colors": {
                    "leader": "\u0023FEDB4F",
                    "audience": "\u0023FFFFFF"
                  }
                }
              },
              "presentation_types": {
                "liturgical_edited": {
                  "kind": "liturgy",
                  "content_source": "description",
                  "description_parser": "liturgical",
                  "output_strategy": "generate_new",
                  "display": { "kind": "single", "role": "information" }
                }
              },
              "item_rules": [
                {
                  "id": "weekly_text",
                  "match": { "title_prefix": ["weekly text"] },
                  "use_type": "liturgical_edited"
                }
              ]
            }
            "#,
    )
    .expect("config should parse");

    let items = vec![Item {
        id: "1".to_string(),
        position: 1,
        title: "Weekly Text".to_string(),
        description: Some("Leader: Grace and peace".to_string()),
        category: Category::Text,
        note: None,
        song: None,
        scripture: None,
    }];

    let plans = build_plan(&items, &config, None, None);
    assert_eq!(plans.len(), 1);
    assert!(matches!(
        plans[0].ready_action(),
        Some(ReadyAction::GenerateDescription { .. })
    ));
    assert!(plans[0].parsed_content().is_some());
}

#[test]
fn use_existing_missing_target_requires_review_instead_of_skip() {
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
                  "id": "static_slide",
                  "match": { "title_prefix": ["welcome bumper"] },
                  "use_type": "static_slide",
                  "target": { "library_file": "Missing Slide.pro" }
                }
              ]
            }
            "#,
    )
    .expect("config should parse");

    let items = vec![Item {
        id: "1".to_string(),
        position: 1,
        title: "Welcome Bumper".to_string(),
        description: None,
        category: Category::Graphic,
        note: None,
        song: None,
        scripture: None,
    }];

    let plans = build_plan(&items, &config, None, None);
    assert_eq!(plans.len(), 1);
    assert!(plans[0].needs_review());
    assert_eq!(
        plans[0].reason,
        "Configured existing file not found: Missing Slide.pro"
    );
}

#[test]
fn v4_split_display_resolves_cue_roles() {
    let config = parse_project_config_str(
        r#"
            {
              "version": 4,
              "cue_roles": {
                "title": {
                  "slide": "Information (Projectors)",
                  "enter_macro": "Name Tag/Title"
                },
                "scripture_prayer": {
                  "slide": "Scripture (Projectors)",
                  "text_slots": { "body": "Scripture Body" },
                  "enter_macro": "Scripture/Prayer"
                }
              },
              "presentation_types": {
                "scripture": {
                  "kind": "scripture",
                  "content_source": "scripture",
                  "output_strategy": "generate_new",
                  "display": {
                    "kind": "split",
                    "title": "title",
                    "content": "scripture_prayer"
                  }
                }
              },
              "item_rules": [
                {
                  "id": "scripture",
                  "match": {
                    "title_prefix": ["scripture"],
                    "has_scripture_ref": true
                  },
                  "use_type": "scripture"
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
            title: "Scripture: John 3:16 NRSVue".to_string(),
            description: None,
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
    let style = plans[0].render_style().expect("generated scripture style");
    assert_eq!(
        style.title().map(RenderRole::slide),
        Some("Information (Projectors)")
    );
    assert_eq!(style.content().slide(), "Scripture (Projectors)");
    assert_eq!(
        style.content().text_slots().get("body").map(String::as_str),
        Some("Scripture Body")
    );
    assert_eq!(
        style
            .title()
            .and_then(RenderRole::cue_macro)
            .map(CueMacro::enter),
        Some("Name Tag/Title")
    );
    assert_eq!(
        style.content().cue_macro().map(CueMacro::enter),
        Some("Scripture/Prayer")
    );
}
