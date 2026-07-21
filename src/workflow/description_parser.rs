//! Description parser for PCO item descriptions.
//!
//! Extracts slide-worthy content from Planning Center descriptions, handling
//! `[SLIDE]`/`[SLIDE/ALL]` markers, responsive reading patterns (`Leader:`/`People:`),
//! and content nametag formatting.

use serde::Serialize;

use super::classify_matching::strip_speaker;
use crate::project_config::DescriptionParserKind;
use crate::propresenter::text_flow::TextFlowSegment;
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

fn parse_liturgical(
    description: &str,
    item_title: &str,
    default_speaker: SpeakerRole,
) -> Option<ParsedContent> {
    let title_text = strip_speaker(item_title);

    // Strategy 1: responsive reading (Leader:/People: prefixes) — takes priority
    // because descriptions sometimes have [SLIDE] metadata on instruction lines
    // alongside LEADER:/ALL: content lines.
    if has_responsive_pattern(description) {
        return parse_responsive(description, &title_text);
    }

    // Strategy 2: marker-based parsing ([SLIDE], [SLIDE/ALL], [no slide])
    if has_slide_markers(description) {
        return parse_markers(description, &title_text, default_speaker);
    }

    // Strategy 3: plain text. Source newlines are soft wraps; only an empty
    // line or a semantic Q./A. transition starts another paragraph.
    let segments = parse_plain_liturgical(description, default_speaker);
    if segments.is_empty() {
        return None;
    }

    Some(ParsedContent::new(segments, Some(title_text)))
}

/// Check for slide-state markers used in Planning Center descriptions.
fn has_slide_markers(description: &str) -> bool {
    description.lines().any(|line| {
        slide_marker(line).is_some() || is_non_slide_marker(line) || is_silent_marker(line)
    })
}

/// Responsive reading leader prefixes (case-insensitive, with or without colon).
const LEADER_PREFIXES: &[&str] = &["leader:", "leader "];
/// Responsive reading congregation prefixes.
const CONGREGATION_PREFIXES: &[&str] =
    &["people:", "people ", "all:", "all ", "unison:", "unison "];

/// Check for responsive reading patterns.
///
/// Requires at least one leader-type AND one congregation-type prefix
/// appearing at the start of a line. Avoids false positives from short
/// prefixes like "l:" matching "url:".
fn has_responsive_pattern(description: &str) -> bool {
    let mut has_leader = false;
    let mut has_congregation = false;

    for source_line in description.lines() {
        for line in split_inline_responses(source_line) {
            let lower = line.trim().to_lowercase();
            if !has_leader {
                has_leader = LEADER_PREFIXES.iter().any(|p| lower.starts_with(p));
            }
            if !has_congregation {
                has_congregation = CONGREGATION_PREFIXES.iter().any(|p| lower.starts_with(p));
            }
            if has_leader && has_congregation {
                return true;
            }
        }
    }
    false
}

/// Split compact Planning Center response lines such as
/// `Leader: ... People: ...` at their explicit speaker transitions.
fn split_inline_responses(line: &str) -> Vec<&str> {
    const INLINE_PREFIXES: &[&str] = &["leader:", "people:", "all:", "unison:"];
    let mut starts = Vec::new();
    for (index, _) in line.char_indices() {
        let preceded_by_space = index == 0
            || line[..index]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace);
        if preceded_by_space
            && INLINE_PREFIXES.iter().any(|prefix| {
                line[index..]
                    .get(..prefix.len())
                    .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
            })
        {
            starts.push(index);
        }
    }
    if starts.len() <= 1 {
        return vec![line];
    }
    if starts[0] != 0 {
        starts.insert(0, 0);
    }
    starts.push(line.len());
    starts
        .windows(2)
        .filter_map(|bounds| {
            let value = line[bounds[0]..bounds[1]].trim();
            (!value.is_empty()).then_some(value)
        })
        .collect()
}

/// Parse marker-based descriptions.
///
/// Scans for slide-state markers. A marker changes the state for its line and
/// subsequent unmarked lines, matching Planning Center descriptions such as
/// `[SLIDE just for the part below]` followed by the actual display text.
fn parse_markers(
    description: &str,
    title_text: &str,
    default_speaker: SpeakerRole,
) -> Option<ParsedContent> {
    #[derive(Clone, Copy)]
    enum MarkerState {
        Hidden,
        Slide,
        SlideAll,
    }

    let mut segments: Vec<ParsedSegment> = Vec::new();
    let mut state = MarkerState::Hidden;
    let mut voice = None;
    let mut starts_block = false;

    for line in description.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !matches!(state, MarkerState::Hidden) {
                push_separator(&mut segments);
                starts_block = true;
            }
            continue;
        }

        if is_non_slide_marker(trimmed) || is_silent_marker(trimmed) {
            state = MarkerState::Hidden;
            voice = None;
            starts_block = false;
            continue;
        }

        if let Some(marker) = slide_marker(trimmed) {
            state = if marker.all {
                MarkerState::SlideAll
            } else {
                MarkerState::Slide
            };
            voice = Some(if marker.all {
                SpeakerRole::Audience
            } else {
                default_speaker
            });
            starts_block = true;
            if let Some(text) = extract_after_slide_marker(trimmed, marker.end) {
                push_liturgical_line(&mut segments, text, &mut voice, true);
                starts_block = false;
            }
            continue;
        }

        match state {
            MarkerState::Hidden => {}
            MarkerState::Slide => {
                push_liturgical_line(&mut segments, trimmed.to_string(), &mut voice, starts_block);
                starts_block = false;
            }
            MarkerState::SlideAll => {
                voice = Some(SpeakerRole::Audience);
                push_liturgical_line(&mut segments, trimmed.to_string(), &mut voice, starts_block);
                starts_block = false;
            }
        }
    }

    trim_trailing_separators(&mut segments);

    if segments.is_empty() {
        return None;
    }

    Some(ParsedContent::new(segments, Some(title_text.to_string())))
}

#[derive(Clone, Copy)]
struct SlideMarker {
    end: usize,
    all: bool,
}

fn slide_marker(line: &str) -> Option<SlideMarker> {
    let upper = line.to_ascii_uppercase();
    let start = upper.find("[SLIDE")?;
    let end = upper[start..].find(']')? + start + 1;
    let marker = upper[start + 1..end - 1].trim();
    let suffix = marker.strip_prefix("SLIDE")?;
    if !suffix.is_empty() && !suffix.starts_with('/') && !suffix.starts_with(char::is_whitespace) {
        return None;
    }

    let compact_suffix: String = suffix
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    Some(SlideMarker {
        end,
        all: compact_suffix.starts_with("/ALL"),
    })
}

fn is_non_slide_marker(line: &str) -> bool {
    let upper = line.to_ascii_uppercase();
    upper.contains("NO SLIDE]") || upper.contains("(NO SLIDE)")
}

fn is_silent_marker(line: &str) -> bool {
    line.to_ascii_uppercase().contains("[SILENT")
}

fn extract_after_slide_marker(line: &str, marker_end: usize) -> Option<String> {
    let rest = line[marker_end..]
        .trim()
        .trim_start_matches(['-', ':'])
        .trim();
    if rest.is_empty() {
        return None;
    }

    let unwrapped = rest
        .strip_prefix('[')
        .and_then(|content| content.strip_suffix(']'))
        .map(str::trim)
        .filter(|content| !content.is_empty())
        .unwrap_or(rest);
    Some(unwrapped.to_string())
}

fn catechism_voice(text: &str) -> Option<SpeakerRole> {
    let normalized = text.trim_start().to_ascii_lowercase();
    if normalized.starts_with("q.") || normalized.starts_with("q:") {
        Some(SpeakerRole::Leader)
    } else if normalized.starts_with("a.") || normalized.starts_with("a:") {
        Some(SpeakerRole::Audience)
    } else {
        None
    }
}

fn push_liturgical_line(
    segments: &mut Vec<ParsedSegment>,
    text: String,
    voice: &mut Option<SpeakerRole>,
    starts_block: bool,
) {
    let explicit_voice = catechism_voice(&text);
    if let Some(explicit_voice) = explicit_voice {
        *voice = Some(explicit_voice);
    }
    push_prose_line(
        segments,
        text,
        voice.unwrap_or(SpeakerRole::Leader),
        starts_block || explicit_voice.is_some(),
    );
}

fn parse_plain_liturgical(description: &str, default_speaker: SpeakerRole) -> Vec<ParsedSegment> {
    let mut segments = Vec::new();
    let mut voice = Some(default_speaker);

    for line in description.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            push_separator(&mut segments);
            continue;
        }
        push_liturgical_line(&mut segments, trimmed.to_string(), &mut voice, false);
    }

    trim_trailing_separators(&mut segments);
    segments
}

/// Parse responsive reading descriptions (`Leader:`/`People:` format).
///
/// Keeps `LEADER:`/`ALL:`/`PEOPLE:` cue prefixes in the output text.
/// Source hard wraps stay inside their speaker block; speaker transitions and
/// explicit blank lines remain separate paragraphs for the slide packer.
/// Metadata lines (containing `[SLIDE]`, `Liturgist:`, etc.) are filtered out.
fn parse_responsive(description: &str, title_text: &str) -> Option<ParsedContent> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum ResponsiveLineKind {
        Leader,
        Congregation,
    }

    let mut segments: Vec<ParsedSegment> = Vec::new();
    // Display text before the first explicit response marker is led from the
    // stage. Audience semantics must always be explicit.
    let mut current_speaker = SpeakerRole::Leader;
    let mut previous_kind: Option<ResponsiveLineKind> = None;

    for source_line in description.lines() {
        let source_trimmed = source_line.trim();

        // Blank lines request a separator between response blocks. The packer
        // decides whether that separator fits on the current slide.
        if source_trimmed.is_empty() {
            push_separator(&mut segments);
            previous_kind = None;
            continue;
        }

        for line in split_inline_responses(source_line) {
            let trimmed = line.trim();

            // Skip metadata/instruction lines
            if is_metadata_line(trimmed) {
                continue;
            }

            let lower = trimmed.to_lowercase();

            if starts_with_any(&lower, LEADER_PREFIXES) {
                push_response_separator(&mut segments, previous_kind.is_some());
                current_speaker = SpeakerRole::Leader;
                push_prose_line(&mut segments, trimmed.to_string(), current_speaker, true);
                previous_kind = Some(ResponsiveLineKind::Leader);
            } else if starts_with_any(&lower, CONGREGATION_PREFIXES) {
                push_response_separator(&mut segments, previous_kind.is_some());
                current_speaker = SpeakerRole::Audience;
                push_prose_line(&mut segments, trimmed.to_string(), current_speaker, true);
                previous_kind = Some(ResponsiveLineKind::Congregation);
            } else {
                // A source hard wrap inherits the current speaker and paragraph.
                push_prose_line(&mut segments, trimmed.to_string(), current_speaker, false);
            }
        }
    }

    trim_trailing_separators(&mut segments);

    if segments.is_empty() {
        return None;
    }

    Some(ParsedContent::new(segments, Some(title_text.to_string())))
}

fn push_response_separator(segments: &mut Vec<ParsedSegment>, previous_kind_exists: bool) {
    if previous_kind_exists {
        push_separator(segments);
    }
}

fn push_separator(segments: &mut Vec<ParsedSegment>) {
    if segments.is_empty() || segments.last().is_some_and(|seg| seg.text.is_empty()) {
        return;
    }
    segments.push(ParsedSegment {
        text: String::new(),
        speaker: SpeakerRole::Neutral,
        bold: None,
        italic: None,
    });
}

fn push_prose_line(
    segments: &mut Vec<ParsedSegment>,
    text: String,
    speaker: SpeakerRole,
    starts_block: bool,
) {
    if !starts_block {
        if let Some(paragraph) = segments
            .last_mut()
            .filter(|segment| !segment.text.is_empty() && segment.speaker == speaker)
        {
            paragraph.text.push(' ');
            paragraph.text.push_str(&text);
            return;
        }
    }

    segments.push(ParsedSegment {
        text,
        speaker,
        bold: None,
        italic: None,
    });
}

fn trim_trailing_separators(segments: &mut Vec<ParsedSegment>) {
    while segments
        .last()
        .is_some_and(|segment| segment.text.is_empty())
    {
        segments.pop();
    }
}

/// Check if a line starts with any of the given prefixes.
fn starts_with_any(lower: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|p| lower.starts_with(p))
}

/// Detect metadata/instruction lines that should not become slide content.
fn is_metadata_line(line: &str) -> bool {
    // Lines with [SLIDE], [NO SLIDE], etc. are PCO cues
    if slide_marker(line).is_some() || is_non_slide_marker(line) || is_silent_marker(line) {
        return true;
    }
    // Operational bullets surrounding an explicit responsive reading are not
    // display text.
    if line.trim_start().starts_with("- ") {
        return true;
    }
    // "Liturgist:" instruction lines (not the same as "Leader:")
    let lower = line.to_lowercase();
    if lower.starts_with("liturgist:") {
        return true;
    }
    false
}

// ---------------------------------------------------------------------------
// Content nametag parsing
// ---------------------------------------------------------------------------

fn parse_content_nametag(description: &str, item_title: &str) -> ParsedContent {
    // Extract piece title from item title after colon
    // e.g. "Organ Prelude: Meditation with Aria" → "Meditation with Aria"
    // When no colon, the title isn't useful nametag content (e.g. "Giving of
    // Tithes and Offerings") — use description lines directly if available.
    let has_colon = item_title.contains(':');
    let piece_title = if has_colon {
        item_title
            .split_once(':')
            .map(|(_, rest)| strip_speaker(rest.trim()))
            .unwrap_or_default()
    } else {
        String::new()
    };

    let mut segments: Vec<ParsedSegment> = Vec::new();
    let display_lines = description
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('*'))
        .collect::<Vec<_>>();
    let display_description = display_lines.join(" ");

    // Parse description for composer/performer info.
    // Format: "Performer, Instrument / Composer / Arranger"
    let raw_parts: Vec<String> = display_description
        .split('/')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if has_colon && !piece_title.is_empty() {
        // Standard nametag: piece title from the item title, details from description
        segments.push(ParsedSegment {
            text: piece_title,
            speaker: SpeakerRole::Neutral,
            bold: None,
            italic: None,
        });

        if !raw_parts.is_empty() {
            let (performer, others) = split_performer_and_others(&raw_parts);
            if !others.is_empty() {
                segments.push(ParsedSegment {
                    text: others.join(" / "),
                    speaker: SpeakerRole::Neutral,
                    bold: None,
                    italic: None,
                });
            }
            if let Some(performer) = performer {
                segments.push(ParsedSegment {
                    text: performer,
                    speaker: SpeakerRole::Neutral,
                    bold: None,
                    italic: None,
                });
            }
        }
    } else {
        // No colon in title — use description lines as content directly
        if display_lines.is_empty() {
            // Last resort: use stripped title
            segments.push(ParsedSegment {
                text: strip_speaker(item_title),
                speaker: SpeakerRole::Neutral,
                bold: None,
                italic: None,
            });
        } else {
            for line in display_lines {
                segments.push(ParsedSegment {
                    text: line.to_string(),
                    speaker: SpeakerRole::Neutral,
                    bold: None,
                    italic: None,
                });
            }
        }
    }

    ParsedContent::new(segments, None)
}

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
