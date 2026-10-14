//! Description parser for PCO item descriptions.
//!
//! Extracts slide-worthy content from Planning Center descriptions, handling
//! `[SLIDE]`/`[SLIDE/ALL]` markers, responsive reading patterns (`Leader:`/`People:`),
//! and content nametag formatting.

mod liturgical;
#[cfg(test)]
use liturgical::has_responsive_pattern;
use liturgical::{catechism_voice, parse_liturgical};
mod nametag;
use nametag::parse_content_nametag;

use serde::Serialize;

use super::classify_matching::strip_speaker;
use crate::project_config::DescriptionParserKind;
use propresenter::text_flow::TextFlowSegment;
/// Semantic speaker for one parsed text run.
///
/// Runtime macros and editor colors are selected from this role. They are
/// deliberately not inferred from one another.
#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SpeakerRole {
    /// Content with no liturgical speaker semantics, such as a nametag.
    #[default]
    Neutral,
    /// A liturgist, pastor, or other person leading from the stage.
    Leader,
    /// Congregational participation (`ALL`, `PEOPLE`, or `UNISON`).
    Audience,
}

/// Packing behavior for parsed description content.
#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DescriptionFlow {
    /// Ordinary paragraphs and responsive blocks maximize each slide.
    #[default]
    Prose,
    /// Keep a catechism/affirmation question separate from its answer whenever
    /// the combined content cannot fit on one slide.
    QuestionAnswer,
}

/// A parsed segment of description text with formatting hints.
#[derive(Debug, Clone, Serialize)]
pub struct ParsedSegment {
    pub text: String,
    pub speaker: SpeakerRole,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
}

impl TextFlowSegment for ParsedSegment {
    fn text(&self) -> &str {
        &self.text
    }

    fn with_text(&self, text: String) -> Self {
        let mut fragment = self.clone();
        fragment.text = text;
        fragment
    }
}

/// Result of parsing a description into slide content.
#[derive(Debug, Clone, Serialize)]
pub struct ParsedContent {
    segments: Vec<ParsedSegment>,
    flow: DescriptionFlow,
    #[serde(skip)]
    question_answer_pairs: Vec<QuestionAnswerPair>,
    #[serde(skip_serializing_if = "Option::is_none")]
    title_text: Option<String>,
}

/// Checked segment boundaries for one explicit `Q.`/`A.` pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct QuestionAnswerPair {
    question_start: usize,
    answer_start: usize,
    end: usize,
}

impl QuestionAnswerPair {
    /// First segment of the question block.
    pub(crate) const fn question_start(self) -> usize {
        self.question_start
    }

    /// First segment carrying the explicit answer marker.
    pub(crate) const fn answer_start(self) -> usize {
        self.answer_start
    }

    /// Exclusive end of this pair, before the next explicit question.
    pub(crate) const fn end(self) -> usize {
        self.end
    }
}

impl ParsedContent {
    /// Build parsed content and derive every Q/A boundary from explicit textual
    /// markers. Callers cannot provide a contradictory flow classification.
    pub(crate) fn new(segments: Vec<ParsedSegment>, title_text: Option<String>) -> Self {
        let question_answer_pairs = explicit_question_answer_pairs(&segments);
        let flow = if question_answer_pairs.is_empty() {
            DescriptionFlow::Prose
        } else {
            DescriptionFlow::QuestionAnswer
        };
        Self {
            segments,
            flow,
            question_answer_pairs,
            title_text,
        }
    }

    /// Return parsed text segments in source order.
    pub fn segments(&self) -> &[ParsedSegment] {
        &self.segments
    }

    /// Return the flow derived from explicit markers in the segments.
    pub const fn flow(&self) -> DescriptionFlow {
        self.flow
    }

    /// Return the checked explicit Q/A pairs used by the slide planner.
    pub(crate) fn question_answer_pairs(&self) -> &[QuestionAnswerPair] {
        &self.question_answer_pairs
    }

    /// Return the optional leading title text.
    pub fn title_text(&self) -> Option<&str> {
        self.title_text.as_deref()
    }
}

/// Description content that cannot safely become operator-visible text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DescriptionParseError {
    /// A Planning Center instruction or blank remains where final text belongs.
    UnresolvedPlaceholder(String),
}

impl std::fmt::Display for DescriptionParseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnresolvedPlaceholder(value) => {
                write!(formatter, "Unresolved description placeholder '{value}'")
            }
        }
    }
}

/// Parse a PCO item description into slide-ready segments.
///
/// The configured parser controls whether content is treated as liturgical text
/// or as a content nametag. Presentation type names do not affect parsing.
///
/// Returns `Ok(None)` if the description has no slide-worthy content. An
/// unresolved editorial placeholder is a typed error so it cannot enter a
/// renderable plan as parsed content.
pub fn parse_description(
    description: &str,
    item_title: &str,
    parser: DescriptionParserKind,
) -> Result<Option<ParsedContent>, DescriptionParseError> {
    if is_unresolved_placeholder(item_title) {
        return Err(DescriptionParseError::UnresolvedPlaceholder(
            item_title.trim().to_string(),
        ));
    }

    let parsed = match parser {
        DescriptionParserKind::Liturgical => {
            parse_liturgical(description, item_title, SpeakerRole::Leader)
        }
        DescriptionParserKind::LiturgicalAudience => {
            parse_liturgical(description, item_title, SpeakerRole::Audience)
        }
        DescriptionParserKind::ContentNametag => {
            Some(parse_content_nametag(description, item_title))
        }
    };

    if let Some(placeholder) = parsed.as_ref().and_then(unresolved_placeholder) {
        return Err(DescriptionParseError::UnresolvedPlaceholder(placeholder));
    }

    Ok(parsed)
}

fn unresolved_placeholder(content: &ParsedContent) -> Option<String> {
    content
        .segments
        .iter()
        .map(|segment| segment.text.as_str())
        .chain(content.title_text.iter().map(String::as_str))
        .find(|text| is_unresolved_placeholder(text))
        .map(|text| text.trim().trim_matches(['[', ']']).trim().to_string())
}

fn is_unresolved_placeholder(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.as_bytes().windows(3).any(|window| window == b"___") {
        return true;
    }

    let normalized = trimmed.trim_matches(['[', ']']).trim().to_ascii_lowercase();
    normalized == "tbd"
        || normalized == "tba"
        || normalized == "todo"
        || normalized.starts_with("insert ")
        || normalized.starts_with("add ")
        || trimmed.to_ascii_lowercase().contains("[insert ")
        || trimmed.to_ascii_lowercase().contains("[add ")
}

// ---------------------------------------------------------------------------
// Liturgical parsing
// ---------------------------------------------------------------------------

fn explicit_question_answer_pairs(segments: &[ParsedSegment]) -> Vec<QuestionAnswerPair> {
    let questions = segments
        .iter()
        .enumerate()
        .filter_map(|(index, segment)| {
            (catechism_voice(&segment.text) == Some(SpeakerRole::Leader)).then_some(index)
        })
        .collect::<Vec<_>>();
    let mut pairs = Vec::new();
    for (position, &question_start) in questions.iter().enumerate() {
        let end = questions
            .get(position + 1)
            .copied()
            .unwrap_or(segments.len());
        let Some(search_start) = question_start.checked_add(1) else {
            continue;
        };
        let answer_start = (search_start..end).find(|&index| {
            segments.get(index).is_some_and(|segment| {
                catechism_voice(&segment.text) == Some(SpeakerRole::Audience)
            })
        });
        if let Some(answer_start) = answer_start {
            pairs.push(QuestionAnswerPair {
                question_start,
                answer_start,
                end,
            });
        }
    }
    pairs
}

/// Split parts into performer (first entry with comma) and everything else.
fn split_performer_and_others(parts: &[String]) -> (Option<String>, Vec<String>) {
    let mut performer: Option<String> = None;
    let mut others: Vec<String> = Vec::new();
    for part in parts {
        if part.contains(',') && performer.is_none() {
            performer = Some(part.clone());
        } else {
            others.push(part.clone());
        }
    }
    (performer, others)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
