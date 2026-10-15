#![allow(clippy::expect_used)]

use super::*;
use crate::project_config::parse_project_config_str;

#[test]
fn compiled_matcher_normalizes_text_and_service_once() {
    let config = parse_project_config_str(
        r#"{
              "version": 4,
              "item_rules": [{
                "id": "normalized",
                "match": {
                  "title_prefix": ["Leader’s Prayer"],
                  "description_contains": ["PEOPLE"],
                  "category": "text",
                  "service_type": ["Sunday Worship"]
                },
                "action": {"kind": "skip", "reason": "fixture"}
              }]
            }"#,
    )
    .expect("config should compile");

    let input = ItemMatchInput::new(
        MatchCategory::Text,
        "LEADER'S PRAYER OF CONFESSION",
        Some("People respond"),
        false,
        Some("sunday worship"),
    );
    assert!(config.compiled_classifications()[0].matches(&input));
}

#[test]
fn canonical_library_identity_has_stronger_precedence_than_item_rules() {
    let config = parse_project_config_str(
        r#"{
              "version": 4,
              "presentation_types": {
                "song": {
                  "kind": "song",
                  "content_source": "song",
                  "output_strategy": "preserve_existing"
                }
              },
              "library_identities": [{
                "id": "g2g_hymn",
                "match": {
                  "kind": "title_prefix",
                  "values": ["g2g #840 it is well with my soul"]
                },
                "use_type": "song",
                "library_file": "[Hymn] It Is Well With My Soul (G2G).pro"
              }],
              "item_rules": [{
                "id": "all_songs",
                "tier": "catch_all",
                "match": {"category": "song"},
                "use_type": "song"
              }]
            }"#,
    )
    .expect("identity config should compile");

    let input = ItemMatchInput::new(
        MatchCategory::Song,
        "G2G #840 It Is Well With My Soul",
        None,
        false,
        Some("10:30am Traditional"),
    );
    let matches = config
        .compiled_classifications()
        .iter()
        .filter(|classification| classification.matches(&input))
        .collect::<Vec<_>>();

    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].id(), "g2g_hymn");
    assert_eq!(matches[0].tier(), ClassificationTier::LibraryIdentity);
    assert!(
        matches[0].tier().precedence() > matches[1].tier().precedence(),
        "exact identity must win without depending on config array order"
    );
}

#[test]
fn compiled_decision_supports_all_only_and_none_only_matchers() {
    let all_only = CompiledDecisionMatcher {
        any: Vec::new(),
        all: vec!["child".to_string(), "baptism".to_string()],
        none: Vec::new(),
    };
    let none_only = CompiledDecisionMatcher {
        any: Vec::new(),
        all: Vec::new(),
        none: vec!["private".to_string()],
    };

    assert!(all_only.matches("child baptism during worship"));
    assert!(!all_only.matches("child dedication"));
    assert!(none_only.matches("public baptism"));
    assert!(!none_only.matches("private baptism"));
}

#[test]
fn required_item_compiles_scope_and_static_policy() {
    let config = parse_project_config_str(
        r#"{
              "version": 4,
              "service_groups": {"traditional": {"service_types": ["10:30am Traditional"]}},
              "presentation_types": {
                "graphic": {"kind":"graphic", "content_source":"static", "output_strategy":"preserve_existing"}
              },
              "required_playlist_items": [{
                "id":"closing", "use_type":"graphic", "library_file":"Closing.pro",
                "placement":"end", "service_group":"traditional"
              }]
            }"#,
    )
    .expect("config should compile");

    let required = &config.compiled_required_playlist_items()[0];
    assert!(required.applies_to(Some("10:30AM TRADITIONAL")));
    assert!(!required.applies_to(Some("9am Contemporary")));
    assert!(matches!(
        required.presentation_for_service(None),
        ResolvedRequiredPresentation::Preserve {
            kind: ItemKind::Graphic,
            ..
        }
    ));
}
