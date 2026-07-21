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
use expansion::{process_expansion, resolve_speaker, SpeakerResolution};
use required::ensure_required_playlist_items;
use scripture::build_scripture_plan;
use song::{build_song_plan, SongPolicy};

use super::classify_matching::{select_matching_rule, strip_speaker, RuleSelection};
pub use super::classify_preview::{
    render_preview, PreviewEntry, PreviewResult, PreviewStatus, PreviewSummary,
};
use super::plan::{
    ItemKind, OutputKey, PlanDisposition, ReadyAction, ResolvedItemPlan, ReviewContext,
};
use crate::planning_center::types::Item;
use crate::project_config::{
    AmbiguousDecisionPolicy, CompiledDecision, CompiledDirectTarget, CompiledRuleOutcome,
    ExistingSource, PresentationPolicy, ProjectConfig, ResolvedPresentationType, ReviewPolicy,
    RuleAction,
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
    let fallback_speaker = resolve_plan_speaker_fallback(items, mappings, service_name);

    for item in items {
        let speaker = resolve_speaker(&item.title, item.description.as_deref(), mappings)
            .with_fallback(fallback_speaker.as_deref());
        let rule = match select_matching_rule(item, mappings, service_name) {
            RuleSelection::None => {
                entries.push(unclassified_item_plan(
                    item,
                    "No matching item rule".to_string(),
                ));
                continue;
            }
            RuleSelection::Ambiguous { tier, rules } => {
                let rule_ids = rules
                    .iter()
                    .map(|rule| format!("'{}'", rule.id()))
                    .collect::<Vec<_>>()
                    .join(", ");
                entries.push(unclassified_item_plan(
                    item,
                    format!("Multiple {} item rules matched: {rule_ids}", tier.as_str()),
                ));
                continue;
            }
            RuleSelection::Selected(rule) => rule,
        };

        let first_output = entries.len();
        let output_key = OutputKey::primary(&item.id);
        match rule.outcome() {
            CompiledRuleOutcome::Expand(expansion) => process_expansion(
                expansion,
                item,
                speaker.as_deref(),
                mappings,
                &mut entries,
                &mut nametag_seen,
                file_index,
                service_name,
            ),
            CompiledRuleOutcome::Action(action) => {
                entries.push(rule_action_plan(action, item, output_key));
            }
            CompiledRuleOutcome::Decision(decision) => entries.push(build_decision_plan(
                decision,
                rule.id(),
                item,
                output_key,
                mappings,
                file_index,
                service_name,
            )),
            CompiledRuleOutcome::UseType {
                presentation,
                target,
            } => {
                entries.push(build_use_type_rule_plan(
                    presentation,
                    target,
                    item,
                    mappings,
                    file_index,
                    service_name,
                ));
            }
        }
        for plan in &mut entries[first_output..] {
            plan.set_classification_rule(rule.id());
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

fn resolve_plan_speaker_fallback(
    items: &[Item],
    mappings: &ProjectConfig,
    service_name: Option<&str>,
) -> Option<String> {
    let fallback_rule = mappings.defaults().speaker_fallback_rule.as_deref()?;
    let mut sources = items.iter().filter(|item| {
        matches!(
            select_matching_rule(item, mappings, service_name),
            RuleSelection::Selected(rule) if rule.id() == fallback_rule
        )
    });
    let source = sources.next()?;
    if sources.next().is_some() {
        return None;
    }

    match resolve_speaker(&source.title, source.description.as_deref(), mappings) {
        SpeakerResolution::Resolved(name) => Some(name),
        SpeakerResolution::Missing | SpeakerResolution::Unrecognized => None,
    }
}

fn unclassified_item_plan(item: &Item, reason: String) -> ResolvedItemPlan {
    ResolvedItemPlan::new(
        OutputKey::primary(&item.id),
        item.position,
        item.title.clone(),
        strip_speaker(&item.title),
        reason,
        ItemKind::Other,
        None,
        PlanDisposition::NeedsReview(ReviewContext::new(None)),
    )
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
    presentation: &ResolvedPresentationType,
    target: &CompiledDirectTarget,
    item: &Item,
    mappings: &ProjectConfig,
    file_index: Option<&LibraryCatalog>,
    service_name: Option<&str>,
) -> ResolvedItemPlan {
    let output_key = OutputKey::primary(&item.id);
    build_type_plan(
        output_key,
        presentation.key(),
        presentation.policy(),
        item,
        target.library_file(),
        mappings,
        file_index,
        service_name,
    )
}

fn build_decision_plan(
    decision: &CompiledDecision,
    rule_id: &str,
    item: &Item,
    output_key: OutputKey,
    mappings: &ProjectConfig,
    file_index: Option<&LibraryCatalog>,
    service_name: Option<&str>,
) -> ResolvedItemPlan {
    let matched = decision.matching_choices(
        &item.title,
        item.description.as_deref(),
        item.note.as_deref(),
    );

    if matched.len() != 1 {
        let disposition = match decision.on_ambiguous() {
            AmbiguousDecisionPolicy::Ask => PlanDisposition::NeedsReview(ReviewContext::new(None)),
            AmbiguousDecisionPolicy::Skip => PlanDisposition::Skip,
        };
        return ResolvedItemPlan::new(
            output_key,
            item.position,
            item.title.clone(),
            strip_speaker(&item.title),
            decision.review_reason(rule_id, &matched),
            ItemKind::Other,
            None,
            disposition,
        );
    }

    let choice = matched[0];
    let presentation = choice.presentation();
    let mut plan = build_type_plan(
        output_key,
        presentation.key(),
        presentation.policy(),
        item,
        Some(choice.library_file()),
        mappings,
        file_index,
        service_name,
    );
    plan.reason = format!("Context choice '{}': {}", choice.key(), plan.reason);
    plan
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
    ResolvedItemPlan::new(
        output_key,
        item.position,
        item.title.clone(),
        strip_speaker(&item.title),
        reason.to_string(),
        kind,
        Some(type_key.to_string()),
        disposition,
    )
}

fn rule_action_plan(action: &RuleAction, item: &Item, output_key: OutputKey) -> ResolvedItemPlan {
    let (disposition, reason) = match action {
        RuleAction::Skip { reason } => (PlanDisposition::Skip, reason.clone()),
        RuleAction::Review { reason } => (
            PlanDisposition::NeedsReview(ReviewContext::new(None)),
            reason.clone(),
        ),
    };

    ResolvedItemPlan::new(
        output_key,
        item.position,
        item.title.clone(),
        strip_speaker(&item.title),
        reason,
        ItemKind::Other,
        None,
        disposition,
    )
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
