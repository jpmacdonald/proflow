//! Template-based slide generation.
//!
//! Loads slide styling from ProPresenter theme files or legacy `.pro` template
//! files, then injects text while preserving fonts, colors, and layout.

use prost::Message;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use unicode_width::UnicodeWidthStr;

use super::generated::rv_data;
use super::rtf::{extract_rtf_options, segments_to_rtf_bytes, StyledSegment};
/// Default maximum visual lines per slide.
pub const DEFAULT_MAX_LINES_PER_SLIDE: usize = 10;

/// Minimum wrap column for slide splitting.
pub const MIN_SLIDE_WRAP: usize = 20;

/// Average character width as a fraction of font size for proportional fonts.
/// 0.55 is a reasonable estimate for Helvetica and similar typefaces.
const CHAR_WIDTH_RATIO: f64 = 0.55;

/// Default line height multiplier when no paragraph style specifies one.
const DEFAULT_LINE_HEIGHT_MULTIPLE: f64 = 1.2;

/// Legacy slide types for backwards compatibility with `.pro` template files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TemplateType {
    /// Bible scripture slides
    Scripture,
    /// Song/hymn lyrics slides
    Song,
    /// Informational/announcement slides
    Info,
}

impl TemplateType {
    /// Get the template filename for this type
    #[must_use]
    pub const fn filename(self) -> &'static str {
        match self {
            Self::Scripture => "__template_scripture__.pro",
            Self::Song => "__template_song__.pro",
            Self::Info => "__template_info__.pro",
        }
    }

    /// Map a legacy config string ("scripture", "song", "info") to a `TemplateType`.
    fn from_config_str(s: &str) -> Option<Self> {
        match s {
            "scripture" => Some(Self::Scripture),
            "song" => Some(Self::Song),
            "info" | "nametag" => Some(Self::Info),
            _ => None,
        }
    }
}

/// Text box dimensions and font metrics extracted from a template slide.
///
/// Used to compute how many characters fit per line and how many lines
/// fit per slide, replacing hardcoded constants.
#[derive(Debug, Clone)]
pub struct SlideMetrics {
    /// Available width for text in points
    pub text_width_pt: f64,
    /// Available height for text in points
    pub text_height_pt: f64,
    /// Font size in points
    pub font_size_pt: f64,
    /// Effective line height in points
    pub line_height_pt: f64,
    /// Estimated characters per line
    pub chars_per_line: usize,
    /// Maximum lines that fit
    pub max_lines: usize,
}

/// Extract text box metrics from the first text element in a template slide.
///
/// Walks the protobuf hierarchy: `PresentationSlide` -> `Slide` -> `Element`
/// -> `graphics::Element` -> (`bounds`, `text`). Computes usable text area
/// from bounds minus margins, then derives character and line capacity from
/// font size and line height.
pub fn extract_slide_metrics(slide: &rv_data::PresentationSlide) -> Option<SlideMetrics> {
    let base_slide = slide.base_slide.as_ref()?;

    for slide_element in &base_slide.elements {
        let graphics_element = slide_element.element.as_ref()?;
        let text = graphics_element.text.as_ref()?;
        let bounds = graphics_element.bounds.as_ref()?;
        let size = bounds.size.as_ref()?;

        // Compute usable text area by subtracting margins
        let (margin_left, margin_right, margin_top, margin_bottom) = text
            .margins
            .as_ref()
            .map_or((0.0, 0.0, 0.0, 0.0), |m| (m.left, m.right, m.top, m.bottom));
        let text_width_pt = size.width - margin_left - margin_right;
        let text_height_pt = size.height - margin_top - margin_bottom;

        if text_width_pt <= 0.0 || text_height_pt <= 0.0 {
            continue;
        }

        // Resolve font size: prefer protobuf attributes, fall back to RTF
        let font_size_pt = resolve_font_size(text);
        if font_size_pt <= 0.0 {
            continue;
        }

        let line_height_pt = resolve_line_height(text, font_size_pt);

        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let chars_per_line = (text_width_pt / (font_size_pt * CHAR_WIDTH_RATIO)).floor() as usize;

        // Compute max lines that fit, then subtract 1 as a safety margin.
        // ProPresenter's internal text rendering reserves space beyond what the
        // raw geometry suggests (descenders, internal padding), so we err
        // conservatively to avoid clipped text on the last line.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let max_lines = ((text_height_pt / line_height_pt).floor() as usize).saturating_sub(1).max(1);

        if chars_per_line == 0 || max_lines == 0 {
            continue;
        }

        return Some(SlideMetrics {
            text_width_pt,
            text_height_pt,
            font_size_pt,
            line_height_pt,
            chars_per_line,
            max_lines,
        });
    }

    None
}

/// Resolve font size from protobuf attributes or RTF data.
fn resolve_font_size(text: &rv_data::graphics::Text) -> f64 {
    // Try protobuf attributes first
    if let Some(attrs) = &text.attributes {
        if let Some(font) = &attrs.font {
            if font.size > 0.0 {
                return font.size;
            }
        }
    }

    // Fall back to RTF extraction
    extract_rtf_options(&text.rtf_data).map_or(0.0, |opts| f64::from(opts.font_size))
}

/// Resolve effective line height from paragraph style attributes.
///
/// `ProPresenter` uses `line_height_multiple` as a base multiplier on font size,
/// then adds `line_spacing` as extra inter-line spacing on top. The result is
/// clamped between `minimum_line_height` and `maximum_line_height`.
fn resolve_line_height(text: &rv_data::graphics::Text, font_size_pt: f64) -> f64 {
    let para = text
        .attributes
        .as_ref()
        .and_then(|a| a.paragraph_style.as_ref());

    let Some(p) = para else {
        return font_size_pt * DEFAULT_LINE_HEIGHT_MULTIPLE;
    };

    // Base height from line_height_multiple (defaults to 1.0 = font size)
    let base = if p.line_height_multiple > 0.0 {
        p.line_height_multiple * font_size_pt
    } else {
        font_size_pt
    };

    // line_spacing is additive — extra points between lines
    let raw = base + p.line_spacing;

    // Apply min/max clamps
    let after_min = if p.minimum_line_height > 1.0 {
        raw.max(p.minimum_line_height)
    } else {
        raw
    };
    if p.maximum_line_height > 0.0 {
        after_min.min(p.maximum_line_height)
    } else {
        after_min
    }
}

// ---------------------------------------------------------------------------
// ThemeCache — primary API for slide template lookup
// ---------------------------------------------------------------------------

/// Cached slide templates loaded from a `ProPresenter` theme file and/or legacy
/// `.pro` template files.
///
/// Primary path: theme file slides, keyed by name.
/// Fallback path: legacy `__template_*.pro` files, keyed by `TemplateType`.
pub struct ThemeCache {
    /// Slides loaded from a theme file, keyed by slide name.
    theme_slides: HashMap<String, rv_data::PresentationSlide>,
    /// Name of the loaded theme (if any).
    theme_name: Option<String>,
    /// Legacy `.pro` template cache — fallback when no theme is configured.
    legacy_slides: HashMap<TemplateType, rv_data::PresentationSlide>,
    /// Search paths for legacy `.pro` template files.
    search_paths: Vec<PathBuf>,
}

impl ThemeCache {
    /// Create a new cache. If `theme_name` is provided, loads slides from the
    /// `ProPresenter` Themes directory. Falls back to `.pro` templates in
    /// `search_paths` for any slide name that matches a legacy `TemplateType`.
    pub fn new(theme_name: Option<&str>, search_paths: Vec<PathBuf>) -> Self {
        let (theme_slides, resolved_name) = theme_name
            .and_then(|name| {
                let path = get_theme_path(name)?;
                let slides = load_theme(&path);
                if slides.is_empty() {
                    eprintln!("Warning: theme '{name}' loaded 0 slides from {}", path.display());
                    None
                } else {
                    Some((slides, Some(name.to_string())))
                }
            })
            .unwrap_or_default();

        Self {
            theme_slides,
            theme_name: resolved_name,
            legacy_slides: HashMap::new(),
            search_paths,
        }
    }

    /// Look up a slide by name. Tries theme slides first, then falls back to
    /// legacy `.pro` templates if the name matches a known `TemplateType`.
    pub fn get(&mut self, slide_name: &str) -> Option<&rv_data::PresentationSlide> {
        // Theme slide — direct name lookup
        if self.theme_slides.contains_key(slide_name) {
            return self.theme_slides.get(slide_name);
        }

        // Legacy fallback — map config string to TemplateType
        let template_type = TemplateType::from_config_str(slide_name)?;
        self.get_legacy(template_type)
    }

    /// Look up a slide by legacy `TemplateType` directly.
    pub fn get_legacy(&mut self, template_type: TemplateType) -> Option<&rv_data::PresentationSlide> {
        if !self.legacy_slides.contains_key(&template_type) {
            let slide = self.load_legacy_slide(template_type)?;
            self.legacy_slides.insert(template_type, slide);
        }
        self.legacy_slides.get(&template_type)
    }

    /// Load a `PresentationSlide` from a legacy `.pro` template file.
    fn load_legacy_slide(&self, template_type: TemplateType) -> Option<rv_data::PresentationSlide> {
        let filename = template_type.filename();
        for search_path in &self.search_paths {
            let path = search_path.join(filename);
            if path.exists() {
                if let Ok(data) = std::fs::read(&path) {
                    if let Ok(presentation) = rv_data::Presentation::decode(data.as_slice()) {
                        return extract_template_slide(&presentation);
                    }
                }
            }
        }
        None
    }

    /// Return the loaded theme name, if any.
    #[must_use]
    pub fn theme_name(&self) -> Option<&str> {
        self.theme_name.as_deref()
    }

    /// Return the names of all slides loaded from the theme.
    #[must_use]
    pub fn theme_slide_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.theme_slides.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    }
}

/// Backwards-compatible alias — `App` references this type for its cache field.
pub type TemplateCache = ThemeCache;

/// Resolve the `ProPresenter` Themes directory path for a given theme name.
fn get_theme_path(theme_name: &str) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let path = home
        .join("Documents/ProPresenter/Themes")
        .join(theme_name)
        .join("Theme");
    path.exists().then_some(path)
}

/// Load all slides from a `ProPresenter` theme file, keyed by slide name.
fn load_theme(path: &Path) -> HashMap<String, rv_data::PresentationSlide> {
    let mut slides = HashMap::new();
    let Ok(data) = std::fs::read(path) else {
        return slides;
    };
    let Ok(doc) = rv_data::template::Document::decode(data.as_slice()) else {
        return slides;
    };
    for ts in &doc.slides {
        if ts.name.is_empty() {
            continue;
        }
        slides.insert(ts.name.clone(), theme_slide_to_presentation_slide(ts));
    }
    slides
}

/// Convert a `template::Slide` (from a theme file) into a `PresentationSlide`.
///
/// Both share the same `base_slide: Slide` — we just wrap it in the
/// `PresentationSlide` envelope that the rest of the pipeline expects.
fn theme_slide_to_presentation_slide(
    ts: &rv_data::template::Slide,
) -> rv_data::PresentationSlide {
    rv_data::PresentationSlide {
        base_slide: ts.base_slide.clone(),
        notes: None,
        template_guidelines: Vec::new(),
        chord_chart: None,
        transition: None,
    }
}

/// Extract the first slide from a legacy `.pro` template presentation.
///
/// Navigates the `Presentation` → `Cue` → `Action` → `PresentationSlide` chain.
/// Only needed for backwards compatibility with `.pro` template files.
fn extract_template_slide(
    presentation: &rv_data::Presentation,
) -> Option<rv_data::PresentationSlide> {
    for cue in &presentation.cues {
        for action in &cue.actions {
            if let Some(rv_data::action::ActionTypeData::Slide(slide_type)) =
                &action.action_type_data
            {
                if let Some(rv_data::action::slide_type::Slide::Presentation(slide)) =
                    &slide_type.slide
                {
                    return Some(slide.clone());
                }
            }
        }
    }
    None
}

/// Clone a template slide and replace its text with styled segments.
///
/// Preserves the template's font, size, and kerning by extracting RTF
/// options from the original. Each segment can have its own color override.
pub fn clone_slide_with_text(
    template_slide: &rv_data::PresentationSlide,
    segments: &[StyledSegment],
) -> rv_data::PresentationSlide {
    let mut slide = template_slide.clone();

    if let Some(ref mut base_slide) = slide.base_slide {
        for slide_element in &mut base_slide.elements {
            if let Some(ref mut graphics_element) = slide_element.element {
                if let Some(ref mut text) = graphics_element.text {
                    let rtf_options = extract_rtf_options(&text.rtf_data).unwrap_or_default();
                    text.rtf_data = segments_to_rtf_bytes(segments, &rtf_options);
                }
            }
        }
        base_slide.uuid = Some(rv_data::Uuid {
            string: uuid::Uuid::new_v4().to_string(),
        });
    }

    slide
}

/// Word-wrap a single line of text to fit within `max_width` characters.
///
/// Splits at word boundaries (spaces). If a single word exceeds `max_width`,
/// it is placed on its own line without breaking.
fn word_wrap(text: &str, max_width: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }

    let mut lines: Vec<String> = Vec::new();
    let mut current_line = String::new();
    let mut current_width: usize = 0;

    for word in text.split_whitespace() {
        let word_width = word.width();

        if current_line.is_empty() {
            // First word on this line — always accept it even if it exceeds max_width
            current_line.push_str(word);
            current_width = word_width;
            continue;
        }

        // +1 for the separating space
        if current_width + 1 + word_width > max_width {
            // Overflow: finalize current line, start a new one
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

/// Split content into slide-sized chunks based on visual line count.
///
/// Long lines are word-wrapped before slide splitting, so a single 500-char
/// scripture line becomes multiple visual lines that are packed into slides
/// respecting `max_lines`.
pub fn split_content_for_slides(
    content: &[String],
    wrap_column: usize,
    max_lines: usize,
) -> Vec<String> {
    let wrap_col = wrap_column.max(MIN_SLIDE_WRAP);
    let max = max_lines.max(1);

    // Pre-process: word-wrap every input line into visual lines
    let mut wrapped_lines: Vec<String> = Vec::new();
    for line in content {
        if line.trim().is_empty() {
            wrapped_lines.push(String::new());
        } else {
            wrapped_lines.extend(word_wrap(line, wrap_col));
        }
    }

    // Pack wrapped lines into slides respecting max visual lines
    let mut slides: Vec<String> = Vec::new();
    let mut current_slide: Vec<String> = Vec::new();
    let mut current_lines: usize = 0;

    for line in &wrapped_lines {
        if line.is_empty() {
            if !current_slide.is_empty() {
                current_slide.push(String::new());
                current_lines += 1;
            }
            continue;
        }

        // Each wrapped line is already within wrap_col, so it counts as 1 visual line
        if current_lines > 0 && current_lines + 1 > max {
            let slide_text = current_slide.join("\n").trim().to_string();
            if !slide_text.is_empty() {
                slides.push(slide_text);
            }
            current_slide.clear();
            current_lines = 0;
        }

        current_slide.push(line.clone());
        current_lines += 1;
    }

    let slide_text = current_slide.join("\n").trim().to_string();
    if !slide_text.is_empty() {
        slides.push(slide_text);
    }

    if slides.is_empty() {
        slides.push(String::new());
    }

    slides
}

/// A scripture passage with its title and verse data, used by the combined
/// scripture presentation builder.
pub struct ScripturePassage {
    /// Display title (e.g., "Isaiah 35:1-6 `NRSVue`").
    pub title: String,
    /// Individual verses for this passage.
    pub verses: Vec<crate::bible::Verse>,
}

/// Assemble a `Presentation` from a sequence of slides.
///
/// Builds the cue/group/UUID scaffolding that `ProPresenter` expects.
fn assemble_presentation(
    name: &str,
    template_slide: &rv_data::PresentationSlide,
    slide_segments: &[Vec<StyledSegment>],
) -> rv_data::Presentation {
    let mut presentation = rv_data::Presentation {
        name: name.to_string(),
        uuid: Some(rv_data::Uuid {
            string: uuid::Uuid::new_v4().to_string(),
        }),
        ..rv_data::Presentation::default()
    };

    let mut cue_uuids = Vec::new();

    for segments in slide_segments {
        let slide = clone_slide_with_text(template_slide, segments);
        let cue_uuid = uuid::Uuid::new_v4();
        let cue = rv_data::Cue {
            uuid: Some(rv_data::Uuid {
                string: cue_uuid.to_string(),
            }),
            name: String::new(),
            actions: vec![rv_data::Action {
                uuid: Some(rv_data::Uuid {
                    string: uuid::Uuid::new_v4().to_string(),
                }),
                name: String::new(),
                label: None,
                delay_time: 0.0,
                old_type: None,
                is_enabled: true,
                layer_identification: None,
                duration: 0.0,
                r#type: rv_data::action::ActionType::PresentationSlide as i32,
                action_type_data: Some(rv_data::action::ActionTypeData::Slide(
                    rv_data::action::SlideType {
                        slide: Some(rv_data::action::slide_type::Slide::Presentation(slide)),
                    },
                )),
            }],
            completion_target_type: rv_data::cue::CompletionTargetType::None as i32,
            completion_target_uuid: None,
            completion_action_type: rv_data::cue::CompletionActionType::Last as i32,
            completion_action_uuid: None,
            trigger_time: None,
            hot_key: Some(rv_data::HotKey {
                code: 0,
                control_identifier: String::new(),
            }),
            pending_imports: Vec::new(),
            is_enabled: true,
            completion_time: 0.0,
        };
        cue_uuids.push(cue_uuid);
        presentation.cues.push(cue);
    }

    if !cue_uuids.is_empty() {
        let group_uuid = uuid::Uuid::new_v4();
        let group = rv_data::presentation::CueGroup {
            group: Some(rv_data::Group {
                uuid: Some(rv_data::Uuid {
                    string: group_uuid.to_string(),
                }),
                name: String::new(),
                color: None,
                hot_key: None,
                application_group_identifier: None,
                application_group_name: String::new(),
            }),
            cue_identifiers: cue_uuids
                .iter()
                .map(|u| rv_data::Uuid {
                    string: u.to_string(),
                })
                .collect(),
        };
        presentation.cue_groups.push(group);
    }

    presentation
}

/// Build a scripture presentation with verse-aware slide splitting.
///
/// Splits at verse boundaries instead of mid-sentence. Optionally prepends
/// a title slide.
pub fn build_scripture_presentation(
    name: &str,
    template_slide: &rv_data::PresentationSlide,
    verses: &[crate::bible::Verse],
    title_text: Option<&str>,
) -> Option<rv_data::Presentation> {
    let (wrap_col, max_lines) = extract_slide_metrics(template_slide)
        .map_or((45, DEFAULT_MAX_LINES_PER_SLIDE), |m| {
            (m.chars_per_line, m.max_lines)
        });

    let slide_texts = split_verses_for_slides(verses, wrap_col, max_lines);

    let mut all_segments: Vec<Vec<StyledSegment>> = Vec::new();

    if let Some(title) = title_text {
        if !title.is_empty() {
            all_segments.push(vec![StyledSegment::unstyled(title)]);
        }
    }

    for text in &slide_texts {
        if !text.trim().is_empty() {
            all_segments.push(vec![StyledSegment::unstyled(text.as_str())]);
        }
    }

    if all_segments.is_empty() {
        return None;
    }

    Some(assemble_presentation(name, template_slide, &all_segments))
}

/// Build a combined presentation for multiple scripture passages.
///
/// Layout: [title → content slides → blank divider] repeated for each passage,
/// with no blank divider after the final passage.
pub fn build_combined_scripture_presentation(
    name: &str,
    template_slide: &rv_data::PresentationSlide,
    passages: &[ScripturePassage],
) -> Option<rv_data::Presentation> {
    if passages.is_empty() {
        return None;
    }

    let (wrap_col, max_lines) = extract_slide_metrics(template_slide)
        .map_or((45, DEFAULT_MAX_LINES_PER_SLIDE), |m| {
            (m.chars_per_line, m.max_lines)
        });

    let mut all_segments: Vec<Vec<StyledSegment>> = Vec::new();

    for (i, passage) in passages.iter().enumerate() {
        // Title slide
        if !passage.title.is_empty() {
            all_segments.push(vec![StyledSegment::unstyled(&passage.title)]);
        }

        // Content slides
        let slide_texts = split_verses_for_slides(&passage.verses, wrap_col, max_lines);
        for text in &slide_texts {
            if !text.trim().is_empty() {
                all_segments.push(vec![StyledSegment::unstyled(text.as_str())]);
            }
        }

        // Blank divider between passages (not after the last one)
        if i + 1 < passages.len() {
            all_segments.push(vec![StyledSegment::unstyled("")]);
        }
    }

    if all_segments.is_empty() {
        return None;
    }

    Some(assemble_presentation(name, template_slide, &all_segments))
}

/// Split verses into slide-sized chunks, preferring verse boundaries.
///
/// Each verse is prepended with its superscript number, word-wrapped, and
/// greedily packed onto slides. When a single verse exceeds `max_lines`,
/// it is split at natural punctuation (`. `, `; `, `: `, `, `) to avoid
/// breaking mid-sentence.
pub fn split_verses_for_slides(
    verses: &[crate::bible::Verse],
    wrap_column: usize,
    max_lines: usize,
) -> Vec<String> {
    let wrap_col = wrap_column.max(MIN_SLIDE_WRAP);
    let max = max_lines.max(1);

    // Pre-process: wrap each verse into visual lines
    let mut verse_blocks: Vec<Vec<String>> = Vec::new();
    for verse in verses {
        let prefixed = format!(
            "{} {}",
            crate::bible::to_superscript(verse.number),
            verse.text
        );
        let wrapped = word_wrap(&prefixed, wrap_col);

        if wrapped.len() <= max {
            verse_blocks.push(wrapped);
        } else {
            // Verse too long — split at punctuation, then re-wrap each fragment.
            // Prepend superscript to the full text so the first fragment's size
            // accounts for it during splitting.
            let fragments = split_at_punctuation(&prefixed, wrap_col, max);
            for fragment in &fragments {
                verse_blocks.push(word_wrap(fragment, wrap_col));
            }
        }
    }

    // Greedily pack blocks onto slides
    let mut slides: Vec<String> = Vec::new();
    let mut current_lines: Vec<String> = Vec::new();
    let mut current_count: usize = 0;

    for block in &verse_blocks {
        if current_count > 0 && current_count + block.len() > max {
            slides.push(current_lines.join("\n"));
            current_lines.clear();
            current_count = 0;
        }
        current_lines.extend_from_slice(block);
        current_count += block.len();
    }

    if !current_lines.is_empty() {
        slides.push(current_lines.join("\n"));
    }

    if slides.is_empty() {
        slides.push(String::new());
    }

    slides
}

/// Split text at natural punctuation boundaries so each fragment fits
/// within `max_lines` when word-wrapped to `wrap_col`.
///
/// Tries delimiters in order of preference: sentence end (`. `),
/// semicolon, colon, comma. Recursively splits oversized fragments
/// at finer-grained delimiters.
fn split_at_punctuation(text: &str, wrap_col: usize, max_lines: usize) -> Vec<String> {
    split_at_punctuation_level(text, wrap_col, max_lines, 0)
}

/// Delimiter tiers, ordered from strongest to weakest boundary.
const PUNCTUATION_DELIMITERS: &[&str] = &[". ", "; ", ": ", ", "];

/// Recursive implementation that tries increasingly fine-grained delimiters.
fn split_at_punctuation_level(
    text: &str,
    wrap_col: usize,
    max_lines: usize,
    level: usize,
) -> Vec<String> {
    // Base case: no more delimiters to try
    if level >= PUNCTUATION_DELIMITERS.len() {
        return vec![text.to_string()];
    }

    let delim = PUNCTUATION_DELIMITERS[level];
    let segments: Vec<&str> = text.split(delim).collect();

    if segments.len() < 2 {
        // This delimiter doesn't appear — try the next one
        return split_at_punctuation_level(text, wrap_col, max_lines, level + 1);
    }

    // Group segments greedily so each group fits within max_lines
    let mut result: Vec<String> = Vec::new();
    let mut current = String::new();

    for (i, seg) in segments.iter().enumerate() {
        let candidate = if current.is_empty() {
            seg.to_string()
        } else {
            format!("{current}{delim}{seg}")
        };

        let line_count = word_wrap(&candidate, wrap_col).len();
        if line_count > max_lines && !current.is_empty() {
            result.push(format!("{current}{delim}"));
            current = seg.to_string();
        } else {
            current = candidate;
        }

        if i == segments.len() - 1 && !current.is_empty() {
            result.push(current.clone());
        }
    }

    // Recursively split any fragment that's still too large
    let mut final_result: Vec<String> = Vec::new();
    for fragment in &result {
        if word_wrap(fragment, wrap_col).len() > max_lines {
            final_result.extend(split_at_punctuation_level(
                fragment,
                wrap_col,
                max_lines,
                level + 1,
            ));
        } else {
            final_result.push(fragment.clone());
        }
    }

    if final_result.len() > 1 {
        final_result
    } else {
        // Splitting at this level didn't help — try next level on the whole text
        split_at_punctuation_level(text, wrap_col, max_lines, level + 1)
    }
}

/// Build a presentation from a template slide with custom wrap/split options.
///
/// Takes a `PresentationSlide` directly (from theme or legacy template).
/// Constructs a fresh `Presentation` shell and populates it with cloned slides.
///
/// If the template slide contains text box metrics (bounds, font size),
/// those are used to compute `chars_per_line` and `max_lines` automatically.
/// The passed-in `wrap_column` and `max_lines_per_slide` serve as fallbacks
/// when metric extraction fails.
///
/// If `title_text` is provided, a title slide is prepended before content.
#[allow(clippy::too_many_lines)]
pub fn build_presentation_from_template_with_options(
    name: &str,
    template_slide: &rv_data::PresentationSlide,
    content: &[StyledSegment],
    wrap_column: usize,
    max_lines_per_slide: usize,
    title_text: Option<&str>,
) -> Option<rv_data::Presentation> {
    // Derive wrap/split parameters from template geometry when possible
    let (effective_wrap, effective_max_lines) = extract_slide_metrics(template_slide)
        .map_or((wrap_column, max_lines_per_slide), |m| {
            (m.chars_per_line, m.max_lines)
        });

    // Split content into slide-sized chunks using plain text for wrapping logic
    let plain: Vec<String> = content.iter().map(|s| s.text.clone()).collect();
    let slide_texts = split_content_for_slides(&plain, effective_wrap, effective_max_lines);

    // Build a fresh Presentation shell
    let mut presentation = rv_data::Presentation {
        name: name.to_string(),
        uuid: Some(rv_data::Uuid {
            string: uuid::Uuid::new_v4().to_string(),
        }),
        ..rv_data::Presentation::default()
    };

    let mut cue_uuids = Vec::new();

    let mut push_slide_cue =
        |presentation: &mut rv_data::Presentation, segments: &[StyledSegment]| {
            let slide = clone_slide_with_text(template_slide, segments);
            let cue_uuid = uuid::Uuid::new_v4();
            let cue = rv_data::Cue {
                uuid: Some(rv_data::Uuid {
                    string: cue_uuid.to_string(),
                }),
                name: String::new(),
                actions: vec![rv_data::Action {
                    uuid: Some(rv_data::Uuid {
                        string: uuid::Uuid::new_v4().to_string(),
                    }),
                    name: String::new(),
                    label: None,
                    delay_time: 0.0,
                    old_type: None,
                    is_enabled: true,
                    layer_identification: None,
                    duration: 0.0,
                    r#type: rv_data::action::ActionType::PresentationSlide as i32,
                    action_type_data: Some(rv_data::action::ActionTypeData::Slide(
                        rv_data::action::SlideType {
                            slide: Some(rv_data::action::slide_type::Slide::Presentation(slide)),
                        },
                    )),
                }],
                completion_target_type: rv_data::cue::CompletionTargetType::None as i32,
                completion_target_uuid: None,
                completion_action_type: rv_data::cue::CompletionActionType::Last as i32,
                completion_action_uuid: None,
                trigger_time: None,
                hot_key: Some(rv_data::HotKey {
                    code: 0,
                    control_identifier: String::new(),
                }),
                pending_imports: Vec::new(),
                is_enabled: true,
                completion_time: 0.0,
            };
            cue_uuids.push(cue_uuid);
            presentation.cues.push(cue);
        };

    // Title slide (plain text, default color)
    if let Some(title) = title_text {
        if !title.is_empty() {
            push_slide_cue(&mut presentation, &[StyledSegment::unstyled(title)]);
        }
    }

    // Content slides — resolve styled segments for each slide chunk
    let mut seg_idx = 0;
    for slide_text in &slide_texts {
        if slide_text.trim().is_empty() {
            continue;
        }

        let slide_line_count = slide_text.lines().count();
        let mut slide_segments: Vec<StyledSegment> = Vec::new();
        let mut consumed = 0;

        while consumed < slide_line_count && seg_idx < content.len() {
            let seg = &content[seg_idx];
            seg_idx += 1;

            if seg.text.trim().is_empty() {
                continue;
            }
            slide_segments.push(seg.clone());
            consumed += 1;
        }

        if slide_segments.is_empty() {
            push_slide_cue(
                &mut presentation,
                &[StyledSegment::unstyled(slide_text.as_str())],
            );
        } else {
            push_slide_cue(&mut presentation, &slide_segments);
        }
    }

    // Create a single group containing all cues
    if !cue_uuids.is_empty() {
        let group_uuid = uuid::Uuid::new_v4();
        let group = rv_data::presentation::CueGroup {
            group: Some(rv_data::Group {
                uuid: Some(rv_data::Uuid {
                    string: group_uuid.to_string(),
                }),
                name: String::new(),
                color: None,
                hot_key: None,
                application_group_identifier: None,
                application_group_name: String::new(),
            }),
            cue_identifiers: cue_uuids
                .iter()
                .map(|u| rv_data::Uuid {
                    string: u.to_string(),
                })
                .collect(),
        };
        presentation.cue_groups.push(group);
    }

    Some(presentation)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    fn get_template_path() -> PathBuf {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("data");
        path.push("templates");
        path
    }

    fn get_scripture_slide() -> rv_data::PresentationSlide {
        let mut cache = ThemeCache::new(None, vec![get_template_path()]);
        cache
            .get("scripture")
            .expect("scripture template should load")
            .clone()
    }

    #[test]
    fn test_theme_cache_legacy_load() {
        let mut cache = ThemeCache::new(None, vec![get_template_path()]);
        assert!(cache.get("scripture").is_some());
        assert!(cache.get("song").is_some());
        assert!(cache.get("info").is_some());
    }

    #[test]
    fn test_extract_slide_metrics() {
        let slide = get_scripture_slide();
        let metrics = extract_slide_metrics(&slide);
        assert!(metrics.is_some(), "should extract metrics from scripture template");

        let m = metrics.unwrap();
        assert!(m.font_size_pt > 0.0, "font size should be positive");
        assert!(m.chars_per_line > 0, "chars_per_line should be positive");
        assert!(m.max_lines > 0, "max_lines should be positive");
        assert!(m.text_width_pt > 0.0, "text width should be positive");
        assert!(m.text_height_pt > 0.0, "text height should be positive");
    }

    #[test]
    fn test_word_wrap_basic() {
        let result = word_wrap("hello world foo bar", 10);
        assert_eq!(result, vec!["hello", "world foo", "bar"]);
    }

    #[test]
    fn test_word_wrap_long_word() {
        let result = word_wrap("superlongword short", 5);
        assert_eq!(result, vec!["superlongword", "short"]);
    }

    #[test]
    fn test_word_wrap_empty() {
        let result = word_wrap("", 40);
        assert_eq!(result, vec![""]);
    }

    #[test]
    fn test_split_content_single_long_line() {
        let long_line = "The wilderness and dry land shall be glad the desert shall rejoice and blossom like the crocus it shall blossom abundantly and rejoice with joy and singing".to_string();
        let content = vec![long_line];
        let slides = split_content_for_slides(&content, 40, 3);

        assert!(slides.len() > 1, "long line should produce multiple slides, got {}", slides.len());
        for (i, slide) in slides.iter().enumerate() {
            let line_count = slide.lines().count();
            assert!(line_count <= 3, "slide {} has {} lines, expected <= 3", i, line_count);
        }
    }

    #[test]
    fn test_split_content_short_lines_unchanged() {
        let content = vec![
            "Line one".to_string(),
            "Line two".to_string(),
            "Line three".to_string(),
        ];
        let slides = split_content_for_slides(&content, 80, 10);
        assert_eq!(slides.len(), 1);
        assert_eq!(slides[0], "Line one\nLine two\nLine three");
    }

    #[test]
    fn test_build_from_template() {
        let slide = get_scripture_slide();

        let content = StyledSegment::from_plain(&[
            "\u{b9}\u{2075}The wilderness and dry land shall be glad,".to_string(),
            "\u{b9}\u{2076}the desert shall rejoice and blossom;".to_string(),
        ]);

        let presentation = build_presentation_from_template_with_options(
            "Test Scripture",
            &slide,
            &content,
            45,
            DEFAULT_MAX_LINES_PER_SLIDE,
            None,
        );
        assert!(presentation.is_some());

        let pres = presentation.unwrap();
        assert_eq!(pres.name, "Test Scripture");
        assert!(pres.cues.len() >= 1, "should produce at least one slide");
    }

    #[test]
    fn test_long_scripture_generates_multiple_slides() {
        let slide = get_scripture_slide();

        let long_scripture = "\u{b9}\u{2075}The wilderness and the dry land shall be glad; the desert shall rejoice and blossom like the crocus. \u{b9}\u{2076}It shall blossom abundantly and rejoice with joy and singing. The glory of Lebanon shall be given to it, the majesty of Carmel and Sharon. They shall see the glory of the LORD, the majesty of our God. \u{b9}\u{2077}Strengthen the weak hands, and make firm the feeble knees. Say to those who have an anxious heart, Be strong; fear not! Behold, your God will come with vengeance, with the recompense of God. He will come and save you. \u{b9}\u{2078}Then the eyes of the blind shall be opened, and the ears of the deaf unstopped; then shall the lame man leap like a deer, and the tongue of the mute sing for joy.".to_string();

        let content = StyledSegment::from_plain(&[long_scripture]);
        let presentation = build_presentation_from_template_with_options(
            "Isaiah 35:1-6",
            &slide,
            &content,
            40,
            4,
            Some("Isaiah 35:1-6 NRSVue"),
        );
        assert!(presentation.is_some());

        let pres = presentation.unwrap();
        assert!(
            pres.cues.len() > 1,
            "long scripture should produce multiple slides, got {}",
            pres.cues.len()
        );
    }

    #[test]
    fn test_split_verses_breaks_at_boundaries() {
        let verses = vec![
            crate::bible::Verse {
                number: 1,
                text: "The wilderness and the dry land shall be glad.".to_string(),
            },
            crate::bible::Verse {
                number: 2,
                text: "It shall blossom abundantly and rejoice with joy and singing.".to_string(),
            },
            crate::bible::Verse {
                number: 3,
                text: "Strengthen the weak hands, and make firm the feeble knees.".to_string(),
            },
            crate::bible::Verse {
                number: 4,
                text: "Say to those who have an anxious heart, Be strong; fear not!".to_string(),
            },
            crate::bible::Verse {
                number: 5,
                text: "Then the eyes of the blind shall be opened, and the ears of the deaf unstopped.".to_string(),
            },
        ];

        let slides = split_verses_for_slides(&verses, 40, 3);

        // Each slide should contain only complete verses (superscript numbers mark boundaries)
        for (i, slide) in slides.iter().enumerate() {
            let line_count = slide.lines().count();
            // A single verse may exceed max_lines on its own, but we never split mid-verse
            assert!(
                line_count <= 3 || slide.matches('⁰').count()
                    + slide.matches('¹').count()
                    + slide.matches('²').count()
                    + slide.matches('³').count()
                    + slide.matches('⁴').count()
                    + slide.matches('⁵').count() == 1,
                "slide {i} has {line_count} lines but should be ≤3 unless it's a single long verse"
            );
        }

        // All 5 verses should appear across slides (word-wrapped, so check key phrases)
        let all_text = slides.join("\n");
        assert!(all_text.contains("wilderness"), "verse 1 content missing");
        assert!(all_text.contains("blossom abundantly"), "verse 2 content missing");
        assert!(all_text.contains("Strengthen"), "verse 3 content missing");
        assert!(all_text.contains("Be strong"), "verse 4 content missing");
        assert!(all_text.contains("blind shall be"), "verse 5 content missing");

        // Verify verse numbers are present as superscripts
        assert!(all_text.contains('¹'), "superscript 1 missing");
        assert!(all_text.contains('⁵'), "superscript 5 missing");
    }

    #[test]
    fn test_split_verses_long_verse_splits_at_punctuation() {
        // One verse that's way too long for 3 lines at 40 chars
        let verses = vec![crate::bible::Verse {
            number: 28,
            text: "And we know that for those who love God all things work together for good, for those who are called according to his purpose. For those whom he foreknew he also predestined to be conformed to the image of his Son, in order that he might be the firstborn among many brothers.".to_string(),
        }];

        let slides = split_verses_for_slides(&verses, 40, 3);

        // Should produce multiple slides since the verse is too long for one
        assert!(
            slides.len() > 1,
            "long verse should split into multiple slides, got {}",
            slides.len()
        );

        // Each slide should fit within the line limit
        for (i, slide) in slides.iter().enumerate() {
            let line_count = slide.lines().count();
            assert!(
                line_count <= 3,
                "slide {i} has {line_count} lines, expected ≤3"
            );
        }

        // The superscript verse number should only appear on the first slide
        assert!(slides[0].contains('²'), "first slide should have verse number");

        // Content should be preserved across all slides
        let all_text = slides.join(" ");
        assert!(all_text.contains("foreknew"), "content should be preserved");
        assert!(all_text.contains("firstborn"), "content should be preserved");
    }

    #[test]
    fn test_build_scripture_presentation() {
        let slide = get_scripture_slide();
        let verses = vec![
            crate::bible::Verse {
                number: 1,
                text: "In the beginning God created the heavens and the earth.".to_string(),
            },
            crate::bible::Verse {
                number: 2,
                text: "The earth was formless and empty, and darkness covered the deep waters.".to_string(),
            },
        ];

        let pres = build_scripture_presentation(
            "Genesis 1v1-2 NRSVue",
            &slide,
            &verses,
            Some("Genesis 1:1-2 NRSVue"),
        );
        assert!(pres.is_some());

        let p = pres.unwrap();
        // Title slide + at least 1 content slide
        assert!(p.cues.len() >= 2, "expected title + content, got {}", p.cues.len());
    }

    #[test]
    fn test_build_combined_scripture_presentation() {
        let slide = get_scripture_slide();
        let passages = vec![
            ScripturePassage {
                title: "Isaiah 35:1-2 NRSVue".to_string(),
                verses: vec![
                    crate::bible::Verse {
                        number: 1,
                        text: "The wilderness and the dry land shall be glad.".to_string(),
                    },
                    crate::bible::Verse {
                        number: 2,
                        text: "It shall blossom abundantly.".to_string(),
                    },
                ],
            },
            ScripturePassage {
                title: "Luke 2:1-2 NRSVue".to_string(),
                verses: vec![
                    crate::bible::Verse {
                        number: 1,
                        text: "In those days a decree went out from Caesar Augustus.".to_string(),
                    },
                    crate::bible::Verse {
                        number: 2,
                        text: "This was the first registration and was taken while Quirinius was governor of Syria.".to_string(),
                    },
                ],
            },
        ];

        let pres = build_combined_scripture_presentation(
            "Isaiah 35v1-2, Luke 2v1-2 NRSVue",
            &slide,
            &passages,
        );
        assert!(pres.is_some());

        let p = pres.unwrap();
        // Should have: title1 + content1 + blank divider + title2 + content2
        assert!(
            p.cues.len() >= 5,
            "combined presentation should have ≥5 cues (title+content+blank+title+content), got {}",
            p.cues.len()
        );
    }
}
