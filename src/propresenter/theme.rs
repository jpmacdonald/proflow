//! Installed ProPresenter theme loading and text-box geometry.
//!
//! A theme owns native visual templates. Semantic field selection stays in the
//! renderer, and geometry is read from the exact field selected by that role.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use prost::Message;

use super::generated::rv_data;
use super::media::{presentation_slide_media_dependencies, MediaDependency};
use super::presentation_spec::TextField;
use super::render::{ResolvedCueRole, SlideTemplate, TemplateSlotError};
use super::rtf::extract_text_options;

/// Default maximum visual lines when a template has no usable geometry.
pub(crate) const DEFAULT_MAX_LINES_PER_SLIDE: usize = 10;

const CHAR_WIDTH_RATIO: f64 = 0.50;
const DEFAULT_LINE_HEIGHT_MULTIPLE: f64 = 1.2;
const RTF_KERNING_UNITS_PER_POINT: f64 = 4.0;

/// Text-box capacity derived from one exact native text element.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SlideMetrics {
    pub(crate) chars_per_line: usize,
    pub(crate) max_lines: usize,
}

/// Read layout capacity from the native element bound to `field`.
///
/// Role resolution has already made implicit body selection unambiguous or
/// bound an explicit semantic field to an exact native element. Reusing that
/// binding here prevents unrelated title, footer, or helper elements from
/// controlling content splitting.
pub(crate) fn extract_role_metrics(
    role: &ResolvedCueRole<'_>,
    field: &TextField,
) -> Result<Option<SlideMetrics>, TemplateSlotError> {
    let index = role.field_index(field)?;
    let graphics = role
        .slide()
        .base_slide
        .as_ref()
        .and_then(|slide| slide.elements.get(index))
        .and_then(|element| element.element.as_ref())
        .ok_or(TemplateSlotError::InvalidNativeSlot { index })?;
    let text = graphics
        .text
        .as_ref()
        .ok_or(TemplateSlotError::InvalidNativeSlot { index })?;
    Ok(metrics_from_text_element(graphics, text))
}

fn metrics_from_text_element(
    graphics: &rv_data::graphics::Element,
    text: &rv_data::graphics::Text,
) -> Option<SlideMetrics> {
    let size = graphics.bounds.as_ref()?.size.as_ref()?;
    let (left, right, top, bottom) = text
        .margins
        .as_ref()
        .map_or((0.0, 0.0, 0.0, 0.0), |margins| {
            (margins.left, margins.right, margins.top, margins.bottom)
        });
    let width = size.width - left - right;
    let height = size.height - top - bottom;
    if width <= 0.0 || height <= 0.0 {
        return None;
    }

    let font_size = resolve_font_size(text);
    if font_size <= 0.0 {
        return None;
    }
    let character_width = font_size.mul_add(CHAR_WIDTH_RATIO, resolve_character_spacing(text));
    if character_width <= 0.0 {
        return None;
    }
    let line_height = resolve_line_height(text, font_size);
    if line_height <= 0.0 {
        return None;
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let chars_per_line = (width / character_width).floor() as usize;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let max_lines = ((height / line_height).floor() as usize).max(1);
    (chars_per_line > 0).then_some(SlideMetrics {
        chars_per_line,
        max_lines,
    })
}

fn resolve_character_spacing(text: &rv_data::graphics::Text) -> f64 {
    text.attributes.as_ref().map_or_else(
        || f64::from(extract_text_options(text).kerning) / RTF_KERNING_UNITS_PER_POINT,
        |attributes| attributes.kerning,
    )
}

fn resolve_font_size(text: &rv_data::graphics::Text) -> f64 {
    if let Some(size) = text
        .attributes
        .as_ref()
        .and_then(|attributes| attributes.font.as_ref())
        .map(|font| font.size)
        .filter(|size| size.is_finite() && *size > 0.0)
    {
        return size;
    }
    f64::from(extract_text_options(text).font_size)
}

fn resolve_line_height(text: &rv_data::graphics::Text, font_size: f64) -> f64 {
    let Some(paragraph) = text
        .attributes
        .as_ref()
        .and_then(|attributes| attributes.paragraph_style.as_ref())
    else {
        return font_size * DEFAULT_LINE_HEIGHT_MULTIPLE;
    };

    let base = if paragraph.line_height_multiple > 0.0 {
        paragraph.line_height_multiple * font_size
    } else {
        font_size
    };
    let raw = base + paragraph.line_spacing;
    let minimum = if paragraph.minimum_line_height > 1.0 {
        raw.max(paragraph.minimum_line_height)
    } else {
        raw
    };
    if paragraph.maximum_line_height > 0.0 {
        minimum.min(paragraph.maximum_line_height)
    } else {
        minimum
    }
}

/// Cached native slide templates loaded from one installed theme.
pub struct ThemeCache {
    theme_slides: HashMap<String, CachedThemeSlide>,
    theme_name: Option<String>,
}

struct CachedThemeSlide {
    slide: rv_data::PresentationSlide,
    action_count: usize,
}

/// Read-only facts needed to configure one installed theme slide without
/// guessing native element names or opening the protobuf directly.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ThemeSlideFacts {
    pub(crate) name: String,
    pub(crate) canvas_size: Option<ThemeSlideCanvas>,
    pub(crate) named_text_slots: Vec<String>,
    pub(crate) default_text_slot_candidates: usize,
    pub(crate) embedded_action_count: usize,
    pub(crate) generation_issues: Vec<String>,
}

/// Native canvas dimensions reported by an installed theme slide.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ThemeSlideCanvas {
    pub(crate) width: f64,
    pub(crate) height: f64,
}

/// Failure to load a configured `ProPresenter` theme.
#[derive(Debug, thiserror::Error)]
pub enum ThemeCacheLoadError {
    /// No document with the configured name exists in the explicit theme root.
    #[error("theme '{name}' was not found in: {searched:?}")]
    NotFound {
        /// Configured theme name.
        name: String,
        /// Candidate paths that were checked.
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
    /// The theme document was not valid protobuf data.
    #[error("failed to decode theme at {path}: {source}")]
    Decode {
        /// Theme document path.
        path: PathBuf,
        /// Protobuf failure.
        source: prost::DecodeError,
    },
    /// A valid theme contained no named slides.
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

/// Failure to resolve one configured theme slide for generated text.
#[derive(Debug, thiserror::Error)]
pub enum ThemeSlideError {
    /// The configured slide is absent from the loaded theme.
    #[error("theme slide '{name}' was not found")]
    NotFound {
        /// Exact configured slide name.
        name: String,
    },
    /// An implicit body does not have exactly one meaningful destination.
    #[error("theme slide '{name}' has {count} text elements; exactly one is required")]
    TextElementCount {
        /// Exact configured slide name.
        name: String,
        /// Number of candidate text elements.
        count: usize,
    },
    /// Theme-level actions cannot be represented without making behavior implicit.
    #[error(
        "theme slide '{name}' has {count} embedded actions; cue-role actions must be explicit"
    )]
    EmbeddedActions {
        /// Exact configured slide name.
        name: String,
        /// Number of attached theme actions.
        count: usize,
    },
    /// Named native text fields are duplicated or otherwise invalid.
    #[error("theme slide '{name}' has invalid text slots: {source}")]
    InvalidTextSlots {
        /// Exact configured slide name.
        name: String,
        /// Native template-field failure.
        source: TemplateSlotError,
    },
}

impl ThemeCache {
    /// Load one theme from an explicitly owned `ProPresenter/Themes` directory.
    pub fn load_from_dir(
        theme_name: Option<&str>,
        themes_dir: &Path,
    ) -> Result<Self, ThemeCacheLoadError> {
        let Some(name) = theme_name else {
            return Ok(Self::empty());
        };
        let searched = vec![themes_dir.join(name).join("Theme")];
        let path = searched
            .iter()
            .find(|candidate| candidate.is_file())
            .cloned()
            .ok_or_else(|| ThemeCacheLoadError::NotFound {
                name: name.to_string(),
                searched,
            })?;
        let theme_slides = load_theme(&path)?;
        if theme_slides.is_empty() {
            return Err(ThemeCacheLoadError::Empty {
                name: name.to_string(),
                path,
            });
        }
        Ok(Self {
            theme_slides,
            theme_name: Some(name.to_string()),
        })
    }

    /// Create a cache with no configured theme.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            theme_slides: HashMap::new(),
            theme_name: None,
        }
    }

    /// Resolve a slide whose implicit `body` has exactly one destination.
    pub fn text_template(
        &self,
        slide_name: &str,
    ) -> Result<&rv_data::PresentationSlide, ThemeSlideError> {
        let template = self.slide_template(slide_name)?;
        let count = template.default_candidate_count();
        if count != 1 {
            return Err(ThemeSlideError::TextElementCount {
                name: slide_name.to_string(),
                count,
            });
        }
        Ok(template.slide())
    }

    /// Resolve a theme slide with uniquely named native fields exposed.
    pub fn slide_template(&self, slide_name: &str) -> Result<SlideTemplate<'_>, ThemeSlideError> {
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
        SlideTemplate::inspect(&cached.slide).map_err(|source| ThemeSlideError::InvalidTextSlots {
            name: slide_name.to_string(),
            source,
        })
    }

    /// Return media files inherited by cues rendered from one configured slide.
    pub(crate) fn slide_media_dependencies(
        &self,
        slide_name: &str,
    ) -> Result<Vec<MediaDependency>, ThemeSlideError> {
        let template = self.slide_template(slide_name)?;
        Ok(presentation_slide_media_dependencies(template.slide()))
    }

    /// Return the configured theme name.
    #[must_use]
    pub fn theme_name(&self) -> Option<&str> {
        self.theme_name.as_deref()
    }

    /// Return loaded slide names in deterministic order.
    #[must_use]
    pub fn theme_slide_names(&self) -> Vec<&str> {
        let mut names = self
            .theme_slides
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        names.sort_unstable();
        names
    }

    /// Return deterministic discovery facts for every installed theme slide.
    ///
    /// Discovery deliberately reports unusable slides with explicit issues;
    /// onboarding needs to see why a native template cannot be selected rather
    /// than having that template disappear from the catalog.
    pub(crate) fn theme_slide_facts(&self) -> Vec<ThemeSlideFacts> {
        let mut facts = self
            .theme_slides
            .iter()
            .map(|(name, cached)| {
                let mut generation_issues = Vec::new();
                if cached.action_count != 0 {
                    generation_issues.push(format!(
                        "contains {} embedded actions; cue-role actions must be explicit",
                        cached.action_count
                    ));
                }
                let (named_text_slots, default_text_slot_candidates) =
                    match SlideTemplate::inspect(&cached.slide) {
                        Ok(template) => (
                            template.named_slots().map(str::to_string).collect(),
                            template.default_candidate_count(),
                        ),
                        Err(error) => {
                            generation_issues.push(error.to_string());
                            (Vec::new(), 0)
                        }
                    };
                let canvas_size = cached
                    .slide
                    .base_slide
                    .as_ref()
                    .and_then(|slide| slide.size.as_ref())
                    .map(|size| ThemeSlideCanvas {
                        width: size.width,
                        height: size.height,
                    });
                ThemeSlideFacts {
                    name: name.clone(),
                    canvas_size,
                    named_text_slots,
                    default_text_slot_candidates,
                    embedded_action_count: cached.action_count,
                    generation_issues,
                }
            })
            .collect::<Vec<_>>();
        facts.sort_by(|first, second| first.name.cmp(&second.name));
        facts
    }
}

fn load_theme(path: &Path) -> Result<HashMap<String, CachedThemeSlide>, ThemeCacheLoadError> {
    let data = std::fs::read(path).map_err(|source| ThemeCacheLoadError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let document = rv_data::template::Document::decode(data.as_slice()).map_err(|source| {
        ThemeCacheLoadError::Decode {
            path: path.to_path_buf(),
            source,
        }
    })?;
    let mut slides = HashMap::new();
    let mut canonical_names = HashMap::<String, String>::new();
    for template in document.slides {
        if template.name.is_empty() {
            continue;
        }
        let canonical = template.name.to_lowercase();
        if let Some(first) = canonical_names.insert(canonical, template.name.clone()) {
            return Err(ThemeCacheLoadError::DuplicateSlideName {
                path: path.to_path_buf(),
                first,
                duplicate: template.name,
            });
        }
        slides.insert(
            template.name,
            CachedThemeSlide {
                slide: rv_data::PresentationSlide {
                    base_slide: template.base_slide,
                    ..rv_data::PresentationSlide::default()
                },
                action_count: template.actions.len(),
            },
        );
    }
    Ok(slides)
}

#[cfg(test)]
#[path = "theme/tests.rs"]
mod tests;
