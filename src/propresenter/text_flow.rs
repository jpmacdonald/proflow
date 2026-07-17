//! Pure, checked text flow for presentation slides.
//!
//! This module owns the estimate used to decide whether text fits. Callers
//! construct one [`crate::propresenter::text_flow::TextLayout`] and use that same
//! value for fragmentation and postcondition checks, so a zero-line slide cannot
//! enter the renderer.

use std::collections::BTreeMap;
use std::num::NonZeroUsize;

use unicode_width::UnicodeWidthStr;

use super::rtf::StyledSegment;

/// Smallest supported estimated wrap width for a presentation text box.
pub const MIN_SLIDE_WRAP: usize = 20;

/// Cloneable text content that can flow across presentation slides.
///
/// Implementations own any semantic metadata attached to the text. Fragmenting
/// a segment replaces only its text, so speaker roles and other planning state
/// survive the layout boundary unchanged.
pub trait TextFlowSegment: Clone {
    /// Text measured and fragmented by the slide packer.
    fn text(&self) -> &str;

    /// Clone this segment with replacement text and unchanged metadata.
    #[must_use]
    fn with_text(&self, text: String) -> Self;
}

impl TextFlowSegment for StyledSegment {
    fn text(&self) -> &str {
        &self.text
    }

    fn with_text(&self, text: String) -> Self {
        let mut fragment = self.clone();
        fragment.text = text;
        fragment
    }
}

/// A checked visual capacity for one presentation slide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextLayout {
    wrap_column: NonZeroUsize,
    max_lines: NonZeroUsize,
}

impl TextLayout {
    /// Validate an estimated wrap width and line capacity.
    ///
    /// Widths below [`MIN_SLIDE_WRAP`] are rejected instead of silently
    /// changing the caller's requested layout. A slide must fit at least one
    /// visual line.
    pub fn new(wrap_column: usize, max_lines: usize) -> Result<Self, TextLayoutError> {
        if wrap_column < MIN_SLIDE_WRAP {
            return Err(TextLayoutError::WrapColumnTooSmall {
                actual: wrap_column,
                minimum: MIN_SLIDE_WRAP,
            });
        }
        let max_lines =
            NonZeroUsize::new(max_lines).ok_or(TextLayoutError::ZeroMaxLinesPerSlide)?;
        let wrap_column =
            NonZeroUsize::new(wrap_column).ok_or(TextLayoutError::WrapColumnTooSmall {
                actual: wrap_column,
                minimum: MIN_SLIDE_WRAP,
            })?;
        Ok(Self {
            wrap_column,
            max_lines,
        })
    }

    /// Estimated maximum display columns on one visual line.
    #[must_use]
    pub const fn wrap_column(self) -> usize {
        self.wrap_column.get()
    }

    /// Maximum estimated visual lines on one slide.
    #[must_use]
    pub const fn max_lines(self) -> usize {
        self.max_lines.get()
    }

    /// Estimate the visual lines occupied by one paragraph.
    #[must_use]
    pub fn estimated_lines(self, text: &str) -> usize {
        word_wrap(text, self.wrap_column()).len()
    }
}

/// Invalid visual-capacity input.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TextLayoutError {
    /// The estimated wrap width is too small for presentation layout.
    #[error("wrap column {actual} is below the supported minimum {minimum}")]
    WrapColumnTooSmall {
        /// Requested wrap width.
        actual: usize,
        /// Smallest accepted wrap width.
        minimum: usize,
    },
    /// A slide cannot have zero visual-line capacity.
    #[error("maximum lines per slide must be greater than zero")]
    ZeroMaxLinesPerSlide,
}

/// Word-wrap text for capacity estimation.
///
/// Whitespace is normalized for the estimate. An indivisible word wider than
/// the wrap column occupies one estimated visual line; content fragmentation
/// never invents a break inside a word.
pub(crate) fn word_wrap(text: &str, max_width: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }

    let mut lines = Vec::new();
    let mut current_line = String::new();
    let mut current_width = 0;

    for word in text.split_whitespace() {
        let word_width = word.width();

        if current_line.is_empty() {
            current_line.push_str(word);
            current_width = word_width;
        } else if current_width + 1 + word_width > max_width {
            lines.push(current_line);
            current_line = word.to_string();
            current_width = word_width;
        } else {
            current_line.push(' ');
            current_line.push_str(word);
            current_width += 1 + word_width;
        }
    }

    if !current_line.is_empty() {
        lines.push(current_line);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Pack text segments onto checked slide capacities.
///
/// A paragraph that exceeds one slide is globally partitioned. The optimizer
/// first prevents a one- or two-word final slide, then prefers sentence,
/// clause, and word boundaries in that order. Equally natural partitions use
/// the fewest slides and favor nonincreasing, front-loaded word counts. A
/// paragraph starts on a fresh slide when it does not fit the remaining space,
/// keeping its global partition independent of content that came before it.
///
/// Every fragment retains all metadata carried by its source segment. Blank
/// paragraphs are kept only between content that fits on the same slide; they
/// are never left at a slide boundary.
#[must_use]
pub fn pack_segments_for_slides<S: TextFlowSegment>(
    segments: &[S],
    layout: TextLayout,
) -> Vec<Vec<S>> {
    let mut slides = Vec::new();
    let mut current = Vec::new();
    let mut current_lines = 0;
    let mut pending_blanks = Vec::new();

    for segment in segments {
        if segment.text().trim().is_empty() {
            if !current.is_empty() {
                pending_blanks.push(segment.clone());
            }
            continue;
        }

        let blank_lines = pending_blanks.len();
        let available = layout
            .max_lines()
            .saturating_sub(current_lines + blank_lines);
        let segment_lines = layout.estimated_lines(segment.text());

        if segment_lines <= available {
            if current.is_empty() {
                pending_blanks.clear();
            } else {
                current.append(&mut pending_blanks);
                current_lines += blank_lines;
            }
            current.push(segment.clone());
            current_lines += segment_lines;
            continue;
        }

        if let Some((previous, next, next_lines)) =
            rebalance_tiny_boundary(&current, &pending_blanks, segment, layout)
        {
            slides.push(previous);
            current = next;
            current_lines = next_lines;
            pending_blanks.clear();
            continue;
        }

        push_slide(&mut slides, &mut current);
        current_lines = 0;
        pending_blanks.clear();

        let mut fragments = partition_paragraph(segment.text(), layout)
            .into_iter()
            .peekable();
        while let Some(fragment) = fragments.next() {
            current_lines += layout.estimated_lines(&fragment);
            current.push(segment.with_text(fragment));
            if fragments.peek().is_some() {
                push_slide(&mut slides, &mut current);
                current_lines = 0;
            }
        }
    }

    push_slide(&mut slides, &mut current);
    slides
}

fn push_slide<S: TextFlowSegment>(slides: &mut Vec<Vec<S>>, current: &mut Vec<S>) {
    while current
        .last()
        .is_some_and(|segment| segment.text().trim().is_empty())
    {
        current.pop();
    }
    if !current.is_empty() {
        slides.push(std::mem::take(current));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct BoundaryRebalanceScore {
    break_preference: usize,
    rising_words: usize,
    moved_words: usize,
}

fn rebalance_tiny_boundary<S: TextFlowSegment>(
    current: &[S],
    pending_blanks: &[S],
    incoming: &S,
    layout: TextLayout,
) -> Option<(Vec<S>, Vec<S>, usize)> {
    let incoming_words = incoming.text().split_whitespace().count();
    if incoming_words == 0 || incoming_words >= MIN_TRAILING_FRAGMENT_WORDS {
        return None;
    }

    let previous_index = current
        .iter()
        .rposition(|segment| !segment.text().trim().is_empty())?;
    let previous_segment = current.get(previous_index)?;
    let words = previous_segment
        .text()
        .split_whitespace()
        .collect::<Vec<_>>();
    if words.len() < 2 {
        return None;
    }

    let mut best: Option<(BoundaryRebalanceScore, Vec<S>, Vec<S>, usize)> = None;
    for prefix_words in 1..words.len() {
        let Some(prefix_slice) = words.get(..prefix_words) else {
            continue;
        };
        let Some(suffix_slice) = words.get(prefix_words..) else {
            continue;
        };
        let prefix = prefix_slice.join(" ");
        let suffix = suffix_slice.join(" ");
        let mut previous = current.get(..previous_index)?.to_vec();
        previous.push(previous_segment.with_text(prefix));
        let previous_lines = estimated_slide_lines(&previous, layout);
        if previous_lines > layout.max_lines() {
            continue;
        }

        let mut next = Vec::with_capacity(pending_blanks.len() + 2);
        next.push(previous_segment.with_text(suffix));
        next.extend_from_slice(pending_blanks);
        next.push(incoming.clone());
        let next_lines = estimated_slide_lines(&next, layout);
        if next_lines > layout.max_lines() {
            continue;
        }

        let previous_word_count = slide_word_count(&previous);
        let next_word_count = slide_word_count(&next);
        if next_word_count < MIN_TRAILING_FRAGMENT_WORDS {
            continue;
        }
        let Some(boundary_word) = words.get(prefix_words - 1) else {
            continue;
        };
        let score = BoundaryRebalanceScore {
            break_preference: break_between(boundary_word).rank(),
            rising_words: next_word_count.saturating_sub(previous_word_count),
            moved_words: words.len() - prefix_words,
        };
        if best
            .as_ref()
            .is_none_or(|(best_score, ..)| score < *best_score)
        {
            best = Some((score, previous, next, next_lines));
        }
    }

    best.map(|(_, previous, next, next_lines)| (previous, next, next_lines))
}

fn estimated_slide_lines<S: TextFlowSegment>(segments: &[S], layout: TextLayout) -> usize {
    segments
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

fn slide_word_count<S: TextFlowSegment>(segments: &[S]) -> usize {
    segments
        .iter()
        .map(|segment| segment.text().split_whitespace().count())
        .sum()
}

const MIN_TRAILING_FRAGMENT_WORDS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BreakPreference {
    Sentence,
    Clause,
    Word,
}

impl BreakPreference {
    const fn rank(self) -> usize {
        match self {
            Self::Sentence => 0,
            Self::Clause => 1,
            Self::Word => 2,
        }
    }
}

fn break_between(previous: &str) -> BreakPreference {
    let trailing = previous
        .trim_end_matches(['"', '\'', '’', '”', ')', ']', '}'])
        .chars()
        .next_back();
    if trailing.is_some_and(|character| matches!(character, '.' | '?' | '!')) {
        BreakPreference::Sentence
    } else if trailing.is_some_and(|character| matches!(character, ';' | ',' | ':' | '—')) {
        BreakPreference::Clause
    } else {
        BreakPreference::Word
    }
}

#[derive(Debug, Clone, Default)]
struct ParagraphPartition {
    ends: Vec<usize>,
    word_counts: Vec<usize>,
    word_breaks: usize,
    clause_breaks: usize,
    rising_transitions: usize,
    rising_words: usize,
}

// Declaration order is the optimizer's lexicographic priority. A natural
// boundary may justify another slide, but equally natural plans stay compact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct OptimizationScore {
    tiny_tail: usize,
    word_breaks: usize,
    clause_breaks: usize,
    slide_count: usize,
    rising_transitions: usize,
    rising_words: usize,
    relative_tail: usize,
}

impl ParagraphPartition {
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
            clause_breaks: self.clause_breaks,
            slide_count: self.ends.len(),
            rising_transitions: self.rising_transitions,
            rising_words: self.rising_words,
            relative_tail,
        };
        let other_score = OptimizationScore {
            tiny_tail: other_tiny_tail,
            word_breaks: other.word_breaks,
            clause_breaks: other.clause_breaks,
            slide_count: other.ends.len(),
            rising_transitions: other.rising_transitions,
            rising_words: other.rising_words,
            relative_tail: other_relative_tail,
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
        let tiny_tail = MIN_TRAILING_FRAGMENT_WORDS.saturating_sub(last);
        let relative_tail = preceding.last().map_or(0, |previous| {
            previous.saturating_sub(last.saturating_mul(2))
        });
        (tiny_tail, relative_tail)
    }
}

fn partition_paragraph(text: &str, layout: TextLayout) -> Vec<String> {
    let words = text.split_whitespace().collect::<Vec<_>>();
    let Some(partition) = optimal_paragraph_partition(&words, layout) else {
        return Vec::new();
    };

    let mut words = words.into_iter();
    partition
        .word_counts
        .into_iter()
        .map(|word_count| {
            words
                .by_ref()
                .take(word_count)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}

fn optimal_paragraph_partition(words: &[&str], layout: TextLayout) -> Option<ParagraphPartition> {
    if words.is_empty() {
        return None;
    }

    let mut prefixes = vec![BTreeMap::<usize, ParagraphPartition>::new(); words.len()];
    let mut finals = Vec::new();

    for start in 0..words.len() {
        let prior_partitions = if start == 0 {
            Vec::new()
        } else {
            let Some(partitions) = prefixes.get(start) else {
                continue;
            };
            partitions.values().cloned().collect::<Vec<_>>()
        };
        if start > 0 && prior_partitions.is_empty() {
            continue;
        }

        let boundary = start
            .checked_sub(1)
            .and_then(|previous| words.get(previous))
            .map(|previous| break_between(previous));
        let mut fragment = String::new();
        for end in (start + 1)..=words.len() {
            let Some(word_index) = end.checked_sub(1) else {
                continue;
            };
            let Some(word) = words.get(word_index) else {
                break;
            };
            if !fragment.is_empty() {
                fragment.push(' ');
            }
            fragment.push_str(word);
            let word_count = end - start;
            if layout.estimated_lines(&fragment) > layout.max_lines() {
                break;
            }

            if start == 0 {
                retain_partition(
                    ParagraphPartition::first(end, word_count),
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

    finals.into_iter().reduce(|best, candidate| {
        if candidate.is_better_than(&best, true) {
            candidate
        } else {
            best
        }
    })
}

fn retain_partition(
    candidate: ParagraphPartition,
    end: usize,
    word_len: usize,
    prefixes: &mut [BTreeMap<usize, ParagraphPartition>],
    finals: &mut Vec<ParagraphPartition>,
) {
    if end == word_len {
        finals.push(candidate);
        return;
    }

    let Some(&word_count) = candidate.word_counts.last() else {
        return;
    };
    let Some(prefix) = prefixes.get_mut(end) else {
        return;
    };
    let current = prefix.get(&word_count);
    if current.is_none_or(|current| candidate.is_better_than(current, false)) {
        prefix.insert(word_count, candidate);
    }
}

#[cfg(test)]
mod tests;
