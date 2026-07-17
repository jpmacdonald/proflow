//! Native text-style resolution and structured RTF style scanning.

use std::{collections::BTreeMap, fmt::Write};

use super::{is_ignored_destination, RtfFontFamily, RtfOptions};
use crate::propresenter::generated::rv_data;

/// Extract RTF options from existing RTF data.
///
/// Parses RTF to extract font name, size, and color settings.
/// Missing or malformed controls retain the native generation defaults, so
/// callers always receive one complete style baseline.
pub fn extract_rtf_options(rtf_data: &[u8]) -> RtfOptions {
    let rtf = String::from_utf8_lossy(rtf_data);
    let parsed = RtfStyleScan::parse(&rtf);
    let mut options = RtfOptions::default();
    if let Some(version) = parsed.cocoa_rtf_version {
        options.cocoa_rtf_version = version;
    }
    if let Some(paragraph_controls) = parsed.paragraph_controls {
        options.paragraph_controls = paragraph_controls;
    }
    let run = parsed
        .first_visible_run
        .as_ref()
        .unwrap_or(&parsed.fallback_run);
    let font = run
        .font_index
        .or(parsed.default_font_index)
        .and_then(|index| parsed.fonts.get(&index))
        .or_else(|| parsed.fonts.get(&0))
        .or_else(|| parsed.fonts.values().next());
    if let Some(font) = font {
        if !font.name.is_empty() {
            options.font_name.clone_from(&font.name);
        }
        if let Some(family) = font.family {
            options.font_family = family;
        }
    }
    if let Some(half_points) = run.font_size_half_points.filter(|size| *size >= 2) {
        options.font_size = half_points / 2;
    }
    options.color = run
        .color_index
        .and_then(|index| parsed.colors.get(index).copied().flatten())
        .or_else(|| parsed.colors.iter().flatten().copied().next())
        .unwrap_or(options.color);
    if let Some(kerning) = run.kerning {
        options.kerning = kerning;
    }
    if let Some(bold) = run.bold {
        options.bold = bold;
    }
    if let Some(italic) = run.italic {
        options.italic = italic;
    }

    options
}

/// Resolve the effective RTF baseline for one native text element.
///
/// Native attributes own values represented in the protobuf. The RTF scan is
/// the fallback and remains the source for Cocoa, paragraph, and font-family
/// controls that the native attribute message does not encode.
pub fn extract_text_options(text: &rv_data::graphics::Text) -> RtfOptions {
    let mut options = extract_rtf_options(&text.rtf_data);
    let Some(attributes) = text.attributes.as_ref() else {
        return options;
    };
    if let Some(font) = attributes.font.as_ref() {
        if let Some(name) = [font.name.as_str(), font.family.as_str()]
            .into_iter()
            .find(|name| !name.trim().is_empty())
        {
            options.font_name = name.to_string();
        }
        if let Some(size) = rounded_positive_u32(font.size) {
            options.font_size = size;
        }
        options.bold = font.bold;
        options.italic = font.italic;
    }
    if let Some(color) = attributes.fill.as_ref().and_then(native_solid_text_color) {
        options.color = color;
    }
    options
}

#[derive(Debug, Clone, Copy, Default)]
struct RtfRunStyle {
    font_index: Option<i32>,
    font_size_half_points: Option<u32>,
    color_index: Option<usize>,
    kerning: Option<i32>,
    bold: Option<bool>,
    italic: Option<bool>,
}

#[derive(Debug, Clone)]
struct RtfFontEntry {
    name: String,
    family: Option<RtfFontFamily>,
}

#[derive(Debug, Default)]
struct RtfFontEntryBuilder {
    index: Option<i32>,
    name: String,
    family: Option<RtfFontFamily>,
}

#[derive(Debug, Default)]
struct RtfColorBuilder {
    red: Option<u8>,
    green: Option<u8>,
    blue: Option<u8>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum RtfStyleDestination {
    #[default]
    Content,
    FontTable,
    ColorTable,
    Ignored,
}

#[derive(Debug, Clone, Default)]
struct RtfStyleState {
    destination: RtfStyleDestination,
    run: RtfRunStyle,
    group_start: bool,
}

#[derive(Debug, Default)]
struct RtfStyleScan {
    cocoa_rtf_version: Option<u32>,
    paragraph_controls: Option<String>,
    paragraph_capture: Option<String>,
    default_font_index: Option<i32>,
    fonts: BTreeMap<i32, RtfFontEntry>,
    font_entry: Option<RtfFontEntryBuilder>,
    colors: Vec<Option<(u8, u8, u8)>>,
    color: RtfColorBuilder,
    first_visible_run: Option<RtfRunStyle>,
    fallback_run: RtfRunStyle,
}

impl RtfStyleScan {
    fn parse(rtf: &str) -> Self {
        let mut scan = Self::default();
        let mut state = RtfStyleState {
            group_start: true,
            ..RtfStyleState::default()
        };
        let mut stack = Vec::new();
        let mut index = 0;
        while index < rtf.len() {
            match rtf.as_bytes()[index] {
                b'{' => {
                    stack.push(state.clone());
                    state.group_start = true;
                    index += 1;
                }
                b'}' => {
                    state = stack.pop().unwrap_or_default();
                    index += 1;
                }
                b'\\' => {
                    let Some((control, next)) = parse_style_control(rtf, index) else {
                        index += 1;
                        continue;
                    };
                    scan.handle_control(&mut state, control);
                    index = next;
                }
                _ => {
                    let start = index;
                    while index < rtf.len() && !matches!(rtf.as_bytes()[index], b'{' | b'}' | b'\\')
                    {
                        index += 1;
                    }
                    scan.handle_text(&mut state, &rtf[start..index]);
                }
            }
        }
        scan.finish_paragraph_capture();
        scan.finish_font_entry();
        scan
    }

    fn handle_control(&mut self, state: &mut RtfStyleState, control: RtfStyleControl<'_>) {
        if matches!(control, RtfStyleControl::Symbol(b'*')) && state.group_start {
            state.destination = RtfStyleDestination::Ignored;
            state.group_start = false;
            return;
        }
        let RtfStyleControl::Word { name, parameter } = control else {
            if state.destination == RtfStyleDestination::Content
                && matches!(
                    control,
                    RtfStyleControl::Symbol(b'\'' | b'\\' | b'{' | b'}' | b'~' | b'-' | b'_')
                )
            {
                self.mark_visible_run(state.run);
            }
            state.group_start = false;
            return;
        };

        if state.group_start {
            let destination = match name {
                "fonttbl" => Some(RtfStyleDestination::FontTable),
                "colortbl" => Some(RtfStyleDestination::ColorTable),
                _ if is_ignored_destination(name) => Some(RtfStyleDestination::Ignored),
                _ => None,
            };
            if let Some(destination) = destination {
                state.destination = destination;
                state.group_start = false;
                return;
            }
        }
        state.group_start = false;

        match state.destination {
            RtfStyleDestination::Content => self.handle_content_control(state, name, parameter),
            RtfStyleDestination::FontTable => self.handle_font_control(name, parameter),
            RtfStyleDestination::ColorTable => self.handle_color_control(name, parameter),
            RtfStyleDestination::Ignored => {}
        }
    }

    fn handle_content_control(
        &mut self,
        state: &mut RtfStyleState,
        name: &str,
        parameter: Option<i32>,
    ) {
        self.capture_paragraph_control(name, parameter);
        if is_visible_content_control(name) {
            self.mark_visible_run(state.run);
        }
        if name == "cocoartf" {
            self.cocoa_rtf_version = parameter.and_then(|value| u32::try_from(value).ok());
        } else if name == "deff" {
            self.default_font_index = parameter.filter(|value| *value >= 0);
        }

        let changed = match name {
            "plain" => {
                state.run = RtfRunStyle::default();
                true
            }
            "f" => parameter.filter(|value| *value >= 0).is_some_and(|value| {
                state.run.font_index = Some(value);
                true
            }),
            "fs" => parameter
                .and_then(|value| u32::try_from(value).ok())
                .is_some_and(|value| {
                    state.run.font_size_half_points = Some(value);
                    true
                }),
            "cf" => parameter
                .and_then(|value| usize::try_from(value).ok())
                .is_some_and(|value| {
                    state.run.color_index = Some(value);
                    true
                }),
            "expnd" => parameter.is_some_and(|value| {
                state.run.kerning = Some(value);
                true
            }),
            "b" => {
                state.run.bold = Some(parameter != Some(0));
                true
            }
            "i" => {
                state.run.italic = Some(parameter != Some(0));
                true
            }
            _ => false,
        };
        if changed {
            self.fallback_run = state.run;
        }
    }

    fn handle_font_control(&mut self, name: &str, parameter: Option<i32>) {
        if name == "f" {
            if self
                .font_entry
                .as_ref()
                .is_some_and(|entry| entry.index.is_some())
            {
                self.finish_font_entry();
            }
            self.font_entry = Some(RtfFontEntryBuilder {
                index: parameter.filter(|value| *value >= 0),
                ..RtfFontEntryBuilder::default()
            });
        } else if let Some(family) = RtfFontFamily::from_control_word(name) {
            self.font_entry
                .get_or_insert_with(RtfFontEntryBuilder::default)
                .family = Some(family);
        }
    }

    fn handle_color_control(&mut self, name: &str, parameter: Option<i32>) {
        let value = parameter.and_then(|value| u8::try_from(value).ok());
        match name {
            "red" => self.color.red = value,
            "green" => self.color.green = value,
            "blue" => self.color.blue = value,
            _ => {}
        }
    }

    fn handle_text(&mut self, state: &mut RtfStyleState, text: &str) {
        match state.destination {
            RtfStyleDestination::Content => {
                if text.chars().any(|value| !value.is_whitespace()) {
                    self.finish_paragraph_capture();
                    self.mark_visible_run(state.run);
                    state.group_start = false;
                }
            }
            RtfStyleDestination::FontTable => {
                for value in text.chars() {
                    if value == ';' {
                        self.finish_font_entry();
                    } else if !matches!(value, '\r' | '\n') {
                        self.font_entry
                            .get_or_insert_with(RtfFontEntryBuilder::default)
                            .name
                            .push(value);
                    }
                }
            }
            RtfStyleDestination::ColorTable => {
                for _ in text.chars().filter(|value| *value == ';') {
                    self.finish_color();
                }
            }
            RtfStyleDestination::Ignored => {}
        }
    }

    fn capture_paragraph_control(&mut self, name: &str, parameter: Option<i32>) {
        if self.paragraph_controls.is_some() {
            return;
        }
        if self.paragraph_capture.is_none() {
            if name != "pard" {
                return;
            }
            self.paragraph_capture = Some(String::new());
        } else if !is_paragraph_control(name) {
            self.finish_paragraph_capture();
            return;
        }
        if let Some(capture) = &mut self.paragraph_capture {
            capture.push('\\');
            capture.push_str(name);
            if let Some(parameter) = parameter {
                let _ = write!(capture, "{parameter}");
            }
        }
    }

    fn finish_paragraph_capture(&mut self) {
        if self.paragraph_controls.is_none() {
            self.paragraph_controls = self
                .paragraph_capture
                .take()
                .filter(|value| !value.is_empty());
        }
    }

    fn finish_font_entry(&mut self) {
        let Some(entry) = self.font_entry.take() else {
            return;
        };
        let Some(index) = entry.index else {
            return;
        };
        self.fonts.entry(index).or_insert_with(|| RtfFontEntry {
            name: entry.name.trim().to_string(),
            family: entry.family,
        });
    }

    fn finish_color(&mut self) {
        self.colors.push(
            self.color
                .red
                .zip(self.color.green)
                .zip(self.color.blue)
                .map(|((red, green), blue)| (red, green, blue)),
        );
        self.color = RtfColorBuilder::default();
    }

    fn mark_visible_run(&mut self, run: RtfRunStyle) {
        self.first_visible_run.get_or_insert(run);
    }
}

fn is_visible_content_control(name: &str) -> bool {
    matches!(
        name,
        "u" | "bullet" | "emdash" | "endash" | "lquote" | "rquote" | "ldblquote" | "rdblquote"
    )
}

#[derive(Debug, Clone, Copy)]
enum RtfStyleControl<'a> {
    Word {
        name: &'a str,
        parameter: Option<i32>,
    },
    Symbol(u8),
}

fn parse_style_control(rtf: &str, start: usize) -> Option<(RtfStyleControl<'_>, usize)> {
    let bytes = rtf.as_bytes();
    let mut index = start.checked_add(1)?;
    let &first = bytes.get(index)?;
    if !first.is_ascii_alphabetic() {
        index += 1;
        if first == b'\'' {
            index = (index + 2).min(bytes.len());
        } else if first == b'\r' && bytes.get(index) == Some(&b'\n') {
            index += 1;
        }
        return Some((RtfStyleControl::Symbol(first), index));
    }

    let name_start = index;
    while bytes.get(index).is_some_and(u8::is_ascii_alphabetic) {
        index += 1;
    }
    let name = &rtf[name_start..index];
    let parameter_start = index;
    if bytes.get(index) == Some(&b'-') {
        index += 1;
    }
    let digits_start = index;
    while bytes.get(index).is_some_and(u8::is_ascii_digit) {
        index += 1;
    }
    let parameter = (index > digits_start)
        .then(|| rtf[parameter_start..index].parse::<i32>().ok())
        .flatten();
    if bytes.get(index) == Some(&b' ') {
        index += 1;
    }
    Some((RtfStyleControl::Word { name, parameter }, index))
}

fn is_paragraph_control(name: &str) -> bool {
    matches!(
        name,
        "pard"
            | "pardeftab"
            | "li"
            | "fi"
            | "ri"
            | "ql"
            | "qr"
            | "qc"
            | "qj"
            | "sb"
            | "sa"
            | "sl"
            | "slmult"
            | "slleading"
            | "pardirnatural"
            | "partightenfactor"
            | "tx"
            | "tqr"
            | "tqc"
            | "tqdec"
            | "tb"
            | "ltrpar"
            | "rtlpar"
            | "keep"
            | "keepn"
            | "pagebb"
            | "widctlpar"
            | "nowidctlpar"
            | "hyphpar"
            | "outlinelevel"
            | "contextualspace"
            | "nosnaplinegrid"
    )
}

fn rounded_positive_u32(value: f64) -> Option<u32> {
    let rounded = value.round();
    (rounded.is_finite() && (1.0..=f64::from(u32::MAX)).contains(&rounded)).then_some({
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        {
            rounded as u32
        }
    })
}

fn native_solid_text_color(
    fill: &rv_data::graphics::text::attributes::Fill,
) -> Option<(u8, u8, u8)> {
    let rv_data::graphics::text::attributes::Fill::TextSolidFill(color) = fill else {
        return None;
    };
    [color.red, color.green, color.blue]
        .into_iter()
        .all(f32::is_finite)
        .then(|| {
            (
                native_color_component(color.red),
                native_color_component(color.green),
                native_color_component(color.blue),
            )
        })
}

fn native_color_component(value: f32) -> u8 {
    let scaled = if value <= 1.0 { value * 255.0 } else { value };
    let rounded = scaled.clamp(0.0, 255.0).round();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        rounded as u8
    }
}
