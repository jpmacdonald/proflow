//! Native layout evidence and its Rust-side postconditions.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::client::TextFitError;
use super::request::{TextFitRequest, TextScaleBehavior};

const FIT_DIMENSION_TOLERANCE_POINTS: f64 = 0.02;

/// Stable identity of one rendering destination proved by native layout.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TextFitDestinationIdentity {
    /// The authored presentation theme displayed in the operator UI.
    SourceTheme {
        /// Exact semantic role whose configured theme slide was measured.
        cue_role: String,
        /// Semantic text field measured within the role.
        field: String,
        /// Native UUID of the configured source theme slide, when present.
        #[serde(skip_serializing_if = "Option::is_none")]
        theme_slide_uuid: Option<String>,
    },
    /// Exact text element retained from an existing presentation.
    ExistingPresentation {
        /// Native UUID of the operator cue containing the text.
        cue_uuid: String,
        /// Native UUID of the one measured text element.
        text_element_uuid: String,
        /// Stable native field identity for this text element.
        field: String,
    },
    /// One exact audience screen selected through a cue macro and Audience Look.
    AudienceScreen {
        /// Semantic text field measured on this screen.
        field: String,
        /// Stable native screen UUID.
        screen_uuid: String,
        /// Operator-visible screen name.
        screen_name: String,
        /// Exact installed macro name.
        macro_name: String,
        /// Stable native Audience Look UUID.
        audience_look_uuid: String,
        /// Operator-visible Audience Look name.
        audience_look_name: String,
        /// How the Look renders presentation foregrounds on this screen.
        rendering: AudienceTextRendering,
    },
}

/// Presentation-foreground rendering selected for one audience screen.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AudienceTextRendering {
    /// Retain the presentation's authored foreground theme.
    SourcePresentation,
    /// Restyle with one exact theme document and slide.
    ThemeOverride {
        /// SHA-256 of the decoded theme document source bytes.
        theme_document_sha256: String,
        /// Stable native theme-slide UUID.
        theme_slide_uuid: String,
    },
}

/// One font actually resolved while shaping a receipt destination.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ResolvedFontSummary {
    postscript_name: String,
    family_name: String,
    point_size: f64,
    /// Exact local font-program path resolved by CoreText.
    font_program_path: PathBuf,
    /// SHA-256 of the exact TTC/OTF font program resolved by CoreText.
    font_program_sha256: String,
}

/// Stable identity of the native measurement implementation used by a build.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct TextFitContractSummary {
    schema: String,
    protocol_version: u32,
    helper_sha256: String,
    producer_version: String,
}

impl TextFitContractSummary {
    pub(super) fn new(schema: &str, protocol_version: u32, helper_sha256: String) -> Self {
        Self {
            schema: schema.to_string(),
            protocol_version,
            helper_sha256,
            producer_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    #[cfg(test)]
    pub(crate) fn diagnostic() -> Self {
        Self::new(
            super::TEXT_FIT_EVIDENCE_SCHEMA,
            super::TEXT_FIT_PROTOCOL_VERSION,
            "0".repeat(64),
        )
    }

    #[cfg(test)]
    pub(crate) fn helper_sha256(&self) -> &str {
        &self.helper_sha256
    }
}

/// Stable native-layout proof for one cue destination.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct TextFitDestinationSummary {
    destination: TextFitDestinationIdentity,
    native_layout_runtime: NativeLayoutRuntimeSummary,
    fits_bounds: bool,
    used_x: f64,
    used_y: f64,
    used_width: f64,
    used_height: f64,
    line_count: usize,
    /// Contiguous attributed runs whose metrics can alter native layout.
    metric_style_run_count: usize,
    fitted_utf16_location: usize,
    fitted_utf16_length: usize,
    input_utf16_length: usize,
    effective_scale: f64,
    resolved_fonts: Vec<ResolvedFontSummary>,
}

/// Exact Apple text-stack runtime that produced one native measurement.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct NativeLayoutRuntimeSummary {
    operating_system: String,
    appkit: String,
    core_text: String,
}

/// Complete destination layout evidence for one rendered cue.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct CueTextFitSummary {
    cue_index: usize,
    destinations: Vec<TextFitDestinationSummary>,
}

impl CueTextFitSummary {
    pub(crate) const fn new(
        cue_index: usize,
        destinations: Vec<TextFitDestinationSummary>,
    ) -> Self {
        Self {
            cue_index,
            destinations,
        }
    }

    pub(crate) const fn cue_index(&self) -> usize {
        self.cue_index
    }

    pub(crate) const fn destination_count(&self) -> usize {
        self.destinations.len()
    }

    pub(super) fn font_programs(&self) -> impl Iterator<Item = (&Path, &str)> {
        self.destinations
            .iter()
            .flat_map(|destination| &destination.resolved_fonts)
            .map(|font| {
                (
                    font.font_program_path.as_path(),
                    font.font_program_sha256.as_str(),
                )
            })
    }
}

/// Width and height required by the fully laid-out text.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
pub struct UsedTextRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl UsedTextRect {
    /// Required width in points.
    pub(crate) const fn width(self) -> f64 {
        self.width
    }

    /// Required height in points.
    pub(crate) const fn height(self) -> f64 {
        self.height
    }
}

/// UTF-16 character range that `TextKit` could place inside the content box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct FittedUtf16Range {
    location: usize,
    length: usize,
}

impl FittedUtf16Range {
    /// Number of fitted UTF-16 code units.
    #[cfg(test)]
    pub(crate) const fn length(self) -> usize {
        self.length
    }
}

/// A font actually resolved by `AppKit` while decoding and laying out the RTF.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ResolvedFontEvidence {
    postscript_name: String,
    family_name: String,
    point_size: f64,
    font_program_path: PathBuf,
    font_program_sha256: String,
}

impl ResolvedFontEvidence {
    /// Resolved font family name.
    #[cfg(test)]
    pub(crate) fn family_name(&self) -> &str {
        &self.family_name
    }

    /// SHA-256 of the resolved CoreText font program.
    #[cfg(test)]
    pub(crate) fn font_program_sha256(&self) -> &str {
        &self.font_program_sha256
    }

    /// Exact local font-program path resolved by CoreText.
    #[cfg(test)]
    pub(crate) fn font_program_path(&self) -> &Path {
        &self.font_program_path
    }
}

/// Validated physical-layout evidence returned by the native helper.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct TextFitEvidence {
    fits_bounds: bool,
    used_rect: UsedTextRect,
    line_count: usize,
    metric_style_run_count: usize,
    fitted_utf16_range: FittedUtf16Range,
    input_utf16_length: usize,
    effective_scale: f64,
    resolved_fonts: Vec<ResolvedFontEvidence>,
    native_layout_runtime: NativeLayoutRuntimeSummary,
}

impl TextFitEvidence {
    /// Whether every glyph fits within the measured content rectangle.
    pub(crate) const fn fits_bounds(&self) -> bool {
        self.fits_bounds
    }

    /// Full size required to render the text at the effective scale.
    pub(crate) const fn used_rect(&self) -> UsedTextRect {
        self.used_rect
    }

    /// Number of visual lines in the complete layout.
    pub(crate) const fn line_count(&self) -> usize {
        self.line_count
    }

    /// Number of contiguous metric-affecting attributed runs.
    pub(crate) const fn metric_style_run_count(&self) -> usize {
        self.metric_style_run_count
    }

    /// UTF-16 range that fits in the constrained content rectangle.
    #[cfg(test)]
    pub(crate) const fn fitted_utf16_range(&self) -> FittedUtf16Range {
        self.fitted_utf16_range
    }

    /// Total visible-text length measured in UTF-16 code units.
    #[cfg(test)]
    pub(crate) const fn input_utf16_length(&self) -> usize {
        self.input_utf16_length
    }

    /// Uniform font scale used for the reported layout.
    #[cfg(test)]
    pub(crate) const fn effective_scale(&self) -> f64 {
        self.effective_scale
    }

    /// Fonts resolved by `AppKit` for visible attributed runs.
    #[cfg(test)]
    pub(crate) fn resolved_fonts(&self) -> &[ResolvedFontEvidence] {
        &self.resolved_fonts
    }

    pub(crate) fn summarize(
        &self,
        destination: TextFitDestinationIdentity,
    ) -> TextFitDestinationSummary {
        TextFitDestinationSummary {
            destination,
            native_layout_runtime: self.native_layout_runtime.clone(),
            fits_bounds: self.fits_bounds,
            used_x: self.used_rect.x,
            used_y: self.used_rect.y,
            used_width: self.used_rect.width,
            used_height: self.used_rect.height,
            line_count: self.line_count,
            metric_style_run_count: self.metric_style_run_count,
            fitted_utf16_location: self.fitted_utf16_range.location,
            fitted_utf16_length: self.fitted_utf16_range.length,
            input_utf16_length: self.input_utf16_length,
            effective_scale: self.effective_scale,
            resolved_fonts: self
                .resolved_fonts
                .iter()
                .map(|font| ResolvedFontSummary {
                    postscript_name: font.postscript_name.clone(),
                    family_name: font.family_name.clone(),
                    point_size: font.point_size,
                    font_program_path: font.font_program_path.clone(),
                    font_program_sha256: font.font_program_sha256.clone(),
                })
                .collect(),
        }
    }

    /// Build non-native evidence for pure renderer tests.
    ///
    /// Production code cannot call this constructor: native helper responses
    /// are the only source of production evidence.
    #[cfg(test)]
    pub(crate) fn diagnostic(line_count: usize, fits_bounds: bool) -> Self {
        Self {
            fits_bounds,
            used_rect: UsedTextRect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            },
            line_count,
            metric_style_run_count: 0,
            fitted_utf16_range: FittedUtf16Range {
                location: 0,
                length: 0,
            },
            input_utf16_length: 0,
            effective_scale: 1.0,
            resolved_fonts: Vec::new(),
            native_layout_runtime: NativeLayoutRuntimeSummary {
                operating_system: "diagnostic".to_string(),
                appkit: "diagnostic".to_string(),
                core_text: "diagnostic".to_string(),
            },
        }
    }
}

pub(super) fn validate_evidence(
    mut evidence: TextFitEvidence,
    request: &TextFitRequest,
) -> Result<TextFitEvidence, TextFitError> {
    for (name, value) in [
        ("used x", evidence.used_rect.x),
        ("used y", evidence.used_rect.y),
        ("used width", evidence.used_rect.width),
        ("used height", evidence.used_rect.height),
    ] {
        if !value.is_finite() {
            return Err(TextFitError::HelperProtocol(format!(
                "{name} must be finite, got {value}"
            )));
        }
    }
    if evidence.used_rect.width < 0.0 || evidence.used_rect.height < 0.0 {
        return Err(TextFitError::HelperProtocol(
            "used rectangle dimensions must be nonnegative".to_string(),
        ));
    }
    if !evidence.effective_scale.is_finite()
        || evidence.effective_scale <= 0.0
        || evidence.effective_scale > 1.0
    {
        return Err(TextFitError::HelperProtocol(format!(
            "effective scale must be within (0, 1], got {}",
            evidence.effective_scale
        )));
    }
    let (_, minimum_scale) = request.scale_behavior.wire();
    if evidence.effective_scale + f64::EPSILON < minimum_scale
        || (request.scale_behavior == TextScaleBehavior::None
            && (evidence.effective_scale - 1.0).abs() > f64::EPSILON)
    {
        return Err(TextFitError::HelperProtocol(format!(
            "effective scale {} violates requested behavior {:?}",
            evidence.effective_scale, request.scale_behavior
        )));
    }
    let range_end = evidence
        .fitted_utf16_range
        .location
        .checked_add(evidence.fitted_utf16_range.length)
        .ok_or_else(|| TextFitError::HelperProtocol("fitted range overflows usize".to_string()))?;
    if evidence.fitted_utf16_range.location != 0 || range_end > evidence.input_utf16_length {
        return Err(TextFitError::HelperProtocol(format!(
            "fitted UTF-16 range {:?} is outside input length {}",
            evidence.fitted_utf16_range, evidence.input_utf16_length
        )));
    }
    let complete_range = range_end == evidence.input_utf16_length;
    let max_x = evidence.used_rect.x + evidence.used_rect.width;
    let max_y = evidence.used_rect.y + evidence.used_rect.height;
    let maximum_content_height = request
        .scale_behavior
        .maximum_content_height(request.geometry.content_height());
    let within_dimensions = max_x.is_finite()
        && max_y.is_finite()
        && evidence.used_rect.x >= -FIT_DIMENSION_TOLERANCE_POINTS
        && evidence.used_rect.y >= -FIT_DIMENSION_TOLERANCE_POINTS
        && max_x <= request.geometry.content_width() + FIT_DIMENSION_TOLERANCE_POINTS
        && max_y <= maximum_content_height + FIT_DIMENSION_TOLERANCE_POINTS;
    if evidence.fits_bounds != (complete_range && within_dimensions) {
        return Err(TextFitError::HelperProtocol(
            "fits_bounds disagrees with fitted range or used dimensions".to_string(),
        ));
    }
    if evidence.input_utf16_length > 0 && evidence.resolved_fonts.is_empty() {
        return Err(TextFitError::HelperProtocol(
            "nonempty text has no resolved-font evidence".to_string(),
        ));
    }
    let expected_metric_runs = usize::from(evidence.input_utf16_length > 0);
    if evidence.metric_style_run_count < expected_metric_runs
        || evidence.metric_style_run_count > evidence.input_utf16_length
    {
        return Err(TextFitError::HelperProtocol(format!(
            "metric style run count {} is inconsistent with input length {}",
            evidence.metric_style_run_count, evidence.input_utf16_length
        )));
    }
    if evidence
        .native_layout_runtime
        .operating_system
        .trim()
        .is_empty()
        || evidence.native_layout_runtime.appkit.trim().is_empty()
        || evidence.native_layout_runtime.core_text.trim().is_empty()
    {
        return Err(TextFitError::HelperProtocol(
            "native layout runtime identity is incomplete".to_string(),
        ));
    }
    validate_resolved_fonts(&mut evidence.resolved_fonts)?;
    Ok(evidence)
}

fn validate_resolved_fonts(fonts: &mut [ResolvedFontEvidence]) -> Result<(), TextFitError> {
    for font in &*fonts {
        if font.postscript_name.trim().is_empty()
            || font.family_name.trim().is_empty()
            || !font.point_size.is_finite()
            || font.point_size <= 0.0
            || !font.font_program_path.is_absolute()
            || !is_lowercase_sha256(&font.font_program_sha256)
        {
            return Err(TextFitError::HelperProtocol(
                "resolved-font evidence contains an invalid name, point size, program path, or digest"
                    .to_string(),
            ));
        }
    }
    fonts.sort_by(|left, right| {
        left.postscript_name
            .cmp(&right.postscript_name)
            .then_with(|| left.family_name.cmp(&right.family_name))
            .then_with(|| left.point_size.total_cmp(&right.point_size))
            .then_with(|| left.font_program_path.cmp(&right.font_program_path))
            .then_with(|| left.font_program_sha256.cmp(&right.font_program_sha256))
    });
    Ok(())
}

fn is_lowercase_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
