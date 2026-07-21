//! Versioned JSON-lines contract shared with the native helper.

use serde::{Deserialize, Serialize};

use super::client::TextFitError;
use super::evidence::TextFitEvidence;
use super::request::{TextBoxGeometry, TextFitRequest, TextVerticalAlignment};
use super::TEXT_FIT_PROTOCOL_VERSION;

#[derive(Serialize)]
pub(super) struct WireRequest<'a> {
    protocol_version: u32,
    request_id: u64,
    rtf_hex: String,
    geometry: TextBoxGeometry,
    scale_behavior: &'static str,
    minimum_scale: f64,
    maximum_container_height: f64,
    transform: &'static str,
    vertical_alignment: TextVerticalAlignment,
    required_fonts: &'a [String],
}

impl<'a> WireRequest<'a> {
    pub(super) fn from_request(request_id: u64, request: &'a TextFitRequest) -> Self {
        let (scale_behavior, minimum_scale) = request.scale_behavior.wire();
        let maximum_container_height = request
            .scale_behavior
            .maximum_content_height(request.geometry.content_height());
        Self {
            protocol_version: TEXT_FIT_PROTOCOL_VERSION,
            request_id,
            rtf_hex: encode_hex(&request.rtf.0),
            geometry: request.geometry,
            scale_behavior,
            minimum_scale,
            maximum_container_height,
            transform: request.transform.wire(),
            vertical_alignment: request.vertical_alignment,
            required_fonts: &request.required_fonts.0,
        }
    }
}

#[derive(Deserialize)]
pub(super) struct WireResponse {
    pub(super) protocol_version: u32,
    pub(super) request_id: u64,
    pub(super) status: String,
    pub(super) evidence: Option<TextFitEvidence>,
    pub(super) error: Option<WireHelperError>,
}

#[derive(Deserialize)]
pub(super) struct WireHelperError {
    code: String,
    message: String,
    #[serde(default)]
    details: Vec<String>,
}

pub(super) fn map_helper_error(error: WireHelperError) -> TextFitError {
    match error.code.as_str() {
        "missing_font" => TextFitError::MissingFonts(error.details),
        "invalid_rtf" => TextFitError::InvalidRtf(error.message),
        "unsupported_rtf_content" => TextFitError::UnsupportedRtfContent(error.message),
        "font_program_unavailable" => TextFitError::FontProgramUnavailable(error.message),
        "unsupported_scale_behavior" => TextFitError::UnsupportedScaleBehavior(error.message),
        "unsupported_transform" => TextFitError::UnsupportedTransform(error.message),
        "unsupported_vertical_alignment" => {
            TextFitError::UnsupportedVerticalAlignment(error.message)
        }
        "runtime_identity_unavailable" => TextFitError::RuntimeIdentityUnavailable(error.message),
        "layout_failed" => TextFitError::LayoutFailed(error.message),
        _ => TextFitError::HelperRejected {
            code: error.code,
            message: error.message,
        },
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}
