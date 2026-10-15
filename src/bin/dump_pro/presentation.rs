//! Print native presentation structure and action payloads.

use super::{dump_presentation_slide, load_presentation, rv_data};
use std::path::Path;

#[allow(clippy::too_many_lines)]
pub fn dump_presentation(path: &Path) {
    let presentation = load_presentation(path);

    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!(
        "║ ProPresenter File Analysis: {}",
        path.file_name()
            .expect("path has no filename")
            .to_string_lossy()
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
            println!("   ├─ App Group Name: {:?}", g.application_group_name);
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
