//! Dump a `ProPresenter` theme file for inspection.
#![allow(missing_docs, clippy::unwrap_used, clippy::expect_used)]

use proflow::propresenter::generated::rv_data;
use prost::Message;
use std::env;

fn main() {
    let path = env::args().nth(1).expect("Usage: dump_theme <path>");
    let data = std::fs::read(&path).expect("Failed to read file");

    match rv_data::template::Document::decode(data.as_slice()) {
        Ok(doc) => {
            println!("Theme: {} slides", doc.slides.len());
            if let Some(ref app) = doc.application_info {
                if let Some(ref ver) = app.application_version {
                    println!(
                        "ProPresenter version: {}.{}.{}",
                        ver.major_version, ver.minor_version, ver.patch_version
                    );
                }
            }
            for (i, slide) in doc.slides.iter().enumerate() {
                println!("\n--- Slide {i} ---");
                println!("Name: {:?}", slide.name);
                println!("Actions: {}", slide.actions.len());
                if let Some(ref base) = slide.base_slide {
                    println!(
                        "UUID: {:?}",
                        base.uuid.as_ref().map(|uuid| uuid.string.as_str())
                    );
                    println!("Elements: {}", base.elements.len());
                    println!("Background color: {:?}", base.background_color);
                    println!("Draws bg: {}", base.draws_background_color);
                    println!("Size: {:?}", base.size);
                    for (j, elem) in base.elements.iter().enumerate() {
                        if let Some(ref ge) = elem.element {
                            println!("  Element {j}: {:?}", ge.name);
                            if let Some(ref bounds) = ge.bounds {
                                println!(
                                    "    Bounds: origin=({}, {}), size=({}, {})",
                                    bounds
                                        .origin
                                        .as_ref()
                                        .and_then(|origin| origin.x)
                                        .unwrap_or_default(),
                                    bounds.origin.as_ref().map_or(0.0, |o| o.y),
                                    bounds.size.as_ref().map_or(0.0, |s| s.width),
                                    bounds.size.as_ref().map_or(0.0, |s| s.height),
                                );
                            }
                            if let Some(ref text) = ge.text {
                                println!(
                                    "    Layout: scale={} transform={} vertical={} margins={:?}",
                                    text.scale_behavior,
                                    text.transform,
                                    text.vertical_alignment,
                                    text.margins
                                );
                                println!(
                                    "    Native text features: standardized_superscript={} chord_pro={:?} alternates={} capitalization={} superscript={} ligatures={} custom_attributes={}",
                                    text.is_superscript_standardized,
                                    text.chord_pro,
                                    text.alternate_texts.len(),
                                    text.attributes
                                        .as_ref()
                                        .map_or(0, |attributes| attributes.capitalization),
                                    text.attributes
                                        .as_ref()
                                        .map_or(0, |attributes| attributes.superscript),
                                    text.attributes
                                        .as_ref()
                                        .map_or(0, |attributes| attributes.ligature_style),
                                    text.attributes
                                        .as_ref()
                                        .map_or(0, |attributes| attributes.custom_attributes.len())
                                );
                                let rtf = String::from_utf8_lossy(&text.rtf_data);
                                // Show first 200 chars of RTF
                                let preview: String = rtf.chars().take(200).collect();
                                println!("    RTF: {preview}...");
                                if let Some(ref attrs) = text.attributes {
                                    if let Some(ref font) = attrs.font {
                                        println!(
                                            "    Font: {} size={} bold={} italic={}",
                                            font.name, font.size, font.bold, font.italic
                                        );
                                    }
                                }
                            }
                            if ge.text.is_none() {
                                println!("    (non-text element)");
                            }
                        }
                    }
                }
                // Check for background actions
                for (k, action) in slide.actions.iter().enumerate() {
                    println!("  Action {k}: type={}", action.r#type);
                    if let Some(ref atd) = action.action_type_data {
                        println!("    Data: {atd:?}");
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to decode as Template::Document: {e}");
        }
    }
}
