use super::{
    ActionLabelSignature, BibleReferenceSummary, CueStructureSummary, IntRangeSummary,
    TextStyleSignature,
};
use crate::propresenter::generated::rv_data::{self, action};
use crate::propresenter::macros::macro_action_name;
use crate::propresenter::native_url;
use crate::propresenter::rtf::{extract_text_options, rtf_to_text};

pub(super) fn summarize_cue(
    index: usize,
    cue: &rv_data::Cue,
    mut group_names: Vec<String>,
) -> CueStructureSummary {
    group_names.sort();

    let text = cue_text(cue);
    CueStructureSummary {
        index,
        uuid: cue.uuid.as_ref().map(|uuid| uuid.string.clone()),
        name: cue.name.clone(),
        group_names,
        text_lines: text.lines().map(str::to_string).collect(),
        is_blank: !text.chars().any(char::is_alphanumeric),
        text,
        macros: cue
            .actions
            .iter()
            .filter_map(macro_action_name)
            .map(str::to_string)
            .collect(),
        slide_labels: cue.actions.iter().filter_map(slide_action_label).collect(),
        background_media: cue
            .actions
            .iter()
            .filter_map(background_media_basename)
            .collect(),
        action_kinds: cue.actions.iter().map(action_kind).collect(),
        text_styles: cue_text_styles(cue),
    }
}

pub(super) fn summarize_bible_reference(
    reference: &rv_data::presentation::BibleReference,
) -> BibleReferenceSummary {
    BibleReferenceSummary {
        book_index: reference.book_index,
        book_name: reference.book_name.clone(),
        chapter_range: reference.chapter_range.as_ref().map(summarize_int_range),
        verse_range: reference.verse_range.as_ref().map(summarize_int_range),
        translation_name: reference.translation_name.clone(),
        translation_display_abbreviation: reference.translation_display_abbreviation.clone(),
        translation_internal_abbreviation: reference.translation_internal_abbreviation.clone(),
        book_key: reference.book_key.clone(),
    }
}

const fn summarize_int_range(range: &rv_data::IntRange) -> IntRangeSummary {
    IntRangeSummary {
        start: range.start,
        end: range.end,
    }
}

fn slide_action_label(action: &rv_data::Action) -> Option<ActionLabelSignature> {
    if !matches!(
        &action.action_type_data,
        Some(action::ActionTypeData::Slide(_))
    ) {
        return None;
    }
    action.label.as_ref().map(|label| ActionLabelSignature {
        text: label.text.clone(),
        color: label.color.as_ref().map(color_signature),
    })
}

fn cue_text_styles(cue: &rv_data::Cue) -> Vec<TextStyleSignature> {
    let mut styles = Vec::new();
    for action in &cue.actions {
        let Some(action::ActionTypeData::Slide(slide_type)) = &action.action_type_data else {
            continue;
        };
        let Some(action::slide_type::Slide::Presentation(slide)) = &slide_type.slide else {
            continue;
        };
        let Some(base_slide) = &slide.base_slide else {
            continue;
        };
        let slide_size = base_slide.size.as_ref().map(size_signature);
        for element in &base_slide.elements {
            let Some(graphics) = &element.element else {
                continue;
            };
            let Some(text) = &graphics.text else {
                continue;
            };
            styles.push(text_style_signature(graphics, text, slide_size.as_deref()));
        }
    }
    styles
}

fn text_style_signature(
    graphics: &rv_data::graphics::Element,
    text: &rv_data::graphics::Text,
    slide_size: Option<&str>,
) -> TextStyleSignature {
    let rtf_options = extract_text_options(text);
    let font = text
        .attributes
        .as_ref()
        .and_then(|attributes| attributes.font.as_ref());
    let fill_color = text
        .attributes
        .as_ref()
        .and_then(|attributes| attributes.fill.as_ref())
        .and_then(text_fill_color);

    TextStyleSignature {
        element_name: graphics.name.clone(),
        bounds: graphics.bounds.as_ref().map(rect_signature),
        slide_size: slide_size.map(str::to_string),
        font_name: Some(
            font.map(|font| {
                if font.name.is_empty() {
                    font.family.clone()
                } else {
                    font.name.clone()
                }
            })
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| rtf_options.font_name.clone()),
        ),
        font_size: font
            .and_then(|font| rounded_font_size(font.size))
            .or(Some(rtf_options.font_size)),
        color: fill_color.or_else(|| Some(rgb_signature(rtf_options.color))),
        bold: font.map(|font| font.bold).or(Some(rtf_options.bold)),
        italic: font.map(|font| font.italic).or(Some(rtf_options.italic)),
        vertical_alignment: enum_suffix(
            rv_data::graphics::text::VerticalAlignment::try_from(text.vertical_alignment)
                .ok()
                .map_or("UNKNOWN", |value| value.as_str_name()),
        ),
        scale_behavior: enum_suffix(
            rv_data::graphics::text::ScaleBehavior::try_from(text.scale_behavior)
                .ok()
                .map_or("UNKNOWN", |value| value.as_str_name()),
        ),
        transform: enum_suffix(
            rv_data::graphics::text::Transform::try_from(text.transform)
                .ok()
                .map_or("UNKNOWN", |value| value.as_str_name()),
        ),
        margins: text.margins.as_ref().map(edge_insets_signature),
    }
}

fn rounded_font_size(value: f64) -> Option<u32> {
    let rounded = value.round();
    if !rounded.is_finite() || !(1.0..=f64::from(u32::MAX)).contains(&rounded) {
        return None;
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some(rounded as u32)
}

fn text_fill_color(fill: &rv_data::graphics::text::attributes::Fill) -> Option<String> {
    match fill {
        rv_data::graphics::text::attributes::Fill::TextSolidFill(color) => {
            Some(color_signature(color))
        }
        _ => None,
    }
}

pub(super) fn color_signature(color: &rv_data::Color) -> String {
    format!(
        "#{:02X}{:02X}{:02X}{:02X}",
        color_component(color.red),
        color_component(color.green),
        color_component(color.blue),
        color_component(color.alpha)
    )
}

fn rgb_signature(color: (u8, u8, u8)) -> String {
    format!("#{:02X}{:02X}{:02X}", color.0, color.1, color.2)
}

fn color_component(value: f32) -> u8 {
    if !value.is_finite() {
        return 0;
    }
    let value = if value <= 1.0 { value * 255.0 } else { value };
    let rounded = value.clamp(0.0, 255.0).round();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        rounded as u8
    }
}

fn rect_signature(rect: &rv_data::graphics::Rect) -> String {
    let origin = rect.origin.as_ref();
    let size = rect.size.as_ref();
    format!(
        "{},{},{},{}",
        format_coord(origin.and_then(|origin| origin.x).unwrap_or_default()),
        format_coord(origin.map_or(0.0, |origin| origin.y)),
        format_coord(size.map_or(0.0, |size| size.width)),
        format_coord(size.map_or(0.0, |size| size.height))
    )
}

fn size_signature(size: &rv_data::graphics::Size) -> String {
    format!("{}x{}", format_coord(size.width), format_coord(size.height))
}

fn edge_insets_signature(insets: &rv_data::graphics::EdgeInsets) -> String {
    format!(
        "{},{},{},{}",
        format_coord(insets.left),
        format_coord(insets.right),
        format_coord(insets.top),
        format_coord(insets.bottom)
    )
}

fn format_coord(value: f64) -> String {
    format!("{value:.1}")
}

fn enum_suffix(value: &str) -> String {
    value.rsplit('_').next().unwrap_or(value).to_lowercase()
}

fn cue_text(cue: &rv_data::Cue) -> String {
    let mut texts = Vec::new();
    for action in &cue.actions {
        let Some(action::ActionTypeData::Slide(slide_type)) = &action.action_type_data else {
            continue;
        };
        let Some(action::slide_type::Slide::Presentation(slide)) = &slide_type.slide else {
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
            if let Some(text) = rtf_to_text(&rtf) {
                texts.push(text.replace("\r\n", "\n").replace('\r', "\n"));
            }
        }
    }
    texts.join("\n\n")
}

fn action_kind(action: &rv_data::Action) -> String {
    match &action.action_type_data {
        Some(action::ActionTypeData::Slide(_)) => "slide".to_string(),
        Some(action::ActionTypeData::Macro(_)) => macro_action_name(action)
            .map_or_else(|| "macro".to_string(), |name| format!("macro:{name}")),
        Some(action::ActionTypeData::Media(media)) => {
            let layer = action::LayerType::try_from(media.layer_type)
                .ok()
                .map_or_else(
                    || media.layer_type.to_string(),
                    |layer| {
                        layer
                            .as_str_name()
                            .trim_start_matches("LAYER_TYPE_")
                            .to_lowercase()
                    },
                );
            format!("media:{layer}")
        }
        Some(_) => format!("other:{}", action.r#type),
        None => format!("none:{}", action.r#type),
    }
}

fn background_media_basename(action: &rv_data::Action) -> Option<String> {
    let Some(action::ActionTypeData::Media(media_type)) = &action.action_type_data else {
        return None;
    };
    if action.r#type != action::ActionType::BackgroundMedia as i32
        && media_type.layer_type != action::LayerType::Background as i32
    {
        return None;
    }
    media_type
        .element
        .as_ref()
        .and_then(|media| media.url.as_ref())
        .and_then(native_url::preferred_source)
        .and_then(native_url::decoded_basename_lossy)
}
