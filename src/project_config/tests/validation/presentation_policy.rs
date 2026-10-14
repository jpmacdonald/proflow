use super::*;

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
