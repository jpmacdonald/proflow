#![allow(clippy::expect_used)]

use std::collections::BTreeMap;

use super::item::OutputKeyError;
use super::*;
use crate::propresenter::SlideType;

#[test]
fn output_keys_reject_invalid_ad_hoc_identities() {
    assert_eq!(OutputKey::new(String::new()), Err(OutputKeyError::Blank));
    assert_eq!(
        OutputKey::new(" padded".to_string()),
        Err(OutputKeyError::Padded)
    );
    assert_eq!(
        OutputKey::new("key\u{7f}".to_string()),
        Err(OutputKeyError::ControlCharacter)
    );
}

#[test]
fn generated_output_key_segments_are_collision_free_and_serialize_as_strings() {
    let encoded = OutputKey::primary("line\n%0A:tail");
    let literal = OutputKey::primary("line%0A:tail");

    assert_eq!(encoded.as_str(), "pco:line%0A%250A%3Atail:main");
    assert_eq!(literal.as_str(), "pco:line%250A%3Atail:main");
    assert_ne!(encoded, literal);
    assert_ne!(
        OutputKey::expanded("item:part", 1, "type"),
        OutputKey::expanded("item", 1, "part:type")
    );
    assert_eq!(
        serde_json::to_value(&encoded).expect("serialize output key"),
        serde_json::json!("pco:line%0A%250A%3Atail:main")
    );
}

fn text_slots(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries
        .iter()
        .map(|(slot, element)| ((*slot).to_string(), (*element).to_string()))
        .collect()
}

#[test]
fn scripture_content_has_one_checked_source_form() {
    let single = ScriptureContent::single("John 3:16".to_string(), "NRSVue".to_string())
        .expect("non-blank scripture identity should be valid");
    assert!(matches!(
        single.request(),
        ScriptureRequest::Single {
            reference: "John 3:16",
            bible_version: "NRSVue"
        }
    ));
    let excerpt = ScriptureContent::prefix_excerpt(
        "Exodus 16:1-4".to_string(),
        "Exodus 16:1-4a".to_string(),
        "NRSVue".to_string(),
        "1 Whole passage 4 verified prefix".to_string(),
    )
    .expect("partial scripture source should be complete");
    assert!(matches!(
        excerpt.request(),
        ScriptureRequest::PrefixExcerpt {
            reference: "Exodus 16:1-4",
            display_reference: "Exodus 16:1-4a",
            bible_version: "NRSVue",
            excerpt_text: "1 Whole passage 4 verified prefix",
        }
    ));

    assert!(ScriptureContent::combined(Vec::new()).is_none());
    assert!(ScriptureContent::combined(vec![ScriptureRefInfo::new(
        "John 3:16".to_string(),
        "NRSVue".to_string()
    )
    .expect("non-blank scripture identity should be valid"),])
    .is_none());

    let combined = ScriptureContent::combined(vec![
        ScriptureRefInfo::new("Psalm 23:1-2".to_string(), "NRSVue".to_string())
            .expect("non-blank scripture identity should be valid"),
        ScriptureRefInfo::new("John 3:16".to_string(), "NIV".to_string())
            .expect("non-blank scripture identity should be valid"),
    ])
    .expect("two references form a valid combined source");
    assert!(matches!(
        combined.request(),
        ScriptureRequest::Combined(references) if references.len() == 2
    ));
}

#[test]
fn scripture_content_serialization_preserves_preview_shape() {
    let scripture = ScriptureContent::single("Ephesians 4:4-6".to_string(), "NRSVue".to_string())
        .expect("non-blank scripture identity should be valid");
    let value = serde_json::to_value(scripture).expect("serialize scripture source");

    assert_eq!(
        value,
        serde_json::json!({
            "reference": "Ephesians 4:4-6",
            "bible_version": "NRSVue"
        })
    );
}

#[test]
fn partial_scripture_serialization_keeps_lookup_and_display_references_distinct() {
    let scripture = ScriptureContent::prefix_excerpt(
        "Exodus 16:1-4".to_string(),
        "Exodus 16:1-4a".to_string(),
        "NRSVue".to_string(),
        "1 Passage 4 prefix".to_string(),
    )
    .expect("partial scripture source");

    assert_eq!(
        serde_json::to_value(scripture).expect("serialize partial scripture"),
        serde_json::json!({
            "reference": "Exodus 16:1-4",
            "display_reference": "Exodus 16:1-4a",
            "bible_version": "NRSVue",
            "excerpt_text": "1 Passage 4 prefix"
        })
    );
}

#[test]
fn scripture_identity_rejects_blank_padded_and_control_text() {
    for (reference, expected) in [
        ("", ScripturePlanError::BlankReference),
        ("   ", ScripturePlanError::BlankReference),
        (" John 3:16", ScripturePlanError::PaddedReference),
        (
            "John\u{7} 3:16",
            ScripturePlanError::ControlCharacterInReference,
        ),
    ] {
        assert_eq!(
            ScriptureRefInfo::new(reference.to_string(), "NRSVue".to_string()).err(),
            Some(expected)
        );
        assert_eq!(
            ScriptureContent::single(reference.to_string(), "NRSVue".to_string()).err(),
            Some(expected)
        );
    }

    for (version, expected) in [
        ("", ScripturePlanError::BlankVersion),
        ("   ", ScripturePlanError::BlankVersion),
        ("NRSVue ", ScripturePlanError::PaddedVersion),
        ("NRS\u{7}Vue", ScripturePlanError::ControlCharacterInVersion),
    ] {
        assert_eq!(
            ScriptureRefInfo::new("John 3:16".to_string(), version.to_string()).err(),
            Some(expected)
        );
        assert_eq!(
            ScriptureContent::single("John 3:16".to_string(), version.to_string()).err(),
            Some(expected)
        );
    }
}

#[test]
fn partial_scripture_rejects_a_display_reference_that_disagrees_with_lookup() {
    assert_eq!(
        ScriptureContent::prefix_excerpt(
            "Exodus 16:1-4".to_string(),
            "Exodus 16:1-4b".to_string(),
            "NRSVue".to_string(),
            "1 Passage 4 segment".to_string(),
        )
        .err(),
        Some(ScripturePlanError::MismatchedExcerptReference)
    );
}

#[test]
fn canonical_item_kinds_preserve_propresenter_slide_semantics() {
    for (item_kind, expected) in [
        (ItemKind::Song, SlideType::Lyrics),
        (ItemKind::Scripture, SlideType::Scripture),
        (ItemKind::Nametag, SlideType::Title),
        (ItemKind::Announcement, SlideType::Graphic),
        (ItemKind::Graphic, SlideType::Graphic),
        (ItemKind::Liturgy, SlideType::Text),
        (ItemKind::Other, SlideType::Text),
    ] {
        let plan = ResolvedItemPlan {
            output_key: OutputKey::new("test:slide-type".to_string())
                .expect("valid test output key"),
            position: 0,
            pco_title: "Test".to_string(),
            playlist_name: "Test".to_string(),
            reason: "Test fixture".to_string(),
            item_kind,
            item_type: None,
            disposition: PlanDisposition::Skip,
        };
        assert_eq!(plan.slide_type(), expected);
    }
}

#[test]
fn render_metadata_rejects_blank_bindings_and_zero_line_bounds() {
    assert_eq!(
        CueMacro::new("  ".to_string(), None),
        Err(RenderPlanError::BlankCueMacro)
    );
    assert_eq!(
        CueMacro::new("Content".to_string(), Some(String::new())),
        Err(RenderPlanError::BlankLeaderCueMacro)
    );

    assert!(matches!(
        RenderRole::new(
            String::new(),
            "Content".to_string(),
            BTreeMap::new(),
            None,
            None
        ),
        Err(RenderPlanError::BlankRoleId)
    ));
    assert!(matches!(
        RenderRole::new(
            "content".to_string(),
            "  ".to_string(),
            BTreeMap::new(),
            None,
            None,
        ),
        Err(RenderPlanError::BlankRoleSlide { role_id }) if role_id == "content"
    ));

    let role = RenderRole::new(
        "content".to_string(),
        "Content".to_string(),
        BTreeMap::new(),
        None,
        None,
    )
    .expect("non-blank role should be valid");
    assert!(matches!(
        RenderStyle::new(None, role.clone(), None, Some(0)),
        Err(RenderPlanError::ZeroMaxLines)
    ));
    let duplicate_title = RenderRole::new(
        "content".to_string(),
        "Title".to_string(),
        BTreeMap::new(),
        None,
        None,
    )
    .expect("individually valid title role");
    assert!(matches!(
        RenderStyle::new(None, role, Some(duplicate_title), None),
        Err(RenderPlanError::DuplicateRoleId { role_id }) if role_id == "content"
    ));
}

#[test]
fn cue_macro_rejects_padded_and_control_character_names() {
    assert_eq!(
        CueMacro::new(" Content".to_string(), None),
        Err(RenderPlanError::InvalidCueMacro {
            name: " Content".to_string(),
            problem: IdentifierProblem::SurroundingWhitespace,
        })
    );
    assert_eq!(
        CueMacro::new("Con\ntent".to_string(), None),
        Err(RenderPlanError::InvalidCueMacro {
            name: "Con\ntent".to_string(),
            problem: IdentifierProblem::ControlCharacter,
        })
    );
    assert_eq!(
        CueMacro::new("Content".to_string(), Some(" Highlighted".to_string())),
        Err(RenderPlanError::InvalidLeaderCueMacro {
            name: " Highlighted".to_string(),
            problem: IdentifierProblem::SurroundingWhitespace,
        })
    );
    assert_eq!(
        CueMacro::new("Content".to_string(), Some("High\tlighted".to_string())),
        Err(RenderPlanError::InvalidLeaderCueMacro {
            name: "High\tlighted".to_string(),
            problem: IdentifierProblem::ControlCharacter,
        })
    );
}

#[test]
fn render_role_rejects_inexact_role_and_slide_identifiers() {
    assert!(matches!(
        RenderRole::new(
            " content".to_string(),
            "Content".to_string(),
            BTreeMap::new(),
            None,
            None,
        ),
        Err(RenderPlanError::InvalidRoleId {
            problem: IdentifierProblem::SurroundingWhitespace,
            ..
        })
    ));
    assert!(matches!(
        RenderRole::new(
            "con\ntent".to_string(),
            "Content".to_string(),
            BTreeMap::new(),
            None,
            None,
        ),
        Err(RenderPlanError::InvalidRoleId {
            problem: IdentifierProblem::ControlCharacter,
            ..
        })
    ));
    assert!(matches!(
        RenderRole::new(
            "content".to_string(),
            "Content ".to_string(),
            BTreeMap::new(),
            None,
            None,
        ),
        Err(RenderPlanError::InvalidRoleSlide {
            problem: IdentifierProblem::SurroundingWhitespace,
            ..
        })
    ));
    assert!(matches!(
        RenderRole::new(
            "content".to_string(),
            "Con\ntent".to_string(),
            BTreeMap::new(),
            None,
            None,
        ),
        Err(RenderPlanError::InvalidRoleSlide {
            problem: IdentifierProblem::ControlCharacter,
            ..
        })
    ));
}

#[test]
fn render_role_rejects_inexact_text_slot_identifiers() {
    assert!(matches!(
        RenderRole::new(
            "content".to_string(),
            "Content".to_string(),
            text_slots(&[(" body", "Body Text")]),
            None,
            None,
        ),
        Err(RenderPlanError::InvalidTextSlotName {
            problem: IdentifierProblem::SurroundingWhitespace,
            ..
        })
    ));
    assert!(matches!(
        RenderRole::new(
            "content".to_string(),
            "Content".to_string(),
            text_slots(&[("bo\tdy", "Body Text")]),
            None,
            None,
        ),
        Err(RenderPlanError::InvalidTextSlotName {
            problem: IdentifierProblem::ControlCharacter,
            ..
        })
    ));
    assert!(matches!(
        RenderRole::new(
            "content".to_string(),
            "Content".to_string(),
            text_slots(&[("body", " Body Text")]),
            None,
            None,
        ),
        Err(RenderPlanError::InvalidTextSlotElement {
            problem: IdentifierProblem::SurroundingWhitespace,
            ..
        })
    ));
    assert!(matches!(
        RenderRole::new(
            "content".to_string(),
            "Content".to_string(),
            text_slots(&[("body", "Body\nText")]),
            None,
            None,
        ),
        Err(RenderPlanError::InvalidTextSlotElement {
            problem: IdentifierProblem::ControlCharacter,
            ..
        })
    ));
}

#[test]
fn render_role_rejects_ambiguous_or_incomplete_explicit_text_slots() {
    assert!(matches!(
        RenderRole::new(
            "content".to_string(),
            "Content".to_string(),
            text_slots(&[("body", "Text"), ("heading", "Text")]),
            None,
            None,
        ),
        Err(RenderPlanError::DuplicateTextSlotElement {
            first_slot,
            duplicate_slot,
            element,
            ..
        }) if first_slot == "body" && duplicate_slot == "heading" && element == "Text"
    ));
    assert!(matches!(
        RenderRole::new(
            "content".to_string(),
            "Content".to_string(),
            text_slots(&[("heading", "Heading")]),
            None,
            None,
        ),
        Err(RenderPlanError::MissingBodyTextSlot { role_id }) if role_id == "content"
    ));
}

#[test]
fn render_role_rejects_partial_responsive_style() {
    let responsive_macro = CueMacro::new(
        "Content".to_string(),
        Some("Content Highlighted".to_string()),
    )
    .expect("responsive macro names should be valid");
    assert!(matches!(
        RenderRole::new(
            "content".to_string(),
            "Content".to_string(),
            BTreeMap::new(),
            Some(responsive_macro),
            None,
        ),
        Err(RenderPlanError::IncompleteResponsiveRole { role_id }) if role_id == "content"
    ));
    assert!(matches!(
        RenderRole::new(
            "content".to_string(),
            "Content".to_string(),
            BTreeMap::new(),
            None,
            Some(SpeakerPalette::new((254, 219, 79), (255, 255, 255))),
        ),
        Err(RenderPlanError::IncompleteResponsiveRole { role_id }) if role_id == "content"
    ));
}

#[test]
fn checked_render_metadata_exposes_only_valid_values() {
    let macro_binding = CueMacro::new(
        "Content".to_string(),
        Some("Content Highlighted".to_string()),
    )
    .expect("non-blank macro should be valid");
    let role = RenderRole::new(
        "content".to_string(),
        "Content".to_string(),
        text_slots(&[("body", "Body Text")]),
        Some(macro_binding),
        Some(SpeakerPalette::new((254, 219, 79), (255, 255, 255))),
    )
    .expect("complete role should be valid");
    let style =
        RenderStyle::new(None, role, None, Some(6)).expect("positive line bound should be valid");

    assert_eq!(style.content().id(), "content");
    assert_eq!(style.content().slide(), "Content");
    assert_eq!(
        style.content().text_slots().get("body").map(String::as_str),
        Some("Body Text")
    );
    assert_eq!(
        style.content().cue_macro().map(CueMacro::enter),
        Some("Content")
    );
    assert_eq!(style.max_lines_per_slide(), Some(6));
}
