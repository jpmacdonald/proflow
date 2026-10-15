#![allow(clippy::expect_used, clippy::panic)]

use super::*;
use crate::project_config::{parse_project_config_str, RawProjectConfig};

#[test]
fn compiled_policy_preserves_wire_json() {
    let json = include_str!("../../../tests/fixtures/workflow/v4_config.json");
    let checked = parse_project_config_str(json).expect("fixture should compile");
    let raw: RawProjectConfig = serde_json::from_str(json).expect("fixture json");
    let original = serde_json::to_value(raw).expect("raw config should serialize");
    let serialized = serde_json::to_value(&checked).expect("checked config should serialize");
    assert_eq!(serialized, original);
}

#[test]
fn service_background_and_arrangement_are_compiled_by_capability() {
    let checked = parse_project_config_str(
        r##"{
              "version": 4,
              "service_groups": {"seasonal": {"service_types": ["Christmas Eve"]}},
              "backgrounds": {
                "ordinary": "backgrounds/ordinary.png",
                "seasonal": "backgrounds/seasonal.png"
              },
              "cue_roles": {"body": {
                "slide": "Body",
                "enter_macro": "Scripture/Prayer",
                "leader_enter_macro": "Scripture/Prayer (Highlighted)",
                "speaker_colors": {"leader": "#FEDB4F", "audience": "#FFFFFF"}
              }},
              "presentation_types": {
                "song": {"kind":"song", "content_source":"song", "output_strategy": "preserve_existing", "arrangement":"Default"},
                "text": {"kind":"liturgy", "content_source":"description", "description_parser":"liturgical", "output_strategy":"generate_new", "display":{"kind":"single", "role":"body"}, "background":"ordinary"}
              },
              "overrides": [{"when":{"service_group":"seasonal"}, "arrangement":"Seasonal", "background":"seasonal"}]
            }"##,
    )
    .expect("config should compile");

    let song = checked.presentation_policy("song").expect("song policy");
    let PresentationPolicy::PreserveExisting { arrangement, .. } = song else {
        panic!("song should compile as use-existing");
    };
    assert_eq!(arrangement.for_service(None).as_deref(), Some("Default"));
    assert_eq!(
        arrangement.for_service(Some("Christmas Eve")).as_deref(),
        Some("Seasonal")
    );

    let text = checked.presentation_policy("text").expect("text policy");
    let PresentationPolicy::GenerateDescription { render, .. } = text else {
        panic!("text should compile as generated description");
    };
    assert_eq!(
        render
            .for_service(None)
            .background()
            .map(|background| background.id().as_str()),
        Some("ordinary")
    );
    assert_eq!(
        render
            .for_service(Some("Christmas Eve"))
            .background()
            .map(|background| background.id().as_str()),
        Some("seasonal")
    );
}
