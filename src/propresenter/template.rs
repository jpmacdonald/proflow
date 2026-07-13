//! Template-based slide generation.
//!
//! Loads slide styling from installed ProPresenter themes, then injects text
//! while preserving fonts, colors, and layout.

use prost::Message;
use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use unicode_width::UnicodeWidthStr;

use super::generated::rv_data;
use super::rtf::{extract_rtf_options, segments_to_rtf_bytes, StyledSegment};
/// Default maximum visual lines per slide.
pub const DEFAULT_MAX_LINES_PER_SLIDE: usize = 10;

/// Minimum wrap column for slide splitting.
pub const MIN_SLIDE_WRAP: usize = 20;

/// Average character width as a fraction of font size for proportional fonts.
/// `ProPresenter` projector themes use large, readable text boxes; a 0.50
/// estimate matches observed scripture wrapping more closely than an overly
/// conservative Helvetica average.
const CHAR_WIDTH_RATIO: f64 = 0.50;

/// Default line height multiplier when no paragraph style specifies one.
const DEFAULT_LINE_HEIGHT_MULTIPLE: f64 = 1.2;

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
        let Some(graphics_element) = slide_element.element.as_ref() else {
            continue;
        };
        let Some(text) = graphics_element.text.as_ref() else {
            continue;
        };
        let Some(bounds) = graphics_element.bounds.as_ref() else {
            continue;
        };
        let Some(size) = bounds.size.as_ref() else {
            continue;
        };

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

        // Compute max lines that fit from the template geometry. The template
        // bounds already include the intended operator/projector padding, so an
        // extra line penalty makes scripture splitting too sparse.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let max_lines = ((text_height_pt / line_height_pt).floor() as usize).max(1);

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

/// Cached slide templates loaded from one installed `ProPresenter` theme.
pub struct ThemeCache {
    /// Slides loaded from a theme file, keyed by slide name.
    theme_slides: HashMap<String, CachedThemeSlide>,
    /// Name of the loaded theme (if any).
    theme_name: Option<String>,
}

struct CachedThemeSlide {
    slide: rv_data::PresentationSlide,
    action_count: usize,
}

/// Failure to load a configured `ProPresenter` theme.
#[derive(Debug, thiserror::Error)]
pub enum ThemeCacheLoadError {
    /// No theme document with the configured name exists in any search root.
    #[error("theme '{name}' was not found in: {searched:?}")]
    NotFound {
        /// Configured theme name.
        name: String,
        /// Candidate theme document paths.
        searched: Vec<PathBuf>,
    },
    /// The theme document could not be read.
    #[error("failed to read theme at {path}: {source}")]
    Read {
        /// Theme document path.
        path: PathBuf,
        /// Filesystem failure.
        source: std::io::Error,
    },
    /// The theme document was not valid `ProPresenter` protobuf data.
    #[error("failed to decode theme at {path}: {source}")]
    Decode {
        /// Theme document path.
        path: PathBuf,
        /// Protobuf decoding failure.
        source: prost::DecodeError,
    },
    /// A valid theme document did not contain any named slides.
    #[error("theme '{name}' at {path} contains no named slides")]
    Empty {
        /// Configured theme name.
        name: String,
        /// Theme document path.
        path: PathBuf,
    },
    /// Two theme slides have the same canonical name.
    #[error("theme at {path} contains ambiguous slide names '{first}' and '{duplicate}'")]
    DuplicateSlideName {
        /// Theme document path.
        path: PathBuf,
        /// First installed spelling.
        first: String,
        /// Conflicting installed spelling.
        duplicate: String,
    },
}

/// Failure to use one configured theme slide as a generated-text template.
#[derive(Debug, thiserror::Error)]
pub enum ThemeSlideError {
    /// The configured slide name is not present in the loaded theme.
    #[error("theme slide '{name}' was not found")]
    NotFound {
        /// Exact configured slide name.
        name: String,
    },
    /// Generated text has no unambiguous destination in the slide.
    #[error("theme slide '{name}' has {count} text elements; exactly one is required")]
    TextElementCount {
        /// Exact configured slide name.
        name: String,
        /// Number of text-bearing graphics elements in the slide.
        count: usize,
    },
    /// Theme-level actions cannot be represented by the text renderer without
    /// making macro/media behavior implicit.
    #[error(
        "theme slide '{name}' has {count} embedded actions; cue-role actions must be explicit"
    )]
    EmbeddedActions {
        /// Exact configured slide name.
        name: String,
        /// Number of actions attached to the theme slide.
        count: usize,
    },
}

impl ThemeCache {
    /// Load one installed theme. A configured theme that is missing, unreadable,
    /// or malformed is an error. No configured theme produces an empty cache.
    pub fn load(theme_name: Option<&str>) -> Result<Self, ThemeCacheLoadError> {
        let (theme_slides, resolved_name) = if let Some(name) = theme_name {
            let searched = theme_candidate_paths(name);
            let path = searched
                .iter()
                .find(|path| path.is_file())
                .cloned()
                .ok_or_else(|| ThemeCacheLoadError::NotFound {
                    name: name.to_string(),
                    searched,
                })?;
            let theme = load_theme(&path)?;
            if theme.slides.is_empty() {
                return Err(ThemeCacheLoadError::Empty {
                    name: name.to_string(),
                    path,
                });
            }
            (theme.slides, Some(name.to_string()))
        } else {
            (HashMap::new(), None)
        };

        Ok(Self {
            theme_slides,
            theme_name: resolved_name,
        })
    }

    /// Look up an exact slide in the configured `ProPresenter` theme.
    pub fn get_theme_slide(&self, slide_name: &str) -> Option<&rv_data::PresentationSlide> {
        self.theme_slides
            .get(slide_name)
            .map(|cached| &cached.slide)
    }

    /// Resolve one slide that has exactly one generated-text destination.
    ///
    /// Theme slides may contain several text graphics (including empty helper
    /// layers). Replacing all of them with the same generated content is not a
    /// valid default, so configured cue roles must select an unambiguous slide.
    pub fn text_template(
        &self,
        slide_name: &str,
    ) -> Result<&rv_data::PresentationSlide, ThemeSlideError> {
        let cached =
            self.theme_slides
                .get(slide_name)
                .ok_or_else(|| ThemeSlideError::NotFound {
                    name: slide_name.to_string(),
                })?;
        if cached.action_count != 0 {
            return Err(ThemeSlideError::EmbeddedActions {
                name: slide_name.to_string(),
                count: cached.action_count,
            });
        }
        let slide = &cached.slide;
        let count = slide_text_element_count(slide);
        if count == 1 {
            Ok(slide)
        } else {
            Err(ThemeSlideError::TextElementCount {
                name: slide_name.to_string(),
                count,
            })
        }
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

fn slide_text_element_count(slide: &rv_data::PresentationSlide) -> usize {
    slide
        .base_slide
        .as_ref()
        .map(|base| {
            base.elements
                .iter()
                .filter(|element| {
                    element
                        .element
                        .as_ref()
                        .is_some_and(|graphics| graphics.text.is_some())
                })
                .count()
        })
        .unwrap_or_default()
}

/// Resolve the `ProPresenter` Themes directory path for a given theme name.
fn theme_candidate_paths(theme_name: &str) -> Vec<PathBuf> {
    theme_search_dirs()
        .into_iter()
        .map(|dir| dir.join(theme_name).join("Theme"))
        .collect()
}

fn theme_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Some(themes_dir) = env_path("THEMES_DIR") {
        push_unique_path(&mut dirs, themes_dir);
    }

    if let Some(root) = env_path("PROPRESENTER_DIR") {
        push_unique_path(&mut dirs, root.join("Themes"));
    }

    if let Some(home) = dirs::home_dir() {
        push_unique_path(&mut dirs, home.join("Documents/ProPresenter/Themes"));
    }

    dirs
}

fn env_path(name: &str) -> Option<PathBuf> {
    env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|value| PathBuf::from(shellexpand::tilde(&value).to_string()))
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

/// Load all slides from a `ProPresenter` theme file, keyed by slide name.
struct LoadedTheme {
    slides: HashMap<String, CachedThemeSlide>,
}

fn load_theme(path: &Path) -> Result<LoadedTheme, ThemeCacheLoadError> {
    let mut slides = HashMap::new();
    let mut canonical_names = HashMap::<String, String>::new();
    let data = std::fs::read(path).map_err(|source| ThemeCacheLoadError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let doc = rv_data::template::Document::decode(data.as_slice()).map_err(|source| {
        ThemeCacheLoadError::Decode {
            path: path.to_path_buf(),
            source,
        }
    })?;
    for ts in &doc.slides {
        if ts.name.is_empty() {
            continue;
        }
        let canonical = ts.name.to_lowercase();
        if let Some(first) = canonical_names.insert(canonical, ts.name.clone()) {
            return Err(ThemeCacheLoadError::DuplicateSlideName {
                path: path.to_path_buf(),
                first,
                duplicate: ts.name.clone(),
            });
        }
        slides.insert(
            ts.name.clone(),
            CachedThemeSlide {
                slide: theme_slide_to_presentation_slide(ts),
                action_count: ts.actions.len(),
            },
        );
    }
    Ok(LoadedTheme { slides })
}

/// Convert a `template::Slide` (from a theme file) into a `PresentationSlide`.
///
/// Both share the same `base_slide: Slide` — we just wrap it in the
/// `PresentationSlide` envelope that the rest of the pipeline expects.
fn theme_slide_to_presentation_slide(ts: &rv_data::template::Slide) -> rv_data::PresentationSlide {
    rv_data::PresentationSlide {
        base_slide: ts.base_slide.clone(),
        notes: None,
        template_guidelines: Vec::new(),
        chord_chart: None,
        transition: None,
    }
}

/// Extract the first presentation slide from a `.pro` document.
///
/// Navigates the `Presentation` → `Cue` → `Action` → `PresentationSlide` chain.
#[cfg(test)]
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

fn presentation_with_native_envelope(name: &str) -> rv_data::Presentation {
    rv_data::Presentation {
        uuid: Some(rv_data::Uuid {
            string: uuid::Uuid::new_v4().to_string(),
        }),
        name: name.to_string(),
        background: Some(rv_data::Background {
            is_enabled: false,
            fill: None,
        }),
        chord_chart: Some(rv_data::Url::default()),
        ccli: Some(rv_data::presentation::Ccli::default()),
        timeline: Some(native_empty_timeline()),
        content_destination: rv_data::action::ContentDestination::Global as i32,
        ..rv_data::Presentation::default()
    }
}

fn native_empty_timeline() -> rv_data::presentation::Timeline {
    rv_data::presentation::Timeline {
        duration: 300.0,
        ..rv_data::presentation::Timeline::default()
    }
}

/// Apply producer metadata captured for the current runtime.
pub(crate) fn apply_application_info(
    presentation: &mut rv_data::Presentation,
    application_info: Option<&rv_data::ApplicationInfo>,
) {
    presentation.application_info = application_info.cloned();
}

/// Preserve document-owned metadata while replacing rendered cue state.
///
/// Timeline cue/action references are deliberately discarded because they can
/// point at cues removed by the render. An empty native timeline replaces them.
pub(crate) fn preserve_presentation_envelope(
    presentation: &mut rv_data::Presentation,
    existing: &rv_data::Presentation,
) {
    presentation
        .application_info
        .clone_from(&existing.application_info);
    presentation.uuid.clone_from(&existing.uuid);
    presentation
        .last_date_used
        .clone_from(&existing.last_date_used);
    presentation
        .last_modified_date
        .clone_from(&existing.last_modified_date);
    presentation.category.clone_from(&existing.category);
    presentation.notes.clone_from(&existing.notes);
    presentation.background.clone_from(&existing.background);
    presentation.chord_chart.clone_from(&existing.chord_chart);
    presentation.ccli.clone_from(&existing.ccli);
    presentation
        .bible_reference
        .clone_from(&existing.bible_reference);
    presentation.transition.clone_from(&existing.transition);
    presentation.content_destination = existing.content_destination;
    presentation
        .multi_tracks_licensing
        .clone_from(&existing.multi_tracks_licensing);
    presentation.music_key.clone_from(&existing.music_key);
    presentation.music.clone_from(&existing.music);
    presentation.slide_show.clone_from(&existing.slide_show);
    presentation.timeline = Some(native_empty_timeline());
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

/// Replace the first presentation slide action on a cue with `template_slide`,
/// carrying over the cue's current plain text.
///
/// This is used when normalizing existing library files to the projector-facing
/// theme associated with their macro. It intentionally keeps media and macro
/// actions on the cue unchanged.
pub fn replace_cue_slide_template_preserving_text(
    cue: &mut rv_data::Cue,
    template_slide: &rv_data::PresentationSlide,
) -> bool {
    let segments = cue_text_segments(cue);
    replace_cue_slide_template_with_segments(cue, template_slide, &segments)
}

/// Replace the first presentation slide action on a cue with `template_slide`,
/// using caller-provided text segments.
pub fn replace_cue_slide_template_with_segments(
    cue: &mut rv_data::Cue,
    template_slide: &rv_data::PresentationSlide,
    segments: &[StyledSegment],
) -> bool {
    for action in &mut cue.actions {
        let Some(rv_data::action::ActionTypeData::Slide(slide_type)) = &mut action.action_type_data
        else {
            continue;
        };
        let Some(rv_data::action::slide_type::Slide::Presentation(slide)) = &mut slide_type.slide
        else {
            continue;
        };
        *slide = clone_slide_with_text(template_slide, segments);
        return true;
    }
    false
}

fn cue_text_segments(cue: &rv_data::Cue) -> Vec<StyledSegment> {
    let text = cue_plain_text(cue);
    if text.trim().is_empty() {
        return vec![StyledSegment::unstyled("")];
    }

    let segments = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(StyledSegment::unstyled)
        .collect::<Vec<_>>();
    if segments.is_empty() {
        vec![StyledSegment::unstyled("")]
    } else {
        segments
    }
}

fn cue_plain_text(cue: &rv_data::Cue) -> String {
    let mut texts = Vec::new();
    for action in &cue.actions {
        let Some(rv_data::action::ActionTypeData::Slide(slide_type)) = &action.action_type_data
        else {
            continue;
        };
        let Some(rv_data::action::slide_type::Slide::Presentation(slide)) = &slide_type.slide
        else {
            continue;
        };
        let Some(base_slide) = &slide.base_slide else {
            continue;
        };
        for element in &base_slide.elements {
            let Some(graphics) = &element.element else {
                continue;
            };
            let Some(text) = &graphics.text else {
                continue;
            };
            let rtf = String::from_utf8_lossy(&text.rtf_data);
            if let Some(text) = super::rtf::rtf_to_text(&rtf) {
                texts.push(text);
            }
        }
    }
    texts.join("\n")
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

/// Pack styled segments onto slides greedily by line capacity.
///
/// Each segment is word-wrapped to estimate its visual line count, then
/// segments are packed onto slides until adding the next would exceed
/// `max_lines`. This matches how scripture verses are packed — content
/// flows across slides based on how much text fits.
pub fn pack_segments_for_slides(
    segments: &[StyledSegment],
    wrap_column: usize,
    max_lines: usize,
) -> Vec<Vec<StyledSegment>> {
    let wrap_col = wrap_column.max(MIN_SLIDE_WRAP);
    let max = max_lines.max(1);

    let mut slides: Vec<Vec<StyledSegment>> = Vec::new();
    let mut current: Vec<StyledSegment> = Vec::new();
    let mut current_count: usize = 0;
    let mut pending_blank: Option<StyledSegment> = None;

    for seg in segments {
        if seg.text.is_empty() {
            pending_blank = Some(seg.clone());
            continue;
        }

        let line_count = word_wrap(&seg.text, wrap_col).len();
        let pending_blank_count = usize::from(pending_blank.is_some());
        if current_count > 0 && current_count + pending_blank_count + line_count > max {
            trim_trailing_blank_segments(&mut current, &mut current_count);
            if !current.is_empty() {
                slides.push(std::mem::take(&mut current));
            }
            current_count = 0;
            pending_blank = None;
        }

        if current_count > 0 {
            if let Some(blank) = pending_blank.take() {
                current.push(blank);
                current_count += 1;
            }
        }

        current.push(seg.clone());
        current_count += line_count;
    }

    trim_trailing_blank_segments(&mut current, &mut current_count);
    if !current.is_empty() {
        slides.push(current);
    }

    slides
}

fn trim_trailing_blank_segments(segments: &mut Vec<StyledSegment>, line_count: &mut usize) {
    while segments.last().is_some_and(|seg| seg.text.is_empty()) {
        segments.pop();
        *line_count = line_count.saturating_sub(1);
    }
}

/// A scripture passage with its title and verse data, used by the combined
/// scripture presentation builder.
pub struct ScripturePassage {
    /// Display title (e.g., "Isaiah 35:1-6 `NRSVue`").
    pub title: String,
    /// Individual verses for this passage.
    pub verses: Vec<crate::bible::Verse>,
}

/// One rendered scripture slide and the verses that contribute text to it.
///
/// The provenance is kept with the rendered text so the presentation boundary
/// can emit a native slide-action label without re-parsing superscripts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptureSlide {
    text: String,
    verse_numbers: Vec<u32>,
}

impl ScriptureSlide {
    /// Rendered text, including the superscript number at each verse start.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Verse numbers represented on this slide, in source order.
    #[must_use]
    pub fn verse_numbers(&self) -> &[u32] {
        &self.verse_numbers
    }

    /// Human-readable native slide label (`7` or `7-9`).
    #[must_use]
    pub fn label(&self) -> String {
        format_verse_ranges(&self.verse_numbers)
    }

    fn from_fragment(text: String, verse_number: u32) -> Self {
        Self {
            text,
            verse_numbers: vec![verse_number],
        }
    }

    fn append_fragment(&mut self, text: &str, verse_number: u32) {
        if !self.text.is_empty() {
            self.text.push(' ');
        }
        self.text.push_str(text);
        if self.verse_numbers.last().copied() != Some(verse_number) {
            self.verse_numbers.push(verse_number);
        }
    }
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
            end = number;
            continue;
        }
        ranges.push(format_verse_range(start, end));
        start = number;
        end = number;
    }
    ranges.push(format_verse_range(start, end));
    ranges.join(", ")
}

fn format_verse_range(start: u32, end: u32) -> String {
    if start == end {
        start.to_string()
    } else {
        format!("{start}-{end}")
    }
}

/// Cue indices where rendered semantic regions begin.
///
/// A region can begin more than once in one presentation, such as each title
/// and verse block in a combined scripture reading. The indices are created in
/// the same scope as the cues, so macro placement does not have to infer roles
/// from cue position or template identity later.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct RenderedCueRoles {
    title_entries: Vec<usize>,
    content_entries: Vec<usize>,
}

impl RenderedCueRoles {
    pub(crate) fn title_entries(&self) -> &[usize] {
        &self.title_entries
    }

    pub(crate) fn content_entries(&self) -> &[usize] {
        &self.content_entries
    }

    pub(crate) fn first_entry(&self) -> Option<usize> {
        self.title_entries
            .first()
            .into_iter()
            .chain(self.content_entries.first())
            .copied()
            .min()
    }

    fn record_title(&mut self, cue_index: usize) {
        self.title_entries.push(cue_index);
    }

    fn record_content(&mut self, cue_index: usize) {
        self.content_entries.push(cue_index);
    }
}

/// A rendered presentation bundled with the cue-role entries derived while
/// rendering it.
pub(crate) struct RenderedPresentation {
    pub(crate) presentation: rv_data::Presentation,
    pub(crate) cue_roles: RenderedCueRoles,
}

/// Assemble a `Presentation` from a sequence of slides.
///
/// Builds the cue/group/UUID scaffolding that `ProPresenter` expects.
pub fn assemble_presentation(
    name: &str,
    template_slide: &rv_data::PresentationSlide,
    slide_segments: &[Vec<StyledSegment>],
) -> rv_data::Presentation {
    let title_template: Option<&rv_data::PresentationSlide> = None;
    assemble_presentation_with_title_template(
        name,
        template_slide,
        title_template,
        None,
        slide_segments,
    )
    .unwrap_or_else(|| presentation_with_native_envelope(name))
}

/// Assemble a `Presentation` with an optional distinct template for the first
/// title slide. Content slides always use `content_template`.
pub fn assemble_presentation_with_title_template(
    name: &str,
    content_template: &rv_data::PresentationSlide,
    title_template: Option<&rv_data::PresentationSlide>,
    title_segments: Option<&[StyledSegment]>,
    content_segments: &[Vec<StyledSegment>],
) -> Option<rv_data::Presentation> {
    assemble_presentation_with_title_template_and_roles(
        name,
        content_template,
        title_template,
        title_segments,
        content_segments,
    )
    .map(|rendered| rendered.presentation)
}

/// Assemble a presentation while preserving its actual title/content region
/// entries for later macro placement.
pub(crate) fn assemble_presentation_with_title_template_and_roles(
    name: &str,
    content_template: &rv_data::PresentationSlide,
    title_template: Option<&rv_data::PresentationSlide>,
    title_segments: Option<&[StyledSegment]>,
    content_segments: &[Vec<StyledSegment>],
) -> Option<RenderedPresentation> {
    let mut presentation = presentation_with_native_envelope(name);

    let mut cue_uuids = Vec::new();
    let mut cue_roles = RenderedCueRoles::default();

    if let Some(segments) = title_segments {
        cue_roles.record_title(presentation.cues.len());
        push_presentation_cue(
            &mut presentation,
            &mut cue_uuids,
            title_template.unwrap_or(content_template),
            segments,
            None,
        );
    }

    if !content_segments.is_empty() {
        cue_roles.record_content(presentation.cues.len());
    }
    for segments in content_segments {
        push_presentation_cue(
            &mut presentation,
            &mut cue_uuids,
            content_template,
            segments,
            None,
        );
    }

    if cue_uuids.is_empty() {
        return None;
    }

    push_cue_group(&mut presentation, &cue_uuids);

    Some(RenderedPresentation {
        presentation,
        cue_roles,
    })
}

fn push_presentation_cue(
    presentation: &mut rv_data::Presentation,
    cue_uuids: &mut Vec<uuid::Uuid>,
    template_slide: &rv_data::PresentationSlide,
    segments: &[StyledSegment],
    label: Option<&str>,
) {
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
            label: label.map(|text| rv_data::action::Label {
                text: text.to_string(),
                color: None,
            }),
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

fn push_scripture_cue(
    presentation: &mut rv_data::Presentation,
    cue_uuids: &mut Vec<uuid::Uuid>,
    template_slide: &rv_data::PresentationSlide,
    scripture_slide: &ScriptureSlide,
    reference_prefix: Option<&str>,
) {
    let verse_range = scripture_slide.label();
    let label = reference_prefix.map_or_else(
        || verse_range.clone(),
        |prefix| format!("{prefix}{verse_range}"),
    );
    push_presentation_cue(
        presentation,
        cue_uuids,
        template_slide,
        &[StyledSegment::unstyled(scripture_slide.text())],
        Some(&label),
    );
}

fn push_cue_group(presentation: &mut rv_data::Presentation, cue_uuids: &[uuid::Uuid]) {
    let group_uuid = uuid::Uuid::new_v4();
    let group = rv_data::presentation::CueGroup {
        group: Some(rv_data::Group {
            uuid: Some(rv_data::Uuid {
                string: group_uuid.to_string(),
            }),
            name: String::new(),
            color: None,
            hot_key: Some(rv_data::HotKey::default()),
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
    build_scripture_presentation_dual_template(
        name,
        template_slide,
        template_slide,
        verses,
        title_text,
    )
}

/// Build a scripture presentation with a separate title slide template.
///
/// The title slide uses `title_template` (e.g., Information/Projectors) while
/// content slides use `content_template` (e.g., Scripture/Projectors). This
/// supports having a different visual style for the title vs verses.
pub fn build_scripture_presentation_dual_template(
    name: &str,
    title_template: &rv_data::PresentationSlide,
    content_template: &rv_data::PresentationSlide,
    verses: &[crate::bible::Verse],
    title_text: Option<&str>,
) -> Option<rv_data::Presentation> {
    build_scripture_presentation_dual_template_with_roles(
        name,
        title_template,
        content_template,
        verses,
        title_text,
        None,
    )
    .map(|rendered| rendered.presentation)
}

/// Build a scripture presentation and preserve its actual cue-role entries.
/// An explicit line limit overrides the content template's derived capacity.
pub(crate) fn build_scripture_presentation_dual_template_with_roles(
    name: &str,
    title_template: &rv_data::PresentationSlide,
    content_template: &rv_data::PresentationSlide,
    verses: &[crate::bible::Verse],
    title_text: Option<&str>,
    max_lines_override: Option<usize>,
) -> Option<RenderedPresentation> {
    let (wrap_col, max_lines) = extract_slide_metrics(content_template)
        .map_or((45, DEFAULT_MAX_LINES_PER_SLIDE), |m| {
            (m.chars_per_line, m.max_lines)
        });
    let max_lines = max_lines_override.unwrap_or(max_lines);

    let scripture_slides = split_verses_for_slides(verses, wrap_col, max_lines);
    let reference_prefix = title_text
        .and_then(scripture_label_prefix)
        .or_else(|| scripture_label_prefix(name));
    let mut presentation = presentation_with_native_envelope(name);
    let mut cue_uuids = Vec::new();
    let mut cue_roles = RenderedCueRoles::default();

    if let Some(title) = title_text.filter(|title| !title.is_empty()) {
        cue_roles.record_title(presentation.cues.len());
        push_presentation_cue(
            &mut presentation,
            &mut cue_uuids,
            title_template,
            &[StyledSegment::unstyled(title)],
            None,
        );
    }

    if !scripture_slides.is_empty() {
        cue_roles.record_content(presentation.cues.len());
    }
    for scripture_slide in &scripture_slides {
        push_scripture_cue(
            &mut presentation,
            &mut cue_uuids,
            content_template,
            scripture_slide,
            reference_prefix.as_deref(),
        );
    }

    if cue_uuids.is_empty() {
        return None;
    }
    push_cue_group(&mut presentation, &cue_uuids);

    Some(RenderedPresentation {
        presentation,
        cue_roles,
    })
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
    build_combined_scripture_presentation_dual_template(
        name,
        template_slide,
        template_slide,
        passages,
    )
}

/// Build a combined scripture presentation with separate title/content
/// templates for each passage.
pub fn build_combined_scripture_presentation_dual_template(
    name: &str,
    title_template: &rv_data::PresentationSlide,
    content_template: &rv_data::PresentationSlide,
    passages: &[ScripturePassage],
) -> Option<rv_data::Presentation> {
    build_combined_scripture_presentation_dual_template_with_roles(
        name,
        title_template,
        content_template,
        passages,
        None,
    )
    .map(|rendered| rendered.presentation)
}

/// Build combined scripture and preserve every passage's actual title/content
/// region entries. Divider cues deliberately have no semantic role.
pub(crate) fn build_combined_scripture_presentation_dual_template_with_roles(
    name: &str,
    title_template: &rv_data::PresentationSlide,
    content_template: &rv_data::PresentationSlide,
    passages: &[ScripturePassage],
    max_lines_override: Option<usize>,
) -> Option<RenderedPresentation> {
    if passages.is_empty() {
        return None;
    }

    let (wrap_col, max_lines) = extract_slide_metrics(content_template)
        .map_or((45, DEFAULT_MAX_LINES_PER_SLIDE), |m| {
            (m.chars_per_line, m.max_lines)
        });
    let max_lines = max_lines_override.unwrap_or(max_lines);

    let mut presentation = presentation_with_native_envelope(name);

    let mut cue_uuids = Vec::new();
    let mut cue_roles = RenderedCueRoles::default();

    for (i, passage) in passages.iter().enumerate() {
        if !passage.title.is_empty() {
            cue_roles.record_title(presentation.cues.len());
            push_presentation_cue(
                &mut presentation,
                &mut cue_uuids,
                title_template,
                &[StyledSegment::unstyled(&passage.title)],
                None,
            );
        }

        let scripture_slides = split_verses_for_slides(&passage.verses, wrap_col, max_lines);
        let reference_prefix = scripture_label_prefix(&passage.title);
        if !scripture_slides.is_empty() {
            cue_roles.record_content(presentation.cues.len());
        }
        for scripture_slide in &scripture_slides {
            push_scripture_cue(
                &mut presentation,
                &mut cue_uuids,
                content_template,
                scripture_slide,
                reference_prefix.as_deref(),
            );
        }

        if i + 1 < passages.len() {
            push_presentation_cue(
                &mut presentation,
                &mut cue_uuids,
                content_template,
                &[StyledSegment::unstyled("")],
                None,
            );
        }
    }

    if cue_uuids.is_empty() {
        return None;
    }

    push_cue_group(&mut presentation, &cue_uuids);

    Some(RenderedPresentation {
        presentation,
        cue_roles,
    })
}

fn scripture_label_prefix(reference_text: &str) -> Option<String> {
    let reference = crate::bible::parse_scripture_ref(reference_text)?;
    Some(format!("{} {}:", reference.book, reference.chapter))
}

/// Split verses into slide-sized chunks, preferring verse boundaries.
///
/// Each verse is prepended with its superscript number and greedily packed.
/// On overflow, a punctuation boundary in the final visual line is preferred;
/// otherwise the latest fitting word boundary is mandatory. Every returned
/// slide therefore fits the estimated line capacity, including continuations
/// of a single long verse.
///
/// `ProPresenter` handles text wrapping within the text box, so slide text is
/// emitted as continuous runs without embedded line breaks. `word_wrap` is
/// used only to *estimate* visual line counts for deciding slide splits.
pub fn split_verses_for_slides(
    verses: &[crate::bible::Verse],
    wrap_column: usize,
    max_lines: usize,
) -> Vec<ScriptureSlide> {
    let wrap_col = wrap_column.max(MIN_SLIDE_WRAP);
    let max = max_lines.max(1);

    let mut slides = Vec::new();
    let mut current: Option<ScriptureSlide> = None;

    for verse in verses {
        let number = crate::bible::to_superscript(verse.number);
        let verse_text = verse.text.trim();
        let mut pending = if verse_text.is_empty() {
            number
        } else {
            format!("{number} {verse_text}")
        };

        while !pending.is_empty() {
            if estimated_joined_lines(current.as_ref(), &pending, wrap_col) <= max {
                append_scripture_fragment(&mut current, &pending, verse.number);
                break;
            }

            if let Some((fragment, remainder)) =
                fitting_fragment(current.as_ref(), &pending, wrap_col, max)
            {
                append_scripture_fragment(&mut current, &fragment, verse.number);
                pending = remainder;
                if let Some(finished) = current.take() {
                    slides.push(finished);
                }
            } else if let Some(finished) = current.take() {
                // The current slide has no room for the next complete word.
                slides.push(finished);
            }
        }
    }

    if let Some(finished) = current {
        slides.push(finished);
    }
    slides
}

fn append_scripture_fragment(
    current: &mut Option<ScriptureSlide>,
    fragment: &str,
    verse_number: u32,
) {
    if let Some(slide) = current {
        slide.append_fragment(fragment, verse_number);
    } else {
        *current = Some(ScriptureSlide::from_fragment(
            fragment.to_string(),
            verse_number,
        ));
    }
}

fn estimated_joined_lines(
    current: Option<&ScriptureSlide>,
    fragment: &str,
    wrap_col: usize,
) -> usize {
    let candidate = current.map_or_else(
        || fragment.to_string(),
        |slide| format!("{} {fragment}", slide.text()),
    );
    word_wrap(&candidate, wrap_col).len()
}

/// Return the best fitting prefix and the unconsumed suffix.
///
/// The fullest word boundary defines the hard capacity. A punctuation break
/// is preferred only when it is on the same final visual line, preventing a
/// distant punctuation mark from leaving most of a slide empty.
fn fitting_fragment(
    current: Option<&ScriptureSlide>,
    pending: &str,
    wrap_col: usize,
    max_lines: usize,
) -> Option<(String, String)> {
    let boundaries = candidate_break_indices(pending);
    let mut fullest_end = None;
    let mut fullest_lines = 0;

    for &end in &boundaries {
        let prefix = pending[..end].trim_end();
        let lines = estimated_joined_lines(current, prefix, wrap_col);
        if lines > max_lines {
            break;
        }
        fullest_end = Some(end);
        fullest_lines = lines;
    }

    let fullest_end = fullest_end?;
    let chosen_end = boundaries
        .iter()
        .rev()
        .copied()
        .filter(|end| *end <= fullest_end)
        .find(|end| {
            let prefix = pending[..*end].trim_end();
            is_preferred_break(prefix)
                && estimated_joined_lines(current, prefix, wrap_col) == fullest_lines
                && pending[*end..fullest_end].width() <= wrap_col
        })
        .unwrap_or(fullest_end);

    let fragment = pending[..chosen_end].trim_end().to_string();
    let remainder = pending[chosen_end..].trim_start().to_string();
    Some((fragment, remainder))
}

fn candidate_break_indices(text: &str) -> Vec<usize> {
    let mut boundaries = Vec::new();
    let mut in_word = false;

    for (index, character) in text.char_indices() {
        if character.is_whitespace() {
            if in_word {
                push_unique_boundary(&mut boundaries, index);
                in_word = false;
            }
        } else {
            in_word = true;
            if is_preferred_break_character(character) {
                push_unique_boundary(&mut boundaries, index + character.len_utf8());
            }
        }
    }
    if in_word {
        push_unique_boundary(&mut boundaries, text.len());
    }
    boundaries
}

fn push_unique_boundary(boundaries: &mut Vec<usize>, boundary: usize) {
    if boundaries.last().copied() != Some(boundary) {
        boundaries.push(boundary);
    }
}

fn is_preferred_break(text: &str) -> bool {
    let without_closing_marks = text.trim_end_matches(['"', '\'', '’', '”', ')', ']', '}']);
    without_closing_marks
        .chars()
        .next_back()
        .is_some_and(is_preferred_break_character)
}

const fn is_preferred_break_character(character: char) -> bool {
    matches!(character, ';' | ',' | '.' | '?' | '!' | ':' | '—')
}

/// Build a presentation from a template slide with custom wrap/split options.
///
/// Takes a `PresentationSlide` directly from an installed theme.
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
    let mut presentation = presentation_with_native_envelope(name);

    let mut cue_uuids = Vec::new();

    let mut push_slide_cue = |presentation: &mut rv_data::Presentation,
                              segments: &[StyledSegment]| {
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
                hot_key: Some(rv_data::HotKey::default()),
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

    fn get_scripture_slide() -> rv_data::PresentationSlide {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("data/templates/__template_scripture__.pro");
        let data = std::fs::read(path).expect("scripture fixture should be readable");
        let presentation = rv_data::Presentation::decode(data.as_slice())
            .expect("scripture fixture should decode");
        extract_template_slide(&presentation).expect("scripture fixture should contain a slide")
    }

    fn expected_scripture_text(verses: &[crate::bible::Verse]) -> String {
        verses
            .iter()
            .map(|verse| {
                let number = crate::bible::to_superscript(verse.number);
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

    fn assert_scripture_slides(
        verses: &[crate::bible::Verse],
        slides: &[ScriptureSlide],
        wrap_column: usize,
        max_lines: usize,
    ) {
        let wrap_column = wrap_column.max(MIN_SLIDE_WRAP);
        let max_lines = max_lines.max(1);
        let rendered_text = slides.iter().map(ScriptureSlide::text).collect::<String>();
        assert_eq!(
            rendered_text
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect::<String>(),
            expected_scripture_text(verses)
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect::<String>(),
            "splitting must preserve every non-whitespace source character and its order"
        );
        assert!(slides.iter().all(|slide| !slide.text().is_empty()));
        for (index, slide) in slides.iter().enumerate() {
            let estimated_lines = word_wrap(slide.text(), wrap_column).len();
            assert!(
                estimated_lines <= max_lines,
                "slide {index} estimated {estimated_lines} lines at width {wrap_column}, limit {max_lines}: {:?}",
                slide.text()
            );
            assert!(!slide.verse_numbers().is_empty());
        }

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
            verses.iter().map(|verse| verse.number).collect::<Vec<_>>(),
            "slide provenance must retain source verse order"
        );
    }

    fn slide_action_labels(presentation: &rv_data::Presentation) -> Vec<Option<String>> {
        presentation
            .cues
            .iter()
            .map(|cue| {
                cue.actions
                    .iter()
                    .find(|action| {
                        matches!(
                            &action.action_type_data,
                            Some(rv_data::action::ActionTypeData::Slide(_))
                        )
                    })
                    .and_then(|action| action.label.as_ref())
                    .map(|label| label.text.clone())
            })
            .collect()
    }

    #[test]
    fn configured_missing_theme_is_an_error() {
        let name = format!("missing-theme-{}", uuid::Uuid::new_v4());

        assert!(matches!(
            ThemeCache::load(Some(&name)),
            Err(ThemeCacheLoadError::NotFound { .. })
        ));
    }

    #[test]
    fn configured_text_template_requires_one_text_destination() {
        let single = get_scripture_slide();
        let mut none = single.clone();
        none.base_slide
            .as_mut()
            .expect("base slide")
            .elements
            .clear();
        let mut multiple = single.clone();
        let duplicate = multiple.base_slide.as_ref().expect("base slide").elements[0].clone();
        multiple
            .base_slide
            .as_mut()
            .expect("base slide")
            .elements
            .push(duplicate);
        let cache = ThemeCache {
            theme_slides: HashMap::from([
                (
                    "single".to_string(),
                    CachedThemeSlide {
                        slide: single,
                        action_count: 0,
                    },
                ),
                (
                    "none".to_string(),
                    CachedThemeSlide {
                        slide: none,
                        action_count: 0,
                    },
                ),
                (
                    "multiple".to_string(),
                    CachedThemeSlide {
                        slide: multiple,
                        action_count: 0,
                    },
                ),
                (
                    "implicit_actions".to_string(),
                    CachedThemeSlide {
                        slide: get_scripture_slide(),
                        action_count: 1,
                    },
                ),
            ]),
            theme_name: Some("test".to_string()),
        };

        assert!(cache.text_template("single").is_ok());
        assert!(matches!(
            cache.text_template("none"),
            Err(ThemeSlideError::TextElementCount { count: 0, .. })
        ));
        assert!(matches!(
            cache.text_template("multiple"),
            Err(ThemeSlideError::TextElementCount { count: 2, .. })
        ));
        assert!(matches!(
            cache.text_template("implicit_actions"),
            Err(ThemeSlideError::EmbeddedActions { count: 1, .. })
        ));
    }

    #[test]
    fn fresh_render_has_native_document_envelope() {
        let template = get_scripture_slide();
        let presentation = assemble_presentation(
            "Native Envelope",
            &template,
            &[vec![StyledSegment::unstyled("Content")]],
        );

        assert!(matches!(
            presentation.background,
            Some(rv_data::Background {
                is_enabled: false,
                fill: None
            })
        ));
        assert!(presentation.chord_chart.is_some());
        assert!(presentation.ccli.is_some());
        assert!(presentation.cue_groups.iter().all(|group| group
            .group
            .as_ref()
            .is_some_and(|group| group.hot_key == Some(rv_data::HotKey::default()))));
        assert_eq!(
            presentation
                .timeline
                .as_ref()
                .map(|timeline| timeline.duration),
            Some(300.0)
        );
        assert_eq!(
            crate::propresenter::resolution::inspect_presentation_size(&presentation),
            crate::propresenter::PresentationSizeStatus::Uniform {
                size: crate::propresenter::PresentationSize::new(1920, 1080)
                    .expect("valid full HD size"),
            }
        );
    }

    #[test]
    fn preserving_document_envelope_keeps_metadata_but_drops_stale_timeline_cues() {
        let template = get_scripture_slide();
        let mut existing = assemble_presentation(
            "Existing",
            &template,
            &[vec![StyledSegment::unstyled("Old content")]],
        );
        existing.application_info = Some(rv_data::ApplicationInfo {
            platform: rv_data::application_info::Platform::Macos as i32,
            ..rv_data::ApplicationInfo::default()
        });
        existing.category = "Liturgy".to_string();
        existing.notes = "Preserve this".to_string();
        existing.timeline = Some(rv_data::presentation::Timeline {
            cues: vec![rv_data::presentation::timeline::Cue {
                trigger_time: 1.0,
                name: "stale cue".to_string(),
                trigger_info: None,
            }],
            duration: 42.0,
            ..rv_data::presentation::Timeline::default()
        });

        let mut edited = assemble_presentation_with_title_template(
            &existing.name,
            &template,
            None,
            None,
            &[vec![StyledSegment::unstyled("New content")]],
        )
        .expect("replacement should render");
        preserve_presentation_envelope(&mut edited, &existing);

        assert_eq!(edited.uuid, existing.uuid);
        assert_eq!(edited.application_info, existing.application_info);
        assert_eq!(edited.category, "Liturgy");
        assert_eq!(edited.notes, "Preserve this");
        let timeline = edited.timeline.expect("native timeline");
        assert!(timeline.cues.is_empty());
        assert_eq!(timeline.duration, 300.0);
    }

    #[test]
    fn duplicate_canonical_theme_slide_names_are_rejected() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("Theme");
        let document = rv_data::template::Document {
            application_info: None,
            slides: vec![
                rv_data::template::Slide {
                    base_slide: None,
                    name: "Scripture".to_string(),
                    actions: Vec::new(),
                },
                rv_data::template::Slide {
                    base_slide: None,
                    name: "scripture".to_string(),
                    actions: Vec::new(),
                },
            ],
        };
        std::fs::write(&path, document.encode_to_vec()).expect("write theme document");

        assert!(matches!(
            load_theme(&path),
            Err(ThemeCacheLoadError::DuplicateSlideName { .. })
        ));
    }

    #[test]
    fn test_extract_slide_metrics() {
        let slide = get_scripture_slide();
        let metrics = extract_slide_metrics(&slide);
        assert!(
            metrics.is_some(),
            "should extract metrics from scripture template"
        );

        let m = metrics.unwrap();
        assert!(m.font_size_pt > 0.0, "font size should be positive");
        assert!(m.chars_per_line > 0, "chars_per_line should be positive");
        assert!(m.max_lines > 0, "max_lines should be positive");
        assert!(m.text_width_pt > 0.0, "text width should be positive");
        assert!(m.text_height_pt > 0.0, "text height should be positive");
    }

    #[test]
    fn extract_slide_metrics_skips_non_text_elements() {
        let mut slide = get_scripture_slide();
        let base_slide = slide.base_slide.as_mut().expect("base slide");
        let mut decorative = base_slide.elements[0].clone();
        decorative.element.as_mut().expect("element").text = None;
        base_slide.elements.insert(0, decorative);

        assert!(extract_slide_metrics(&slide).is_some());
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

        assert!(
            slides.len() > 1,
            "long line should produce multiple slides, got {}",
            slides.len()
        );
        for (i, slide) in slides.iter().enumerate() {
            let line_count = slide.lines().count();
            assert!(
                line_count <= 3,
                "slide {i} has {line_count} lines, expected <= 3"
            );
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
    fn test_pack_segments_preserves_blank_responsive_lines() {
        let segments = vec![
            StyledSegment::unstyled("LEADER: First section."),
            StyledSegment::unstyled("ALL: Response one."),
            StyledSegment::unstyled(""),
            StyledSegment::unstyled("LEADER: Second section."),
            StyledSegment::unstyled("ALL: Response two."),
        ];

        let slides = pack_segments_for_slides(&segments, 80, 5);

        assert_eq!(slides.len(), 1);
        assert_eq!(slides[0].len(), 5);
        assert!(slides[0][2].text.is_empty());
    }

    #[test]
    fn test_pack_segments_does_not_leave_trailing_blank_on_overflow() {
        let segments = vec![
            StyledSegment::unstyled("LEADER: First section."),
            StyledSegment::unstyled("ALL: Response one."),
            StyledSegment::unstyled(""),
            StyledSegment::unstyled("LEADER: Second section."),
            StyledSegment::unstyled("ALL: Response two."),
        ];

        let slides = pack_segments_for_slides(&segments, 80, 3);

        assert_eq!(slides.len(), 2);
        assert!(!slides[0].last().unwrap().text.is_empty());
        assert_eq!(slides[1][0].text, "LEADER: Second section.");
    }

    #[test]
    fn test_cue_text_segments_drops_extracted_blank_paragraphs() {
        let slide = get_scripture_slide();
        let presentation = assemble_presentation_with_title_template(
            "Test",
            &slide,
            None,
            None,
            &[vec![
                StyledSegment::unstyled("Line one"),
                StyledSegment::unstyled(""),
                StyledSegment::unstyled("Line two"),
            ]],
        )
        .expect("presentation should build");

        let segments = cue_text_segments(&presentation.cues[0]);
        let text = segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(text, vec!["Line one", "Line two"]);
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
        assert!(!pres.cues.is_empty(), "should produce at least one slide");
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

        assert_scripture_slides(&verses, &slides, 40, 3);

        let all_text = slides
            .iter()
            .map(ScriptureSlide::text)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(all_text.contains("wilderness"), "verse 1 content missing");
        assert!(
            all_text.contains("blossom abundantly"),
            "verse 2 content missing"
        );
        assert!(all_text.contains("Strengthen"), "verse 3 content missing");
        assert!(all_text.contains("Be strong"), "verse 4 content missing");
        assert!(
            all_text.contains("blind shall be"),
            "verse 5 content missing"
        );

        // Verify verse numbers are present as superscripts
        assert!(all_text.contains('¹'), "superscript 1 missing");
        assert!(all_text.contains('⁵'), "superscript 5 missing");
    }

    #[test]
    fn test_split_verses_packs_adjacent_luke_verses_when_they_fit() {
        let verses = vec![
            crate::bible::Verse {
                number: 26,
                text: "Then they arrived at the region of the Gerasenes, which is opposite Galilee."
                    .to_string(),
            },
            crate::bible::Verse {
                number: 27,
                text: "As he stepped out on shore, a man from the city who had demons met him. For a long time he had not worn any clothes, and he did not live in a house but in the tombs."
                    .to_string(),
            },
            crate::bible::Verse {
                number: 28,
                text: "When he saw Jesus, he cried out and fell down before him, shouting, What have you to do with me, Jesus, Son of the Most High God? I beg you, do not torment me."
                    .to_string(),
            },
        ];

        let slides = split_verses_for_slides(&verses, 39, 8);

        assert!(
            slides[0].text().contains(&crate::bible::to_superscript(26)),
            "first slide should include verse 26"
        );
        assert!(
            slides[0].text().contains(&crate::bible::to_superscript(27)),
            "first slide should include verse 27"
        );
        assert!(
            word_wrap(slides[0].text(), 39).len() <= 8,
            "first slide should fit the estimated line budget"
        );
        assert_eq!(slides[0].verse_numbers(), &[26, 27, 28]);
        assert_eq!(slides[0].label(), "26-28");
        assert_scripture_slides(&verses, &slides, 39, 8);
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

        assert_scripture_slides(&verses, &slides, 40, 3);

        // The superscript verse number should only appear on the first slide
        assert!(
            slides[0].text().contains('²'),
            "first slide should have verse number"
        );
        assert!(slides.iter().all(|slide| slide.label() == "28"));

        // Content should be preserved across all slides
        let all_text = slides
            .iter()
            .map(ScriptureSlide::text)
            .collect::<Vec<_>>()
            .join(" ");
        assert!(all_text.contains("foreknew"), "content should be preserved");
        assert!(
            all_text.contains("firstborn"),
            "content should be preserved"
        );
    }

    #[test]
    fn scripture_split_prefers_supported_punctuation_on_the_last_visual_line() {
        for punctuation in [';', ',', '.', '?', '!', ':', '—'] {
            let verses = vec![crate::bible::Verse {
                number: 1,
                text: format!("Alpha beta{punctuation}gamma delta epsilon zeta eta theta"),
            }];

            let slides = split_verses_for_slides(&verses, 20, 1);

            assert!(slides.len() > 1);
            assert!(
                slides[0].text().ends_with(punctuation),
                "expected {punctuation:?} boundary, got {:?}",
                slides[0].text()
            );
            assert_scripture_slides(&verses, &slides, 20, 1);
        }
    }

    #[test]
    fn scripture_split_falls_back_to_the_latest_fitting_word_boundary() {
        let verses = vec![crate::bible::Verse {
            number: 7,
            text: "alpha beta gamma delta epsilon zeta eta theta iota kappa".to_string(),
        }];

        let slides = split_verses_for_slides(&verses, 20, 1);

        assert!(slides.len() > 1);
        assert!(slides[0].text().ends_with("gamma"));
        assert_scripture_slides(&verses, &slides, 20, 1);
    }

    #[test]
    fn scripture_split_preserves_bounds_content_and_provenance_across_capacities() {
        let verses = vec![
            crate::bible::Verse {
                number: 8,
                text: "Then came a sentence, with a comma; then a clause: and a question? Yes!"
                    .to_string(),
            },
            crate::bible::Verse {
                number: 9,
                text: "This intentionally long continuation has no punctuation and therefore exercises the mandatory word boundary fallback repeatedly across several different capacities"
                    .to_string(),
            },
            crate::bible::Verse {
                number: 10,
                text: "The final verse—kept in order—ends here.".to_string(),
            },
        ];

        for wrap_column in [20, 21, 32, 45, 64] {
            for max_lines in 1..=5 {
                let slides = split_verses_for_slides(&verses, wrap_column, max_lines);
                assert_scripture_slides(&verses, &slides, wrap_column, max_lines);
            }
        }
    }

    #[test]
    fn scripture_actions_receive_native_verse_range_labels() {
        let slide = get_scripture_slide();
        let verses = (4..=6)
            .map(|number| crate::bible::Verse {
                number,
                text: format!("Short verse {number}."),
            })
            .collect::<Vec<_>>();

        let rendered = build_scripture_presentation_dual_template_with_roles(
            "Ephesians 4v4-6 NRSVue",
            &slide,
            &slide,
            &verses,
            Some("Scripture\nEphesians 4:4-6 NRSVue"),
            Some(10),
        )
        .expect("scripture should render");

        assert_eq!(
            slide_action_labels(&rendered.presentation),
            vec![None, Some("Ephesians 4:4-6".to_string())]
        );
        let content_action = &rendered.presentation.cues[1].actions[0];
        assert!(content_action
            .label
            .as_ref()
            .is_some_and(|label| label.color.is_none()));
    }

    #[test]
    fn long_verse_continuations_keep_the_same_native_label() {
        let slide = get_scripture_slide();
        let verses = vec![crate::bible::Verse {
            number: 12,
            text: std::iter::repeat_n("unpunctuated", 30)
                .collect::<Vec<_>>()
                .join(" "),
        }];

        let rendered = build_scripture_presentation_dual_template_with_roles(
            "John 3v12 NRSVue",
            &slide,
            &slide,
            &verses,
            None,
            Some(1),
        )
        .expect("scripture should render");
        let labels = slide_action_labels(&rendered.presentation);

        assert!(labels.len() > 1);
        assert!(labels
            .iter()
            .all(|label| label.as_deref() == Some("John 3:12")));
    }

    #[test]
    fn verse_labels_preserve_non_contiguous_source_ranges() {
        assert_eq!(format_verse_ranges(&[1, 2, 3, 5, 7, 8]), "1-3, 5, 7-8");
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
                text: "The earth was formless and empty, and darkness covered the deep waters."
                    .to_string(),
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
        assert!(
            p.cues.len() >= 2,
            "expected title + content, got {}",
            p.cues.len()
        );
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
        assert_eq!(
            slide_action_labels(&p),
            vec![
                None,
                Some("Isaiah 35:1-2".to_string()),
                None,
                None,
                Some("Luke 2:1-2".to_string()),
            ]
        );
    }

    #[test]
    fn combined_scripture_records_each_rendered_role_transition() {
        let slide = get_scripture_slide();
        let passages = vec![
            ScripturePassage {
                title: "First reading".to_string(),
                verses: vec![crate::bible::Verse {
                    number: 1,
                    text: "First passage.".to_string(),
                }],
            },
            ScripturePassage {
                title: "Second reading".to_string(),
                verses: vec![crate::bible::Verse {
                    number: 1,
                    text: "Second passage.".to_string(),
                }],
            },
        ];

        let rendered = build_combined_scripture_presentation_dual_template_with_roles(
            "Readings",
            &slide,
            &slide,
            &passages,
            Some(10),
        )
        .expect("combined scripture should render");

        // title, content, divider, title, content
        assert_eq!(rendered.presentation.cues.len(), 5);
        assert_eq!(rendered.cue_roles.title_entries(), &[0, 3]);
        assert_eq!(rendered.cue_roles.content_entries(), &[1, 4]);
    }

    #[test]
    fn split_scripture_without_title_starts_content_at_first_cue() {
        let title_slide = get_scripture_slide();
        let content_slide = get_scripture_slide();
        let verses = vec![crate::bible::Verse {
            number: 1,
            text: "The reading begins without a title cue.".to_string(),
        }];

        let rendered = build_scripture_presentation_dual_template_with_roles(
            "Reading",
            &title_slide,
            &content_slide,
            &verses,
            None,
            Some(10),
        )
        .expect("scripture should render");

        assert!(rendered.cue_roles.title_entries().is_empty());
        assert_eq!(rendered.cue_roles.content_entries(), &[0]);
        assert_eq!(rendered.cue_roles.first_entry(), Some(0));
    }

    #[test]
    fn scripture_line_override_controls_single_passage_packing() {
        let slide = get_scripture_slide();
        let verses = (1..=3)
            .map(|number| crate::bible::Verse {
                number,
                text: format!("Short verse {number}."),
            })
            .collect::<Vec<_>>();

        let limited = build_scripture_presentation_dual_template_with_roles(
            "Reading",
            &slide,
            &slide,
            &verses,
            None,
            Some(1),
        )
        .expect("limited scripture should render");
        let compact = build_scripture_presentation_dual_template_with_roles(
            "Reading",
            &slide,
            &slide,
            &verses,
            None,
            Some(10),
        )
        .expect("compact scripture should render");

        assert!(limited.presentation.cues.len() > compact.presentation.cues.len());
        assert_eq!(compact.presentation.cues.len(), 1);
    }

    #[test]
    fn scripture_line_override_controls_combined_passage_packing() {
        let slide = get_scripture_slide();
        let passage = |title: &str| {
            ScripturePassage {
            title: title.to_string(),
            verses: (1..=2)
                .map(|number| crate::bible::Verse {
                    number,
                    text: format!(
                        "This is a deliberately longer verse {number} that proves the line override controls packing."
                    ),
                })
                .collect(),
        }
        };
        let passages = vec![passage("First reading"), passage("Second reading")];

        let limited = build_combined_scripture_presentation_dual_template_with_roles(
            "Readings",
            &slide,
            &slide,
            &passages,
            Some(1),
        )
        .expect("limited combined scripture should render");
        let compact = build_combined_scripture_presentation_dual_template_with_roles(
            "Readings",
            &slide,
            &slide,
            &passages,
            Some(10),
        )
        .expect("compact combined scripture should render");

        assert!(limited.presentation.cues.len() > compact.presentation.cues.len());
        assert_eq!(compact.presentation.cues.len(), 5);
    }
}
