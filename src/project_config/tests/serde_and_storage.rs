use super::super::*;
use std::path::Path;
use tempfile::tempdir;

const VALID_V4_CONFIG: &str = r##"
        {
          "version": 4,
          "defaults": {
            "theme": "VPC Theme",
            "background": "default",
            "presentation_size": { "width": 1920, "height": 1080 }
          },
          "backgrounds": {
            "default": "backgrounds/default.png"
          },
          "cue_roles": {
            "title": {
              "slide": "Information (Projectors)",
              "enter_macro": "Name Tag/Title"
            },
            "responsive": {
              "slide": "Scripture (Projectors) (Responsive)",
              "enter_macro": "Scripture/Prayer",
              "leader_enter_macro": "Scripture/Prayer (Highlighted)",
              "speaker_colors": {
                "leader": "#FEDB4F",
                "audience": "#FFFFFF"
              }
            }
          },
          "service_groups": {
            "seasonal": {
              "service_types": ["Christmas Eve"]
            }
          },
          "presentation_types": {
            "liturgical_weekly": {
              "kind": "liturgy",
              "content_source": "description",
              "description_parser": "liturgical",
              "output_strategy": "edit_in_place",
              "display": {
                "kind": "split",
                "title": "title",
                "content": "responsive"
              },
              "background": "default",
              "max_lines_per_slide": 8,
              "description": "Weekly liturgy"
            },
            "person_nametag": {
              "kind": "nametag",
              "content_source": "static",
              "output_strategy": "preserve_existing"
            }
          },
          "item_rules": [
            {
              "id": "call_to_worship",
              "match": {
                "title_prefix": ["call to worship"]
              },
              "use_type": "liturgical_weekly",
              "target": {
                "library_file": "Call to Worship.pro"
              }
            },
            {
              "id": "welcome_bundle",
              "match": {
                "title_prefix": ["welcome"]
              },
              "expand": [
                {
                  "use_type": "person_nametag",
                  "speaker": "resolved"
                },
                {
                  "use_type": "liturgical_weekly"
                }
              ]
            }
          ],
          "people": {
            "Robert": {
              "last": "Austell",
              "role": "pastor",
              "nametag": "Robert Nametag"
            }
          }
        }
        "##;

#[test]
fn parse_v4_config() {
    let config = parse_project_config_str(VALID_V4_CONFIG).expect("v4 config should parse");
    assert_eq!(config.as_raw().version, 4);
    assert_eq!(config.defaults().library.as_str(), "Default");
    assert_eq!(config.defaults().theme.as_deref(), Some("VPC Theme"));
    let presentation_size = config.defaults().presentation_size;
    assert_eq!(presentation_size.width(), 1920);
    assert_eq!(presentation_size.height(), 1080);
    assert_eq!(
        config
            .defaults()
            .background
            .as_ref()
            .map(BackgroundId::as_str),
        Some("default")
    );
    let default_background = BackgroundId::new("default").expect("valid background id");
    assert_eq!(
        config.backgrounds()[&default_background].as_path(),
        Path::new("backgrounds/default.png")
    );
    assert!(config
        .as_raw()
        .presentation_types
        .contains_key("liturgical_weekly"));
    assert!(config.people().contains_key("Robert"));
    assert_eq!(config.as_raw().item_rules.len(), 2);
    assert_eq!(config.as_raw().item_rules[0].id, "call_to_worship");
    let target_file = match &config.as_raw().item_rules[0].outcome {
        ItemRuleOutcome::UseType { target, .. } => {
            target.as_ref().and_then(TargetSpec::library_file)
        }
        ItemRuleOutcome::Action(_) | ItemRuleOutcome::Decision(_) | ItemRuleOutcome::Expand(_) => {
            None
        }
    };
    assert_eq!(target_file, Some("Call to Worship.pro"));
}

#[test]
fn library_name_is_one_typed_path_component() {
    let config = parse_project_config_str(
        r#"{
            "version": 4,
            "defaults": { "library": "Sunday" }
        }"#,
    )
    .expect("normal library name should parse");
    assert_eq!(config.defaults().library.as_str(), "Sunday");

    for invalid in [
        "",
        " ../Default",
        "../Default",
        "Nested/Default",
        "Default\\Other",
    ] {
        let json = serde_json::json!({
            "version": 4,
            "defaults": { "library": invalid }
        });
        parse_project_config_value(json).expect_err("unsafe library name must fail");
    }
}

#[test]
fn presentation_size_rejects_zero_dimensions() {
    for value in [
        serde_json::json!({"width": 0, "height": 1080}),
        serde_json::json!({"width": 1920, "height": 0}),
    ] {
        serde_json::from_value::<crate::propresenter::PresentationSize>(value)
            .expect_err("zero dimensions must not deserialize");
    }
}

#[test]
fn target_spec_requires_exactly_one_target_kind() {
    for invalid in [
        serde_json::json!({}),
        serde_json::json!({
            "library_file": "Welcome.pro",
            "name_template": "{speaker} Welcome"
        }),
    ] {
        let error = serde_json::from_value::<TargetSpec>(invalid)
            .expect_err("ambiguous target should not deserialize");
        assert!(error
            .to_string()
            .contains("exactly one of library_file or name_template"));
    }

    for valid in [
        serde_json::json!({"library_file": "Welcome.pro"}),
        serde_json::json!({"name_template": "{speaker} Welcome"}),
    ] {
        let target = serde_json::from_value::<TargetSpec>(valid.clone())
            .expect("single target kind should deserialize");
        assert_eq!(
            serde_json::to_value(target).expect("target should serialize"),
            valid
        );
    }
}

#[test]
fn typed_library_identity_round_trips() {
    let value = serde_json::json!({
        "id": "edition_specific_hymn",
        "match": {
            "kind": "title_prefix",
            "values": ["g2g #840 it is well with my soul"]
        },
        "use_type": "hymn",
        "library_file": "[Hymn] It Is Well With My Soul (G2G).pro",
        "notes": "Distinct wording"
    });

    let identity = serde_json::from_value::<LibraryIdentityConfig>(value.clone())
        .expect("tagged identity should deserialize");
    assert!(matches!(
        identity.match_spec,
        LibraryIdentityMatch::TitlePrefix { ref values }
            if values.iter().map(String::as_str).eq(["g2g #840 it is well with my soul"])
    ));
    assert_eq!(
        serde_json::to_value(identity).expect("identity should serialize"),
        value
    );
}

#[test]
fn parses_tagged_single_and_split_display_bindings() {
    let json = r##"
        {
          "version": 4,
          "cue_roles": {
            "title": {
              "slide": "Information (Projectors)"
            },
            "content": {
              "slide": "Scripture (Projectors)",
              "enter_macro": "Scripture/Prayer",
              "leader_enter_macro": "Scripture/Prayer (Highlighted)",
              "speaker_colors": {
                "leader": "#FEDB4F",
                "audience": "#FFFFFF"
              }
            }
          },
          "presentation_types": {
            "single": {
              "kind": "liturgy",
              "content_source": "description",
              "description_parser": "liturgical",
              "output_strategy": "generate_new",
              "display": {
                "kind": "single",
                "role": "content"
              }
            },
            "split": {
              "kind": "scripture",
              "content_source": "scripture",
              "output_strategy": "generate_new",
              "display": {
                "kind": "split",
                "title": "title",
                "content": "content"
              }
            }
          }
        }
        "##;

    let config = parse_project_config_str(json).expect("tagged bindings should parse");
    assert!(matches!(
        &config.as_raw().presentation_types["single"].display,
        Some(DisplayBindingConfig::Single { role }) if role == "content"
    ));
    assert!(matches!(
        &config.as_raw().presentation_types["split"].display,
        Some(DisplayBindingConfig::Split {
            title,
            content
        }) if title == "title" && content == "content"
    ));
}

#[test]
fn rejects_invalid_background_ids() {
    let invalid_ids = [
        "",
        "Uppercase",
        "-leading-dash",
        "contains.dot",
        "nonascii-é",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ];
    for id in invalid_ids {
        let json = format!(r#"{{"version":4,"backgrounds":{{"{id}":"backgrounds/default.png"}}}}"#);
        let error = parse_project_config_str(&json)
            .expect_err("invalid background id must be rejected during parsing");
        assert!(
            matches!(error, ProjectConfigLoadError::Parse(_)),
            "unexpected error for {id:?}: {error}"
        );
    }
    assert!(BackgroundId::new("default-1_2").is_ok());
}

#[test]
fn rejects_invalid_background_asset_paths() {
    for path in [
        "../secret.png",
        "/tmp/background.png",
        "backgrounds/./default.png",
        "backgrounds/default.gif",
        "backgrounds\\default.png",
        "C:/background.png",
        "backgrounds/default\0.png",
    ] {
        let value = serde_json::json!({
            "version": 4,
            "backgrounds": { "default": path }
        });
        let error = parse_project_config_value(value)
            .expect_err("invalid background path must be rejected during parsing");
        assert!(
            matches!(error, ProjectConfigLoadError::Parse(_)),
            "unexpected error for {path:?}: {error}"
        );
    }
    assert!(BackgroundAssetPath::new("backgrounds/default.TIFF").is_ok());
}

#[test]
fn reject_legacy_config() {
    let json = r#"{ "theme": "Legacy", "item_types": {} }"#;
    let err = parse_project_config_str(json).unwrap_err();
    assert!(
        matches!(err, ProjectConfigLoadError::MissingVersion),
        "expected MissingVersion, got: {err}"
    );
}

#[test]
fn reject_v1_config() {
    let json = r#"{ "version": 1, "theme": "Legacy" }"#;
    let err = parse_project_config_str(json).unwrap_err();
    assert!(
        matches!(err, ProjectConfigLoadError::UnsupportedVersion(1)),
        "expected UnsupportedVersion(1), got: {err}"
    );
}

#[test]
fn starter_project_config_is_valid() {
    parse_project_config_str(include_str!("../../../examples/starter-config.json"))
        .expect("starter config should parse and validate");
}

#[test]
fn rejects_v3_config() {
    let value = serde_json::json!({
        "version": 3,
        "metadata": {
            "name": "Example"
        }
    });

    let error = parse_project_config_value(value).expect_err("v3 must be rejected");
    assert!(matches!(
        error,
        ProjectConfigLoadError::UnsupportedVersion(3)
    ));
}

#[test]
fn write_project_config_round_trips() {
    let dir = tempdir().expect("tempdir should create");
    let path = dir.path().join("proflow.config.json");
    let mut config = RawProjectConfig::default();
    config.metadata.name = Some("Round Trip".to_string());

    write_project_config(&path, &config).expect("config should write");
    let loaded = load_project_config(&path).expect("config should reload");

    assert_eq!(loaded.as_raw().metadata.name.as_deref(), Some("Round Trip"));
}

#[test]
fn raw_config_must_compile_before_runtime_use() {
    let mut raw = RawProjectConfig::default();
    raw.defaults.days_ahead = Some(0);

    let error = ProjectConfig::try_from(raw).expect_err("invalid raw config must not compile");

    assert!(error
        .issues()
        .iter()
        .any(|issue| issue.path == "defaults.days_ahead"));
}

#[test]
fn write_rejects_an_invalid_raw_candidate() {
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("proflow.config.json");
    let mut raw = RawProjectConfig::default();
    raw.defaults.days_ahead = Some(0);

    let error = write_project_config(&path, &raw).expect_err("invalid raw config must not write");

    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(!path.exists());
}

#[test]
fn checked_config_preserves_the_raw_json_shape() {
    let checked = parse_project_config_str(VALID_V4_CONFIG).expect("valid config should compile");

    assert_eq!(
        serde_json::to_value(&checked).expect("checked config should serialize"),
        serde_json::to_value(checked.as_raw()).expect("raw config should serialize")
    );
}

#[test]
fn config_maps_serialize_in_stable_key_order() {
    let mut raw = RawProjectConfig::default();
    raw.backgrounds.insert(
        BackgroundId::new("zeta").expect("valid background id"),
        BackgroundAssetPath::new("backgrounds/zeta.png").expect("valid background path"),
    );
    raw.backgrounds.insert(
        BackgroundId::new("alpha").expect("valid background id"),
        BackgroundAssetPath::new("backgrounds/alpha.png").expect("valid background path"),
    );

    let serialized = serde_json::to_string_pretty(&raw).expect("serialize raw config");
    let alpha = serialized.find("\"alpha\"").expect("alpha key");
    let zeta = serialized.find("\"zeta\"").expect("zeta key");

    assert!(alpha < zeta, "config map keys must be deterministic");
}

#[test]
fn typed_rule_outcomes_round_trip_through_flat_json() {
    let config = parse_project_config_str(include_str!("../../../examples/starter-config.json"))
        .expect("starter config should parse");

    assert!(matches!(
        &config.as_raw().item_rules[0].outcome,
        ItemRuleOutcome::Action(RuleAction::Skip { .. })
    ));
    assert!(matches!(
        &config.as_raw().item_rules[1].outcome,
        ItemRuleOutcome::UseType { type_key, .. } if type_key == "song"
    ));
    assert!(matches!(
        &config.as_raw().item_rules[3].outcome,
        ItemRuleOutcome::Expand(expansion) if expansion.iter().count() == 2
    ));

    let serialized = serialize_project_config(config.as_raw()).expect("config should serialize");
    let value: serde_json::Value =
        serde_json::from_str(&serialized).expect("serialized config should be JSON");
    assert_eq!(value["item_rules"][1]["use_type"], "song");
    assert!(value["item_rules"][1].get("action").is_none());
    assert_eq!(
        value["item_rules"][3]["expand"].as_array().map(Vec::len),
        Some(2)
    );

    parse_project_config_str(&serialized).expect("serialized config should parse again");
}

#[test]
fn item_rule_tier_is_explicit_and_primary_is_omitted() {
    let config = parse_project_config_str(
        r#"{
          "version": 4,
          "item_rules": [
            {
              "id": "specific",
              "match": { "title_prefix": ["sermon"] },
              "action": { "kind": "skip", "reason": "specific" }
            },
            {
              "id": "catch_all",
              "tier": "catch_all",
              "match": { "category": "text" },
              "action": { "kind": "skip", "reason": "fallback" }
            }
          ]
        }"#,
    )
    .expect("tiered rules should parse");

    assert_eq!(config.as_raw().item_rules[0].tier, RuleTier::Primary);
    assert_eq!(config.as_raw().item_rules[1].tier, RuleTier::CatchAll);

    let value = serde_json::to_value(config.as_raw()).expect("config should serialize");
    assert!(value["item_rules"][0].get("tier").is_none());
    assert_eq!(value["item_rules"][1]["tier"], "catch_all");
}

#[test]
fn rejects_unknown_nested_field() {
    let json = r#"
        {
          "version": 4,
          "item_rules": [{
            "id": "typo",
            "match": { "title_prefx": ["sermon"] },
            "action": { "kind": "skip", "reason": "manual" }
          }]
        }
        "#;

    let error = parse_project_config_str(json).expect_err("typo must be rejected");
    assert!(matches!(error, ProjectConfigLoadError::Parse(_)));
    assert!(error.to_string().contains("title_prefx"));
}

#[test]
fn rejects_contradictory_rule_outcomes() {
    let json = r#"
        {
          "version": 4,
          "presentation_types": {
            "static": { "output_strategy": "preserve_existing" }
          },
          "item_rules": [{
            "id": "contradictory",
            "match": { "title_prefix": ["sermon"] },
            "use_type": "static",
            "action": { "kind": "skip", "reason": "manual" }
          }]
        }
        "#;

    let error =
        parse_project_config_str(json).expect_err("contradictory outcomes must be rejected");
    assert!(matches!(error, ProjectConfigLoadError::Parse(_)));
    assert!(error.to_string().contains("exactly one outcome"));
}

#[test]
fn rejects_missing_or_empty_rule_outcome() {
    for rule_body in ["", r#", "expand": []"#] {
        let json = format!(
            r#"
                {{
                  "version": 4,
                  "item_rules": [{{
                    "id": "missing",
                    "match": {{ "title_prefix": ["sermon"] }}{rule_body}
                  }}]
                }}
                "#
        );
        let error =
            parse_project_config_str(&json).expect_err("missing or empty outcome must be rejected");
        assert!(matches!(error, ProjectConfigLoadError::Parse(_)));
        assert!(error.to_string().contains("exactly one outcome"));
    }
}
