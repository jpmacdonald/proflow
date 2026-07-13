//! Test tool to verify slide generation against a checked-in `.pro` fixture.
//!
//! Usage:
//!   `cargo run --bin test_template`

use anyhow::{Context, Result};
use proflow::propresenter::generated::rv_data;
use proflow::propresenter::rtf::StyledSegment;
use proflow::propresenter::template::{
    build_presentation_from_template_with_options, clone_slide_with_text,
    DEFAULT_MAX_LINES_PER_SLIDE,
};
use prost::Message;
use std::path::PathBuf;

fn main() -> Result<()> {
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("data")
        .join("templates")
        .join("__template_scripture__.pro");
    println!("Loading fixture: {}", fixture_path.display());
    let data = std::fs::read(&fixture_path)
        .with_context(|| format!("read fixture {}", fixture_path.display()))?;
    let fixture = rv_data::Presentation::decode(data.as_slice())
        .with_context(|| format!("decode fixture {}", fixture_path.display()))?;
    let template_slide = first_presentation_slide(&fixture)
        .context("scripture fixture does not contain a presentation slide")?;

    println!("\n--- Loaded scripture template slide ---");

    // Print original RTF
    if let Some(base_slide) = &template_slide.base_slide {
        if let Some(elem) = base_slide.elements.first() {
            if let Some(graphics_elem) = &elem.element {
                if let Some(text) = &graphics_elem.text {
                    let rtf_str = String::from_utf8_lossy(&text.rtf_data);
                    println!("\nOriginal RTF:\n{rtf_str}");
                }
            }
        }
    }

    // Clone slide with new text including superscripts
    let test_text = "¹⁵Until a spirit from on high is poured out on us,\nand the wilderness becomes a fruitful field,\nand the fruitful field is deemed a forest.";
    let segments = StyledSegment::from_plain(&[test_text.to_string()]);
    let new_slide = clone_slide_with_text(&template_slide, &segments);

    println!("\nCloned slide with new text");

    // Print new RTF
    if let Some(base_slide) = &new_slide.base_slide {
        if let Some(elem) = base_slide.elements.first() {
            if let Some(graphics_elem) = &elem.element {
                if let Some(text) = &graphics_elem.text {
                    let rtf_str = String::from_utf8_lossy(&text.rtf_data);
                    println!("\nGenerated RTF:\n{rtf_str}");

                    if rtf_str.contains(r"\cf2") {
                        println!("\nRTF contains color reference (\\cf2)");
                    } else {
                        println!("\nRTF missing color reference!");
                    }

                    if rtf_str.contains(r"\red255\green255\blue255") {
                        println!("RTF contains white color in color table");
                    } else {
                        println!("RTF missing white color!");
                    }

                    if rtf_str.contains(r"\super") {
                        println!("RTF contains superscript tags");
                    } else {
                        println!("RTF missing superscript tags!");
                    }
                }
            }
        }
    }

    // Build full presentation
    let content = StyledSegment::from_plain(&[
        "¹⁵Until a spirit from on high is poured out on us,".to_string(),
        "¹⁶and the wilderness becomes a fruitful field,".to_string(),
        "¹⁷and the fruitful field is deemed a forest.".to_string(),
    ]);

    let presentation = build_presentation_from_template_with_options(
        "Test Scripture - Isaiah 32:15-17",
        &template_slide,
        &content,
        45,
        DEFAULT_MAX_LINES_PER_SLIDE,
        None,
    )
    .context("failed to build presentation from scripture fixture")?;

    println!("\nBuilt presentation: {}", presentation.name);
    println!("   {} cues", presentation.cues.len());
    println!("   {} cue groups", presentation.cue_groups.len());
    println!("   {} arrangements", presentation.arrangements.len());

    // Encode and write to file for inspection
    let encoded = presentation.encode_to_vec();
    let output_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test_scripture.pro");
    std::fs::write(&output_path, &encoded)
        .with_context(|| format!("write generated presentation {}", output_path.display()))?;

    println!("\nWritten test file to: {}", output_path.display());
    println!("   {} bytes", encoded.len());

    println!(
        "\nRun 'cargo run --bin dump_pro -- {}' to inspect",
        output_path.display()
    );
    Ok(())
}

fn first_presentation_slide(
    presentation: &rv_data::Presentation,
) -> Option<rv_data::PresentationSlide> {
    presentation.cues.iter().find_map(|cue| {
        cue.actions.iter().find_map(|action| {
            let rv_data::action::ActionTypeData::Slide(slide_type) =
                action.action_type_data.as_ref()?
            else {
                return None;
            };
            let rv_data::action::slide_type::Slide::Presentation(slide) =
                slide_type.slide.as_ref()?
            else {
                return None;
            };
            Some(slide.clone())
        })
    })
}
