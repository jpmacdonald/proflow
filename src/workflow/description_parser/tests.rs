#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use super::*;

#[test]
fn test_responsive_reading() {
    let desc = "Leader: The Lord is my shepherd;\nPeople: I shall not want.\nLeader: He makes me lie down in green pastures.\nAll: He restores my soul.";
    let result = parse_description(
        desc,
        "Call to Worship (Robert)",
        DescriptionParserKind::Liturgical,
    )
    .expect("description should parse");
    assert!(result.is_some());
    let content = result.unwrap();
    assert_eq!(content.title_text.as_deref(), Some("Call to Worship"));
    assert_eq!(content.segments.len(), 7);
    // Speaker identity survives parsing independently of native styling.
    assert!(content.segments[0].text.starts_with("Leader:"));
    assert_eq!(content.segments[0].speaker, SpeakerRole::Leader);
    // People/All lines keep their prefixes and audience identity.
    assert!(content.segments[1].text.is_empty());
    assert!(content.segments[2].text.starts_with("People:"));
    assert_eq!(content.segments[2].speaker, SpeakerRole::Audience);
    assert!(content.segments[3].text.is_empty());
    assert!(content.segments[6].text.starts_with("All:"));
    assert_eq!(content.segments[6].speaker, SpeakerRole::Audience);
}

#[test]
fn inline_communion_responses_split_speakers_and_drop_operational_bullets() {
    let desc = "- Invitation/Explanation: CONNIE\n- Great Thanksgiving: CONNIE\nLeader: The Lord be with you. People: And also with you\nLeader: Lift up your hearts. People: We lift them up to the Lord.\nLeader: Let us give thanks to the Lord our God. People: It is right to give our thanks and praise.\n- Communion Prayers";
    let content = parse_description(
        desc,
        "Communion: Connie, Adrian",
        DescriptionParserKind::Liturgical,
    )
    .expect("description should parse")
    .expect("responsive content");
    let spoken = content
        .segments
        .iter()
        .filter(|segment| !segment.text.is_empty())
        .collect::<Vec<_>>();

    assert_eq!(spoken.len(), 6);
    assert!(spoken[0].text.starts_with("Leader:"));
    assert!(spoken[1].text.starts_with("People:"));
    assert_eq!(spoken[0].speaker, SpeakerRole::Leader);
    assert_eq!(spoken[1].speaker, SpeakerRole::Audience);
    assert!(spoken.iter().all(|segment| !segment.text.starts_with('-')));
}

#[test]
fn test_marker_parsing() {
    let desc = "[CONFESSION no slide] - If we say that we have no sin...\n[SLIDE/ALL] - [Precious Lord, the cross is ever before us...]\n[SILENT CONFESSION]\n[ASSURANCE no slide] - Rejoice!";
    let result = parse_description(
        desc,
        "Prayer of Confession (Hope)",
        DescriptionParserKind::Liturgical,
    )
    .expect("description should parse");
    assert!(result.is_some());
    let content = result.unwrap();
    assert_eq!(content.segments.len(), 1);
    assert!(content.segments[0].text.contains("Precious Lord"));
    assert_eq!(content.segments[0].speaker, SpeakerRole::Audience);
}

#[test]
fn marker_state_excludes_intro_and_keeps_following_slide_lines() {
    let desc = "Robert Intro (no slide): Explain the catechism.\n\
                [SLIDE just for the part below]\n\
                Q. What does daily bread mean?\n\
                A. Trust God alone.";
    let content = parse_description(
        desc,
        "Affirmation of Faith (Robert)",
        DescriptionParserKind::Liturgical,
    )
    .expect("description should parse")
    .expect("slide marker should expose following content");

    assert_eq!(content.title_text.as_deref(), Some("Affirmation of Faith"));
    assert_eq!(content.segments.len(), 2);
    assert_eq!(content.segments[0].text, "Q. What does daily bread mean?");
    assert_eq!(content.segments[1].text, "A. Trust God alone.");
    assert_eq!(content.segments[0].speaker, SpeakerRole::Leader);
    assert_eq!(content.segments[1].speaker, SpeakerRole::Audience);
    assert_eq!(content.flow, DescriptionFlow::QuestionAnswer);
}

#[test]
fn catechism_answer_continuations_stay_white() {
    let desc = "[SLIDE just for the part below]\n\
                Q. What does daily bread mean?\n\
                A. Trust God for every need,\n\
                and give up our trust in creatures.";
    let content = parse_description(
        desc,
        "Affirmation of Faith",
        DescriptionParserKind::Liturgical,
    )
    .expect("description should parse")
    .expect("catechism should produce content");

    assert_eq!(content.segments.len(), 2);
    assert_eq!(content.segments[0].speaker, SpeakerRole::Leader);
    assert_eq!(
        content.segments[1].text,
        "A. Trust God for every need, and give up our trust in creatures."
    );
    assert_eq!(content.segments[1].speaker, SpeakerRole::Audience);
}

#[test]
fn heidelberg_q125_hard_wraps_are_one_answer_paragraph() {
    let desc = r#"Robert Intro (no slide): The Heidelberg Catechism was written in 1562 as a joint affirmation of faith between German Lutherans and Reformed (Presbyterians). It is part of the collection of creeds and catechisms of the Presbyterian Church USA of which we are a part. It is in a question and answer format, so I will read the question and invite you to respond in answer. This question is about the term "daily bread" in the Lord's Prayer, which draws on God's provision of manna in our sermon text today. Please stand...
 [SLIDE just for the part below]
Q. What does "give us this day our daily bread" mean in the Lord's Prayer?
A. “Give us this day our daily bread” means:
Do take care of all our physical needs
so that we come to know
that you are the only source of everything good,
and that neither our work and worry, nor your gifts,
can do us any good without your blessing.
And so help us to give up our trust in creatures
and trust in you alone."#;
    let content = parse_description(
        desc,
        "Affirmation of Faith: The Heidelberg Catechism (1562), Q.125 (Robert)",
        DescriptionParserKind::Liturgical,
    )
    .expect("description should parse")
    .expect("catechism should produce content");

    assert_eq!(
        content.title_text.as_deref(),
        Some("Affirmation of Faith: The Heidelberg Catechism (1562), Q.125")
    );
    assert_eq!(content.segments.len(), 2);
    assert_eq!(
        content.segments[0].text,
        "Q. What does \"give us this day our daily bread\" mean in the Lord's Prayer?"
    );
    assert_eq!(content.segments[0].speaker, SpeakerRole::Leader);
    assert_eq!(
        content.segments[1].text,
        "A. “Give us this day our daily bread” means: Do take care of all our physical needs so that we come to know that you are the only source of everything good, and that neither our work and worry, nor your gifts, can do us any good without your blessing. And so help us to give up our trust in creatures and trust in you alone."
    );
    assert_eq!(content.segments[1].speaker, SpeakerRole::Audience);
}

#[test]
fn unresolved_editorial_placeholders_are_typed_errors() {
    for (description, title, parser, expected) in [
        (
            "[CONFESSION no slide] - introduction\n[SLIDE/ALL] - [insert prayer]\n[SILENT CONFESSION]",
            "Prayer of Confession",
            DescriptionParserKind::Liturgical,
            "insert prayer",
        ),
        (
            "[ADD TITLE]",
            "Weekly Liturgy",
            DescriptionParserKind::Liturgical,
            "ADD TITLE",
        ),
        (
            "[INSERT TRANSLATION]",
            "Weekly Liturgy",
            DescriptionParserKind::Liturgical,
            "INSERT TRANSLATION",
        ),
        (
            "[SLIDE] ___",
            "Weekly Liturgy",
            DescriptionParserKind::Liturgical,
            "___",
        ),
        (
            "Robert, Speaker",
            "Offertory [ADD TITLE, COMPOSER LAST NAME]",
            DescriptionParserKind::ContentNametag,
            "Offertory [ADD TITLE, COMPOSER LAST NAME]",
        ),
    ] {
        let error = parse_description(description, title, parser)
            .expect_err("placeholder must not become parsed content");
        assert_eq!(
            error,
            DescriptionParseError::UnresolvedPlaceholder(expected.to_string())
        );
    }
}

#[test]
fn test_content_nametag() {
    let desc = "Marilyn Shenenberger, Organ / Darwin Wolford / Eugene Butler";
    let result = parse_description(
        desc,
        "Organ Prelude: Meditation with Aria",
        DescriptionParserKind::ContentNametag,
    )
    .expect("description should parse");
    assert!(result.is_some());
    let content = result.unwrap();
    assert_eq!(content.segments[0].text, "Meditation with Aria");
    assert!(content.title_text.is_none());
}

#[test]
fn content_nametag_normalizes_source_soft_wraps() {
    let content = parse_description(
        "Marilyn Shenenberger, Organ /\nDarwin Wolford /\nEugene Butler",
        "Organ Prelude: Meditation with Aria",
        DescriptionParserKind::ContentNametag,
    )
    .expect("description should parse")
    .expect("content nametag always produces content");

    assert!(content
        .segments
        .iter()
        .all(|segment| !segment.text.contains(['\n', '\r'])));
    assert_eq!(content.segments[1].text, "Darwin Wolford / Eugene Butler");
    assert_eq!(content.segments[2].text, "Marilyn Shenenberger, Organ");
}

#[test]
fn explicit_question_answer_pairs_ignore_audience_preamble_and_cover_each_pair() {
    let content = parse_description(
        "A. Audience preamble.\nQ. First question?\nA. First answer.\nQ. Second question?\nA. Second answer.",
        "Affirmation of Faith",
        DescriptionParserKind::Liturgical,
    )
    .expect("description should parse")
    .expect("question and answer text should produce content");

    assert_eq!(content.flow, DescriptionFlow::QuestionAnswer);
    assert_eq!(
        content.question_answer_pairs,
        vec![
            QuestionAnswerPair {
                question_start: 1,
                answer_start: 2,
                end: 3,
            },
            QuestionAnswerPair {
                question_start: 3,
                answer_start: 4,
                end: 5,
            },
        ]
    );
}

#[test]
fn content_nametag_excludes_starred_operator_notes() {
    let desc = "Talking about CRU\n*Announce: Alex is available between services";
    let content = parse_description(
        desc,
        "Moment for Mission: Alex Viera",
        DescriptionParserKind::ContentNametag,
    )
    .expect("description should parse")
    .expect("content nametag always produces content");

    let text = content
        .segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect::<Vec<_>>();
    assert_eq!(text, ["Alex Viera", "Talking about CRU"]);
}

#[test]
fn audience_liturgy_defaults_unmarked_prose_to_audience() {
    let content = parse_description(
        "Together we pray.",
        "Unison Prayer",
        DescriptionParserKind::LiturgicalAudience,
    )
    .expect("description should parse")
    .expect("plain audience text should produce content");
    assert_eq!(content.segments[0].speaker, SpeakerRole::Audience);
}

#[test]
fn test_no_content_returns_none() {
    let result = parse_description("", "Empty Item", DescriptionParserKind::Liturgical)
        .expect("empty description should not be invalid");
    assert!(result.is_none());
}

#[test]
fn test_plain_text_fallback() {
    let desc = "Grace and peace to you.\nIn Christ we are made whole.";
    let result = parse_description(
        desc,
        "Affirmation of Faith",
        DescriptionParserKind::Liturgical,
    )
    .expect("description should parse");
    assert!(result.is_some());
    let content = result.unwrap();
    assert_eq!(content.segments.len(), 1);
    assert_eq!(
        content.segments[0].text,
        "Grace and peace to you. In Christ we are made whole."
    );
    assert_eq!(content.segments[0].speaker, SpeakerRole::Leader);
}

#[test]
fn plain_text_preserves_an_explicit_blank_paragraph() {
    let desc = "First paragraph is hard\n    wrapped at the source.\n   \nSecond paragraph also\n  continues here.";
    let content = parse_description(
        desc,
        "Affirmation of Faith",
        DescriptionParserKind::Liturgical,
    )
    .expect("description should parse")
    .expect("paragraphs should produce content");

    let text = content
        .segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        text,
        [
            "First paragraph is hard wrapped at the source.",
            "",
            "Second paragraph also continues here."
        ]
    );
}

#[test]
fn test_short_prefix_no_false_positive() {
    // "l:" and "p:" should NOT trigger responsive reading detection
    let desc = "See the full color: blue.\nVisit url: example.com\nAll: together now.";
    // Has "all:" but no "leader:" — should NOT be responsive
    assert!(!has_responsive_pattern(desc));
}

#[test]
fn test_slide_all_without_brackets() {
    let desc = "[SLIDE/ALL] Hear our prayer, O Lord.";
    let result = parse_description(desc, "Prayer (Hope)", DescriptionParserKind::Liturgical)
        .expect("description should parse");
    assert!(result.is_some());
    let content = result.unwrap();
    assert_eq!(content.segments.len(), 1);
    assert!(content.segments[0].text.contains("Hear our prayer"));
    assert_eq!(content.segments[0].speaker, SpeakerRole::Audience);
}

#[test]
fn test_content_nametag_no_colon() {
    let desc = "Special offering for missions";
    let result = parse_description(
        desc,
        "Giving of Tithes and Offerings",
        DescriptionParserKind::ContentNametag,
    )
    .expect("description should parse");
    assert!(result.is_some());
    let content = result.unwrap();
    // Should use description content, not the full title
    assert_eq!(content.segments[0].text, "Special offering for missions");
}

#[test]
fn test_responsive_without_colon() {
    // "Leader " (space, no colon) should also work
    let desc = "Leader The Lord is good.\nPeople We give thanks.";
    let result = parse_description(
        desc,
        "Responsive Reading",
        DescriptionParserKind::Liturgical,
    )
    .expect("description should parse");
    assert!(result.is_some());
    let content = result.unwrap();
    assert_eq!(content.segments.len(), 3);
    // Prefixes kept in text
    assert!(content.segments[0].text.starts_with("Leader"));
    assert_eq!(content.segments[0].speaker, SpeakerRole::Leader);
    assert!(content.segments[1].text.is_empty());
    assert!(content.segments[2].text.starts_with("People"));
    assert_eq!(content.segments[2].speaker, SpeakerRole::Audience);
}

#[test]
fn test_responsive_filters_metadata() {
    let desc = "Liturgist: Bill Ichord; Scripture/Liturgy [SLIDE]\nLEADER: The Lord reigns.\nALL: Let the earth rejoice.";
    let result = parse_description(desc, "Call to Worship", DescriptionParserKind::Liturgical)
        .expect("description should parse");
    assert!(result.is_some());
    let content = result.unwrap();
    // Metadata line filtered out, only LEADER and ALL remain
    assert_eq!(content.segments.len(), 3);
    assert!(content.segments[0].text.starts_with("LEADER:"));
    assert!(content.segments[1].text.is_empty());
    assert!(content.segments[2].text.starts_with("ALL:"));
}

#[test]
fn test_responsive_blank_line_separators() {
    let desc =
        "LEADER: First section.\nALL: Response one.\n\nLEADER: Second section.\nALL: Response two.";
    let result = parse_description(desc, "Call to Worship", DescriptionParserKind::Liturgical)
        .expect("description should parse");
    assert!(result.is_some());
    let content = result.unwrap();
    // 4 content segments + separators between every response = 7
    assert_eq!(content.segments.len(), 7);
    assert!(content.segments[1].text.is_empty());
    assert!(content.segments[3].text.is_empty());
    assert!(content.segments[5].text.is_empty());
}

#[test]
fn test_responsive_inserts_separator_between_response_blocks() {
    let desc =
        "LEADER: First section.\nALL: Response one.\nLEADER: Second section.\nALL: Response two.";
    let result = parse_description(desc, "Call to Worship", DescriptionParserKind::Liturgical)
        .expect("description should parse");
    assert!(result.is_some());
    let content = result.unwrap();
    assert_eq!(content.segments.len(), 7);
    assert!(content.segments[1].text.is_empty());
    assert!(content.segments[3].text.is_empty());
    assert!(content.segments[4].text.starts_with("LEADER: Second"));
}

#[test]
fn responsive_source_wraps_stay_inside_their_speaker_blocks() {
    let desc = "LEADER: This sentence is hard\n    wrapped at the source.\nALL: This response is also\n    source wrapped.";
    let content = parse_description(desc, "Call to Worship", DescriptionParserKind::Liturgical)
        .expect("description should parse")
        .expect("responsive text should produce content");

    assert_eq!(content.segments.len(), 3);
    assert_eq!(
        content.segments[0].text,
        "LEADER: This sentence is hard wrapped at the source."
    );
    assert!(content.segments[1].text.is_empty());
    assert_eq!(
        content.segments[2].text,
        "ALL: This response is also source wrapped."
    );
    assert_eq!(content.segments[2].speaker, SpeakerRole::Audience);
}

#[test]
fn responsive_unmarked_display_text_defaults_to_leader() {
    let desc = "Opening words.\nLEADER: The Lord is with you.\nALL: And also with you.";
    let content = parse_description(desc, "Call to Worship", DescriptionParserKind::Liturgical)
        .expect("description should parse")
        .expect("responsive text should produce content");

    assert_eq!(content.segments[0].text, "Opening words.");
    assert_eq!(content.segments[0].speaker, SpeakerRole::Leader);
}
