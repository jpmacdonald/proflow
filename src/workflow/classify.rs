//! Compile Planning Center items into explicit presentation actions.
//!
//! This coordinator chooses the matching rule and delegates source-specific
//! behavior to small concrete compilers in `classify/`.

mod description;
mod expansion;
mod required;
mod scripture;
mod song;

use std::collections::{BTreeMap, HashSet};

use description::{build_description_plan, build_static_plan, DescriptionPolicy, StaticPolicy};
use expansion::{process_expansion, resolve_speaker};
use required::ensure_required_playlist_items;
use scripture::build_scripture_plan;
use song::{build_song_plan, SongPolicy};

use super::classify_matching::{
    decision_choice_matches, decision_context_text, decision_review_reason, find_matching_rule,
    normalize_apostrophes, strip_speaker,
};
pub use super::classify_preview::{
    render_preview, PreviewEntry, PreviewResult, PreviewStatus, PreviewSummary,
};
use super::plan::{
    ItemKind, OutputKey, PlanDisposition, ReadyAction, ResolvedItemPlan, ReviewContext,
};
use crate::planning_center::types::Item;
use crate::project_config::{
    AmbiguousDecisionPolicy, DecisionChoiceConfig, DecisionConfig, ExistingSource, ItemRuleConfig,
    ItemRuleOutcome, PresentationPolicy, ProjectConfig, ReviewPolicy, RuleAction,
};
use crate::propresenter::library::LibraryCatalog;

/// Build typed workflow plans for a set of Planning Center items.
pub fn build_plan(
    items: &[Item],
    mappings: &ProjectConfig,
    file_index: Option<&LibraryCatalog>,
    service_name: Option<&str>,
) -> Vec<ResolvedItemPlan> {
    let mut entries = Vec::new();
    let mut nametag_seen: HashSet<String> = HashSet::new();

    for item in items {
        let title_lower = normalize_apostrophes(&item.title.to_lowercase());
        let speaker = resolve_speaker(&item.title, item.description.as_deref(), mappings);
        let Some(rule) = find_matching_rule(item, &title_lower, mappings, service_name) else {
            entries.push(ResolvedItemPlan {
                output_key: OutputKey::primary(&item.id),
                position: item.position,
                pco_title: item.title.clone(),
                playlist_name: strip_speaker(&item.title),
                reason: "No matching item rule".to_string(),
                item_kind: ItemKind::Other,
                item_type: None,
                disposition: PlanDisposition::NeedsReview(ReviewContext::new(None)),
            });
            continue;
        };

        let output_key = OutputKey::primary(&item.id);
        match &rule.outcome {
            ItemRuleOutcome::Expand(expansion) => process_expansion(
                expansion,
                item,
                speaker.as_deref(),
                mappings,
                &mut entries,
                &mut nametag_seen,
                file_index,
                service_name,
            ),
            ItemRuleOutcome::Action(action) => {
                entries.push(rule_action_plan(action, item, output_key));
            }
            ItemRuleOutcome::Decision(decision) => entries.push(build_decision_plan(
                decision,
                rule,
                item,
                output_key,
                mappings,
                file_index,
                service_name,
            )),
            ItemRuleOutcome::UseType { type_key, target } => {
                entries.push(build_use_type_rule_plan(
                    type_key,
                    target.as_ref(),
                    item,
                    mappings,
                    file_index,
                    service_name,
                ));
            }
        }
    }

    ensure_required_playlist_items(&mut entries, mappings, file_index, service_name);
    audit_selected_presentation_sizes(
        &mut entries,
        mappings.defaults().presentation_size,
        file_index,
    );
    audit_mutable_presentation_target_collisions(&mut entries);
    entries
}

/// Build the operator-facing preview for a set of Planning Center items.
pub fn build_preview(
    items: &[Item],
    mappings: &ProjectConfig,
    file_index: Option<&LibraryCatalog>,
    service_name: Option<&str>,
) -> Vec<PreviewEntry> {
    render_preview(&build_plan(items, mappings, file_index, service_name))
}

struct MutableTargetGroup {
    description: &'static str,
    display_name: String,
    plan_indexes: Vec<usize>,
}

fn audit_mutable_presentation_target_collisions(entries: &mut [ResolvedItemPlan]) {
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
                let name = match crate::propresenter::playlist::canonical_presentation_name(
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

fn audit_selected_presentation_sizes(
    entries: &mut [ResolvedItemPlan],
    expected: crate::propresenter::PresentationSize,
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

fn build_use_type_rule_plan(
    type_key: &str,
    target: Option<&crate::project_config::TargetSpec>,
    item: &Item,
    mappings: &ProjectConfig,
    file_index: Option<&LibraryCatalog>,
    service_name: Option<&str>,
) -> ResolvedItemPlan {
    let output_key = OutputKey::primary(&item.id);
    let Some(policy) = mappings.presentation_policy(type_key) else {
        return ResolvedItemPlan {
            output_key,
            position: item.position,
            pco_title: item.title.clone(),
            playlist_name: strip_speaker(&item.title),
            reason: format!("Unknown presentation type '{type_key}'"),
            item_kind: ItemKind::Other,
            item_type: Some(type_key.to_string()),
            disposition: PlanDisposition::NeedsReview(ReviewContext::new(None)),
        };
    };

    build_type_plan(
        output_key,
        type_key,
        policy,
        item,
        target.and_then(crate::project_config::TargetSpec::library_file),
        mappings,
        file_index,
        service_name,
    )
}

fn build_decision_plan(
    decision: &DecisionConfig,
    rule: &ItemRuleConfig,
    item: &Item,
    output_key: OutputKey,
    mappings: &ProjectConfig,
    file_index: Option<&LibraryCatalog>,
    service_name: Option<&str>,
) -> ResolvedItemPlan {
    match decision {
        DecisionConfig::ChooseExistingFile {
            context_fields,
            instructions,
            choices,
            on_ambiguous,
        } => {
            let context_text = decision_context_text(item, context_fields);
            let matched: Vec<_> = choices
                .iter()
                .filter(|(_, choice)| decision_choice_matches(choice, &context_text))
                .collect();

            if matched.len() != 1 {
                let disposition = match on_ambiguous.unwrap_or_default() {
                    AmbiguousDecisionPolicy::Ask => {
                        PlanDisposition::NeedsReview(ReviewContext::new(None))
                    }
                    AmbiguousDecisionPolicy::Skip => PlanDisposition::Skip,
                };
                let reason =
                    decision_review_reason(rule, instructions.as_deref(), choices, &matched);
                return ResolvedItemPlan {
                    output_key,
                    position: item.position,
                    pco_title: item.title.clone(),
                    playlist_name: strip_speaker(&item.title),
                    reason,
                    item_kind: ItemKind::Other,
                    item_type: None,
                    disposition,
                };
            }

            let (choice_key, choice) = matched[0];
            let Some(type_key) = choice.use_type.as_deref() else {
                return ResolvedItemPlan {
                    output_key,
                    position: item.position,
                    pco_title: item.title.clone(),
                    playlist_name: strip_speaker(&item.title),
                    reason: format!("Decision choice '{choice_key}' has no use_type"),
                    item_kind: ItemKind::Other,
                    item_type: None,
                    disposition: PlanDisposition::NeedsReview(ReviewContext::new(None)),
                };
            };
            let Some(policy) = mappings.presentation_policy(type_key) else {
                return ResolvedItemPlan {
                    output_key,
                    position: item.position,
                    pco_title: item.title.clone(),
                    playlist_name: strip_speaker(&item.title),
                    reason: format!(
                        "Decision choice '{choice_key}' uses unknown type '{type_key}'"
                    ),
                    item_kind: ItemKind::Other,
                    item_type: Some(type_key.to_string()),
                    disposition: PlanDisposition::NeedsReview(ReviewContext::new(None)),
                };
            };

            let target_library_file = choice_library_file(choice);
            let mut plan = build_type_plan(
                output_key,
                type_key,
                policy,
                item,
                target_library_file.as_deref(),
                mappings,
                file_index,
                service_name,
            );
            plan.reason = format!("Context choice '{choice_key}': {}", plan.reason);
            plan
        }
    }
}

fn choice_library_file(choice: &DecisionChoiceConfig) -> Option<String> {
    choice
        .target
        .as_ref()
        .and_then(crate::project_config::TargetSpec::library_file)
        .map(str::to_string)
        .or_else(|| choice.file.clone())
}

#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "one exhaustive dispatch keeps the output identity and every checked presentation policy locally auditable"
)]
pub(super) fn build_type_plan(
    output_key: OutputKey,
    type_key: &str,
    policy: &PresentationPolicy,
    item: &Item,
    target_library_file: Option<&str>,
    mappings: &ProjectConfig,
    file_index: Option<&LibraryCatalog>,
    service_name: Option<&str>,
) -> ResolvedItemPlan {
    match policy {
        PresentationPolicy::Skip { kind } => configured_disposition_plan(
            output_key,
            type_key,
            item,
            *kind,
            PlanDisposition::Skip,
            "Configured to skip",
        ),
        PresentationPolicy::Review(ReviewPolicy::Static { kind }) => build_static_plan(
            output_key,
            type_key,
            *kind,
            StaticPolicy::Review,
            item,
            target_library_file,
            file_index,
        ),
        PresentationPolicy::Review(ReviewPolicy::Description {
            kind,
            parser,
            render,
        }) => build_description_plan(
            output_key,
            type_key,
            *kind,
            *parser,
            DescriptionPolicy::Review {
                render: render
                    .as_ref()
                    .map(|render| render.for_service(service_name)),
            },
            item,
            target_library_file,
            file_index,
        ),
        PresentationPolicy::Review(ReviewPolicy::Scripture) => configured_disposition_plan(
            output_key,
            type_key,
            item,
            ItemKind::Scripture,
            PlanDisposition::NeedsReview(ReviewContext::new(None)),
            "Configured to require review",
        ),
        PresentationPolicy::Review(ReviewPolicy::Song) => build_song_plan(
            output_key,
            type_key,
            &SongPolicy::Review,
            item,
            target_library_file,
            file_index,
        ),
        PresentationPolicy::PreserveExisting {
            kind,
            source: ExistingSource::Static,
            arrangement,
        } => build_static_plan(
            output_key,
            type_key,
            *kind,
            StaticPolicy::PreserveExisting {
                arrangement: arrangement.for_service(service_name),
            },
            item,
            target_library_file,
            file_index,
        ),
        PresentationPolicy::PreserveExisting {
            source: ExistingSource::Song,
            arrangement,
            ..
        } => build_song_plan(
            output_key,
            type_key,
            &SongPolicy::PreserveExisting {
                arrangement: arrangement.for_service(service_name),
            },
            item,
            target_library_file,
            file_index,
        ),
        PresentationPolicy::RestyleExisting {
            kind,
            source: ExistingSource::Static,
            arrangement,
            transform,
        } => build_static_plan(
            output_key,
            type_key,
            *kind,
            StaticPolicy::RestyleExisting {
                arrangement: arrangement.for_service(service_name),
                transform: transform.for_service(service_name),
            },
            item,
            target_library_file,
            file_index,
        ),
        PresentationPolicy::RestyleExisting {
            source: ExistingSource::Song,
            arrangement,
            transform,
            ..
        } => build_song_plan(
            output_key,
            type_key,
            &SongPolicy::RestyleExisting {
                arrangement: arrangement.for_service(service_name),
                transform: transform.for_service(service_name),
            },
            item,
            target_library_file,
            file_index,
        ),
        PresentationPolicy::EditDescription {
            kind,
            parser,
            render,
        } => build_description_plan(
            output_key,
            type_key,
            *kind,
            *parser,
            DescriptionPolicy::Edit {
                render: render.for_service(service_name),
            },
            item,
            target_library_file,
            file_index,
        ),
        PresentationPolicy::GenerateDescription {
            kind,
            parser,
            render,
        } => build_description_plan(
            output_key,
            type_key,
            *kind,
            *parser,
            DescriptionPolicy::Generate {
                render: render.for_service(service_name),
            },
            item,
            target_library_file,
            file_index,
        ),
        PresentationPolicy::GenerateScripture { render } => build_scripture_plan(
            output_key,
            type_key,
            item,
            render.for_service(service_name),
            mappings.defaults().bible_version,
        ),
    }
}

fn configured_disposition_plan(
    output_key: OutputKey,
    type_key: &str,
    item: &Item,
    kind: ItemKind,
    disposition: PlanDisposition,
    reason: &str,
) -> ResolvedItemPlan {
    ResolvedItemPlan {
        output_key,
        position: item.position,
        pco_title: item.title.clone(),
        playlist_name: strip_speaker(&item.title),
        reason: reason.to_string(),
        item_kind: kind,
        item_type: Some(type_key.to_string()),
        disposition,
    }
}

fn rule_action_plan(action: &RuleAction, item: &Item, output_key: OutputKey) -> ResolvedItemPlan {
    let (disposition, reason) = match action {
        RuleAction::Skip { reason } => (PlanDisposition::Skip, reason.clone()),
        RuleAction::Review { reason } => (
            PlanDisposition::NeedsReview(ReviewContext::new(None)),
            reason.clone(),
        ),
    };

    ResolvedItemPlan {
        output_key,
        position: item.position,
        pco_title: item.title.clone(),
        playlist_name: strip_speaker(&item.title),
        reason,
        item_kind: ItemKind::Other,
        item_type: None,
        disposition,
    }
}

pub(super) fn file_stem(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .and_then(|segment| segment.to_str())
        .unwrap_or(path)
        .to_string()
}

#[cfg(test)]
mod tests;
