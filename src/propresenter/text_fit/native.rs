//! One translation from resolved `ProPresenter` text fields to fit requests.

use crate::propresenter::generated::rv_data;
use crate::propresenter::generated::rv_data::graphics::text::attributes::{
    Capitalization, LigatureStyle,
};
use crate::propresenter::presentation_spec::TextField;
use crate::propresenter::render::{ResolvedCueRole, TemplateSlotError};
use crate::propresenter::rtf::{
    extract_text_options, has_visible_superscript, segments_to_rtf_bytes, visible_font_names,
    StyledSegment,
};

use super::request::{
    FinalRtf, RequiredFonts, TextBoxGeometry, TextFitRequest, TextFitRequestError, TextMargins,
    TextScaleBehavior, TextTransform, TextVerticalAlignment,
};

impl TextFitRequest {
    /// Build the exact request that rendering these segments into this semantic
    /// field will produce.
    pub(crate) fn from_resolved_segments(
        role: &ResolvedCueRole<'_>,
        field: &TextField,
        segments: &[StyledSegment],
    ) -> Result<Self, NativeTextRequestError> {
        let (graphics, text) = resolved_native_text(role, field, role.slide())?;
        let options = extract_text_options(text);
        let rtf = segments_to_rtf_bytes(segments, &options);
        Self::from_native_text(graphics, text, rtf, Some(role.slide()))
    }

    /// Build a request from the exact final RTF stored in one rendered semantic
    /// text field.
    pub(crate) fn from_rendered_field(
        role: &ResolvedCueRole<'_>,
        field: &TextField,
        rendered_slide: &rv_data::PresentationSlide,
    ) -> Result<Self, NativeTextRequestError> {
        let (graphics, text) = resolved_native_text(role, field, rendered_slide)?;
        Self::from_native_text(graphics, text, text.rtf_data.clone(), Some(role.slide()))
    }

    /// Build a request from one exact text element retained in an existing
    /// presentation.
    #[cfg(test)]
    pub(crate) fn from_native_element(
        graphics: &rv_data::graphics::Element,
        text: &rv_data::graphics::Text,
    ) -> Result<Self, NativeTextRequestError> {
        Self::from_native_text(graphics, text, text.rtf_data.clone(), None)
    }

    /// Build a request for an element whose containing slide supplies the
    /// canvas envelope required by dynamic-height text.
    pub(crate) fn from_native_slide_element(
        slide: &rv_data::PresentationSlide,
        graphics: &rv_data::graphics::Element,
        text: &rv_data::graphics::Text,
    ) -> Result<Self, NativeTextRequestError> {
        Self::from_native_text(graphics, text, text.rtf_data.clone(), Some(slide))
    }

    fn from_native_text(
        graphics: &rv_data::graphics::Element,
        text: &rv_data::graphics::Text,
        rtf: Vec<u8>,
        slide: Option<&rv_data::PresentationSlide>,
    ) -> Result<Self, NativeTextRequestError> {
        validate_native_text_features(text)?;
        let size = graphics
            .bounds
            .as_ref()
            .and_then(|bounds| bounds.size.as_ref())
            .ok_or(NativeTextRequestError::MissingTextBounds)?;
        let margins = text.margins.as_ref().map_or_else(
            || TextMargins::new(0.0, 0.0, 0.0, 0.0),
            |margins| TextMargins::new(margins.top, margins.left, margins.bottom, margins.right),
        )?;
        let geometry = TextBoxGeometry::new(size.width, size.height, margins)?;
        let scale_behavior = native_scale_behavior(text.scale_behavior, graphics, geometry, slide)?;
        let transform = native_transform(text.transform)?;
        let vertical_alignment = native_vertical_alignment(text.vertical_alignment)?;
        let baseline_font = extract_text_options(text).font_name;
        let mut required_fonts = visible_font_names(&rtf);
        required_fonts.push(baseline_font);
        Ok(Self::new(
            FinalRtf::new(rtf)?,
            geometry,
            scale_behavior,
            transform,
            vertical_alignment,
            RequiredFonts::new(required_fonts)?,
        )?)
    }
}

fn validate_native_text_features(
    text: &rv_data::graphics::Text,
) -> Result<(), NativeTextRequestError> {
    if text
        .chord_pro
        .as_ref()
        .is_some_and(|chord_pro| chord_pro.enabled)
    {
        return Err(NativeTextRequestError::UnsupportedNativeTextFeature(
            "ChordPro",
        ));
    }
    if !text.alternate_texts.is_empty() {
        return Err(NativeTextRequestError::UnsupportedNativeTextFeature(
            "alternate text",
        ));
    }
    // Current native documents mark RTF whose superscript metrics have passed
    // ProPresenter's canonicalization migration. TextKit can then measure the
    // exact stored attributed run. An older unstandardized run may be rewritten
    // by ProPresenter on open, so its stored RTF is not sufficient evidence.
    if !text.is_superscript_standardized && has_visible_superscript(&text.rtf_data) {
        return Err(NativeTextRequestError::UnsupportedNativeTextFeature(
            "unstandardized visible superscript",
        ));
    }
    let Some(attributes) = text.attributes.as_ref() else {
        return Ok(());
    };
    match Capitalization::try_from(attributes.capitalization) {
        Ok(Capitalization::None) => {}
        Ok(_) => {
            return Err(NativeTextRequestError::UnsupportedNativeTextFeature(
                "capitalization",
            ));
        }
        Err(_) => {
            return Err(NativeTextRequestError::UnknownNativeCapitalization(
                attributes.capitalization,
            ));
        }
    }
    match LigatureStyle::try_from(attributes.ligature_style) {
        Ok(LigatureStyle::Default) => {}
        Ok(LigatureStyle::None) => {
            return Err(NativeTextRequestError::UnsupportedNativeTextFeature(
                "disabled ligatures",
            ));
        }
        Err(_) => {
            return Err(NativeTextRequestError::UnknownNativeLigatureStyle(
                attributes.ligature_style,
            ));
        }
    }
    if attributes.superscript != 0 {
        return Err(NativeTextRequestError::UnsupportedNativeTextFeature(
            "element-level superscript",
        ));
    }
    let has_rendering_custom_attribute = attributes.custom_attributes.iter().any(|custom| {
        !matches!(
            custom.attribute,
            Some(rv_data::graphics::text::attributes::custom_attribute::Attribute::OriginalFont(_))
        )
    });
    if has_rendering_custom_attribute {
        return Err(NativeTextRequestError::UnsupportedNativeTextFeature(
            "rendering custom text attributes",
        ));
    }
    Ok(())
}

fn resolved_native_text<'a>(
    role: &ResolvedCueRole<'_>,
    field: &TextField,
    slide: &'a rv_data::PresentationSlide,
) -> Result<(&'a rv_data::graphics::Element, &'a rv_data::graphics::Text), NativeTextRequestError> {
    let index = role.field_index(field)?;
    let graphics = slide
        .base_slide
        .as_ref()
        .and_then(|base| base.elements.get(index))
        .and_then(|element| element.element.as_ref())
        .ok_or(NativeTextRequestError::InvalidNativeSlot { index })?;
    let text = graphics
        .text
        .as_ref()
        .ok_or(NativeTextRequestError::InvalidNativeSlot { index })?;
    Ok((graphics, text))
}

fn native_scale_behavior(
    value: i32,
    graphics: &rv_data::graphics::Element,
    geometry: TextBoxGeometry,
    slide: Option<&rv_data::PresentationSlide>,
) -> Result<TextScaleBehavior, NativeTextRequestError> {
    use rv_data::graphics::text::ScaleBehavior;

    match ScaleBehavior::try_from(value) {
        Ok(ScaleBehavior::None) => Ok(TextScaleBehavior::None),
        Ok(ScaleBehavior::AdjustContainerHeight) => {
            let slide = slide.ok_or(NativeTextRequestError::MissingDynamicContainerCanvas)?;
            let bounds = graphics
                .bounds
                .as_ref()
                .ok_or(NativeTextRequestError::MissingTextBounds)?;
            let origin = bounds
                .origin
                .as_ref()
                .ok_or(NativeTextRequestError::MissingDynamicContainerOrigin)?;
            let box_size = bounds
                .size
                .as_ref()
                .ok_or(NativeTextRequestError::MissingTextBounds)?;
            let canvas = slide
                .base_slide
                .as_ref()
                .and_then(|base| base.size.as_ref())
                .ok_or(NativeTextRequestError::MissingDynamicContainerCanvas)?;
            let top_space = origin.y;
            let bottom_space = canvas.height - origin.y - box_size.height;
            if !top_space.is_finite()
                || !bottom_space.is_finite()
                || top_space < 0.0
                || bottom_space < 0.0
            {
                return Err(NativeTextRequestError::DynamicContainerOutsideCanvas {
                    y: origin.y,
                    height: box_size.height,
                    canvas_height: canvas.height,
                });
            }
            // ProPresenter's private anchoring behavior is deliberately not
            // guessed. Limiting growth to the smaller free edge proves the
            // result stays on canvas whether the top, middle, or bottom moves.
            let maximum_content_height = geometry.content_height() + top_space.min(bottom_space);
            Ok(TextScaleBehavior::AdjustContainerHeight {
                maximum_content_height,
            })
        }
        Ok(ScaleBehavior::ScaleFontDown) => {
            Err(NativeTextRequestError::UnevidencedFontScaleMinimum)
        }
        Ok(ScaleBehavior::ScaleFontUp) => Err(
            NativeTextRequestError::UnsupportedNativeScaleBehavior("scale_font_up"),
        ),
        Ok(ScaleBehavior::ScaleFontUpDown) => Err(
            NativeTextRequestError::UnsupportedNativeScaleBehavior("scale_font_up_down"),
        ),
        Err(_) => Err(NativeTextRequestError::UnknownNativeScaleBehavior(value)),
    }
}

fn native_transform(value: i32) -> Result<TextTransform, NativeTextRequestError> {
    use rv_data::graphics::text::Transform;

    match Transform::try_from(value) {
        Ok(Transform::None) => Ok(TextTransform::None),
        Ok(transform) => Err(NativeTextRequestError::UnsupportedNativeTransform(
            transform.as_str_name(),
        )),
        Err(_) => Err(NativeTextRequestError::UnknownNativeTransform(value)),
    }
}

fn native_vertical_alignment(value: i32) -> Result<TextVerticalAlignment, NativeTextRequestError> {
    use rv_data::graphics::text::VerticalAlignment;

    match VerticalAlignment::try_from(value) {
        Ok(VerticalAlignment::Top) => Ok(TextVerticalAlignment::Top),
        Ok(VerticalAlignment::Middle) => Ok(TextVerticalAlignment::Middle),
        Ok(VerticalAlignment::Bottom) => Ok(TextVerticalAlignment::Bottom),
        Err(_) => Err(NativeTextRequestError::UnknownNativeVerticalAlignment(
            value,
        )),
    }
}

/// A resolved native field cannot be measured faithfully.
#[derive(Debug, thiserror::Error)]
pub enum NativeTextRequestError {
    /// Semantic-to-native field binding failed.
    #[error(transparent)]
    Template(#[from] TemplateSlotError),
    /// The cloned/native slide no longer contains the resolved text field.
    #[error("resolved native text slot {index} is unavailable")]
    InvalidNativeSlot {
        /// Resolved native element index.
        index: usize,
    },
    /// Text measurement requires explicit local bounds.
    #[error("resolved native text field has no bounded size")]
    MissingTextBounds,
    /// Dynamic container growth cannot be bounded without its slide canvas.
    #[error("dynamic-height text requires a containing slide canvas")]
    MissingDynamicContainerCanvas,
    /// Dynamic container growth cannot be bounded without an authored origin.
    #[error("dynamic-height text requires an authored container origin")]
    MissingDynamicContainerOrigin,
    /// The authored dynamic text box is already outside its slide canvas.
    #[error(
        "dynamic-height text box y={y}, height={height} is outside canvas height {canvas_height}"
    )]
    DynamicContainerOutsideCanvas {
        /// Authored top edge.
        y: f64,
        /// Authored box height.
        height: f64,
        /// Slide canvas height.
        canvas_height: f64,
    },
    /// Native geometry or fit policy was invalid.
    #[error(transparent)]
    Request(#[from] TextFitRequestError),
    /// A future native enum value cannot be interpreted safely.
    #[error("unknown native text scale behavior value {0}")]
    UnknownNativeScaleBehavior(i32),
    /// Container-growth/font-growth semantics are not yet evidenced.
    #[error("native text scale behavior '{0}' is not supported for generated or restyled output")]
    UnsupportedNativeScaleBehavior(&'static str),
    /// `ProPresenter` has no evidenced minimum for automatic font shrinking.
    #[error("native scale-font-down has no evidenced minimum readable scale")]
    UnevidencedFontScaleMinimum,
    /// A future native transform value cannot be interpreted safely.
    #[error("unknown native text transform value {0}")]
    UnknownNativeTransform(i32),
    /// `ProPresenter` text transformation semantics are not yet evidenced.
    #[error("native text transform '{0}' is not supported for generated or restyled output")]
    UnsupportedNativeTransform(&'static str),
    /// A ProPresenter-only text feature could alter layout outside the exact
    /// Cocoa RTF supplied to `TextKit`.
    #[error("native text feature '{0}' is not supported by the fit oracle")]
    UnsupportedNativeTextFeature(&'static str),
    /// A future native capitalization value cannot be interpreted safely.
    #[error("unknown native capitalization value {0}")]
    UnknownNativeCapitalization(i32),
    /// A future native ligature-style value cannot be interpreted safely.
    #[error("unknown native ligature-style value {0}")]
    UnknownNativeLigatureStyle(i32),
    /// A future vertical-alignment value cannot be interpreted safely.
    #[error("unknown native text vertical-alignment value {0}")]
    UnknownNativeVerticalAlignment(i32),
}
