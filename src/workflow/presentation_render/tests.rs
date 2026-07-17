#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::Path;

use prost::Message;

use super::*;
use crate::propresenter::generated::rv_data;
use crate::propresenter::macros::{cue_has_macro_named, macro_action_name};
use crate::propresenter::rtf::rtf_to_text;
use crate::workflow::plan::{CueMacro, SpeakerPalette};

fn fixture_slide() -> rv_data::PresentationSlide {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/propresenter/native/templates/scripture-template.pro");
    let bytes = std::fs::read(path).expect("read presentation fixture");
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

fn two_field_slide() -> rv_data::PresentationSlide {
    let mut slide = fixture_slide();
    let base = slide.base_slide.as_mut().expect("base slide");
    let mut unrelated = base.elements[0].clone();
    base.elements[0]
        .element
        .as_mut()
        .expect("body element")
        .name = "Body".to_string();
    unrelated.element.as_mut().expect("unrelated element").name = "Footer".to_string();
    base.elements.push(unrelated);
    slide
}

fn set_element_metrics(element: &mut rv_data::slide::Element, name: &str, width: f64, height: f64) {
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

fn differently_sized_fields() -> rv_data::PresentationSlide {
    let mut slide = fixture_slide();
    let base = slide.base_slide.as_mut().expect("base slide");
    let mut body = base.elements[0].clone();
    set_element_metrics(&mut base.elements[0], "Footer", 2_000.0, 400.0);
    set_element_metrics(&mut body, "Body", 200.0, 20.0);
    base.elements.push(body);
    slide
}

fn theme_cache(slides: Vec<(&str, rv_data::PresentationSlide)>) -> (tempfile::TempDir, ThemeCache) {
    let root = tempfile::tempdir().expect("temporary theme root");
    let directory = root.path().join("Themes/Test");
    std::fs::create_dir_all(&directory).expect("create theme directory");
    let document = rv_data::template::Document {
        application_info: None,
        slides: slides
            .into_iter()
            .map(|(name, slide)| rv_data::template::Slide {
                base_slide: slide.base_slide,
                name: name.to_string(),
                actions: Vec::new(),
            })
            .collect(),
    };
    std::fs::write(directory.join("Theme"), document.encode_to_vec()).expect("write theme");
    let cache =
        ThemeCache::load_from_dir(Some("Test"), &root.path().join("Themes")).expect("load theme");
    (root, cache)
}

fn role(
    id: &str,
    slide: &str,
    text_slots: BTreeMap<String, String>,
    cue_macro: Option<CueMacro>,
) -> RenderRole {
    RenderRole::new(
        id.to_string(),
        slide.to_string(),
        text_slots,
        cue_macro,
        None,
    )
    .expect("valid render role")
}

fn liturgy_role(id: &str, slide: &str, cue_macro: CueMacro) -> RenderRole {
    RenderRole::new(
        id.to_string(),
        slide.to_string(),
        BTreeMap::new(),
        Some(cue_macro),
        Some(SpeakerPalette::new((254, 219, 79), (255, 255, 255))),
    )
    .expect("valid liturgy role")
}

fn parsed_segment(text: &str, speaker: SpeakerRole) -> ParsedSegment {
    ParsedSegment {
        text: text.to_string(),
        speaker,
        bold: None,
        italic: None,
    }
}

fn description(text: &str, title: Option<&str>) -> ParsedContent {
    ParsedContent::new(
        vec![ParsedSegment {
            text: text.to_string(),
            speaker: SpeakerRole::Neutral,
            bold: None,
            italic: None,
        }],
        title.map(str::to_string),
    )
}

fn body_rtf(slide: &rv_data::PresentationSlide, name: &str) -> Vec<u8> {
    slide
        .base_slide
        .as_ref()
        .expect("base slide")
        .elements
        .iter()
        .find_map(|element| {
            let graphics = element.element.as_ref()?;
            (graphics.name == name)
                .then(|| graphics.text.as_ref().map(|text| text.rtf_data.clone()))
                .flatten()
        })
        .expect("named text element")
}

fn rendered_slide(rendered: &RenderedPresentation, index: usize) -> &rv_data::PresentationSlide {
    rendered
        .presentation
        .cues
        .get(index)
        .and_then(|cue| cue.actions.first())
        .and_then(|action| action.action_type_data.as_ref())
        .and_then(|action| match action {
            rv_data::action::ActionTypeData::Slide(slide) => slide.slide.as_ref(),
            _ => None,
        })
        .and_then(|slide| match slide {
            rv_data::action::slide_type::Slide::Presentation(slide) => Some(slide),
            rv_data::action::slide_type::Slide::Prop(_) => None,
        })
        .expect("rendered presentation slide")
}

fn first_rendered_slide(rendered: &RenderedPresentation) -> &rv_data::PresentationSlide {
    rendered_slide(rendered, 0)
}

fn macro_cache(names: &[&str]) -> MacroCache {
    let root = tempfile::tempdir().expect("macro root");
    let path = root.path().join("Macros");
    let document = rv_data::MacrosDocument {
        macros: names
            .iter()
            .enumerate()
            .map(|(index, name)| native_macro(name, &format!("macro-{index}")))
            .collect(),
        ..rv_data::MacrosDocument::default()
    };
    std::fs::write(&path, document.encode_to_vec()).expect("write macros");
    MacroCache::load_from(&path).expect("load macros")
}

fn slide_labels(rendered: &RenderedPresentation) -> Vec<Option<&str>> {
    rendered
        .presentation
        .cues
        .iter()
        .map(|cue| {
            cue.actions
                .iter()
                .find_map(|action| action.label.as_ref().map(|label| label.text.as_str()))
        })
        .collect()
}

fn rendered_text(rendered: &RenderedPresentation, index: usize) -> String {
    let slide = rendered_slide(rendered, index);
    slide
        .base_slide
        .as_ref()
        .expect("base slide")
        .elements
        .iter()
        .find_map(|element| {
            let text = element.element.as_ref()?.text.as_ref()?;
            rtf_to_text(&String::from_utf8_lossy(&text.rtf_data))
        })
        .expect("rendered text")
}

#[test]
fn explicit_body_slot_changes_only_the_named_native_field() {
    let template = two_field_slide();
    let original_footer = body_rtf(&template, "Footer");
    let (_root, themes) = theme_cache(vec![("Content", template)]);
    let style = RenderStyle::new(
        None,
        role(
            "body-role",
            "Content",
            BTreeMap::from([("body".to_string(), "Body".to_string())]),
            None,
        ),
        None,
        None,
    )
    .expect("valid style");

    let content = description("Replacement body", None);
    let rendered = render_source(
        "Named fields",
        PresentationSource::Description(&content),
        &style,
        &themes,
    )
    .expect("render named field");
    let slide = first_rendered_slide(&rendered);

    assert_eq!(
        rtf_to_text(&String::from_utf8_lossy(&body_rtf(slide, "Body"))).as_deref(),
        Some("Replacement body")
    );
    assert_eq!(body_rtf(slide, "Footer"), original_footer);
}

#[test]
fn explicit_body_geometry_controls_content_splitting() {
    let (_root, themes) = theme_cache(vec![("Content", differently_sized_fields())]);
    let style = RenderStyle::new(
        None,
        role(
            "body-role",
            "Content",
            BTreeMap::from([("body".to_string(), "Body".to_string())]),
            None,
        ),
        None,
        Some(8),
    )
    .expect("valid style");
    let text = std::iter::repeat_n("bounded words", 20)
        .collect::<Vec<_>>()
        .join(" ");
    let content = description(&text, None);
    let expected = pack_segments_for_slides(
        content.segments(),
        TextLayout::new(20, 1).expect("body element geometry is valid"),
    );

    let rendered = render_source(
        "Body metrics",
        PresentationSource::Description(&content),
        &style,
        &themes,
    )
    .expect("render from selected body geometry");

    assert!(expected.len() > 1);
    assert_eq!(rendered.presentation.cues.len(), expected.len());
}

#[test]
fn scripture_continuations_keep_their_native_verse_label() {
    let (_root, themes) = theme_cache(vec![("Scripture", fixture_slide())]);
    let style = RenderStyle::new(
        None,
        role("content", "Scripture", BTreeMap::new(), None),
        None,
        Some(1),
    )
    .expect("valid style");
    let verses = [Verse {
        number: 12,
        text: std::iter::repeat_n("unpunctuated", 40)
            .collect::<Vec<_>>()
            .join(" "),
    }];

    let rendered = render_source(
        "John 3v12 NRSVue",
        PresentationSource::Scripture {
            title: "John 3:12 NRSVue",
            label_prefix: "John 3:",
            verses: &verses,
        },
        &style,
        &themes,
    )
    .expect("render scripture");
    let labels = slide_labels(&rendered);

    assert!(labels.len() > 2);
    assert_eq!(labels[0], None);
    assert!(labels[1..].iter().all(|label| *label == Some("John 3:12")));
}

#[test]
fn mixed_responses_share_one_slide_and_use_the_first_speakers_macro() {
    let (_root, themes) = theme_cache(vec![("Liturgy", fixture_slide())]);
    let style = RenderStyle::new(
        None,
        liturgy_role(
            "liturgy",
            "Liturgy",
            CueMacro::new(
                "Scripture/Prayer".to_string(),
                Some("Scripture/Prayer (Highlighted)".to_string()),
            )
            .expect("valid cue macros"),
        ),
        None,
        Some(7),
    )
    .expect("valid style");
    let macros = macro_cache(&["Scripture/Prayer", "Scripture/Prayer (Highlighted)"]);

    for (first, second, expected_macro) in [
        (
            SpeakerRole::Leader,
            SpeakerRole::Audience,
            "Scripture/Prayer (Highlighted)",
        ),
        (
            SpeakerRole::Audience,
            SpeakerRole::Leader,
            "Scripture/Prayer",
        ),
    ] {
        let content = ParsedContent::new(
            vec![
                parsed_segment("First response.", first),
                parsed_segment("", SpeakerRole::Neutral),
                parsed_segment("Second response.", second),
            ],
            None,
        );
        let mut rendered = render_source(
            "Mixed liturgy",
            PresentationSource::Description(&content),
            &style,
            &themes,
        )
        .expect("render mixed liturgy");
        apply_role_macros(&mut rendered, &style, &macros).expect("apply cue look");

        assert_eq!(rendered.presentation.cues.len(), 1);
        assert!(cue_has_macro_named(
            &rendered.presentation.cues[0],
            expected_macro
        ));
        let rtf = rendered_slide(&rendered, 0)
            .base_slide
            .as_ref()
            .expect("base slide")
            .elements
            .iter()
            .find_map(|element| element.element.as_ref()?.text.as_ref())
            .expect("rendered text");
        let rtf = String::from_utf8_lossy(&rtf.rtf_data);
        assert!(rtf.contains("\\red254\\green219\\blue79"));
        assert!(rtf.contains("\\red255\\green255\\blue255"));
        assert!(rendered_text(&rendered, 0).contains("First response"));
        assert!(rendered_text(&rendered, 0).contains("Second response"));
    }
}

#[test]
fn long_question_answer_flow_separates_roles_and_keeps_the_answer_front_loaded() {
    let (_root, themes) = theme_cache(vec![
        ("Title", fixture_slide()),
        ("Liturgy", fixture_slide()),
    ]);
    let style = RenderStyle::new(
        None,
        liturgy_role(
            "liturgy",
            "Liturgy",
            CueMacro::new(
                "Scripture/Prayer".to_string(),
                Some("Scripture/Prayer (Highlighted)".to_string()),
            )
            .expect("valid cue macros"),
        ),
        Some(role(
            "title",
            "Title",
            BTreeMap::new(),
            Some(CueMacro::new("Name Tag/Title".to_string(), None).expect("title macro")),
        )),
        Some(2),
    )
    .expect("valid style");
    let macros = macro_cache(&[
        "Name Tag/Title",
        "Scripture/Prayer",
        "Scripture/Prayer (Highlighted)",
    ]);
    let answer = "A. Do take care of all our physical needs so that we come to know that you are the only source of everything good, and that neither our work and worry, nor your gifts, can do us any good without your blessing. And so help us to give up our trust in creatures and trust in you alone.";
    let content = ParsedContent::new(
        vec![
            parsed_segment("Q. What is our hope?", SpeakerRole::Leader),
            parsed_segment(answer, SpeakerRole::Audience),
        ],
        Some("Affirmation of Faith".to_string()),
    );

    let mut rendered = render_source(
        "Catechism",
        PresentationSource::Description(&content),
        &style,
        &themes,
    )
    .expect("render catechism");
    apply_role_macros(&mut rendered, &style, &macros).expect("apply cue looks");

    assert!(rendered.presentation.cues.len() >= 3);
    assert_eq!(rendered_text(&rendered, 1), "Q. What is our hope?");
    assert!(!rendered_text(&rendered, 1).contains("A."));
    assert!(cue_has_macro_named(
        &rendered.presentation.cues[1],
        "Scripture/Prayer (Highlighted)"
    ));
    assert!(cue_has_macro_named(
        &rendered.presentation.cues[2],
        "Scripture/Prayer"
    ));
    let answer_slides = (2..rendered.presentation.cues.len())
        .map(|index| rendered_text(&rendered, index))
        .collect::<Vec<_>>();
    let answer_word_counts = answer_slides
        .iter()
        .map(|slide| slide.split_whitespace().count())
        .collect::<Vec<_>>();
    assert_eq!(
        answer_slides
            .join(" ")
            .split_whitespace()
            .collect::<String>(),
        answer.split_whitespace().collect::<String>()
    );
    assert!(
        answer_word_counts.last().is_some_and(|count| *count >= 3),
        "tiny catechism tail: {answer_slides:?}"
    );
    assert!(
        answer_word_counts
            .first()
            .zip(answer_word_counts.last())
            .is_some_and(|(first, last)| first >= last),
        "catechism answer should trend front-heavy: {answer_slides:?}"
    );
    assert!(rendered
        .presentation
        .cues
        .iter()
        .all(|cue| { cue.actions.iter().filter_map(macro_action_name).count() <= 1 }));
}

#[test]
fn each_explicit_question_answer_pair_is_partitioned_from_its_own_answer() {
    let (_root, themes) = theme_cache(vec![("Liturgy", fixture_slide())]);
    let style = RenderStyle::new(
        None,
        liturgy_role(
            "liturgy",
            "Liturgy",
            CueMacro::new(
                "Scripture/Prayer".to_string(),
                Some("Scripture/Prayer (Highlighted)".to_string()),
            )
            .expect("valid cue macros"),
        ),
        None,
        Some(1),
    )
    .expect("valid style");
    let macros = macro_cache(&["Scripture/Prayer", "Scripture/Prayer (Highlighted)"]);
    let content = ParsedContent::new(
        vec![
            parsed_segment("ALL: Preamble.", SpeakerRole::Audience),
            parsed_segment("Q. First question?", SpeakerRole::Leader),
            parsed_segment("A. First answer.", SpeakerRole::Audience),
            parsed_segment("Q. Second question?", SpeakerRole::Leader),
            parsed_segment("A. Second answer.", SpeakerRole::Audience),
        ],
        None,
    );
    let layout = TextLayout::new(20, 1).expect("valid layout");

    let slides = pack_description_for_slides(&content, layout);
    let text = slides
        .iter()
        .map(|slide| {
            slide
                .iter()
                .map(|segment| segment.text.as_str())
                .collect::<Vec<_>>()
                .join(" | ")
        })
        .collect::<Vec<_>>();

    assert_eq!(
        text,
        vec![
            "ALL: Preamble.",
            "Q. First question?",
            "A. First answer.",
            "Q. Second question?",
            "A. Second answer.",
        ]
    );
    assert_eq!(
        slides
            .iter()
            .map(|slide| slide[0].speaker)
            .collect::<Vec<_>>(),
        vec![
            SpeakerRole::Audience,
            SpeakerRole::Leader,
            SpeakerRole::Audience,
            SpeakerRole::Leader,
            SpeakerRole::Audience,
        ]
    );

    let mut rendered = render_source(
        "Multiple catechism pairs",
        PresentationSource::Description(&content),
        &style,
        &themes,
    )
    .expect("render explicit pairs");
    apply_role_macros(&mut rendered, &style, &macros).expect("apply pair macros");
    assert_eq!(rendered.presentation.cues.len(), 5);
    for (index, expected) in [
        "Scripture/Prayer",
        "Scripture/Prayer (Highlighted)",
        "Scripture/Prayer",
        "Scripture/Prayer (Highlighted)",
        "Scripture/Prayer",
    ]
    .into_iter()
    .enumerate()
    {
        assert!(cue_has_macro_named(
            &rendered.presentation.cues[index],
            expected
        ));
        assert_eq!(
            rendered.presentation.cues[index]
                .actions
                .iter()
                .filter_map(macro_action_name)
                .count(),
            1
        );
    }
}

#[test]
fn macros_follow_configured_role_transitions_instead_of_fixed_role_names() {
    let (_root, themes) = theme_cache(vec![
        ("Heading", fixture_slide()),
        ("Paragraph", fixture_slide()),
    ]);
    let macro_root = tempfile::tempdir().expect("macro root");
    let macro_path = macro_root.path().join("Macros");
    let document = rv_data::MacrosDocument {
        macros: vec![
            native_macro("Heading Macro", "heading-id"),
            native_macro("Body Macro", "body-id"),
        ],
        ..rv_data::MacrosDocument::default()
    };
    std::fs::write(&macro_path, document.encode_to_vec()).expect("write macros");
    let macros = MacroCache::load_from(&macro_path).expect("load macros");
    let style = RenderStyle::new(
        None,
        role(
            "paragraph-region",
            "Paragraph",
            BTreeMap::new(),
            Some(CueMacro::new("Body Macro".to_string(), None).expect("body macro")),
        ),
        Some(role(
            "heading-region",
            "Heading",
            BTreeMap::new(),
            Some(CueMacro::new("Heading Macro".to_string(), None).expect("heading macro")),
        )),
        None,
    )
    .expect("valid style");
    let content = description("Body", Some("Heading"));
    let mut rendered = render_source(
        "Role macros",
        PresentationSource::Description(&content),
        &style,
        &themes,
    )
    .expect("render role transitions");

    apply_role_macros(&mut rendered, &style, &macros).expect("apply macros");

    assert_eq!(rendered.cue_roles.transitions().len(), 2);
    assert_eq!(
        rendered.cue_roles.transitions()[0].role().as_str(),
        "heading-region"
    );
    assert_eq!(rendered.cue_roles.transitions()[0].cue_index(), 0);
    assert_eq!(
        rendered.cue_roles.transitions()[1].role().as_str(),
        "paragraph-region"
    );
    assert_eq!(rendered.cue_roles.transitions()[1].cue_index(), 1);
    assert!(cue_has_macro_named(
        &rendered.presentation.cues[0],
        "Heading Macro"
    ));
    assert!(cue_has_macro_named(
        &rendered.presentation.cues[1],
        "Body Macro"
    ));
    assert_eq!(
        rendered.presentation.cues[0]
            .actions
            .iter()
            .filter_map(macro_action_name)
            .collect::<Vec<_>>(),
        vec!["Heading Macro"]
    );
    assert_eq!(
        rendered.presentation.cues[1]
            .actions
            .iter()
            .filter_map(macro_action_name)
            .collect::<Vec<_>>(),
        vec!["Body Macro"]
    );
}

fn native_macro(name: &str, id: &str) -> rv_data::macros_document::Macro {
    rv_data::macros_document::Macro {
        uuid: Some(rv_data::Uuid {
            string: id.to_string(),
        }),
        name: name.to_string(),
        ..rv_data::macros_document::Macro::default()
    }
}
