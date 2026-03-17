//! Debug tool to dump and analyze `ProPresenter` `.pro` files.
//!
//! Usage:
//!   `cargo run --bin dump_pro -- <file.pro>`
//!   `cargo run --bin dump_pro -- <file1.pro> <file2.pro> --diff`
//!
//! This tool outputs a detailed structure of the presentation for debugging
//! slide generation issues.

// Development/debug binary - allow expect/unwrap for simpler error handling
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::uninlined_format_args,
    clippy::too_many_lines
)]

use proflow::propresenter::generated::rv_data;
use prost::Message;
use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <file.pro> [file2.pro --diff]", args[0]);
        eprintln!("       {} <file.pro> --json", args[0]);
        std::process::exit(1);
    }

    let path = Path::new(&args[1]);

    if args.contains(&"--json".to_string()) {
        dump_json(path);
    } else if args.len() >= 4 && args.contains(&"--diff".to_string()) {
        let path2 = Path::new(&args[2]);
        diff_presentations(path, path2);
    } else {
        dump_presentation(path);
    }
}

fn load_presentation(path: &Path) -> rv_data::Presentation {
    let data = fs::read(path).unwrap_or_else(|e| {
        eprintln!("Failed to read {}: {e}", path.display());
        std::process::exit(1);
    });

    rv_data::Presentation::decode(data.as_slice()).unwrap_or_else(|e| {
        eprintln!("Failed to decode {}: {e}", path.display());
        std::process::exit(1);
    })
}

fn dump_json(path: &Path) {
    let presentation = load_presentation(path);
    println!("{}", serde_json::to_string_pretty(&presentation).unwrap());
}

#[allow(clippy::too_many_lines)]
fn dump_presentation(path: &Path) {
    let presentation = load_presentation(path);

    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!(
        "║ ProPresenter File Analysis: {}",
        path.file_name().unwrap().to_string_lossy()
    );
    println!("╚══════════════════════════════════════════════════════════════════╝");
    println!();

    // Basic info
    println!("📄 PRESENTATION INFO");
    println!("├─ Name: {}", presentation.name);
    println!(
        "├─ UUID: {:?}",
        presentation.uuid.as_ref().map(|u| &u.string)
    );
    println!(
        "├─ Category: {:?}",
        if presentation.category.is_empty() {
            None
        } else {
            Some(&presentation.category)
        }
    );
    println!(
        "└─ Notes: {:?}",
        if presentation.notes.is_empty() {
            None
        } else {
            Some(&presentation.notes)
        }
    );
    println!();

    // Application info
    if let Some(app) = &presentation.application_info {
        println!("🖥️  APPLICATION INFO");
        println!("├─ Application: {}", app.application);
        println!("├─ Platform: {}", app.platform);
        if let Some(v) = &app.application_version {
            println!(
                "└─ Version: {}.{}.{} ({})",
                v.major_version, v.minor_version, v.patch_version, v.build
            );
        }
        println!();
    }

    // Cues
    println!("🎬 CUES ({} total)", presentation.cues.len());
    for (i, cue) in presentation.cues.iter().enumerate() {
        let is_last = i == presentation.cues.len() - 1;
        let prefix = if is_last { "└" } else { "├" };
        let child_prefix = if is_last { " " } else { "│" };

        println!("{prefix}─ Cue {i}: \"{}\"", cue.name);
        println!(
            "{child_prefix}  ├─ UUID: {:?}",
            cue.uuid.as_ref().map(|u| &u.string)
        );
        println!("{child_prefix}  ├─ Enabled: {}", cue.is_enabled);
        println!(
            "{child_prefix}  ├─ Completion: target={:?} action={}",
            cue.completion_target_type, cue.completion_action_type
        );
        println!("{child_prefix}  └─ Actions ({} total):", cue.actions.len());

        for (j, action) in cue.actions.iter().enumerate() {
            let is_last_action = j == cue.actions.len() - 1;
            let action_prefix = if is_last_action { "└" } else { "├" };
            let action_child = if is_last_action { " " } else { "│" };

            println!(
                "{child_prefix}     {action_prefix}─ Action {j}: \"{}\" (type={})",
                action.name, action.r#type
            );
            println!(
                "{child_prefix}     {action_child}  ├─ UUID: {:?}",
                action.uuid.as_ref().map(|u| &u.string)
            );
            println!(
                "{child_prefix}     {action_child}  ├─ Enabled: {}",
                action.is_enabled
            );
            println!(
                "{child_prefix}     {action_child}  ├─ Duration: {}",
                action.duration
            );

            // Analyze action type data
            if let Some(ref type_data) = action.action_type_data {
                dump_action_type_data(type_data, child_prefix, action_child);
            }
        }
    }
    println!();

    // Cue Groups
    println!("📁 CUE GROUPS ({} total)", presentation.cue_groups.len());
    for (i, group) in presentation.cue_groups.iter().enumerate() {
        let is_last = i == presentation.cue_groups.len() - 1;
        let prefix = if is_last { "└" } else { "├" };

        if let Some(g) = &group.group {
            println!("{prefix}─ Group: \"{}\"", g.name);
            println!("   ├─ UUID: {:?}", g.uuid.as_ref().map(|u| &u.string));
            println!(
                "   ├─ Color: r={:.2} g={:.2} b={:.2} a={:.2}",
                g.color.as_ref().map_or(0.0, |c| c.red),
                g.color.as_ref().map_or(0.0, |c| c.green),
                g.color.as_ref().map_or(0.0, |c| c.blue),
                g.color.as_ref().map_or(0.0, |c| c.alpha)
            );
            println!(
                "   ├─ App Group ID: {:?}",
                g.application_group_identifier.as_ref().map(|u| &u.string)
            );
            println!(
                "   └─ Cue IDs: {:?}",
                group
                    .cue_identifiers
                    .iter()
                    .map(|u| &u.string)
                    .collect::<Vec<_>>()
            );
        }
    }
    println!();

    // Arrangements
    println!(
        "🎼 ARRANGEMENTS ({} total)",
        presentation.arrangements.len()
    );
    for (i, arr) in presentation.arrangements.iter().enumerate() {
        let is_last = i == presentation.arrangements.len() - 1;
        let prefix = if is_last { "└" } else { "├" };

        println!("{prefix}─ Arrangement: \"{}\"", arr.name);
        println!("   ├─ UUID: {:?}", arr.uuid.as_ref().map(|u| &u.string));
        println!(
            "   └─ Group IDs: {:?}",
            arr.group_identifiers
                .iter()
                .map(|u| &u.string)
                .collect::<Vec<_>>()
        );
    }

    if let Some(sel) = &presentation.selected_arrangement {
        println!("\n   Selected: {}", sel.string);
    }
}

fn dump_action_type_data(
    type_data: &rv_data::action::ActionTypeData,
    parent_prefix: &str,
    child_prefix: &str,
) {
    match type_data {
        rv_data::action::ActionTypeData::Slide(slide_type) => {
            println!("{parent_prefix}     {child_prefix}  └─ SlideType:");
            if let Some(ref slide) = slide_type.slide {
                match slide {
                    rv_data::action::slide_type::Slide::Presentation(pres_slide) => {
                        dump_presentation_slide(pres_slide, parent_prefix, child_prefix);
                    }
                    rv_data::action::slide_type::Slide::Prop(prop_slide) => {
                        println!(
                            "{parent_prefix}     {child_prefix}     └─ PropSlide (not expanded)"
                        );
                        let _ = prop_slide;
                    }
                }
            }
        }
        rv_data::action::ActionTypeData::Media(media_type) => {
            println!("{parent_prefix}     {child_prefix}  └─ MediaType:");
            if let Some(el) = &media_type.element {
                println!(
                    "{parent_prefix}     {child_prefix}     └─ URL: storage={:?} platform={:?}",
                    el.url.as_ref().map(|u| &u.storage),
                    el.url.as_ref().map(|u| u.platform)
                );
            }
        }
        _ => {
            println!("{parent_prefix}     {child_prefix}  └─ (other action type)");
        }
    }
}

fn dump_presentation_slide(
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
                origin.x, origin.y, size.width as i32, size.height as i32
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
                println!("{parent_prefix}     {child_prefix}                       ├─ Paragraph: align={} lineHeight={}",
                    para.alignment, para.line_height_multiple);
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

fn diff_presentations(path1: &Path, path2: &Path) {
    let pres1 = load_presentation(path1);
    let pres2 = load_presentation(path2);

    let json1 = serde_json::to_string_pretty(&pres1).unwrap();
    let json2 = serde_json::to_string_pretty(&pres2).unwrap();

    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║ Comparing:");
    println!("║ 1: {}", path1.file_name().unwrap().to_string_lossy());
    println!("║ 2: {}", path2.file_name().unwrap().to_string_lossy());
    println!("╚══════════════════════════════════════════════════════════════════╝");
    println!();

    if json1 == json2 {
        println!("✅ Files are identical");
        return;
    }

    let lines1: Vec<&str> = json1.lines().collect();
    let lines2: Vec<&str> = json2.lines().collect();

    println!("📊 Size comparison:");
    println!("   File 1: {} lines, {} bytes", lines1.len(), json1.len());
    println!("   File 2: {} lines, {} bytes", lines2.len(), json2.len());
    println!();

    println!("🔍 First 50 differences:");
    let mut diff_count = 0;
    let max_len = lines1.len().max(lines2.len());

    for i in 0..max_len {
        let l1 = lines1.get(i).copied().unwrap_or("");
        let l2 = lines2.get(i).copied().unwrap_or("");

        if l1 != l2 {
            diff_count += 1;
            if diff_count <= 50 {
                println!("Line {}:", i + 1);
                if !l1.is_empty() {
                    println!("  - {}", l1.trim());
                }
                if !l2.is_empty() {
                    println!("  + {}", l2.trim());
                }
            }
        }
    }

    println!();
    println!("📈 Total differences: {diff_count} lines");
}
