//! Checked inputs for one native text measurement.

use std::collections::BTreeMap;

use serde::Serialize;

pub(super) const MAX_FINAL_RTF_BYTES: usize = 4 * 1024 * 1024;

/// Insets inside a native text element, measured in points.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct TextMargins {
    top: f64,
    left: f64,
    bottom: f64,
    right: f64,
}

impl TextMargins {
    /// Construct finite, nonnegative text margins.
    pub(crate) fn new(
        top: f64,
        left: f64,
        bottom: f64,
        right: f64,
    ) -> Result<Self, TextFitRequestError> {
        for (name, value) in [
            ("top", top),
            ("left", left),
            ("bottom", bottom),
            ("right", right),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(TextFitRequestError::InvalidMargin { name, value });
            }
        }
        Ok(Self {
            top,
            left,
            bottom,
            right,
        })
    }

    const fn horizontal(self) -> f64 {
        self.left + self.right
    }

    const fn vertical(self) -> f64 {
        self.top + self.bottom
    }
}

/// Checked native text-box dimensions and margins, measured in points.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct TextBoxGeometry {
    width: f64,
    height: f64,
    margins: TextMargins,
}

impl TextBoxGeometry {
    /// Construct geometry whose inset content rectangle has positive area.
    pub(crate) fn new(
        width: f64,
        height: f64,
        margins: TextMargins,
    ) -> Result<Self, TextFitRequestError> {
        for (name, value) in [("width", width), ("height", height)] {
            if !value.is_finite() || value <= 0.0 {
                return Err(TextFitRequestError::InvalidDimension { name, value });
            }
        }
        let content_width = width - margins.horizontal();
        let content_height = height - margins.vertical();
        if content_width <= 0.0 || content_height <= 0.0 {
            return Err(TextFitRequestError::MarginsConsumeTextBox { width, height });
        }
        Ok(Self {
            width,
            height,
            margins,
        })
    }

    /// Width available after subtracting horizontal margins.
    pub(crate) fn content_width(self) -> f64 {
        self.width - self.margins.horizontal()
    }

    /// Height available after subtracting vertical margins.
    pub(crate) fn content_height(self) -> f64 {
        self.height - self.margins.vertical()
    }
}

/// Checked lower bound for `ProPresenter`'s scale-font-down behavior.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MinimumFontScale(f64);

#[cfg(test)]
// The checked constructor rejects NaN, so equality is reflexive for every
// representable minimum scale.
impl Eq for MinimumFontScale {}

#[cfg(test)]
impl MinimumFontScale {
    /// Construct a finite scale in the interval `(0, 1]`.
    pub(crate) fn new(value: f64) -> Result<Self, TextFitRequestError> {
        if !value.is_finite() || value <= 0.0 || value > 1.0 {
            return Err(TextFitRequestError::InvalidMinimumScale(value));
        }
        Ok(Self(value))
    }

    const fn get(self) -> f64 {
        self.0
    }
}

/// Native `ProPresenter` text scaling behavior.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TextScaleBehavior {
    /// Preserve the authored font sizes and container dimensions.
    None,
    /// Change the container height, within a canvas-safe growth envelope.
    AdjustContainerHeight {
        /// Greatest content height that is safe regardless of which vertical
        /// edge `ProPresenter` keeps fixed while growing the container.
        maximum_content_height: f64,
    },
    /// Reduce fonts until they fit, subject to the supplied lower bound.
    #[cfg(test)]
    ScaleFontDown(MinimumFontScale),
    /// Increase fonts to consume available space.
    #[cfg(test)]
    ScaleFontUp,
    /// Increase or reduce fonts to consume available space.
    #[cfg(test)]
    ScaleFontUpDown,
}

impl TextScaleBehavior {
    pub(super) const fn wire(self) -> (&'static str, f64) {
        match self {
            Self::None => ("none", 1.0),
            #[cfg(test)]
            Self::ScaleFontDown(minimum) => ("scale_font_down", minimum.get()),
            Self::AdjustContainerHeight { .. } => ("adjust_container_height", 1.0),
            #[cfg(test)]
            Self::ScaleFontUp => ("scale_font_up", 1.0),
            #[cfg(test)]
            Self::ScaleFontUpDown => ("scale_font_up_down", 1.0),
        }
    }

    const fn supported(self) -> bool {
        match self {
            Self::None | Self::AdjustContainerHeight { .. } => true,
            #[cfg(test)]
            Self::ScaleFontDown(_) => true,
            #[cfg(test)]
            Self::ScaleFontUp | Self::ScaleFontUpDown => false,
        }
    }

    pub(super) const fn maximum_content_height(self, authored_height: f64) -> f64 {
        match self {
            Self::AdjustContainerHeight {
                maximum_content_height,
            } => maximum_content_height,
            Self::None => authored_height,
            #[cfg(test)]
            Self::ScaleFontDown(_) | Self::ScaleFontUp | Self::ScaleFontUpDown => authored_height,
        }
    }
}

/// Native text transformation applied before layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextTransform {
    /// Measure the exact visible content encoded by the final RTF.
    None,
    /// Force the entire value onto a single line.
    #[cfg(test)]
    SingleLine,
    /// Place one word on each line.
    #[cfg(test)]
    OneWordPerLine,
    /// Place one character on each line.
    #[cfg(test)]
    OneCharacterPerLine,
    /// Replace authored line returns using a configured delimiter.
    #[cfg(test)]
    ReplaceLineReturns,
}

impl TextTransform {
    pub(super) const fn wire(self) -> &'static str {
        match self {
            Self::None => "none",
            #[cfg(test)]
            Self::SingleLine => "single_line",
            #[cfg(test)]
            Self::OneWordPerLine => "one_word_per_line",
            #[cfg(test)]
            Self::OneCharacterPerLine => "one_character_per_line",
            #[cfg(test)]
            Self::ReplaceLineReturns => "replace_line_returns",
        }
    }
}

/// Native vertical placement within a text box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TextVerticalAlignment {
    /// Place text at the top edge.
    Top,
    /// Center text vertically.
    Middle,
    /// Place text at the bottom edge.
    Bottom,
}

/// Exact final RTF bytes supplied to the native layout stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalRtf(pub(super) Vec<u8>);

impl FinalRtf {
    /// Construct a bounded, nonempty exact RTF payload.
    pub(crate) fn new(bytes: Vec<u8>) -> Result<Self, TextFitRequestError> {
        if bytes.is_empty() {
            return Err(TextFitRequestError::EmptyRtf);
        }
        if bytes.len() > MAX_FINAL_RTF_BYTES {
            return Err(TextFitRequestError::RtfTooLarge {
                length: bytes.len(),
                maximum: MAX_FINAL_RTF_BYTES,
            });
        }
        Ok(Self(bytes))
    }
}

/// Font names that must be installed before native measurement is trustworthy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequiredFonts(pub(super) Vec<String>);

impl RequiredFonts {
    /// Normalize, deduplicate, and validate expected font or family names.
    pub(crate) fn new<I, S>(names: I) -> Result<Self, TextFitRequestError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut normalized = BTreeMap::new();
        for name in names {
            let name = name.into();
            let trimmed = name.trim();
            if trimmed.is_empty() {
                return Err(TextFitRequestError::BlankRequiredFont);
            }
            normalized
                .entry(trimmed.to_lowercase())
                .or_insert_with(|| trimmed.to_string());
        }
        if normalized.is_empty() {
            return Err(TextFitRequestError::NoRequiredFonts);
        }
        Ok(Self(normalized.into_values().collect()))
    }
}

/// One fully checked request for native text measurement.
#[derive(Debug, Clone, PartialEq)]
pub struct TextFitRequest {
    pub(super) rtf: FinalRtf,
    pub(super) geometry: TextBoxGeometry,
    pub(super) scale_behavior: TextScaleBehavior,
    pub(super) transform: TextTransform,
    pub(super) vertical_alignment: TextVerticalAlignment,
    pub(super) required_fonts: RequiredFonts,
}

impl TextFitRequest {
    /// Build a request only for native modes the oracle can reproduce.
    pub(crate) fn new(
        rtf: FinalRtf,
        geometry: TextBoxGeometry,
        scale_behavior: TextScaleBehavior,
        transform: TextTransform,
        vertical_alignment: TextVerticalAlignment,
        required_fonts: RequiredFonts,
    ) -> Result<Self, TextFitRequestError> {
        if !scale_behavior.supported() {
            return Err(TextFitRequestError::UnsupportedScaleBehavior(
                scale_behavior.wire().0,
            ));
        }
        if let TextScaleBehavior::AdjustContainerHeight {
            maximum_content_height,
        } = scale_behavior
        {
            if !maximum_content_height.is_finite()
                || maximum_content_height < geometry.content_height()
            {
                return Err(TextFitRequestError::InvalidMaximumContainerHeight {
                    authored: geometry.content_height(),
                    maximum: maximum_content_height,
                });
            }
        }
        if transform != TextTransform::None {
            return Err(TextFitRequestError::UnsupportedTransform(transform.wire()));
        }
        Ok(Self {
            rtf,
            geometry,
            scale_behavior,
            transform,
            vertical_alignment,
            required_fonts,
        })
    }
}

/// Invalid input detected before crossing the native process boundary.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum TextFitRequestError {
    /// A text-box dimension is not finite and positive.
    #[error("text-box {name} must be finite and positive, got {value}")]
    InvalidDimension {
        /// Invalid field name.
        name: &'static str,
        /// Invalid field value.
        value: f64,
    },
    /// A margin is not finite and nonnegative.
    #[error("text margin {name} must be finite and nonnegative, got {value}")]
    InvalidMargin {
        /// Invalid field name.
        name: &'static str,
        /// Invalid field value.
        value: f64,
    },
    /// Margins leave no measurable content rectangle.
    #[error("text margins consume the {width}x{height} text box")]
    MarginsConsumeTextBox {
        /// Outer width.
        width: f64,
        /// Outer height.
        height: f64,
    },
    /// The scale-font-down lower bound is invalid.
    #[cfg(test)]
    #[error("minimum font scale must be finite and within (0, 1], got {0}")]
    InvalidMinimumScale(f64),
    /// Exact final RTF cannot be empty.
    #[error("final RTF must not be empty")]
    EmptyRtf,
    /// A helper request cannot allocate an unbounded JSON-lines frame.
    #[error("final RTF is {length} bytes; native measurement accepts at most {maximum} bytes")]
    RtfTooLarge {
        /// Rejected payload length.
        length: usize,
        /// Hard request-boundary maximum.
        maximum: usize,
    },
    /// Font preflight requires at least one expected font.
    #[error("at least one required font must be declared")]
    NoRequiredFonts,
    /// A required font name cannot be blank.
    #[error("required font names must not be blank")]
    BlankRequiredFont,
    /// The native layout oracle intentionally does not emulate this mode.
    #[error("native text scale behavior '{0}' is not supported by the fit oracle")]
    UnsupportedScaleBehavior(&'static str),
    /// A dynamic container cannot grow inside the known slide canvas.
    #[error(
        "maximum dynamic-container content height must be finite and at least the authored height {authored}, got {maximum}"
    )]
    InvalidMaximumContainerHeight {
        /// Authored inset content height.
        authored: f64,
        /// Rejected maximum content height.
        maximum: f64,
    },
    /// The native layout oracle intentionally does not emulate this transform.
    #[error("native text transform '{0}' is not supported by the fit oracle")]
    UnsupportedTransform(&'static str),
}
