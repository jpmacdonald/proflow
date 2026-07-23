//! Compact summary of a `ProPresenter` `.pro` file.
//!
//! Usage:
//!   `cargo run --features dev-tools --bin summarize_pro -- <file.pro> [...]`

#![allow(clippy::expect_used, clippy::unwrap_used)]

use proflow::propresenter::deserialize::read_presentation_file;
use proflow::propresenter::rtf::rtf_to_text;
use proflow::propresenter::unstable_native::rv_data::{self, action, url};
use std::{collections::HashMap, env, path::Path};

fn main() {
    let paths: Vec<String> = env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: summarize_pro <file.pro> [...]");
        std::process::exit(1);
    }

    for path in paths {
        summarize(Path::new(&path));
    }
}

fn summarize(path: &Path) {
    let presentation = read_presentation_file(path).expect("read presentation");
    println!("== {} ==", path.display());
    println!(
        "name={:?} uuid={:?} cues={} groups={} arrangements={:?} selected={:?} music={:?}",
        presentation.name,
        presentation.uuid.as_ref().map(|u| u.string.as_str()),
        presentation.cues.len(),
        presentation.cue_groups.len(),
        presentation
            .arrangements
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>(),
        presentation
            .selected_arrangement
            .as_ref()
            .map(|u| u.string.as_str()),
        presentation.music
    );

    let cue_to_group = cue_group_names(&presentation);
    let cue_by_id = cue_index_by_uuid(&presentation);
    let group_by_id = cue_group_index_by_uuid(&presentation);
    for arrangement in &presentation.arrangements {
        println!(
            "  arrangement {:?} uuid={:?}",
            arrangement.name,
            arrangement.uuid.as_ref().map(|u| u.string.as_str())
        );
        for (group_idx, group_id) in arrangement.group_identifiers.iter().enumerate() {
            let Some(cue_group_idx) = group_by_id.get(group_id.string.as_str()) else {
                println!("    {group_idx:02}. missing group {}", group_id.string);
                continue;
            };
            let cue_group = &presentation.cue_groups[*cue_group_idx];
            let group_name = cue_group.group.as_ref().map_or("", |g| g.name.as_str());
            let cue_indexes = cue_group
                .cue_identifiers
                .iter()
                .filter_map(|cue_id| cue_by_id.get(cue_id.string.as_str()).copied())
                .map(|idx| idx.to_string())
                .collect::<Vec<_>>()
                .join(",");
            println!("    {group_idx:02}. group={group_name:?} cues=[{cue_indexes}]");
        }
    }
    if presentation.arrangements.is_empty() {
        println!("  cue groups");
        for (group_idx, cue_group) in presentation.cue_groups.iter().enumerate() {
            let group_name = cue_group.group.as_ref().map_or("", |g| g.name.as_str());
            let cue_indexes = cue_group
                .cue_identifiers
                .iter()
                .filter_map(|cue_id| cue_by_id.get(cue_id.string.as_str()).copied())
                .map(|idx| idx.to_string())
                .collect::<Vec<_>>()
                .join(",");
            println!("    {group_idx:02}. group={group_name:?} cues=[{cue_indexes}]");
        }
    }
    for (idx, cue) in presentation.cues.iter().enumerate() {
        let groups = cue
            .uuid
            .as_ref()
            .and_then(|uuid| cue_to_group.get(uuid.string.as_str()))
            .cloned()
            .unwrap_or_default();
        println!(
            "  {idx:02} group={:?} cue={:?} actions={}",
            groups,
            cue.name,
            cue.actions.len()
        );
        for action in &cue.actions {
            println!("      - {}", summarize_action(action));
        }
    }
    println!();
}

fn cue_index_by_uuid(presentation: &rv_data::Presentation) -> HashMap<&str, usize> {
    let mut map = HashMap::new();
    for (idx, cue) in presentation.cues.iter().enumerate() {
        if let Some(uuid) = &cue.uuid {
            map.insert(uuid.string.as_str(), idx);
        }
    }
    map
}

fn cue_group_index_by_uuid(presentation: &rv_data::Presentation) -> HashMap<&str, usize> {
    let mut map = HashMap::new();
    for (idx, cue_group) in presentation.cue_groups.iter().enumerate() {
        let Some(group) = &cue_group.group else {
            continue;
        };
        let Some(uuid) = &group.uuid else {
            continue;
        };
        map.insert(uuid.string.as_str(), idx);
    }
    map
}

fn cue_group_names(presentation: &rv_data::Presentation) -> HashMap<&str, Vec<String>> {
    let mut map: HashMap<&str, Vec<String>> = HashMap::new();
    for cue_group in &presentation.cue_groups {
        let name = cue_group
            .group
            .as_ref()
            .map(|g| g.name.clone())
            .unwrap_or_default();
        for cue_id in &cue_group.cue_identifiers {
            map.entry(cue_id.string.as_str())
                .or_default()
                .push(name.clone());
        }
    }
    map
}

fn summarize_action(action: &rv_data::Action) -> String {
    match &action.action_type_data {
        Some(action::ActionTypeData::Slide(slide)) => {
            format!(
                "slide type={} label={:?} text={:?}",
                action.r#type,
                action.label.as_ref().map(|label| label.text.as_str()),
                slide_text(slide).unwrap_or_default()
            )
        }
        Some(action::ActionTypeData::Media(media)) => {
            format!(
                "media type={} layer={} path={:?}",
                action.r#type,
                media.layer_type,
                media
                    .element
                    .as_ref()
                    .and_then(|element| element.url.as_ref())
                    .and_then(storage_string)
            )
        }
        Some(action::ActionTypeData::Macro(macro_type)) => {
            let name = macro_type
                .identification
                .as_ref()
                .map_or("", |id| id.parameter_name.as_str());
            format!("macro type={} name={name:?}", action.r#type)
        }
        Some(_) => format!("other type={}", action.r#type),
        None => format!("none type={}", action.r#type),
    }
}

fn storage_string(url: &rv_data::Url) -> Option<String> {
    match url.storage.as_ref()? {
        url::Storage::AbsoluteString(value) => Some(value.clone()),
        other @ url::Storage::RelativePath(_) => Some(format!("{other:?}")),
    }
}

fn slide_text(slide_type: &action::SlideType) -> Option<String> {
    let action::slide_type::Slide::Presentation(presentation_slide) = slide_type.slide.as_ref()?
    else {
        return None;
    };
    let base_slide = presentation_slide.base_slide.as_ref()?;
    let mut texts = Vec::new();
    for element in &base_slide.elements {
        let Some(graphics) = element.element.as_ref() else {
            continue;
        };
        let Some(text) = graphics.text.as_ref() else {
            continue;
        };
        let rtf = String::from_utf8_lossy(&text.rtf_data);
        if let Some(text) = rtf_to_text(&rtf) {
            let compact = text
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
                .join(" / ");
            if !compact.is_empty() {
                texts.push(compact);
            }
        }
    }
    if texts.is_empty() {
        None
    } else {
        Some(texts.join(" | "))
    }
}
