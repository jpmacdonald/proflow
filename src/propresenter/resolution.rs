//! Checked presentation-canvas dimensions shared by config, indexing, and build validation.

use serde::{Deserialize, Deserializer, Serialize};

use super::generated::rv_data;

/// Positive slide-canvas dimensions expected by a `ProPresenter` project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PresentationSize {
    width: u32,
    height: u32,
}

impl PresentationSize {
    /// Standard project canvas used when a config omits its entire defaults block.
    pub const FULL_HD: Self = Self {
        width: 1920,
        height: 1080,
    };

    /// Build checked positive dimensions.
    pub const fn new(width: u32, height: u32) -> Result<Self, PresentationSizeError> {
        if width == 0 {
            return Err(PresentationSizeError::ZeroWidth);
        }
        if height == 0 {
            return Err(PresentationSizeError::ZeroHeight);
        }
        Ok(Self { width, height })
    }

    /// Canvas width in pixels.
    #[must_use]
    pub const fn width(self) -> u32 {
        self.width
    }

    /// Canvas height in pixels.
    #[must_use]
    pub const fn height(self) -> u32 {
        self.height
    }
}

impl Default for PresentationSize {
    fn default() -> Self {
        Self::FULL_HD
    }
}

impl<'de> Deserialize<'de> for PresentationSize {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireSize {
            width: u32,
            height: u32,
        }

        let wire = WireSize::deserialize(deserializer)?;
        Self::new(wire.width, wire.height).map_err(serde::de::Error::custom)
    }
}

impl std::fmt::Display for PresentationSize {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}x{}", self.width, self.height)
    }
}

/// Invalid project or native presentation dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PresentationSizeError {
    /// Configured width was zero.
    #[error("presentation width must be greater than zero")]
    ZeroWidth,
    /// Configured height was zero.
    #[error("presentation height must be greater than zero")]
    ZeroHeight,
    /// A native slide had no canvas dimensions.
    #[error("slide has no canvas size")]
    Missing,
    /// Native dimensions were non-finite, fractional, non-positive, or too large.
    #[error("slide has invalid canvas dimensions")]
    Invalid,
}

/// Canvas-size state materialized across a presentation's native slide actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PresentationSizeStatus {
    /// The document contains no native presentation slide actions.
    Empty,
    /// Every slide has one shared size.
    Uniform {
        /// Shared canvas size.
        size: PresentationSize,
    },
    /// A slide has no canvas size.
    Missing {
        /// Zero-based presentation-slide index.
        slide_index: usize,
    },
    /// A slide has unusable native dimensions.
    Invalid {
        /// Zero-based presentation-slide index.
        slide_index: usize,
    },
    /// A later slide conflicts with the first slide's size.
    Mixed {
        /// First slide's size.
        first: PresentationSize,
        /// First conflicting size.
        conflicting: PresentationSize,
        /// Zero-based presentation-slide index of the conflict.
        slide_index: usize,
    },
}

impl PresentationSizeStatus {
    /// Whether every indexed slide has the expected canvas size.
    #[must_use]
    pub fn matches(self, expected: PresentationSize) -> bool {
        matches!(self, Self::Uniform { size } if size == expected)
    }

    /// Compact operator-facing description of the native state.
    #[must_use]
    pub fn describe(self) -> String {
        match self {
            Self::Empty => "no presentation slides".to_string(),
            Self::Uniform { size } => size.to_string(),
            Self::Missing { slide_index } => {
                format!("missing size on slide {}", slide_index + 1)
            }
            Self::Invalid { slide_index } => {
                format!("invalid size on slide {}", slide_index + 1)
            }
            Self::Mixed {
                first,
                conflicting,
                slide_index,
            } => format!(
                "mixed ({first}, {conflicting} on slide {})",
                slide_index + 1
            ),
        }
    }
}

/// Inspect every native presentation slide action in cue order.
#[must_use]
pub fn inspect_presentation_size(presentation: &rv_data::Presentation) -> PresentationSizeStatus {
    let mut first = None;
    let mut slide_index = 0usize;
    for cue in &presentation.cues {
        for action in &cue.actions {
            let Some(rv_data::action::ActionTypeData::Slide(slide_action)) =
                &action.action_type_data
            else {
                continue;
            };
            let Some(rv_data::action::slide_type::Slide::Presentation(slide)) = &slide_action.slide
            else {
                continue;
            };
            let size = match inspect_slide_size(slide) {
                Ok(size) => size,
                Err(PresentationSizeError::Missing) => {
                    return PresentationSizeStatus::Missing { slide_index };
                }
                Err(
                    PresentationSizeError::Invalid
                    | PresentationSizeError::ZeroWidth
                    | PresentationSizeError::ZeroHeight,
                ) => return PresentationSizeStatus::Invalid { slide_index },
            };
            if let Some(first) = first {
                if first != size {
                    return PresentationSizeStatus::Mixed {
                        first,
                        conflicting: size,
                        slide_index,
                    };
                }
            } else {
                first = Some(size);
            }
            slide_index += 1;
        }
    }

    first.map_or(PresentationSizeStatus::Empty, |size| {
        PresentationSizeStatus::Uniform { size }
    })
}

/// Read one slide's positive integral native canvas dimensions.
pub fn inspect_slide_size(
    slide: &rv_data::PresentationSlide,
) -> Result<PresentationSize, PresentationSizeError> {
    let size = slide
        .base_slide
        .as_ref()
        .and_then(|base_slide| base_slide.size.as_ref())
        .ok_or(PresentationSizeError::Missing)?;
    let width = checked_dimension(size.width)?;
    let height = checked_dimension(size.height)?;
    PresentationSize::new(width, height)
}

fn checked_dimension(value: f64) -> Result<u32, PresentationSizeError> {
    if !value.is_finite() || value <= 0.0 || value.fract() != 0.0 || value > f64::from(u32::MAX) {
        return Err(PresentationSizeError::Invalid);
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Ok(value as u32)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn presentation_with_sizes(sizes: &[Option<(f64, f64)>]) -> rv_data::Presentation {
        rv_data::Presentation {
            cues: sizes
                .iter()
                .map(|size| rv_data::Cue {
                    actions: vec![rv_data::Action {
                        action_type_data: Some(rv_data::action::ActionTypeData::Slide(
                            rv_data::action::SlideType {
                                slide: Some(rv_data::action::slide_type::Slide::Presentation(
                                    rv_data::PresentationSlide {
                                        base_slide: Some(rv_data::Slide {
                                            size: size.map(|(width, height)| {
                                                rv_data::graphics::Size { width, height }
                                            }),
                                            ..rv_data::Slide::default()
                                        }),
                                        ..rv_data::PresentationSlide::default()
                                    },
                                )),
                            },
                        )),
                        ..rv_data::Action::default()
                    }],
                    ..rv_data::Cue::default()
                })
                .collect(),
            ..rv_data::Presentation::default()
        }
    }

    #[test]
    fn reports_uniform_mixed_and_missing_sizes() {
        let full_hd = PresentationSize::new(1920, 1080).expect("valid full HD size");
        assert_eq!(
            inspect_presentation_size(&presentation_with_sizes(&[
                Some((1920.0, 1080.0)),
                Some((1920.0, 1080.0)),
            ])),
            PresentationSizeStatus::Uniform { size: full_hd }
        );
        assert_eq!(
            inspect_presentation_size(&presentation_with_sizes(&[
                Some((1920.0, 1080.0)),
                Some((1280.0, 720.0)),
            ])),
            PresentationSizeStatus::Mixed {
                first: full_hd,
                conflicting: PresentationSize::new(1280, 720).expect("valid HD size"),
                slide_index: 1,
            }
        );
        assert_eq!(
            inspect_presentation_size(&presentation_with_sizes(&[None])),
            PresentationSizeStatus::Missing { slide_index: 0 }
        );
    }

    #[test]
    fn zero_config_dimensions_are_rejected() {
        assert_eq!(
            PresentationSize::new(0, 1080),
            Err(PresentationSizeError::ZeroWidth)
        );
        assert_eq!(
            PresentationSize::new(1920, 0),
            Err(PresentationSizeError::ZeroHeight)
        );
    }
}
