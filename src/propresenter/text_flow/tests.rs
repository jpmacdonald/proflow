#![allow(clippy::expect_used, clippy::unwrap_used)]

use proptest::prelude::*;

use super::{
    pack_segments_for_slides, slide_word_count, word_wrap, TextFlowSegment, TextLayout,
    TextLayoutError, MIN_SLIDE_WRAP,
};
use crate::propresenter::rtf::StyledSegment;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Speaker {
    Neutral,
    Leader,
    Audience,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SemanticSegment {
    text: String,
    speaker: Speaker,
    source_id: u8,
}

impl TextFlowSegment for SemanticSegment {
    fn text(&self) -> &str {
        &self.text
    }

    fn with_text(&self, text: String) -> Self {
        Self {
            text,
            ..self.clone()
        }
    }
}

fn normalized(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn slide_line_count<S: TextFlowSegment>(slide: &[S], layout: TextLayout) -> usize {
    slide
        .iter()
        .map(|segment| {
            if segment.text().trim().is_empty() {
                1
            } else {
                layout.estimated_lines(segment.text())
            }
        })
        .sum()
}

#[test]
fn layout_rejects_zero_lines_instead_of_clamping() {
    assert_eq!(
        TextLayout::new(40, 0),
        Err(TextLayoutError::ZeroMaxLinesPerSlide)
    );
}

#[test]
fn layout_rejects_unsupported_wrap_width() {
    assert_eq!(
        TextLayout::new(MIN_SLIDE_WRAP - 1, 3),
        Err(TextLayoutError::WrapColumnTooSmall {
            actual: MIN_SLIDE_WRAP - 1,
            minimum: MIN_SLIDE_WRAP,
        })
    );
}

#[test]
fn word_wraps_using_display_width() {
    assert_eq!(
        word_wrap("hello world foo bar", 10),
        vec!["hello", "world foo", "bar"]
    );
    assert_eq!(word_wrap("", 40), vec![""]);
}

#[test]
fn overlong_styled_segment_is_fragmented_without_losing_style_or_text() {
    let layout = TextLayout::new(20, 2).expect("valid layout");
    let source = StyledSegment {
        text: "Alpha beta gamma; delta epsilon zeta eta theta iota kappa lambda mu.".to_string(),
        color: Some((12, 34, 56)),
        bold: Some(true),
        italic: Some(false),
    };

    let slides = pack_segments_for_slides(std::slice::from_ref(&source), layout);

    assert!(slides.len() > 1);
    assert!(slides
        .iter()
        .all(|slide| slide_line_count(slide, layout) <= layout.max_lines()));
    assert!(slides.iter().flatten().all(|fragment| {
        fragment.color == source.color
            && fragment.bold == source.bold
            && fragment.italic == source.italic
    }));
    assert_eq!(
        slides
            .iter()
            .flatten()
            .map(|fragment| normalized(&fragment.text))
            .collect::<String>(),
        normalized(&source.text)
    );
}

#[test]
fn generic_flow_preserves_semantic_metadata_through_fragmentation_and_packing() {
    let layout = TextLayout::new(20, 2).expect("valid layout");
    let segments = vec![
        SemanticSegment {
            text: "Alpha beta gamma; delta epsilon zeta eta theta iota kappa lambda mu."
                .to_string(),
            speaker: Speaker::Leader,
            source_id: 1,
        },
        SemanticSegment {
            text: "Audience response remains intact.".to_string(),
            speaker: Speaker::Audience,
            source_id: 2,
        },
    ];

    let slides = pack_segments_for_slides(&segments, layout);
    let emitted = slides.iter().flatten().collect::<Vec<_>>();
    let leader_fragments = emitted
        .iter()
        .copied()
        .filter(|fragment| fragment.source_id == 1)
        .collect::<Vec<_>>();
    let audience_fragments = emitted
        .iter()
        .copied()
        .filter(|fragment| fragment.source_id == 2)
        .collect::<Vec<_>>();

    assert!(leader_fragments.len() > 1, "leader text must fragment");
    assert!(!audience_fragments.is_empty());
    assert!(leader_fragments
        .iter()
        .all(|fragment| fragment.speaker == Speaker::Leader));
    assert!(audience_fragments
        .iter()
        .all(|fragment| fragment.speaker == Speaker::Audience));
    assert_eq!(
        leader_fragments
            .iter()
            .map(|fragment| normalized(&fragment.text))
            .collect::<String>(),
        normalized(&segments[0].text)
    );
    assert_eq!(
        audience_fragments
            .iter()
            .map(|fragment| normalized(&fragment.text))
            .collect::<String>(),
        normalized(&segments[1].text)
    );
    assert!(emitted
        .windows(2)
        .all(|pair| pair[0].source_id <= pair[1].source_id));
}

#[test]
fn long_catechism_answer_uses_natural_front_loaded_fragments_without_a_tiny_tail() {
    let layout = TextLayout::new(32, 4).expect("valid layout");
    let answer = "Do take care of all our physical needs so that we come to know that you are the only source of everything good, and that neither our work and worry, nor your gifts, can do us any good without your blessing. And so help us to give up our trust in creatures and trust in you alone.";
    let source = StyledSegment::unstyled(answer);

    let slides = pack_segments_for_slides(std::slice::from_ref(&source), layout);
    let fragments = slides
        .iter()
        .map(|slide| slide[0].text.as_str())
        .collect::<Vec<_>>();
    let word_counts = fragments
        .iter()
        .map(|fragment| fragment.split_whitespace().count())
        .collect::<Vec<_>>();

    assert!(fragments.len() > 1);
    assert!(slides
        .iter()
        .all(|slide| slide_line_count(slide, layout) <= layout.max_lines()));
    assert_eq!(
        normalized(&fragments.concat()),
        normalized(answer),
        "all source words and punctuation must survive"
    );
    assert!(
        word_counts.last().is_some_and(|count| *count >= 3),
        "tiny final fragment: {fragments:?}"
    );
    assert!(
        word_counts.windows(2).all(|pair| pair[0] >= pair[1]),
        "fragments must be front-loaded: {fragments:?}"
    );
    assert!(
        fragments[..fragments.len() - 1].iter().all(|fragment| {
            fragment
                .trim_end_matches(['"', '\'', '’', '”', ')', ']', '}'])
                .ends_with(['.', '?', '!', ';', ',', ':', '—'])
        }),
        "every available natural boundary should be used: {fragments:?}"
    );
    let repeated = pack_segments_for_slides(std::slice::from_ref(&source), layout);
    assert_eq!(
        fragments,
        repeated
            .iter()
            .map(|slide| slide[0].text.as_str())
            .collect::<Vec<_>>(),
        "partitioning must be deterministic"
    );
}

#[test]
fn global_partition_rebalances_the_greedy_one_word_tail() {
    let layout = TextLayout::new(20, 1).expect("valid layout");
    let source = StyledSegment::unstyled("aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii");

    let slides = pack_segments_for_slides(std::slice::from_ref(&source), layout);
    let word_counts = slides
        .iter()
        .map(|slide| slide[0].text.split_whitespace().count())
        .collect::<Vec<_>>();

    assert_eq!(word_counts, vec![3, 3, 3]);
    assert!(slides
        .iter()
        .all(|slide| slide_line_count(slide, layout) <= layout.max_lines()));
    assert_eq!(
        normalized(
            &slides
                .iter()
                .flat_map(|slide| slide.iter())
                .map(|segment| segment.text.as_str())
                .collect::<String>()
        ),
        normalized(&source.text)
    );
}

#[test]
fn adjacent_semantic_segments_rebalance_a_tiny_final_response() {
    let layout = TextLayout::new(20, 3).expect("valid layout");
    let segments = vec![
        SemanticSegment {
            text: "aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii jjjj kkkk llll".to_string(),
            speaker: Speaker::Leader,
            source_id: 1,
        },
        SemanticSegment {
            text: String::new(),
            speaker: Speaker::Neutral,
            source_id: 3,
        },
        SemanticSegment {
            text: "Amen.".to_string(),
            speaker: Speaker::Audience,
            source_id: 2,
        },
    ];

    let slides = pack_segments_for_slides(&segments, layout);

    assert_eq!(slides.len(), 2);
    assert!(slides
        .iter()
        .all(|slide| slide_line_count(slide, layout) <= layout.max_lines()));
    assert_eq!(
        slides[1]
            .iter()
            .map(|segment| (segment.text.as_str(), segment.speaker, segment.source_id))
            .collect::<Vec<_>>(),
        vec![
            ("kkkk llll", Speaker::Leader, 1),
            ("", Speaker::Neutral, 3),
            ("Amen.", Speaker::Audience, 2),
        ]
    );
    assert_eq!(slide_word_count(&slides[1]), 3);
    assert_eq!(
        slides
            .iter()
            .flatten()
            .filter(|segment| segment.source_id == 1)
            .map(|segment| normalized(&segment.text))
            .collect::<String>(),
        normalized(&segments[0].text)
    );
}

#[test]
fn blanks_are_kept_between_fitting_content_but_not_stranded() {
    let layout = TextLayout::new(80, 4).expect("valid layout");
    let segments = vec![
        StyledSegment::unstyled("First"),
        StyledSegment::unstyled(""),
        StyledSegment::unstyled(""),
        StyledSegment::unstyled("Second"),
        StyledSegment::unstyled(""),
        StyledSegment::unstyled("Third"),
        StyledSegment::unstyled(""),
    ];

    let slides = pack_segments_for_slides(&segments, layout);

    assert_eq!(slides.len(), 2);
    assert_eq!(
        slides[0]
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<Vec<_>>(),
        vec!["First", "", "", "Second"]
    );
    assert_eq!(slides[1][0].text, "Third");
    assert!(slides.iter().all(|slide| {
        slide
            .first()
            .is_some_and(|segment| !segment.text.trim().is_empty())
            && slide
                .last()
                .is_some_and(|segment| !segment.text.trim().is_empty())
    }));
}

fn paragraph_strategy() -> impl Strategy<Value = String> {
    prop::collection::vec("[A-Za-z0-9]{1,12}[,;:.!?]?", 1..30).prop_map(|words| {
        words
            .into_iter()
            .enumerate()
            .map(|(index, word)| {
                if index % 3 == 0 {
                    format!("  {word}")
                } else {
                    format!(" {word}")
                }
            })
            .collect::<String>()
    })
}

proptest! {
    #[test]
    fn styled_flow_preserves_bounds_content_order_and_style(
        paragraphs in prop::collection::vec(prop::option::of(paragraph_strategy()), 0..16),
        wrap_column in MIN_SLIDE_WRAP..80usize,
        max_lines in 1..8usize,
    ) {
        let segments = paragraphs
            .into_iter()
            .enumerate()
            .map(|(index, paragraph)| {
                let color_id = u8::try_from(index).expect("generated index fits in u8");
                StyledSegment {
                    text: paragraph.unwrap_or_default(),
                    color: Some((color_id, 255_u8.saturating_sub(color_id), color_id)),
                    bold: Some(index % 2 == 0),
                    italic: Some(index % 3 == 0),
                }
            })
            .collect::<Vec<_>>();
        let layout = TextLayout::new(wrap_column, max_lines).expect("generated valid layout");

        let slides = pack_segments_for_slides(&segments, layout);

        prop_assert!(slides.iter().all(|slide| !slide.is_empty()));
        let no_stranded_blanks = slides.iter().all(|slide| {
            !slide.first().is_some_and(|segment| segment.text.trim().is_empty())
                && !slide.last().is_some_and(|segment| segment.text.trim().is_empty())
        });
        prop_assert!(no_stranded_blanks);
        prop_assert!(slides
            .iter()
            .all(|slide| slide_line_count(slide, layout) <= layout.max_lines()));

        for (index, source) in segments.iter().enumerate().filter(|(_, segment)| !segment.text.trim().is_empty()) {
            let fragments = slides
                .iter()
                .flatten()
                .filter(|fragment| fragment.color == source.color)
                .collect::<Vec<_>>();
            prop_assert!(!fragments.is_empty());
            let styles_match = fragments.iter().all(|fragment| {
                fragment.bold == source.bold && fragment.italic == source.italic
            });
            prop_assert!(styles_match);
            prop_assert_eq!(
                fragments
                    .iter()
                    .map(|fragment| normalized(&fragment.text))
                    .collect::<String>(),
                normalized(&source.text),
                "source segment {} changed during flow",
                index
            );
        }

        let emitted_source_order = slides
            .iter()
            .flatten()
            .filter(|segment| !segment.text.trim().is_empty())
            .map(|segment| segment.color.expect("generated color").0)
            .collect::<Vec<_>>();
        prop_assert!(emitted_source_order.windows(2).all(|pair| pair[0] <= pair[1]));
    }
}
