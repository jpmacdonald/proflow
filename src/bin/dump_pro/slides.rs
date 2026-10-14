//! Print slide graphics, bounds, and text attributes.

use super::rv_data;

pub fn dump_presentation_slide(
    slide: &rv_data::PresentationSlide,
    parent_prefix: &str,
    child_prefix: &str,
) {
    println!("{parent_prefix}     {child_prefix}     ├─ PresentationSlide:");

    if let Some(notes) = &slide.notes {
        let rtf_preview = String::from_utf8_lossy(&notes.rtf_data);
        let preview: String = rtf_preview.chars().take(50).collect();
        println!("{parent_prefix}     {child_prefix}     │  ├─ Notes RTF: \"{preview}...\"");
    }

    println!(
        "{parent_prefix}     {child_prefix}     │  ├─ Transition: {:?}",
        slide.transition.is_some()
    );
    println!(
        "{parent_prefix}     {child_prefix}     │  └─ Guidelines: {} items",
        slide.template_guidelines.len()
    );

    if let Some(base_slide) = &slide.base_slide {
        dump_base_slide(base_slide, parent_prefix, child_prefix);
    }
}

fn dump_base_slide(slide: &rv_data::Slide, parent_prefix: &str, child_prefix: &str) {
    println!("{parent_prefix}     {child_prefix}     └─ BaseSlide:");
    println!(
        "{parent_prefix}     {child_prefix}        ├─ UUID: {:?}",
        slide.uuid.as_ref().map(|u| &u.string)
    );
    println!(
        "{parent_prefix}     {child_prefix}        ├─ Size: {:?}",
        slide
            .size
            .as_ref()
            .map(|s| format!("{}x{}", s.width, s.height))
    );
    println!(
        "{parent_prefix}     {child_prefix}        ├─ Draws BG: {}",
        slide.draws_background_color
    );

    if let Some(bg) = &slide.background_color {
        println!("{parent_prefix}     {child_prefix}        ├─ BG Color: r={:.2} g={:.2} b={:.2} a={:.2}",
            bg.red, bg.green, bg.blue, bg.alpha);
    }

    println!(
        "{parent_prefix}     {child_prefix}        └─ Elements ({}):",
        slide.elements.len()
    );

    for (i, element) in slide.elements.iter().enumerate() {
        let is_last = i == slide.elements.len() - 1;
        let elem_prefix = if is_last { "└" } else { "├" };

        println!("{parent_prefix}     {child_prefix}           {elem_prefix}─ SlideElement {i}:");
        println!(
            "{parent_prefix}     {child_prefix}              ├─ Info: {} (flags)",
            element.info
        );
        println!(
            "{parent_prefix}     {child_prefix}              ├─ Reveal: type={} from_idx={}",
            element.reveal_type, element.reveal_from_index
        );
        println!(
            "{parent_prefix}     {child_prefix}              ├─ DataLinks: {} items",
            element.data_links.len()
        );

        if let Some(graphics_element) = &element.element {
            dump_graphics_element(graphics_element, parent_prefix, child_prefix);
        }
    }
}

#[allow(clippy::cast_possible_truncation)]
#[allow(clippy::too_many_lines)]
fn dump_graphics_element(
    elem: &rv_data::graphics::Element,
    parent_prefix: &str,
    child_prefix: &str,
) {
    println!("{parent_prefix}     {child_prefix}              └─ Graphics.Element:");
    println!(
        "{parent_prefix}     {child_prefix}                 ├─ Name: \"{}\"",
        elem.name
    );
    println!(
        "{parent_prefix}     {child_prefix}                 ├─ UUID: {:?}",
        elem.uuid.as_ref().map(|u| &u.string)
    );

    if let Some(bounds) = &elem.bounds {
        if let (Some(origin), Some(size)) = (&bounds.origin, &bounds.size) {
            println!(
                "{parent_prefix}     {child_prefix}                 ├─ Bounds: ({:.0},{:.0}) {}x{}",
                origin.x.unwrap_or_default(),
                origin.y,
                size.width as i32,
                size.height as i32
            );
        }
    }

    println!(
        "{parent_prefix}     {child_prefix}                 ├─ Rotation: {}",
        elem.rotation
    );
    println!(
        "{parent_prefix}     {child_prefix}                 ├─ Opacity: {}",
        elem.opacity
    );
    println!(
        "{parent_prefix}     {child_prefix}                 ├─ Locked: {}",
        elem.locked
    );
    println!(
        "{parent_prefix}     {child_prefix}                 ├─ Hidden: {}",
        elem.hidden
    );
    println!(
        "{parent_prefix}     {child_prefix}                 ├─ FlipMode: {}",
        elem.flip_mode
    );

    // Fill
    if let Some(fill) = &elem.fill {
        println!(
            "{parent_prefix}     {child_prefix}                 ├─ Fill: enabled={}",
            fill.enable
        );
        if let Some(fill_type) = &fill.fill_type {
            match fill_type {
                rv_data::graphics::fill::FillType::Color(c) => {
                    println!("{parent_prefix}     {child_prefix}                 │  └─ Color: r={:.2} g={:.2} b={:.2} a={:.2}",
                        c.red, c.green, c.blue, c.alpha);
                }
                rv_data::graphics::fill::FillType::Gradient(_) => {
                    println!("{parent_prefix}     {child_prefix}                 │  └─ Gradient");
                }
                rv_data::graphics::fill::FillType::Media(_) => {
                    println!("{parent_prefix}     {child_prefix}                 │  └─ Media");
                }
                rv_data::graphics::fill::FillType::BackgroundEffect(_) => {
                    println!(
                        "{parent_prefix}     {child_prefix}                 │  └─ BackgroundEffect"
                    );
                }
            }
        }
    }

    // Stroke
    if let Some(stroke) = &elem.stroke {
        println!(
            "{parent_prefix}     {child_prefix}                 ├─ Stroke: enabled={} width={}",
            stroke.enable, stroke.width
        );
    }

    // Shadow
    if let Some(shadow) = &elem.shadow {
        println!("{parent_prefix}     {child_prefix}                 ├─ Shadow: enabled={} angle={} offset={} radius={}",
            shadow.enable, shadow.angle, shadow.offset, shadow.radius);
    }

    // Path/Shape
    if let Some(path) = &elem.path {
        if let Some(shape) = &path.shape {
            println!(
                "{parent_prefix}     {child_prefix}                 ├─ Shape: type={}",
                shape.r#type
            );
        }
    }

    // Text (most important for debugging)
    if let Some(text) = &elem.text {
        println!("{parent_prefix}     {child_prefix}                 └─ Text:");

        // RTF data preview
        let rtf_preview = String::from_utf8_lossy(&text.rtf_data);
        let preview: String = rtf_preview.chars().take(80).collect();
        println!("{parent_prefix}     {child_prefix}                    ├─ RTF ({} bytes): \"{preview}...\"",
            text.rtf_data.len());

        println!(
            "{parent_prefix}     {child_prefix}                    ├─ VertAlign: {}",
            text.vertical_alignment
        );
        println!(
            "{parent_prefix}     {child_prefix}                    ├─ ScaleBehavior: {}",
            text.scale_behavior
        );
        println!(
            "{parent_prefix}     {child_prefix}                    ├─ SuperscriptStd: {}",
            text.is_superscript_standardized
        );
        println!(
            "{parent_prefix}     {child_prefix}                    ├─ Transform: {}",
            text.transform
        );

        if let Some(margins) = &text.margins {
            println!("{parent_prefix}     {child_prefix}                    ├─ Margins: L={} R={} T={} B={}",
                margins.left, margins.right, margins.top, margins.bottom);
        }

        // Text Shadow
        if let Some(shadow) = &text.shadow {
            println!("{parent_prefix}     {child_prefix}                    ├─ TextShadow: enabled={} angle={} offset={}",
                shadow.enable, shadow.angle, shadow.offset);
        }

        // Attributes (font, color, etc)
        if let Some(attrs) = &text.attributes {
            println!("{parent_prefix}     {child_prefix}                    └─ Attributes:");

            if let Some(font) = &attrs.font {
                println!("{parent_prefix}     {child_prefix}                       ├─ Font: \"{}\" size={} bold={}",
                    font.name, font.size, font.bold);
                println!("{parent_prefix}     {child_prefix}                       │  italic={} family=\"{}\" face=\"{}\"",
                    font.italic, font.family, font.face);
            }

            // Text fill color
            if let Some(fill) = &attrs.fill {
                match fill {
                    rv_data::graphics::text::attributes::Fill::TextSolidFill(c) => {
                        println!("{parent_prefix}     {child_prefix}                       ├─ TextFill: r={:.2} g={:.2} b={:.2} a={:.2}",
                            c.red, c.green, c.blue, c.alpha);
                    }
                    rv_data::graphics::text::attributes::Fill::TextGradientFill(_) => {
                        println!("{parent_prefix}     {child_prefix}                       ├─ TextFill: Gradient");
                    }
                    _ => {
                        println!("{parent_prefix}     {child_prefix}                       ├─ TextFill: (other)");
                    }
                }
            }

            // Paragraph style
            if let Some(para) = &attrs.paragraph_style {
                println!("{parent_prefix}     {child_prefix}                       ├─ Paragraph: align={} lineHeightMultiple={} lineSpacing={} minLineHeight={} maxLineHeight={} paraSpacing={} paraSpacingBefore={} firstLineIndent={} headIndent={} tailIndent={}",
                    para.alignment, para.line_height_multiple, para.line_spacing,
                    para.minimum_line_height, para.maximum_line_height,
                    para.paragraph_spacing, para.paragraph_spacing_before,
                    para.first_line_head_indent, para.head_indent, para.tail_indent);
            }

            println!(
                "{parent_prefix}     {child_prefix}                       ├─ Kerning: {}",
                attrs.kerning
            );
            println!(
                "{parent_prefix}     {child_prefix}                       └─ Superscript: {}",
                attrs.superscript
            );
        }
    }
}
