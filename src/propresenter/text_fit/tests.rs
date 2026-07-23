use super::*;
use super::{client::NativeTextFitOracle, wire::WireRequest};
use crate::propresenter::generated::rv_data;

#[cfg(target_os = "macos")]
fn executable_script(
    body: &str,
) -> Result<(tempfile::TempDir, std::path::PathBuf), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir()?;
    let path = root.path().join("helper");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n"))?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
    Ok((root, path))
}

fn geometry() -> Result<TextBoxGeometry, Box<dyn std::error::Error>> {
    TextBoxGeometry::new(800.0, 400.0, TextMargins::new(20.0, 30.0, 20.0, 30.0)?)
        .map_err(Into::into)
}

fn request() -> Result<TextFitRequest, Box<dyn std::error::Error>> {
    TextFitRequest::new(
        FinalRtf::new(
            br"{\rtf1\ansi\deff0{\fonttbl{\f0\fswiss Helvetica;}}\f0\fs96 Hello native TextKit}"
                .to_vec(),
        )?,
        geometry()?,
        TextScaleBehavior::None,
        TextTransform::None,
        TextVerticalAlignment::Middle,
        RequiredFonts::new(["Helvetica"])?,
    )
    .map_err(Into::into)
}

fn native_multiline_request(
    scale_behavior: rv_data::graphics::text::ScaleBehavior,
    y: f64,
    box_height: f64,
    canvas_height: f64,
) -> Result<TextFitRequest, NativeTextRequestError> {
    let text = rv_data::graphics::Text {
        rtf_data: br"{\rtf1\ansi\deff0{\fonttbl{\f0\fswiss Helvetica;}}\f0\fs96 Alpha\line Beta}"
            .to_vec(),
        vertical_alignment: rv_data::graphics::text::VerticalAlignment::Middle as i32,
        scale_behavior: scale_behavior as i32,
        is_superscript_standardized: true,
        ..rv_data::graphics::Text::default()
    };
    let graphics = rv_data::graphics::Element {
        bounds: Some(rv_data::graphics::Rect {
            origin: Some(rv_data::graphics::Point { x: Some(0.0), y }),
            size: Some(rv_data::graphics::Size {
                width: 800.0,
                height: box_height,
            }),
        }),
        text: Some(text.clone()),
        ..rv_data::graphics::Element::default()
    };
    let slide = rv_data::PresentationSlide {
        base_slide: Some(rv_data::Slide {
            size: Some(rv_data::graphics::Size {
                width: 800.0,
                height: canvas_height,
            }),
            ..rv_data::Slide::default()
        }),
        ..rv_data::PresentationSlide::default()
    };
    TextFitRequest::from_native_slide_element(&slide, &graphics, &text)
}

#[test]
fn geometry_rejects_nonfinite_or_consumed_dimensions() -> Result<(), Box<dyn std::error::Error>> {
    let margins = TextMargins::new(1.0, 2.0, 1.0, 2.0)?;
    assert!(matches!(
        TextBoxGeometry::new(f64::NAN, 10.0, margins),
        Err(TextFitRequestError::InvalidDimension { name: "width", .. })
    ));
    assert!(matches!(
        TextMargins::new(0.0, -1.0, 0.0, 0.0),
        Err(TextFitRequestError::InvalidMargin { name: "left", .. })
    ));
    assert!(matches!(
        TextBoxGeometry::new(4.0, 10.0, margins),
        Err(TextFitRequestError::MarginsConsumeTextBox { .. })
    ));
    Ok(())
}

#[test]
fn final_rtf_rejects_unbounded_helper_requests() -> Result<(), Box<dyn std::error::Error>> {
    let length = super::request::MAX_FINAL_RTF_BYTES + 1;
    let error = FinalRtf::new(vec![b'x'; length])
        .err()
        .ok_or("oversized RTF was unexpectedly accepted")?;

    assert_eq!(
        error,
        TextFitRequestError::RtfTooLarge {
            length,
            maximum: super::request::MAX_FINAL_RTF_BYTES,
        }
    );
    Ok(())
}

#[test]
fn request_rejects_unreproduced_native_modes() -> Result<(), Box<dyn std::error::Error>> {
    let base = request()?;
    for scale_behavior in [
        TextScaleBehavior::ScaleFontUp,
        TextScaleBehavior::ScaleFontUpDown,
    ] {
        assert!(matches!(
            TextFitRequest::new(
                base.rtf.clone(),
                base.geometry,
                scale_behavior,
                TextTransform::None,
                base.vertical_alignment,
                base.required_fonts.clone(),
            ),
            Err(TextFitRequestError::UnsupportedScaleBehavior(_))
        ));
    }
    assert!(TextFitRequest::new(
        base.rtf.clone(),
        base.geometry,
        TextScaleBehavior::AdjustContainerHeight {
            maximum_content_height: base.geometry.content_height() + 100.0,
        },
        TextTransform::None,
        base.vertical_alignment,
        base.required_fonts.clone(),
    )
    .is_ok());
    for transform in [
        TextTransform::SingleLine,
        TextTransform::OneWordPerLine,
        TextTransform::OneCharacterPerLine,
        TextTransform::ReplaceLineReturns,
    ] {
        assert!(matches!(
            TextFitRequest::new(
                base.rtf.clone(),
                base.geometry,
                TextScaleBehavior::None,
                transform,
                base.vertical_alignment,
                base.required_fonts.clone(),
            ),
            Err(TextFitRequestError::UnsupportedTransform(_))
        ));
    }
    Ok(())
}

#[test]
fn native_request_rejects_propresenter_only_text_features() {
    let graphics = |text| rv_data::graphics::Element {
        bounds: Some(rv_data::graphics::Rect {
            origin: Some(rv_data::graphics::Point {
                x: Some(0.0),
                y: 0.0,
            }),
            size: Some(rv_data::graphics::Size {
                width: 800.0,
                height: 400.0,
            }),
        }),
        text: Some(text),
        ..rv_data::graphics::Element::default()
    };
    let base_text = rv_data::graphics::Text {
        rtf_data: br"{\rtf1\ansi\deff0{\fonttbl{\f0\fswiss Helvetica;}}\f0\fs96 Text}".to_vec(),
        ..rv_data::graphics::Text::default()
    };
    let mut chord_text = base_text.clone();
    chord_text.chord_pro = Some(rv_data::graphics::text::ChordPro {
        enabled: true,
        ..rv_data::graphics::text::ChordPro::default()
    });
    let element = graphics(chord_text.clone());
    assert!(matches!(
        TextFitRequest::from_native_element(&element, &chord_text),
        Err(NativeTextRequestError::UnsupportedNativeTextFeature(
            "ChordPro"
        ))
    ));

    let mut capitalized = base_text;
    capitalized.attributes = Some(rv_data::graphics::text::Attributes {
        capitalization: rv_data::graphics::text::attributes::Capitalization::AllCaps as i32,
        ..rv_data::graphics::text::Attributes::default()
    });
    let element = graphics(capitalized.clone());
    assert!(matches!(
        TextFitRequest::from_native_element(&element, &capitalized),
        Err(NativeTextRequestError::UnsupportedNativeTextFeature(
            "capitalization"
        ))
    ));

    let mut unstandardized_superscript = rv_data::graphics::Text {
        rtf_data: br"{\rtf1\ansi\deff0{\fonttbl{\f0\fswiss Helvetica;}}\f0\fs96 Verse {\super 12}}"
            .to_vec(),
        ..rv_data::graphics::Text::default()
    };
    let element = graphics(unstandardized_superscript.clone());
    assert!(matches!(
        TextFitRequest::from_native_element(&element, &unstandardized_superscript),
        Err(NativeTextRequestError::UnsupportedNativeTextFeature(
            "unstandardized visible superscript"
        ))
    ));

    unstandardized_superscript.is_superscript_standardized = true;
    let element = graphics(unstandardized_superscript.clone());
    assert!(TextFitRequest::from_native_element(&element, &unstandardized_superscript).is_ok());

    let mut original_font_metadata = unstandardized_superscript;
    original_font_metadata
        .attributes
        .get_or_insert_default()
        .custom_attributes
        .push(rv_data::graphics::text::attributes::CustomAttribute {
            range: Some(rv_data::IntRange { start: 0, end: 4 }),
            attribute: Some(
                rv_data::graphics::text::attributes::custom_attribute::Attribute::OriginalFont(
                    rv_data::Font {
                        name: "ArialMT".to_string(),
                        size: 80.0,
                        ..rv_data::Font::default()
                    },
                ),
            ),
        });
    let element = graphics(original_font_metadata.clone());
    assert!(TextFitRequest::from_native_element(&element, &original_font_metadata).is_ok());

    let mut font_scale = original_font_metadata;
    font_scale
        .attributes
        .get_or_insert_default()
        .custom_attributes
        .push(rv_data::graphics::text::attributes::CustomAttribute {
            range: Some(rv_data::IntRange { start: 0, end: 4 }),
            attribute: Some(
                rv_data::graphics::text::attributes::custom_attribute::Attribute::FontScaleFactor(
                    0.8,
                ),
            ),
        });
    let element = graphics(font_scale.clone());
    assert!(matches!(
        TextFitRequest::from_native_element(&element, &font_scale),
        Err(NativeTextRequestError::UnsupportedNativeTextFeature(
            "rendering custom text attributes"
        ))
    ));
}

#[test]
fn native_request_preflights_every_visible_authored_font() -> Result<(), Box<dyn std::error::Error>>
{
    let text = rv_data::graphics::Text {
        rtf_data: br"{\rtf1\ansi\deff0{\fonttbl{\f0\fswiss Helvetica;}{\f1\froman Times New Roman;}{\f2\fmodern Unused Font;}}\f0 First {\f1 second}}"
            .to_vec(),
        ..rv_data::graphics::Text::default()
    };
    let graphics = rv_data::graphics::Element {
        bounds: Some(rv_data::graphics::Rect {
            origin: Some(rv_data::graphics::Point {
                x: Some(0.0),
                y: 0.0,
            }),
            size: Some(rv_data::graphics::Size {
                width: 800.0,
                height: 400.0,
            }),
        }),
        text: Some(text.clone()),
        ..rv_data::graphics::Element::default()
    };

    let request = TextFitRequest::from_native_element(&graphics, &text)?;

    assert_eq!(
        request.required_fonts.0,
        ["Helvetica".to_string(), "Times New Roman".to_string()]
    );
    Ok(())
}

#[test]
fn protocol_preserves_exact_rtf_bytes_and_normalized_fonts(
) -> Result<(), Box<dyn std::error::Error>> {
    let request = TextFitRequest::new(
        FinalRtf::new(vec![0, 1, 127, 128, 255])?,
        geometry()?,
        TextScaleBehavior::ScaleFontDown(MinimumFontScale::new(0.625)?),
        TextTransform::None,
        TextVerticalAlignment::Bottom,
        RequiredFonts::new([" Helvetica ", "helvetica", "Avenir Next"])?,
    )?;
    let wire = WireRequest::from_request(42, &request);
    let json = serde_json::to_value(wire)?;
    assert_eq!(json["protocol_version"], TEXT_FIT_PROTOCOL_VERSION);
    assert_eq!(json["request_id"], 42);
    assert_eq!(json["rtf_hex"], "00017f80ff");
    assert_eq!(json["scale_behavior"], "scale_font_down");
    assert_eq!(json["minimum_scale"], 0.625);
    assert_eq!(
        json["required_fonts"],
        serde_json::json!(["Avenir Next", "Helvetica"])
    );
    Ok(())
}

#[test]
fn response_requires_consistent_complete_fit_evidence() -> Result<(), Box<dyn std::error::Error>> {
    let request = request()?;
    let line = r#"{
        "protocol_version":5,
        "request_id":7,
        "status":"ok",
        "evidence":{
            "fits_bounds":true,
            "used_rect":{"x":20.0,"y":50.0,"width":700.0,"height":300.0},
            "line_count":4,
            "metric_style_run_count":1,
            "fitted_utf16_range":{"location":0,"length":12},
            "input_utf16_length":12,
            "effective_scale":1.0,
            "resolved_fonts":[{
                "postscript_name":"Helvetica-Bold",
                "family_name":"Helvetica",
                "point_size":72.0,
                "font_program_path":"/System/Library/Fonts/Helvetica.ttc",
                "font_program_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            }],
            "native_layout_runtime":{
                "operating_system":"Version 26.6 (Build 25G5065a)",
                "appkit":"2685.70.101",
                "core_text":"877.6.0.2"
            }
        }
    }"#;
    let evidence = NativeTextFitOracle::decode_response(7, &request, line)?;
    assert!(evidence.fits_bounds());
    assert_eq!(evidence.line_count(), 4);
    assert_eq!(evidence.metric_style_run_count(), 1);
    assert_eq!(evidence.fitted_utf16_range().length(), 12);

    let inconsistent = line.replace("\"length\":12", "\"length\":11");
    assert!(matches!(
        NativeTextFitOracle::decode_response(7, &request, &inconsistent),
        Err(TextFitError::HelperProtocol(_))
    ));

    let outside_left_edge = line.replace("\"x\":20.0", "\"x\":-1.0");
    assert!(matches!(
        NativeTextFitOracle::decode_response(7, &request, &outside_left_edge),
        Err(TextFitError::HelperProtocol(_))
    ));

    let relative_font_path = line.replace(
        "/System/Library/Fonts/Helvetica.ttc",
        "relative/Helvetica.ttc",
    );
    assert!(matches!(
        NativeTextFitOracle::decode_response(7, &request, &relative_font_path),
        Err(TextFitError::HelperProtocol(_))
    ));
    Ok(())
}

#[test]
fn protocol_maps_missing_fonts_to_typed_error() -> Result<(), Box<dyn std::error::Error>> {
    let request = request()?;
    let line = r#"{
        "protocol_version":5,
        "request_id":9,
        "status":"error",
        "error":{
            "code":"missing_font",
            "message":"required fonts are unavailable",
            "details":["Church Sans"]
        }
    }"#;
    assert!(matches!(
        NativeTextFitOracle::decode_response(9, &request, line),
        Err(TextFitError::MissingFonts(fonts)) if fonts == ["Church Sans"]
    ));
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn bundled_helper_measures_exact_final_rtf() -> Result<(), Box<dyn std::error::Error>> {
    let request = request()?;
    let mut oracle = NativeTextFitOracle::start_bundled()?;
    let contract = oracle.contract().clone();
    let evidence = oracle.measure(&request)?;

    assert!(evidence.fits_bounds());
    assert_eq!(evidence.line_count(), 1);
    assert_eq!(evidence.metric_style_run_count(), 1);
    assert_eq!(
        evidence.fitted_utf16_range().length(),
        evidence.input_utf16_length()
    );
    assert!((evidence.effective_scale() - 1.0).abs() <= f64::EPSILON);
    assert!(evidence
        .resolved_fonts()
        .iter()
        .any(|font| font.family_name() == "Helvetica"));
    assert!(evidence.resolved_fonts().iter().all(|font| {
        font.font_program_path().is_absolute()
            && font.font_program_sha256().len() == 64
            && font
                .font_program_sha256()
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
    }));
    assert_eq!(contract.helper_sha256().len(), 64);
    assert_eq!(contract, *oracle.contract());
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn bundled_helper_rejects_native_height_overflow() -> Result<(), Box<dyn std::error::Error>> {
    let overflowing = TextFitRequest::new(
        FinalRtf::new(
            br"{\rtf1\ansi\deff0{\fonttbl{\f0\fswiss Helvetica;}}\f0\fs96 Alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu}"
                .to_vec(),
        )?,
        TextBoxGeometry::new(
            160.0,
            55.0,
            TextMargins::new(0.0, 0.0, 0.0, 0.0)?,
        )?,
        TextScaleBehavior::None,
        TextTransform::None,
        TextVerticalAlignment::Top,
        RequiredFonts::new(["Helvetica"] )?,
    )?;
    let mut oracle = NativeTextFitOracle::start_bundled()?;

    let evidence = oracle.measure(&overflowing)?;

    assert!(!evidence.fits_bounds());
    assert!(evidence.line_count() > 1);
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn adjusted_height_fits_only_within_its_canvas_safe_envelope(
) -> Result<(), Box<dyn std::error::Error>> {
    use rv_data::graphics::text::ScaleBehavior;

    let fixed = native_multiline_request(ScaleBehavior::None, 100.0, 55.0, 400.0)?;
    let adjusted =
        native_multiline_request(ScaleBehavior::AdjustContainerHeight, 100.0, 55.0, 400.0)?;
    assert!(matches!(
        adjusted.scale_behavior,
        TextScaleBehavior::AdjustContainerHeight {
            maximum_content_height
        } if (maximum_content_height - 155.0).abs() <= f64::EPSILON
    ));

    let mut oracle = NativeTextFitOracle::start_bundled()?;
    let fixed_evidence = oracle.measure(&fixed)?;
    let adjusted_evidence = oracle.measure(&adjusted)?;

    assert!(!fixed_evidence.fits_bounds());
    assert!(adjusted_evidence.fits_bounds());
    assert!(adjusted_evidence.used_rect().height() > 55.0);
    assert!(adjusted_evidence.used_rect().height() <= 155.0);
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn adjusted_height_rejects_content_beyond_the_canvas_safe_envelope(
) -> Result<(), Box<dyn std::error::Error>> {
    use rv_data::graphics::text::ScaleBehavior;

    let request =
        native_multiline_request(ScaleBehavior::AdjustContainerHeight, 10.0, 55.0, 120.0)?;
    assert!(matches!(
        request.scale_behavior,
        TextScaleBehavior::AdjustContainerHeight {
            maximum_content_height
        } if (maximum_content_height - 65.0).abs() <= f64::EPSILON
    ));

    let mut oracle = NativeTextFitOracle::start_bundled()?;
    let evidence = oracle.measure(&request)?;

    assert!(!evidence.fits_bounds());
    assert!(evidence.used_rect().height() > 65.0);
    Ok(())
}

#[test]
fn adjusted_height_requires_valid_on_canvas_geometry() {
    use rv_data::graphics::text::ScaleBehavior;

    let missing_canvas = {
        let text = rv_data::graphics::Text {
            rtf_data: br"{\rtf1\ansi\deff0{\fonttbl{\f0\fswiss Helvetica;}}\f0\fs96 Text}".to_vec(),
            scale_behavior: ScaleBehavior::AdjustContainerHeight as i32,
            ..rv_data::graphics::Text::default()
        };
        let graphics = rv_data::graphics::Element {
            bounds: Some(rv_data::graphics::Rect {
                origin: Some(rv_data::graphics::Point {
                    x: Some(0.0),
                    y: 10.0,
                }),
                size: Some(rv_data::graphics::Size {
                    width: 800.0,
                    height: 55.0,
                }),
            }),
            text: Some(text.clone()),
            ..rv_data::graphics::Element::default()
        };
        TextFitRequest::from_native_element(&graphics, &text)
    };
    assert!(matches!(
        missing_canvas,
        Err(NativeTextRequestError::MissingDynamicContainerCanvas)
    ));

    let outside_canvas =
        native_multiline_request(ScaleBehavior::AdjustContainerHeight, 80.0, 55.0, 120.0);
    assert!(matches!(
        outside_canvas,
        Err(NativeTextRequestError::DynamicContainerOutsideCanvas {
            y: 80.0,
            height: 55.0,
            canvas_height: 120.0,
        })
    ));
}

#[cfg(target_os = "macos")]
#[test]
fn bundled_helper_reports_metric_runs_but_ignores_color_only_runs(
) -> Result<(), Box<dyn std::error::Error>> {
    let mixed_metrics = TextFitRequest::new(
        FinalRtf::new(
            br"{\rtf1\ansi\deff0{\fonttbl{\f0\fswiss Helvetica;}}{\colortbl;\red255\green255\blue255;\red255\green255\blue0;}\f0\fs96\cf1 Plain \cf2 color {\b bold}}"
                .to_vec(),
        )?,
        geometry()?,
        TextScaleBehavior::None,
        TextTransform::None,
        TextVerticalAlignment::Top,
        RequiredFonts::new(["Helvetica"] )?,
    )?;
    let mut oracle = NativeTextFitOracle::start_bundled()?;

    let evidence = oracle.measure(&mixed_metrics)?;

    assert_eq!(evidence.metric_style_run_count(), 2);
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn bundled_helper_ignores_metric_differences_confined_to_whitespace(
) -> Result<(), Box<dyn std::error::Error>> {
    let whitespace_only_metrics = TextFitRequest::new(
        FinalRtf::new(
            br"{\rtf1\ansi\deff0{\fonttbl{\f0\fswiss Helvetica;}{\f1\froman Times New Roman;}}\f0\fs96 Alpha{\f1\b\i\fs144  \line  }Beta}"
                .to_vec(),
        )?,
        geometry()?,
        TextScaleBehavior::None,
        TextTransform::None,
        TextVerticalAlignment::Top,
        RequiredFonts::new(["Helvetica", "Times New Roman"])?,
    )?;
    let mut oracle = NativeTextFitOracle::start_bundled()?;

    let evidence = oracle.measure(&whitespace_only_metrics)?;

    assert_eq!(evidence.metric_style_run_count(), 1);
    Ok(())
}

#[test]
fn source_destination_evidence_distinguishes_semantic_roles(
) -> Result<(), Box<dyn std::error::Error>> {
    let title = TextFitDestinationIdentity::SourceTheme {
        cue_role: "title".to_string(),
        field: "body".to_string(),
        theme_slide_uuid: Some("title-slide".to_string()),
    };
    let content = TextFitDestinationIdentity::SourceTheme {
        cue_role: "content".to_string(),
        field: "body".to_string(),
        theme_slide_uuid: Some("content-slide".to_string()),
    };
    let title_json = serde_json::to_value(title)?;
    let content_json = serde_json::to_value(content)?;
    assert_eq!(title_json["cue_role"], "title");
    assert_eq!(content_json["cue_role"], "content");
    assert_ne!(title_json, content_json);
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn stalled_helper_times_out_and_permanently_poisons_the_session(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_root, executable) = executable_script("exec /bin/sleep 5")?;
    let mut oracle =
        NativeTextFitOracle::start_with_timeout(&executable, std::time::Duration::from_millis(50))?;

    assert!(matches!(
        oracle.measure(&request()?),
        Err(TextFitError::ResponseTimeout { .. })
    ));
    assert!(matches!(
        oracle.measure(&request()?),
        Err(TextFitError::SessionPoisoned)
    ));
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn oversized_helper_frame_is_rejected_without_unbounded_buffering(
) -> Result<(), Box<dyn std::error::Error>> {
    let body = "/usr/bin/head -c 1048577 /dev/zero | /usr/bin/tr '\\000' x; printf '\\n'";
    let (_root, executable) = executable_script(body)?;
    let mut oracle =
        NativeTextFitOracle::start_with_timeout(&executable, std::time::Duration::from_secs(2))?;

    assert!(matches!(
        oracle.measure(&request()?),
        Err(TextFitError::ResponseFrameTooLarge { limit: 1_048_576 })
    ));
    assert!(matches!(
        oracle.measure(&request()?),
        Err(TextFitError::SessionPoisoned)
    ));
    Ok(())
}
