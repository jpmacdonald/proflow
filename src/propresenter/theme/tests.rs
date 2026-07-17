#![allow(clippy::expect_used)]

use prost::Message;

use super::*;
use crate::propresenter::presentation_spec::{CueRoleId, TextField};
use crate::propresenter::render::ResolvedCueRole;

fn fixture_slide() -> rv_data::PresentationSlide {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/propresenter/native/templates/scripture-template.pro");
    let bytes = std::fs::read(path).expect("read fixture");
    let presentation =
        rv_data::Presentation::decode(bytes.as_slice()).expect("decode presentation fixture");
    presentation
        .cues
        .iter()
        .flat_map(|cue| &cue.actions)
        .find_map(|action| match &action.action_type_data {
            Some(rv_data::action::ActionTypeData::Slide(slide)) => match &slide.slide {
                Some(rv_data::action::slide_type::Slide::Presentation(slide)) => {
                    Some(slide.clone())
                }
                _ => None,
            },
            _ => None,
        })
        .expect("fixture presentation slide")
}

fn cache(
    slides: impl IntoIterator<Item = (&'static str, rv_data::PresentationSlide)>,
) -> ThemeCache {
    ThemeCache {
        theme_slides: slides
            .into_iter()
            .map(|(name, slide)| {
                (
                    name.to_string(),
                    CachedThemeSlide {
                        slide,
                        action_count: 0,
                    },
                )
            })
            .collect(),
        theme_name: Some("test".to_string()),
    }
}

fn set_metrics(element: &mut rv_data::slide::Element, name: &str, width: f64, height: f64) {
    let graphics = element.element.as_mut().expect("graphics element");
    graphics.name = name.to_string();
    graphics.bounds = Some(rv_data::graphics::Rect {
        size: Some(rv_data::graphics::Size { width, height }),
        ..rv_data::graphics::Rect::default()
    });
    let text = graphics.text.as_mut().expect("text element");
    text.margins = Some(rv_data::graphics::EdgeInsets::default());
    text.attributes = Some(rv_data::graphics::text::Attributes {
        font: Some(rv_data::Font {
            size: 20.0,
            ..rv_data::Font::default()
        }),
        paragraph_style: Some(rv_data::graphics::text::attributes::Paragraph {
            line_height_multiple: 1.0,
            ..rv_data::graphics::text::attributes::Paragraph::default()
        }),
        ..rv_data::graphics::text::Attributes::default()
    });
}

#[test]
fn configured_missing_theme_is_an_error() {
    let directory = tempfile::tempdir().expect("temporary theme root");
    assert!(matches!(
        ThemeCache::load_from_dir(Some("Missing"), directory.path()),
        Err(ThemeCacheLoadError::NotFound { .. })
    ));
}

#[test]
fn theme_loading_uses_only_the_explicit_root() {
    let root = tempfile::tempdir().expect("temporary root");
    let directory = root.path().join("Themes/Sunday");
    std::fs::create_dir_all(&directory).expect("create theme directory");
    let document = rv_data::template::Document {
        application_info: None,
        slides: vec![rv_data::template::Slide {
            base_slide: fixture_slide().base_slide,
            name: "Scripture".to_string(),
            actions: Vec::new(),
        }],
    };
    std::fs::write(directory.join("Theme"), document.encode_to_vec()).expect("write theme");

    let loaded = ThemeCache::load_from_dir(Some("Sunday"), &root.path().join("Themes"))
        .expect("load explicit theme");
    assert_eq!(loaded.theme_name(), Some("Sunday"));
    assert_eq!(loaded.theme_slide_names(), vec!["Scripture"]);
}

#[test]
fn discovery_reports_slots_canvas_and_embedded_actions() {
    let mut slide = fixture_slide();
    let base = slide.base_slide.as_mut().expect("base slide");
    base.size = Some(rv_data::graphics::Size {
        width: 1920.0,
        height: 1080.0,
    });
    base.elements[0]
        .element
        .as_mut()
        .expect("graphics element")
        .name = "Body".to_string();
    let themes = ThemeCache {
        theme_slides: HashMap::from([(
            "Content".to_string(),
            CachedThemeSlide {
                slide,
                action_count: 2,
            },
        )]),
        theme_name: Some("test".to_string()),
    };

    let facts = themes.theme_slide_facts();

    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].name, "Content");
    assert_eq!(facts[0].named_text_slots, vec!["Body"]);
    assert_eq!(facts[0].default_text_slot_candidates, 1);
    assert_eq!(facts[0].embedded_action_count, 2);
    assert_eq!(
        facts[0].canvas_size,
        Some(ThemeSlideCanvas {
            width: 1920.0,
            height: 1080.0,
        })
    );
    assert_eq!(facts[0].generation_issues.len(), 1);
    assert!(facts[0].generation_issues[0].contains("2 embedded actions"));
}

#[test]
fn implicit_body_requires_one_meaningful_text_destination() {
    let single = fixture_slide();
    let mut none = single.clone();
    none.base_slide
        .as_mut()
        .expect("base slide")
        .elements
        .clear();

    let mut multiple = single.clone();
    let mut second = multiple.base_slide.as_ref().expect("base slide").elements[0].clone();
    second.element.as_mut().expect("graphics element").name = "Secondary".to_string();
    multiple
        .base_slide
        .as_mut()
        .expect("base slide")
        .elements
        .push(second);

    let mut helper = single.clone();
    let mut empty_helper = helper.base_slide.as_ref().expect("base slide").elements[0].clone();
    let graphics = empty_helper.element.as_mut().expect("graphics element");
    graphics.name.clear();
    graphics
        .text
        .as_mut()
        .expect("text element")
        .rtf_data
        .clear();
    helper
        .base_slide
        .as_mut()
        .expect("base slide")
        .elements
        .push(empty_helper);

    let themes = cache([
        ("single", single),
        ("none", none),
        ("multiple", multiple),
        ("helper", helper),
    ]);
    assert!(themes.text_template("single").is_ok());
    assert!(themes.text_template("helper").is_ok());
    assert!(matches!(
        themes.text_template("none"),
        Err(ThemeSlideError::TextElementCount { count: 0, .. })
    ));
    assert!(matches!(
        themes.text_template("multiple"),
        Err(ThemeSlideError::TextElementCount { count: 2, .. })
    ));
}

#[test]
fn embedded_theme_actions_are_rejected() {
    let themes = ThemeCache {
        theme_slides: HashMap::from([(
            "Content".to_string(),
            CachedThemeSlide {
                slide: fixture_slide(),
                action_count: 1,
            },
        )]),
        theme_name: Some("test".to_string()),
    };
    assert!(matches!(
        themes.slide_template("Content"),
        Err(ThemeSlideError::EmbeddedActions { count: 1, .. })
    ));
}

#[test]
fn canonical_duplicate_slide_names_are_rejected() {
    let root = tempfile::tempdir().expect("temporary root");
    let path = root.path().join("Theme");
    let document = rv_data::template::Document {
        application_info: None,
        slides: ["Scripture", "scripture"]
            .into_iter()
            .map(|name| rv_data::template::Slide {
                base_slide: None,
                name: name.to_string(),
                actions: Vec::new(),
            })
            .collect(),
    };
    std::fs::write(&path, document.encode_to_vec()).expect("write theme");
    assert!(matches!(
        load_theme(&path),
        Err(ThemeCacheLoadError::DuplicateSlideName { .. })
    ));
}

#[test]
fn metrics_come_from_the_resolved_semantic_field() {
    let mut slide = fixture_slide();
    let base = slide.base_slide.as_mut().expect("base slide");
    let mut body = base.elements[0].clone();
    set_metrics(&mut base.elements[0], "Footer", 2_000.0, 400.0);
    set_metrics(&mut body, "Body", 200.0, 20.0);
    base.elements.push(body);

    let role = ResolvedCueRole::with_slots(
        CueRoleId::new("content").expect("role"),
        &slide,
        (TextField::body(), "Body"),
        [],
    )
    .expect("explicit body role");
    let metrics = extract_role_metrics(&role, &TextField::body())
        .expect("resolved metrics")
        .expect("body has geometry");

    assert_eq!(metrics.chars_per_line, 20);
    assert_eq!(metrics.max_lines, 1);
}

#[test]
fn implicit_body_metrics_ignore_empty_unnamed_helpers() {
    let mut slide = fixture_slide();
    let base = slide.base_slide.as_mut().expect("base slide");
    let mut body = base.elements[0].clone();
    set_metrics(&mut base.elements[0], "", 2_000.0, 400.0);
    base.elements[0]
        .element
        .as_mut()
        .expect("helper element")
        .text
        .as_mut()
        .expect("helper text")
        .rtf_data
        .clear();
    set_metrics(&mut body, "Body", 200.0, 20.0);
    base.elements.push(body);

    let role = ResolvedCueRole::body(CueRoleId::new("content").expect("role"), &slide)
        .expect("one meaningful implicit body");
    let metrics = extract_role_metrics(&role, &TextField::body())
        .expect("resolved metrics")
        .expect("body has geometry");

    assert_eq!(metrics.chars_per_line, 20);
    assert_eq!(metrics.max_lines, 1);
}

#[test]
fn metrics_account_for_native_character_spacing() {
    let mut slide = fixture_slide();
    let element = &mut slide.base_slide.as_mut().expect("base slide").elements[0];
    set_metrics(element, "Body", 200.0, 20.0);
    element
        .element
        .as_mut()
        .expect("graphics element")
        .text
        .as_mut()
        .expect("text element")
        .attributes
        .as_mut()
        .expect("text attributes")
        .kerning = 5.0;

    let role = ResolvedCueRole::body(CueRoleId::new("content").expect("role"), &slide)
        .expect("one meaningful body");
    let metrics = extract_role_metrics(&role, &TextField::body())
        .expect("resolved metrics")
        .expect("body has geometry");

    assert_eq!(metrics.chars_per_line, 13);
    assert_eq!(metrics.max_lines, 1);
}
