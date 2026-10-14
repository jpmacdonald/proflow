//! Audit output ownership collisions and native presentation dimensions.

use super::{LibraryCatalog, ReadyAction, ResolvedItemPlan};
use std::collections::BTreeMap;

struct MutableTargetGroup {
    description: &'static str,
    display_name: String,
    plan_indexes: Vec<usize>,
}

pub(super) fn audit_mutable_presentation_target_collisions(entries: &mut [ResolvedItemPlan]) {
    let mut targets: BTreeMap<String, MutableTargetGroup> = BTreeMap::new();
    let mut invalid_targets = Vec::new();

    for (index, plan) in entries.iter().enumerate() {
        let (key, description, display_name) = match plan.ready_action() {
            Some(
                ReadyAction::EditDescription { file_path, .. }
                | ReadyAction::RestyleExisting { file_path, .. },
            ) => {
                let path = file_path.display().to_string();
                (format!("native_file:{path}"), "mutable native file", path)
            }
            Some(
                ReadyAction::GenerateDescription { .. }
                | ReadyAction::GenerateScripture { .. }
                | ReadyAction::GenerateTitle { .. },
            ) => {
                let name = match propresenter::playlist::canonical_presentation_name(
                    &plan.playlist_name,
                    plan.slide_type(),
                ) {
                    Ok(name) => name,
                    Err(error) => {
                        invalid_targets.push((index, error.to_string()));
                        continue;
                    }
                };
                let filename = format!("{name}.pro");
                (
                    format!("generate_new:{}", filename.to_lowercase()),
                    "generated file",
                    filename,
                )
            }
            Some(ReadyAction::UseExisting { .. }) | None => continue,
        };

        targets
            .entry(key)
            .or_insert_with(|| MutableTargetGroup {
                description,
                display_name,
                plan_indexes: Vec::new(),
            })
            .plan_indexes
            .push(index);
    }

    for (index, error) in invalid_targets {
        entries[index].require_review(format!(
            "Generated presentation target is invalid for '{}': {error}",
            entries[index].playlist_name
        ));
    }

    for group in targets
        .values()
        .filter(|group| group.plan_indexes.len() > 1)
    {
        let output_keys = group
            .plan_indexes
            .iter()
            .map(|index| entries[*index].output_key.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let reason = format!(
            "Mutable presentation target collision: {} '{}' is shared by output keys {output_keys}",
            group.description, group.display_name
        );
        for index in &group.plan_indexes {
            entries[*index].require_review(reason.clone());
        }
    }
}

pub(super) fn audit_selected_presentation_sizes(
    entries: &mut [ResolvedItemPlan],
    expected: propresenter::PresentationSize,
    file_index: Option<&LibraryCatalog>,
) {
    let Some(file_index) = file_index else {
        return;
    };
    for plan in entries {
        let Some(action) = plan.ready_action() else {
            continue;
        };
        let (file_path, restyle) = match action {
            ReadyAction::UseExisting { file_path, .. } => (file_path, false),
            ReadyAction::RestyleExisting { file_path, .. } => (file_path, true),
            ReadyAction::EditDescription { .. }
            | ReadyAction::GenerateDescription { .. }
            | ReadyAction::GenerateScripture { .. }
            | ReadyAction::GenerateTitle { .. } => continue,
        };
        let Some(indexed) = file_index.entry_at(file_path) else {
            continue;
        };
        let actual = indexed.presentation_size();
        if actual.matches(expected) {
            continue;
        }
        if restyle {
            match actual.resize_source(expected) {
                Ok(_) => continue,
                Err(error) => {
                    plan.require_review(format!(
                        "Presentation size {} does not match project {} and cannot be normalized automatically: {error}",
                        actual.describe(),
                        expected
                    ));
                    continue;
                }
            }
        }
        plan.require_review(format!(
            "Presentation size {} does not match project {}; set the expected output first, then reapply the theme",
            actual.describe(),
            expected
        ));
    }
}
