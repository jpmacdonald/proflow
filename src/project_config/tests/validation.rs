use super::super::*;

#[test]
fn required_playlist_items_reference_static_existing_types_and_known_groups() {
    let valid = r#"
        {
          "version": 4,
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
          "required_playlist_items": [{
            "id": "pre_service",
            "use_type": "static_graphic",
            "library_file": "Pre-Service.pro",
            "placement": "start",
            "service_group": "weekly"
          }]
        }
        "#;
    let config =
        parse_project_config_str(valid).expect("a required static presentation should validate");
    assert_eq!(config.required_playlist_items().len(), 1);
    assert_eq!(
        config.required_playlist_items()[0].placement,
        RequiredPlaylistPlacement::Start
    );

    let invalid_group = valid.replace("\"weekly\"\n          }]", "\"missing\"\n          }]");
    let error = parse_project_config_str(&invalid_group)
        .expect_err("unknown service groups must fail validation");
    assert!(error
        .to_string()
        .contains("unknown service group 'missing'"));

    let invalid_type = valid.replace("\"preserve_existing\"", "\"generate_new\"");
    let error = parse_project_config_str(&invalid_type)
        .expect_err("required generated presentations must fail validation");
    assert!(error.to_string().contains("static preserve_existing"));
}

fn required_file_scope_config(
    first_group: Option<&str>,
    second_group: Option<&str>,
) -> serde_json::Value {
    serde_json::json!({
        "version": 4,
        "service_groups": {
            "weekly": { "service_types": ["Sunday Morning"] },
            "also_weekly": { "service_types": ["sunday morning"] },
            "seasonal": { "service_types": ["Christmas Eve"] }
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
                "id": "first",
                "use_type": "static_graphic",
                "library_file": "Pre-Service.pro",
                "placement": "start",
                "service_group": first_group
            },
            {
                "id": "second",
                "use_type": "static_graphic",
                "library_file": "pre-service",
                "placement": "end",
                "service_group": second_group
            }
        ]
    })
}

#[test]
fn rejects_required_file_ownership_in_overlapping_service_scopes() {
    for (first_group, second_group) in [
        (Some("weekly"), Some("also_weekly")),
        (None, Some("weekly")),
        (None, None),
    ] {
        let error =
            parse_project_config_value(required_file_scope_config(first_group, second_group))
                .expect_err("one required presentation cannot have overlapping owners");
        let message = error.to_string();
        assert!(
            message.contains("required_playlist_items[1].library_file"),
            "missing duplicate path in {message}"
        );
        assert!(
            message.contains("required_playlist_items[0] ('first')"),
            "missing original owner in {message}"
        );
        assert!(
            message.contains("overlapping service scope"),
            "missing scope reason in {message}"
        );
    }
}

#[test]
fn allows_required_file_ownership_in_disjoint_service_scopes() {
    let config =
        parse_project_config_value(required_file_scope_config(Some("weekly"), Some("seasonal")))
            .expect("disjoint service scopes may reuse one required presentation");

    assert_eq!(config.required_playlist_items().len(), 2);
}

#[test]
fn target_kind_must_match_the_rule_context() {
    let direct_template = r#"
        {
          "version": 4,
          "presentation_types": {
            "generated": {
              "kind": "liturgy",
              "content_source": "description",
              "description_parser": "liturgical",
              "output_strategy": "generate_new",
              "display": { "kind": "single", "role": "content" }
            }
          },
          "cue_roles": { "content": { "slide": "Content" } },
          "item_rules": [{
            "id": "generated",
            "match": { "title_prefix": ["generated"] },
            "use_type": "generated",
            "target": { "name_template": "{title}" }
          }]
        }
        "#;
    let error = parse_project_config_str(direct_template)
        .expect_err("direct rules must not silently ignore name_template");
    assert!(error.to_string().contains("speaker expansion"));

    let library_target = direct_template.replace(
        "\"name_template\": \"{title}\"",
        "\"library_file\": \"Generated.pro\"",
    );
    let error = parse_project_config_str(&library_target)
        .expect_err("generate_new must not accept a library_file target");
    assert!(error.to_string().contains("library_file requires"));
}

#[test]
fn accepts_explicit_body_text_slot_mapping() {
    let json = r#"
        {
          "version": 4,
          "cue_roles": {
            "content": {
              "slide": "Scripture (Projectors)",
              "text_slots": { "body": "Scripture" }
            }
          },
          "presentation_types": {
            "scripture": {
              "kind": "scripture",
              "content_source": "scripture",
              "output_strategy": "generate_new",
              "display": { "kind": "single", "role": "content" }
            }
          }
        }
        "#;

    let config = parse_project_config_str(json)
        .expect("a role with an explicit body text slot should validate");
    assert_eq!(
        config.cue_roles()["content"]
            .text_slots
            .get("body")
            .map(String::as_str),
        Some("Scripture")
    );
}

#[test]
fn rejects_duplicate_native_text_slot_targets() {
    let json = r#"
        {
          "version": 4,
          "cue_roles": {
            "content": {
              "slide": "Name Tag",
              "text_slots": {
                "body": "Name",
                "subtitle": "Name"
              }
            }
          }
        }
        "#;

    let error = parse_project_config_str(json)
        .expect_err("one native text destination cannot serve two semantic fields");
    assert!(error.to_string().contains("mapped more than once"));
}

#[test]
fn rejects_inexact_native_lookup_names() {
    let json = r#"
        {
          "version": 4,
          "cue_roles": {
            " content": {
              "slide": "Content ",
              "text_slots": { "body": "Body\nText" },
              "enter_macro": " Content Macro"
            }
          }
        }
        "#;

    let error = parse_project_config_str(json)
        .expect_err("native lookup names must remain exact and deterministic");
    let message = error.to_string();
    for expected in [
        "cue role name",
        "slide must be unpadded",
        "native text-slot names",
        "enter_macro must be unpadded",
    ] {
        assert!(
            message.contains(expected),
            "missing {expected:?} in {message}"
        );
    }
}

#[test]
fn rejects_display_role_without_required_body_text_slot() {
    let json = r#"
        {
          "version": 4,
          "cue_roles": {
            "content": {
              "slide": "Name Tag",
              "text_slots": { "title": "Name" }
            }
          },
          "presentation_types": {
            "generated": {
              "kind": "liturgy",
              "content_source": "description",
              "description_parser": "liturgical",
              "output_strategy": "generate_new",
              "display": { "kind": "single", "role": "content" }
            }
          }
        }
        "#;

    let error = parse_project_config_str(json)
        .expect_err("rendered display roles require a semantic body field");
    assert!(error.to_string().contains("required 'body' field"));
}

#[test]
fn rejects_split_display_with_one_role_identity() {
    let json = r#"
        {
          "version": 4,
          "cue_roles": {
            "content": { "slide": "Content" }
          },
          "presentation_types": {
            "generated": {
              "kind": "liturgy",
              "content_source": "description",
              "description_parser": "liturgical",
              "output_strategy": "generate_new",
              "display": {
                "kind": "split",
                "title": "content",
                "content": "content"
              }
            }
          }
        }
        "#;

    let error = parse_project_config_str(json)
        .expect_err("split regions must have distinct semantic identities");
    assert!(error
        .to_string()
        .contains("title and content must use different cue roles"));
}

#[test]
fn rejects_unknown_background_and_cue_role_references() {
    let json = r#"
        {
          "version": 4,
          "defaults": {
            "background": "missing_default",
            "presentation_size": { "width": 1920, "height": 1080 }
          },
          "presentation_types": {
            "generated": {
              "content_source": "description",
              "description_parser": "liturgical",
              "output_strategy": "generate_new",
              "display": { "kind": "single", "role": "missing_role" },
              "background": "missing_type"
            }
          },
          "overrides": [{
            "when": { "presentation_type": "generated" },
            "background": "missing_override"
          }]
        }
        "#;

    let error = parse_project_config_str(json).expect_err("unknown references must fail");
    let message = error.to_string();
    for expected in [
        "defaults.background",
        "missing_role",
        "missing_type",
        "missing_override",
    ] {
        assert!(
            message.contains(expected),
            "missing {expected:?} in {message}"
        );
    }
}

#[test]
fn rejects_styling_on_read_only_presentations() {
    let json = r#"
        {
          "version": 4,
          "backgrounds": { "default": "backgrounds/default.png" },
          "cue_roles": { "lyrics": { "slide": "Lyrics" } },
          "presentation_types": {
            "song": {
              "kind": "song",
              "content_source": "song",
              "output_strategy": "preserve_existing",
              "display": { "kind": "single", "role": "lyrics" },
              "background": "default",
              "max_lines_per_slide": 8,
              "arrangement": "Default"
            }
          }
        }
        "#;

    let error = parse_project_config_str(json).expect_err("read-only styling must fail");
    let message = error.to_string();
    for field in ["display", "background", "max_lines_per_slide"] {
        assert!(message.contains(field), "missing {field:?} in {message}");
    }
    assert!(!message.contains("arrangement is not valid"));
}

#[test]
fn validates_rendering_requirements_and_cue_role_macros() {
    let missing_display = r#"
        {
          "version": 4,
          "presentation_types": {
            "generated": {
              "content_source": "description",
              "description_parser": "liturgical",
              "output_strategy": "generate_new"
            }
          }
        }
        "#;
    let error = parse_project_config_str(missing_display)
        .expect_err("generate_new without display must fail");
    assert!(error.to_string().contains("requires a display binding"));

    let alternate_without_entry = r##"
        {
          "version": 4,
          "cue_roles": {
            "responsive": {
              "slide": "Responsive",
              "leader_enter_macro": "Highlighted",
              "speaker_colors": {
                "leader": "#FEDB4F",
                "audience": "#FFFFFF"
              }
            }
          }
        }
        "##;
    let error = parse_project_config_str(alternate_without_entry)
        .expect_err("alternate macro without entry macro must fail");
    assert!(error.to_string().contains("requires enter_macro"));

    let indistinguishable_speakers = r##"
        {
          "version": 4,
          "cue_roles": {
            "responsive": {
              "slide": "Responsive",
              "enter_macro": "Scripture/Prayer",
              "leader_enter_macro": "Highlighted",
              "speaker_colors": {
                "leader": "#FFFFFF",
                "audience": "#FFFFFF"
              }
            }
          }
        }
        "##;
    let error = parse_project_config_str(indistinguishable_speakers)
        .expect_err("speaker colors must preserve an observable distinction");
    assert!(error.to_string().contains("colors must differ"));

    let edit_without_display = r#"
        {
          "version": 4,
          "presentation_types": {
            "weekly": {
              "content_source": "description",
              "description_parser": "liturgical",
              "output_strategy": "edit_in_place"
            }
          }
        }
        "#;
    let error = parse_project_config_str(edit_without_display)
        .expect_err("edit_in_place without a display binding must fail");
    assert!(error
        .to_string()
        .contains("edit_in_place requires a display binding"));

    let edit_with_unbound_line_limit = r#"
        {
          "version": 4,
          "presentation_types": {
            "weekly": {
              "kind": "liturgy",
              "content_source": "description",
              "description_parser": "liturgical",
              "output_strategy": "edit_in_place",
              "max_lines_per_slide": 8
            }
          }
        }
        "#;
    let error = parse_project_config_str(edit_with_unbound_line_limit)
        .expect_err("an edit without a display binding must fail");
    assert!(error
        .to_string()
        .contains("edit_in_place requires a display binding"));
}

#[test]
fn validates_arrangements_only_for_existing_presentations() {
    let direct_rendered_arrangement = r#"
        {
          "version": 4,
          "cue_roles": { "content": { "slide": "Content" } },
          "presentation_types": {
            "rendered": {
              "kind": "liturgy",
              "content_source": "description",
              "description_parser": "liturgical",
              "output_strategy": "generate_new",
              "display": { "kind": "single", "role": "content" },
              "arrangement": "Standard"
            }
          }
        }
        "#;
    let error = parse_project_config_str(direct_rendered_arrangement)
        .expect_err("a rendered presentation must not declare an arrangement");
    assert!(error
        .to_string()
        .contains("arrangement is only valid for preserve_existing"));

    let targeted_rendered_arrangement = r#"
        {
          "version": 4,
          "cue_roles": { "content": { "slide": "Content" } },
          "presentation_types": {
            "rendered": {
              "kind": "liturgy",
              "content_source": "description",
              "description_parser": "liturgical",
              "output_strategy": "generate_new",
              "display": { "kind": "single", "role": "content" }
            }
          },
          "overrides": [{
            "when": { "presentation_type": "rendered" },
            "arrangement": "Seasonal"
          }]
        }
        "#;
    let error = parse_project_config_str(targeted_rendered_arrangement)
        .expect_err("an override must not assign an arrangement to rendered content");
    assert!(error
        .to_string()
        .contains("arrangement cannot target non-preserve_existing/non-restyle_existing"));

    let existing_and_broad = r#"
        {
          "version": 4,
          "cue_roles": { "content": { "slide": "Content" } },
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
        "#;
    parse_project_config_str(existing_and_broad)
        .expect("existing and broad arrangement configuration should remain valid");
}

#[test]
fn rejects_background_override_targeting_preserved_type() {
    let config = r#"
        {
          "version": 4,
          "backgrounds": { "seasonal": "backgrounds/seasonal.png" },
          "presentation_types": {
            "existing": {
              "kind": "graphic",
              "content_source": "static",
              "output_strategy": "preserve_existing"
            }
          },
          "overrides": [{
            "when": { "presentation_type": "existing" },
            "background": "seasonal"
          }]
        }
        "#;

    let error = parse_project_config_str(config)
        .expect_err("a background override must not target read-only existing content");
    assert!(error
        .to_string()
        .contains("background cannot target preserve_existing presentation type 'existing'"));
}

#[test]
fn rejects_conflicting_overrides_that_can_match_the_same_item() {
    let config = r#"
        {
          "version": 4,
          "service_groups": {
            "weekly": { "service_types": ["Sunday Morning"] }
          },
          "backgrounds": {
            "first": "backgrounds/first.png",
            "second": "backgrounds/second.png"
          },
          "overrides": [
            {
              "when": { "service_group": "weekly" },
              "background": "first",
              "arrangement": "First"
            },
            {
              "when": { "service_type": "Sunday Morning" },
              "background": "second",
              "arrangement": "Second"
            }
          ]
        }
        "#;

    let error = parse_project_config_str(config)
        .expect_err("overlapping config overrides must not depend on array order");
    let message = error.to_string();
    assert!(message.contains("overrides[1].background"));
    assert!(message.contains("conflicts with overrides[0].background"));
    assert!(message.contains("overrides[1].arrangement"));
    assert!(message.contains("conflicts with overrides[0].arrangement"));
}

#[test]
fn allows_disjoint_or_equivalent_overrides() {
    let config = r#"
        {
          "version": 4,
          "service_groups": {
            "weekly": { "service_types": ["Sunday Morning"] }
          },
          "backgrounds": {
            "first": "backgrounds/first.png",
            "second": "backgrounds/second.png"
          },
          "overrides": [
            {
              "when": { "service_group": "weekly" },
              "background": "first"
            },
            {
              "when": { "service_type": "Sunday Morning" },
              "background": "first"
            },
            {
              "when": { "service_type": "Monday Evening" },
              "background": "second"
            }
          ]
        }
        "#;

    parse_project_config_str(config)
        .expect("equivalent and mutually exclusive overrides should be order independent");
}

#[test]
fn rejects_an_override_with_an_impossible_service_scope() {
    let config = r#"
        {
          "version": 4,
          "service_groups": {
            "weekly": { "service_types": ["Sunday Morning"] }
          },
          "overrides": [{
            "when": {
              "service_group": "weekly",
              "service_type": "Christmas Eve"
            },
            "arrangement": "Seasonal"
          }]
        }
        "#;

    let error = parse_project_config_str(config)
        .expect_err("an override that can never match is dead configuration");
    assert!(error
        .to_string()
        .contains("service type 'Christmas Eve' is not a member of service group 'weekly'"));
}

#[test]
fn rejects_unsupported_content_output_and_kind_source_combinations() {
    for (name, body, expected) in [
        (
            "existing_description",
            r#"{
                  "kind": "liturgy",
                  "content_source": "description",
                  "description_parser": "liturgical",
                  "output_strategy": "preserve_existing"
                }"#,
            "description content is not supported by preserve_existing",
        ),
        (
            "edited_static",
            r#"{
                  "kind": "graphic",
                  "content_source": "static",
                  "output_strategy": "edit_in_place"
                }"#,
            "static content is not supported by edit_in_place",
        ),
        (
            "generated_song",
            r#"{
                  "kind": "song",
                  "content_source": "song",
                  "output_strategy": "generate_new"
                }"#,
            "song content is not supported by generate_new",
        ),
        (
            "song_source_without_song_kind",
            r#"{
                  "kind": "graphic",
                  "content_source": "song",
                  "output_strategy": "needs_review"
                }"#,
            "song content_source requires song kind",
        ),
        (
            "scripture_source_without_scripture_kind",
            r#"{
                  "kind": "liturgy",
                  "content_source": "scripture",
                  "output_strategy": "needs_review"
                }"#,
            "scripture content_source requires scripture kind",
        ),
    ] {
        let json = format!(
            r#"{{
                  "version": 4,
                  "presentation_types": {{ "{name}": {body} }}
                }}"#
        );
        let error = parse_project_config_str(&json)
            .expect_err("an unsupported presentation contract must fail validation");
        assert!(
            error.to_string().contains(expected),
            "missing {expected:?} in {error}"
        );
    }
}

#[test]
fn allows_song_semantics_from_an_existing_static_presentation() {
    let config = r#"
        {
          "version": 4,
          "presentation_types": {
            "doxology": {
              "kind": "song",
              "content_source": "static",
              "output_strategy": "preserve_existing"
            }
          }
        }
        "#;

    parse_project_config_str(config).expect("an existing static file may carry song semantics");
}

#[test]
fn allows_existing_static_scripture_presentations() {
    let config = r#"
        {
          "version": 4,
          "presentation_types": {
            "scripture_existing": {
              "kind": "scripture",
              "content_source": "static",
              "output_strategy": "preserve_existing"
            }
          }
        }
        "#;

    parse_project_config_str(config)
        .expect("an existing scripture presentation is a valid static source");
}

#[test]
fn validate_project_config_reports_unknown_refs() {
    let mut config = RawProjectConfig::default();
    config.item_rules.push(ItemRuleConfig {
        id: "bad_rule".to_string(),
        match_spec: MatchSpec {
            category: Some("text".to_string()),
            ..MatchSpec::default()
        },
        outcome: ItemRuleOutcome::UseType {
            type_key: "missing_type".to_string(),
            target: None,
        },
        notes: None,
    });
    config.overrides.push(OverrideRuleConfig {
        when: OverrideWhen {
            service_group: Some("missing_group".to_string()),
            presentation_type: Some("missing_type".to_string()),
            ..OverrideWhen::default()
        },
        ..OverrideRuleConfig::default()
    });

    let issues = validate_project_config(&config);
    assert_eq!(issues.len(), 3);
}

#[test]
fn rejects_empty_match() {
    let json = r#"
        {
          "version": 4,
          "item_rules": [{
            "id": "matches_everything",
            "match": {},
            "action": { "kind": "skip", "reason": "manual" }
          }]
        }
        "#;

    let error = parse_project_config_str(json).expect_err("empty match must be rejected");
    assert!(matches!(error, ProjectConfigLoadError::Invalid(_)));
    assert!(error.to_string().contains("at least one criterion"));
}

#[test]
fn rejects_library_paths_where_exact_filenames_are_required() {
    let json = r#"
        {
          "version": 4,
          "presentation_types": {
            "static": {
              "content_source": "static",
              "output_strategy": "preserve_existing"
            }
          },
          "item_rules": [{
            "id": "escaped_target",
            "match": { "title_prefix": ["welcome"] },
            "use_type": "static",
            "target": { "library_file": "folder/Welcome.pro" }
          }]
        }
        "#;

    let error = parse_project_config_str(json).expect_err("library paths must be rejected");
    assert!(matches!(error, ProjectConfigLoadError::Invalid(_)));
    assert!(error.to_string().contains("must be a filename, not a path"));
}

#[test]
fn rejects_duplicate_rule_ids() {
    let json = r#"
        {
          "version": 4,
          "item_rules": [
            {
              "id": "duplicate",
              "match": { "title_prefix": ["one"] },
              "action": { "kind": "skip", "reason": "one" }
            },
            {
              "id": "duplicate",
              "match": { "title_prefix": ["two"] },
              "action": { "kind": "skip", "reason": "two" }
            }
          ]
        }
        "#;

    let error = parse_project_config_str(json).expect_err("duplicate ids must be rejected");
    assert!(matches!(error, ProjectConfigLoadError::Invalid(_)));
    assert!(error.to_string().contains("duplicate item rule id"));
}

#[test]
fn rejects_out_of_range_lookahead_windows() {
    let json = r#"
        {
          "version": 4,
          "defaults": {
            "days_ahead": 0,
            "presentation_size": { "width": 1920, "height": 1080 }
          }
        }
        "#;

    let error = parse_project_config_str(json).expect_err("invalid days must be rejected");

    assert!(matches!(error, ProjectConfigLoadError::Invalid(_)));
    assert!(error.to_string().contains("defaults.days_ahead"));
}
