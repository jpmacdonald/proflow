//! Verse-aware scripture flow independent of native presentation rendering.

use std::collections::BTreeMap;

use crate::bible::{to_superscript, Verse};

use propresenter::text_flow::FitPartitionError;
#[cfg(test)]
use propresenter::text_flow::TextLayout;

#[cfg(test)]
const MAX_SCRIPTURE_LINES: usize = 7;
const MIN_TRAILING_SLIDE_WORDS: usize = 3;

/// One bounded scripture slide and the source verses represented by it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptureSlide {
    text: String,
    verse_numbers: Vec<u32>,
}

impl ScriptureSlide {
    /// Rendered text, including superscript numbers at verse starts.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Source verse numbers represented on this slide, in source order.
    #[must_use]
    #[cfg(test)]
    pub fn verse_numbers(&self) -> &[u32] {
        &self.verse_numbers
    }

    /// Human-readable native cue label (`7`, `7-9`, or `7-9, 12`).
    #[must_use]
    pub fn label(&self) -> String {
        format_verse_ranges(&self.verse_numbers)
    }

    fn new(text: String, verse_number: u32) -> Self {
        Self {
            text,
            verse_numbers: vec![verse_number],
        }
    }

    fn append(&mut self, text: &str, verse_number: u32) {
        if !self.text.is_empty() {
            self.text.push(' ');
        }
        self.text.push_str(text);
        if self.verse_numbers.last().copied() != Some(verse_number) {
            self.verse_numbers.push(verse_number);
        }
    }
}

/// Split verses into globally balanced, bounded slides.
///
/// Every non-whitespace source character is retained in order. Long individual
/// verses continue across slides, and every continuation retains its source
/// verse provenance for native cue labels. The optimizer first minimizes the
/// tiny final slide and mid-sentence word breaks, then minimizes slide count
/// and favors nonincreasing, front-loaded word counts. Equally balanced
/// partitions prefer sentence, clause, and verse boundaries in that order. No
/// scripture slide may exceed seven estimated lines even when the supplied
/// layout allows more. Avoiding a mid-sentence word break can therefore justify
/// one additional slide.
#[must_use]
#[cfg(test)]
pub fn split_verses_for_slides(verses: &[Verse], layout: TextLayout) -> Vec<ScriptureSlide> {
    let max_lines = layout.max_lines().min(MAX_SCRIPTURE_LINES);
    let mut estimated_fit =
        |text: &str| Ok::<_, std::convert::Infallible>(layout.estimated_lines(text) <= max_lines);
    match split_verses_with_fit(verses, &mut estimated_fit) {
        Ok(slides) => slides,
        Err(FitPartitionError::NoFittingPartition) => Vec::new(),
        Err(FitPartitionError::Measurement(unreachable)) => match unreachable {},
    }
}

/// Split verses with an authoritative physical-fit and line-policy predicate.
///
/// Grammar, verse provenance, and front-loading remain pure. The caller owns
/// native shaping and the configured maximum-line decision. The predicate must
/// be prefix-monotone: after one candidate does not fit, appending more source
/// words may not make it fit. This is true for the production contract because
/// it measures fixed-scale attributed text in fixed bounds with no text
/// transforms. The optimizer relies on that property to stop probing a prefix
/// after its first overflow.
pub fn split_verses_with_fit<E, F>(
    verses: &[Verse],
    fits: &mut F,
) -> Result<Vec<ScriptureSlide>, FitPartitionError<E>>
where
    F: FnMut(&str) -> Result<bool, E>,
{
    let words = scripture_words(verses);
    let partition =
        optimal_partition_with_fit(&words, fits)?.ok_or(FitPartitionError::NoFittingPartition)?;
    Ok(render_partition(&words, &partition.ends))
}

#[derive(Debug, Clone)]
struct ScriptureWord {
    text: String,
    verse_number: u32,
    source_word_count: usize,
}

fn scripture_words(verses: &[Verse]) -> Vec<ScriptureWord> {
    let mut words = Vec::new();
    for verse in verses {
        let number = to_superscript(verse.number);
        let mut source_words = verse.text.split_whitespace();
        if let Some(first) = source_words.next() {
            words.push(ScriptureWord {
                text: format!("{number} {first}"),
                verse_number: verse.number,
                source_word_count: 1,
            });
            words.extend(source_words.map(|word| ScriptureWord {
                text: word.to_owned(),
                verse_number: verse.number,
                source_word_count: 1,
            }));
        } else {
            words.push(ScriptureWord {
                text: number,
                verse_number: verse.number,
                source_word_count: 0,
            });
        }
    }
    words
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BreakPreference {
    Sentence,
    Clause,
    Verse,
    Word,
}

fn break_between(previous: &ScriptureWord, current: &ScriptureWord) -> BreakPreference {
    let trailing = previous
        .text
        .trim_end_matches(['"', '\'', '’', '”', ')', ']', '}'])
        .chars()
        .next_back();
    if trailing.is_some_and(|character| matches!(character, '.' | '?' | '!')) {
        BreakPreference::Sentence
    } else if trailing.is_some_and(|character| matches!(character, ';' | ',' | ':' | '—')) {
        BreakPreference::Clause
    } else if previous.verse_number != current.verse_number {
        BreakPreference::Verse
    } else {
        BreakPreference::Word
    }
}

#[derive(Debug, Clone, Default)]
struct Partition {
    ends: Vec<usize>,
    word_counts: Vec<usize>,
    word_breaks: usize,
    verse_breaks: usize,
    clause_breaks: usize,
    rising_transitions: usize,
    rising_words: usize,
}

// Declaration order is the optimizer's lexicographic priority. In particular,
// a tiny tail and mid-sentence word breaks are repaired first. The remaining
// choices minimize slide count, then favor front-loaded balance before ranking
// sentence, clause, and verse boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct OptimizationScore {
    tiny_tail: usize,
    word_breaks: usize,
    slide_count: usize,
    rising_transitions: usize,
    rising_words: usize,
    relative_tail: usize,
    verse_breaks: usize,
    clause_breaks: usize,
}

impl Partition {
    fn first(end: usize, word_count: usize) -> Self {
        Self {
            ends: vec![end],
            word_counts: vec![word_count],
            ..Self::default()
        }
    }

    fn extended(&self, end: usize, word_count: usize, boundary: BreakPreference) -> Self {
        let mut partition = self.clone();
        partition.ends.push(end);
        if let Some(&previous) = partition.word_counts.last() {
            if word_count > previous {
                partition.rising_transitions += 1;
                partition.rising_words += word_count - previous;
            }
        }
        partition.word_counts.push(word_count);
        match boundary {
            BreakPreference::Sentence => {}
            BreakPreference::Clause => partition.clause_breaks += 1,
            BreakPreference::Verse => partition.verse_breaks += 1,
            BreakPreference::Word => partition.word_breaks += 1,
        }
        partition
    }

    fn is_better_than(&self, other: &Self, final_partition: bool) -> bool {
        let (tiny_tail, relative_tail) = if final_partition {
            self.trailing_penalty()
        } else {
            (0, 0)
        };
        let (other_tiny_tail, other_relative_tail) = if final_partition {
            other.trailing_penalty()
        } else {
            (0, 0)
        };
        let score = OptimizationScore {
            tiny_tail,
            word_breaks: self.word_breaks,
            slide_count: self.ends.len(),
            rising_transitions: self.rising_transitions,
            rising_words: self.rising_words,
            relative_tail,
            verse_breaks: self.verse_breaks,
            clause_breaks: self.clause_breaks,
        };
        let other_score = OptimizationScore {
            tiny_tail: other_tiny_tail,
            word_breaks: other.word_breaks,
            slide_count: other.ends.len(),
            rising_transitions: other.rising_transitions,
            rising_words: other.rising_words,
            relative_tail: other_relative_tail,
            verse_breaks: other.verse_breaks,
            clause_breaks: other.clause_breaks,
        };
        score < other_score
            || (score == other_score
                && (self.word_counts > other.word_counts
                    || (self.word_counts == other.word_counts && self.ends > other.ends)))
    }

    fn trailing_penalty(&self) -> (usize, usize) {
        let Some((&last, preceding)) = self.word_counts.split_last() else {
            return (0, 0);
        };
        let tiny_tail = MIN_TRAILING_SLIDE_WORDS.saturating_sub(last);
        let relative_tail = preceding.last().map_or(0, |previous| {
            previous.saturating_sub(last.saturating_mul(2))
        });
        (tiny_tail, relative_tail)
    }
}

fn optimal_partition_with_fit<E, F>(
    words: &[ScriptureWord],
    fits: &mut F,
) -> Result<Option<Partition>, FitPartitionError<E>>
where
    F: FnMut(&str) -> Result<bool, E>,
{
    if words.is_empty() {
        return Ok(None);
    }

    let mut prefixes = vec![BTreeMap::<usize, Partition>::new(); words.len()];
    let mut finals = Vec::new();

    for start in 0..words.len() {
        let prior_partitions = if start == 0 {
            Vec::new()
        } else {
            prefixes[start].values().cloned().collect::<Vec<_>>()
        };
        if start > 0 && prior_partitions.is_empty() {
            continue;
        }

        let boundary = if start == 0 {
            None
        } else {
            Some(break_between(&words[start - 1], &words[start]))
        };
        let mut text = String::new();
        let mut word_count = 0;
        for end in (start + 1)..=words.len() {
            if !text.is_empty() {
                text.push(' ');
            }
            let word = &words[end - 1];
            text.push_str(&word.text);
            word_count += word.source_word_count;
            if !fits(&text).map_err(FitPartitionError::Measurement)? {
                break;
            }

            if start == 0 {
                retain_partition(
                    Partition::first(end, word_count),
                    end,
                    words.len(),
                    &mut prefixes,
                    &mut finals,
                );
            } else if let Some(boundary) = boundary {
                for prior in &prior_partitions {
                    retain_partition(
                        prior.extended(end, word_count, boundary),
                        end,
                        words.len(),
                        &mut prefixes,
                        &mut finals,
                    );
                }
            }
        }
    }

    Ok(finals.into_iter().reduce(|best, candidate| {
        if candidate.is_better_than(&best, true) {
            candidate
        } else {
            best
        }
    }))
}

fn retain_partition(
    candidate: Partition,
    end: usize,
    word_len: usize,
    prefixes: &mut [BTreeMap<usize, Partition>],
    finals: &mut Vec<Partition>,
) {
    if end == word_len {
        finals.push(candidate);
        return;
    }

    let Some(&word_count) = candidate.word_counts.last() else {
        return;
    };
    let current = prefixes[end].get(&word_count);
    if current.is_none_or(|current| candidate.is_better_than(current, false)) {
        prefixes[end].insert(word_count, candidate);
    }
}

fn render_partition(words: &[ScriptureWord], ends: &[usize]) -> Vec<ScriptureSlide> {
    let mut slides = Vec::with_capacity(ends.len());
    let mut start = 0;
    for &end in ends {
        let Some(span) = words.get(start..end) else {
            return Vec::new();
        };
        let Some((first, rest)) = span.split_first() else {
            return Vec::new();
        };
        let mut slide = ScriptureSlide::new(first.text.clone(), first.verse_number);
        for word in rest {
            slide.append(&word.text, word.verse_number);
        }
        slides.push(slide);
        start = end;
    }
    slides
}

fn format_verse_ranges(numbers: &[u32]) -> String {
    let Some((&first, rest)) = numbers.split_first() else {
        return String::new();
    };
    let mut ranges = Vec::new();
    let mut start = first;
    let mut end = first;
    for &number in rest {
        if end.checked_add(1) == Some(number) {
        } else {
            ranges.push(format_range(start, end));
            start = number;
        }
        end = number;
    }
    ranges.push(format_range(start, end));
    ranges.join(", ")
}

fn format_range(start: u32, end: u32) -> String {
    if start == end {
        start.to_string()
    } else {
        format!("{start}-{end}")
    }
}

#[cfg(test)]
mod tests;
