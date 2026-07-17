use super::super::*;

fn invalid_message(json: &str) -> String {
    parse_project_config_str(json)
        .expect_err("config should violate a checked invariant")
        .to_string()
}

fn assert_has(message: &str, expected: &str) {
    assert!(
        message.contains(expected),
        "missing {expected:?} in {message}"
    );
}

#[test]
fn named_runtime_maps_reject_inexact_and_case_ambiguous_keys() {
    let message = invalid_message(
        r#"{
          "version": 4,
          "service_groups": {
            " primary": { "service_types": ["Weekly"] }
          },
          "presentation_types": {
            "Song": {},
            "song": {}
          },
          "people": {
            "Robert": {},
            "robert": {}
          }
        }"#,
    );

    for expected in [
        "service group key must be unpadded",
        "ambiguous presentation type key",
        "ambiguous person key",
    ] {
        assert_has(&message, expected);
    }
}

#[test]
fn service_groups_require_exact_unique_service_type_identities() {
    let message = invalid_message(
        r#"{
          "version": 4,
          "service_groups": {
            "empty": { "service_types": [] },
            "weekly": {
              "service_types": ["Sunday", "sunday", " Christmas Eve ", "Bad\nType"]
            }
          }
        }"#,
    );

    for expected in [
        "at least one service type",
        "duplicate service type",
        "service type must be unpadded",
        "service type must not contain control characters",
    ] {
        assert_has(&message, expected);
    }
}

#[test]
fn exact_theme_arrangement_and_override_names_reject_padding_paths_and_controls() {
    let message = invalid_message(
        r#"{
          "version": 4,
          "defaults": { "theme": "../Live Theme" },
          "presentation_types": {
            "existing": {
              "content_source": "static",
              "output_strategy": "preserve_existing",
              "arrangement": "Default\nArrangement"
            }
          },
          "overrides": [{
            "when": {
              "presentation_type": "existing",
              "service_type": " Christmas Eve"
            },
            "arrangement": "Seasonal "
          }]
        }"#,
    );

    for expected in [
        "theme must be an installed theme name, not a path",
        "arrangement must not contain control characters",
        "service type must be unpadded",
        "arrangement must be unpadded",
    ] {
        assert_has(&message, expected);
    }
}

#[test]
fn rules_reject_inexact_duplicate_match_values_and_unknown_categories() {
    let message = invalid_message(
        r#"{
          "version": 4,
          "item_rules": [
            {
              "id": "weekly",
              "match": {
                "title_prefix": ["sermon", "SERMON", " padded", "bad\nvalue"],
                "category": "unknown"
              },
              "action": { "kind": "skip", "reason": "manual" }
            },
            {
              "id": "WEEKLY",
              "match": { "title_prefix": ["other"] },
              "action": { "kind": "skip", "reason": "manual" }
            }
          ]
        }"#,
    );

    for expected in [
        "duplicate item rule id",
        "duplicate match value",
        "match value must be unpadded",
        "match value must not contain control characters",
        "unknown category 'unknown'",
    ] {
        assert_has(&message, expected);
    }
}

#[test]
fn filenames_people_and_name_templates_reject_non_names() {
    let message = invalid_message(
        r#"{
          "version": 4,
          "presentation_types": {
            "existing": {
              "content_source": "static",
              "output_strategy": "preserve_existing"
            }
          },
          "required_playlist_items": [{
            "id": "required",
            "use_type": "existing",
            "library_file": ".pro",
            "placement": "start"
          }],
          "people": {
            "Mary Ann": {
              "last": " Smith",
              "nametag": "folder/Mary.pro"
            }
          },
          "item_rules": [{
            "id": "speaker",
            "match": { "title_prefix": ["welcome"] },
            "expand": [{
              "use_type": "existing",
              "speaker": "resolved",
              "target": { "name_template": "{unknown}/Nametag" }
            }]
          }]
        }"#,
    );

    for expected in [
        "library_file must name a presentation",
        "must identify one first name",
        "last name must be unpadded",
        "nametag must be a filename, not a path",
        "name_template must produce a filename, not a path",
        "unknown placeholder '{unknown}'",
    ] {
        assert_has(&message, expected);
    }
}

#[test]
fn decisions_reject_ambiguous_keys_fields_and_phrases() {
    let message = invalid_message(
        r#"{
          "version": 4,
          "presentation_types": {
            "existing": {
              "content_source": "static",
              "output_strategy": "preserve_existing"
            }
          },
          "item_rules": [{
            "id": "choice",
            "match": { "title_prefix": ["baptism"] },
            "decision": {
              "kind": "choose_existing_file",
              "context_fields": ["title", "title"],
              "choices": {
                "Him": {
                  "use_type": "existing",
                  "file": "Him.pro",
                  "match": {
                    "any": ["son", "SON", " he "],
                    "none": [" HE ", "bad\nphrase"]
                  }
                },
                "him": {
                  "use_type": "existing",
                  "file": "Other.pro",
                  "match": { "any": ["other"] }
                }
              }
            }
          }]
        }"#,
    );

    for expected in [
        "duplicate context field 'title'",
        "ambiguous decision choice key",
        "duplicate decision match phrase",
        "appears in both 'any' and 'none'",
        "decision match phrase must not contain control characters",
    ] {
        assert_has(&message, expected);
    }
}

#[test]
fn decision_edge_spaces_remain_meaningful_substring_boundaries() {
    parse_project_config_str(
        r#"{
          "version": 4,
          "presentation_types": {
            "existing": {
              "content_source": "static",
              "output_strategy": "preserve_existing"
            }
          },
          "item_rules": [{
            "id": "choice",
            "match": { "title_prefix": ["baptism"] },
            "decision": {
              "kind": "choose_existing_file",
              "choices": {
                "him": {
                  "use_type": "existing",
                  "file": "Him.pro",
                  "match": { "any": [" he "] }
                }
              }
            }
          }]
        }"#,
    )
    .expect("edge spaces remain significant until matching owns word boundaries");
}
