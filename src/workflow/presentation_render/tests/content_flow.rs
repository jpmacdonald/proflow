use super::*;

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
        let rendered = render_with_configured_macros(
            "Mixed liturgy",
            PresentationSource::Description(&content),
            &style,
            vec![("Liturgy", fixture_slide())],
            &["Scripture/Prayer", "Scripture/Prayer (Highlighted)"],
        );

        assert_eq!(rendered.presentation().cues.len(), 1);
        assert!(cue_has_macro_named(
            &rendered.presentation().cues[0],
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
    let answer = "A. Do take care of all our physical needs so that we come to know that you are the only source of everything good, and that neither our work and worry, nor your gifts, can do us any good without your blessing. And so help us to give up our trust in creatures and trust in you alone.";
    let content = ParsedContent::new(
        vec![
            parsed_segment("Q. What is our hope?", SpeakerRole::Leader),
            parsed_segment(answer, SpeakerRole::Audience),
        ],
        Some("Affirmation of Faith".to_string()),
    );

    let rendered = render_with_configured_macros(
        "Catechism",
        PresentationSource::Description(&content),
        &style,
        vec![("Title", fixture_slide()), ("Liturgy", fixture_slide())],
        &[
            "Name Tag/Title",
            "Scripture/Prayer",
            "Scripture/Prayer (Highlighted)",
        ],
    );

    assert!(rendered.presentation().cues.len() >= 3);
    assert_eq!(rendered_text(&rendered, 1), "Q. What is our hope?");
    assert!(!rendered_text(&rendered, 1).contains("A."));
    assert!(cue_has_macro_named(
        &rendered.presentation().cues[1],
        "Scripture/Prayer (Highlighted)"
    ));
    assert!(cue_has_macro_named(
        &rendered.presentation().cues[2],
        "Scripture/Prayer"
    ));
    let answer_slides = (2..rendered.presentation().cues.len())
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
        .presentation()
        .cues
        .iter()
        .all(|cue| { cue.actions.iter().filter_map(macro_action_name).count() <= 1 }));
}

#[test]
fn each_explicit_question_answer_pair_is_partitioned_from_its_own_answer() {
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

    let slides = pack_description_for_slides_estimated(&content, layout);
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

    let rendered = render_with_configured_macros(
        "Multiple catechism pairs",
        PresentationSource::Description(&content),
        &style,
        vec![("Liturgy", fixture_slide())],
        &["Scripture/Prayer", "Scripture/Prayer (Highlighted)"],
    );
    assert_eq!(rendered.presentation().cues.len(), 5);
    assert_alternating_liturgy_macro_contract(&rendered, &style);
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
            &rendered.presentation().cues[index],
            expected
        ));
        assert_eq!(
            rendered.presentation().cues[index]
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
    let rendered = render_with_configured_macros(
        "Role macros",
        PresentationSource::Description(&content),
        &style,
        vec![("Heading", fixture_slide()), ("Paragraph", fixture_slide())],
        &["Heading Macro", "Body Macro"],
    );

    assert_eq!(rendered.cue_roles().transitions().len(), 2);
    assert_eq!(
        rendered.cue_roles().transitions()[0].role().as_str(),
        "heading-region"
    );
    assert_eq!(rendered.cue_roles().transitions()[0].cue_index(), 0);
    assert_eq!(
        rendered.cue_roles().transitions()[1].role().as_str(),
        "paragraph-region"
    );
    assert_eq!(rendered.cue_roles().transitions()[1].cue_index(), 1);
    assert!(cue_has_macro_named(
        &rendered.presentation().cues[0],
        "Heading Macro"
    ));
    assert!(cue_has_macro_named(
        &rendered.presentation().cues[1],
        "Body Macro"
    ));
    assert_eq!(
        rendered.presentation().cues[0]
            .actions
            .iter()
            .filter_map(macro_action_name)
            .collect::<Vec<_>>(),
        vec!["Heading Macro"]
    );
    assert_eq!(
        rendered.presentation().cues[1]
            .actions
            .iter()
            .filter_map(macro_action_name)
            .collect::<Vec<_>>(),
        vec!["Body Macro"]
    );
}
