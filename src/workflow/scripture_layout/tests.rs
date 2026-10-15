#![allow(clippy::expect_used)]

use super::*;

fn layout(wrap: usize, lines: usize) -> TextLayout {
    TextLayout::new(wrap, lines).expect("valid layout")
}

fn normalized(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn expected(verses: &[Verse]) -> String {
    verses
        .iter()
        .map(|verse| {
            let number = to_superscript(verse.number);
            let text = verse.text.trim();
            if text.is_empty() {
                number
            } else {
                format!("{number} {text}")
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn assert_valid(verses: &[Verse], slides: &[ScriptureSlide], layout: TextLayout) {
    assert_eq!(
        normalized(&slides.iter().map(ScriptureSlide::text).collect::<String>()),
        normalized(&expected(verses))
    );
    assert!(slides.iter().all(|slide| !slide.text().is_empty()
        && !slide.verse_numbers().is_empty()
        && layout.estimated_lines(slide.text()) <= layout.max_lines().min(MAX_SCRIPTURE_LINES)));

    let mut first_occurrences = Vec::new();
    for number in slides
        .iter()
        .flat_map(|slide| slide.verse_numbers().iter().copied())
    {
        if first_occurrences.last().copied() != Some(number) {
            first_occurrences.push(number);
        }
    }
    assert_eq!(
        first_occurrences,
        verses.iter().map(|verse| verse.number).collect::<Vec<_>>()
    );
}

#[test]
fn long_verses_split_at_supported_punctuation() {
    for punctuation in [';', ',', '.', '?', '!', ':', '—'] {
        let verses = [Verse {
            number: 1,
            text: format!("Alpha beta{punctuation} gamma delta epsilon zeta eta theta"),
        }];
        let layout = layout(20, 1);
        let slides = split_verses_for_slides(&verses, layout);
        assert!(slides.len() > 1);
        assert!(slides[0].text().ends_with(punctuation));
        assert_valid(&verses, &slides, layout);
    }
}

#[test]
fn unpunctuated_text_uses_latest_fitting_word_boundary() {
    let verses = [Verse {
        number: 7,
        text: "alpha beta gamma delta epsilon zeta eta theta iota kappa".to_string(),
    }];
    let layout = layout(20, 1);
    let slides = split_verses_for_slides(&verses, layout);
    assert!(slides[0].text().ends_with("gamma"));
    assert_valid(&verses, &slides, layout);
}

#[test]
fn exodus_screenshot_partition_keeps_clauses_and_full_final_verse() {
    let verses = [
        Verse {
            number: 1,
            text: "The whole congregation of the Israelites set out from Elim and came to the wilderness of Sin, which is between Elim and Sinai, on the fifteenth day of the second month after they had departed from the land of Egypt.".to_string(),
        },
        Verse {
            number: 2,
            text: "The whole congregation of the Israelites complained against Moses and Aaron in the wilderness.".to_string(),
        },
        Verse {
            number: 3,
            text: "The Israelites said to them, “If only we had died by the hand of the Lord in the land of Egypt, when we sat by the pots of meat and ate our fill of bread, for you have brought us out into this wilderness to kill this whole assembly with hunger.”".to_string(),
        },
        Verse {
            number: 4,
            text: "Then the Lord said to Moses, “I am going to rain bread from heaven for you, and each day the people shall go out and gather enough for that day.".to_string(),
        },
    ];
    // Three slides fit at this width only by breaking inside a sentence.
    // The optimizer deliberately uses four slides to keep natural pauses.
    let layout = layout(38, 7);

    let slides = split_verses_for_slides(&verses, layout);

    assert_eq!(
        slides
            .iter()
            .map(ScriptureSlide::text)
            .collect::<Vec<_>>(),
        vec![
            "¹ The whole congregation of the Israelites set out from Elim and came to the wilderness of Sin, which is between Elim and Sinai, on the fifteenth day of the second month after they had departed from the land of Egypt.",
            "² The whole congregation of the Israelites complained against Moses and Aaron in the wilderness. ³ The Israelites said to them, “If only we had died by the hand of the Lord in the land of Egypt,",
            "when we sat by the pots of meat and ate our fill of bread, for you have brought us out into this wilderness to kill this whole assembly with hunger.”",
            "⁴ Then the Lord said to Moses, “I am going to rain bread from heaven for you, and each day the people shall go out and gather enough for that day.",
        ]
    );
    assert_eq!(
        slides.iter().map(ScriptureSlide::label).collect::<Vec<_>>(),
        vec!["1", "2-3", "3", "4"]
    );
    assert_valid(&verses, &slides, layout);
    assert!(!slides.iter().any(|slide| slide.text().ends_with("ate our")));
    assert!(slides
        .last()
        .is_some_and(|slide| slide.text().starts_with("⁴ Then the Lord")));

    for wrap in 30..=48 {
        let narrow_layout = TextLayout::new(wrap, 7).expect("valid narrow layout");
        let narrow_slides = split_verses_for_slides(&verses, narrow_layout);
        let final_slide = narrow_slides.last().expect("scripture produces content");
        assert!(
            final_slide.text().split_whitespace().count() >= MIN_TRAILING_SLIDE_WORDS,
            "wrap {wrap} left a tiny final slide: {:?}",
            final_slide.text()
        );
        assert_valid(&verses, &narrow_slides, narrow_layout);
    }
}

#[test]
fn scripture_never_uses_more_than_seven_estimated_lines() {
    let verses = [Verse {
        number: 1,
        text: (0..180)
            .map(|index| format!("word{index}"))
            .collect::<Vec<_>>()
            .join(" "),
    }];
    let permissive_layout = layout(40, 20);

    let slides = split_verses_for_slides(&verses, permissive_layout);

    assert!(slides.len() > 1);
    assert!(slides
        .iter()
        .all(|slide| { permissive_layout.estimated_lines(slide.text()) <= MAX_SCRIPTURE_LINES }));
    assert_valid(&verses, &slides, permissive_layout);
    assert_eq!(
        slides,
        split_verses_for_slides(&verses, permissive_layout),
        "the global optimum must be deterministic"
    );
}

#[test]
fn monotone_fit_stops_extending_each_overflowing_prefix() {
    let verses = [Verse {
        number: 1,
        text: (0..180)
            .map(|index| format!("word{index}"))
            .collect::<Vec<_>>()
            .join(" "),
    }];
    let mut calls = 0_usize;
    let mut fit = |candidate: &str| {
        calls += 1;
        Ok::<_, std::convert::Infallible>(candidate.split_whitespace().count() <= 20)
    };

    let slides = split_verses_with_fit(&verses, &mut fit).expect("monotone fit partitions");

    assert!(!slides.is_empty());
    assert!(slides
        .iter()
        .all(|slide| slide.text().split_whitespace().count() <= 20));
    assert!(
        calls <= 180 * 21,
        "overflow probing exceeded the monotone-prefix bound: {calls} calls"
    );
}

#[test]
fn splitting_preserves_content_bounds_and_provenance_across_capacities() {
    let verses = [
        Verse {
            number: 8,
            text: "Then came a sentence, with a comma; then a clause: and a question? Yes!"
                .to_string(),
        },
        Verse {
            number: 9,
            text: "This intentionally long continuation has no punctuation and therefore exercises the mandatory word boundary fallback repeatedly across capacities".to_string(),
        },
        Verse {
            number: 10,
            text: "The final verse—kept in order—ends here.".to_string(),
        },
    ];
    for wrap in [20, 21, 32, 45, 64] {
        for lines in 1..=5 {
            let layout = layout(wrap, lines);
            assert_valid(&verses, &split_verses_for_slides(&verses, layout), layout);
        }
    }
}

#[test]
fn labels_collapse_only_contiguous_source_ranges() {
    assert_eq!(format_verse_ranges(&[1, 2, 3, 5, 7, 8]), "1-3, 5, 7-8");
}
