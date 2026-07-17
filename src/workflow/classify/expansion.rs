//! Classification for multi-output rules and speaker/title presentations.

use std::collections::HashSet;

use super::{build_type_plan, file_stem};
use crate::planning_center::types::Item;
use crate::project_config::{ExpansionRule, ExpansionStep, PresentationPolicy, ProjectConfig};
use crate::propresenter::library::LibraryCatalog;
use crate::workflow::classify_matching::strip_speaker;
use crate::workflow::plan::{
    ItemKind, OutputKey, PlanDisposition, ResolvedItemPlan, ReviewContext,
};

#[allow(clippy::too_many_arguments)]
pub(super) fn process_expansion(
    expansion: &ExpansionRule,
    item: &Item,
    speaker: Option<&str>,
    mappings: &ProjectConfig,
    entries: &mut Vec<ResolvedItemPlan>,
    nametag_seen: &mut HashSet<String>,
    file_index: Option<&LibraryCatalog>,
    service_name: Option<&str>,
) {
    for (step_index, step) in expansion.iter().enumerate() {
        let output_key = OutputKey::expanded(&item.id, step_index, &step.use_type);
        if step.speaker.is_some() {
            if let Some(name) = speaker {
                if matches!(
                    mappings
                        .presentation_policy(&step.use_type)
                        .map(PresentationPolicy::kind),
                    Some(crate::project_config::ItemKind::Nametag)
                ) {
                    let first = first_name(name);
                    if nametag_seen.contains(&first) {
                        continue;
                    }
                    nametag_seen.insert(first);
                }
                entries.push(build_speaker_expansion_plan(
                    step,
                    item,
                    name,
                    mappings,
                    file_index,
                    service_name,
                    output_key,
                ));
            } else {
                let item_kind = mappings
                    .presentation_policy(&step.use_type)
                    .map_or(ItemKind::Other, PresentationPolicy::kind);
                entries.push(ResolvedItemPlan {
                    output_key,
                    position: item.position,
                    pco_title: item.title.clone(),
                    playlist_name: strip_speaker(&item.title),
                    reason: "Speaker could not be resolved for expansion".to_string(),
                    item_kind,
                    item_type: Some(step.use_type.clone()),
                    disposition: PlanDisposition::NeedsReview(ReviewContext::new(None)),
                });
            }
            continue;
        }

        let Some(policy) = mappings.presentation_policy(&step.use_type) else {
            entries.push(ResolvedItemPlan {
                output_key,
                position: item.position,
                pco_title: item.title.clone(),
                playlist_name: strip_speaker(&item.title),
                reason: format!("Unknown presentation type '{}'", step.use_type),
                item_kind: ItemKind::Other,
                item_type: Some(step.use_type.clone()),
                disposition: PlanDisposition::NeedsReview(ReviewContext::new(None)),
            });
            continue;
        };

        let plan = build_type_plan(
            output_key,
            &step.use_type,
            policy,
            item,
            step.target
                .as_ref()
                .and_then(crate::project_config::TargetSpec::library_file),
            mappings,
            file_index,
            service_name,
        );
        entries.push(plan);
    }
}

fn build_speaker_expansion_plan(
    step: &ExpansionStep,
    item: &Item,
    name: &str,
    mappings: &ProjectConfig,
    file_index: Option<&LibraryCatalog>,
    service_name: Option<&str>,
    output_key: OutputKey,
) -> ResolvedItemPlan {
    let Some(policy) = mappings.presentation_policy(&step.use_type) else {
        return ResolvedItemPlan {
            output_key,
            position: item.position,
            pco_title: item.title.clone(),
            playlist_name: strip_speaker(&item.title),
            reason: format!("Unknown presentation type '{}'", step.use_type),
            item_kind: ItemKind::Other,
            item_type: Some(step.use_type.clone()),
            disposition: PlanDisposition::NeedsReview(ReviewContext::new(None)),
        };
    };

    if matches!(policy.kind(), crate::project_config::ItemKind::Nametag) {
        let Some(target) = resolve_nametag_target(step, item, name, mappings) else {
            return ResolvedItemPlan {
                output_key,
                position: item.position,
                pco_title: item.title.clone(),
                playlist_name: strip_speaker(&item.title),
                reason: format!("Configured person '{name}' has no nametag target"),
                item_kind: ItemKind::Nametag,
                item_type: Some(step.use_type.clone()),
                disposition: PlanDisposition::NeedsReview(ReviewContext::new(None)),
            };
        };
        let mut plan = build_type_plan(
            output_key,
            &step.use_type,
            policy,
            item,
            Some(&target.library_file),
            mappings,
            file_index,
            service_name,
        );
        plan.playlist_name = target.playlist_name;
        return plan;
    }

    let resolved_target = step
        .target
        .as_ref()
        .and_then(crate::project_config::TargetSpec::library_file)
        .map(str::to_string)
        .or_else(|| {
            step.target
                .as_ref()
                .and_then(crate::project_config::TargetSpec::name_template)
                .map(|template| render_name_template(template, item, name))
        });
    let mut plan = build_type_plan(
        output_key,
        &step.use_type,
        policy,
        item,
        resolved_target.as_deref(),
        mappings,
        file_index,
        service_name,
    );
    if let Some(target_name) = resolved_target.as_deref() {
        if plan.file_path().is_none() || plan.playlist_name == strip_speaker(&item.title) {
            plan.playlist_name = file_stem(target_name);
        }
    }
    plan
}

struct ResolvedNametagTarget {
    library_file: String,
    playlist_name: String,
}

fn resolve_nametag_target(
    step: &ExpansionStep,
    item: &Item,
    person_name: &str,
    mappings: &ProjectConfig,
) -> Option<ResolvedNametagTarget> {
    let explicit_target = step
        .target
        .as_ref()
        .and_then(crate::project_config::TargetSpec::library_file);
    let playlist_name = mappings
        .people()
        .get(&first_name(person_name))
        .and_then(|person| person.nametag.clone())
        .or_else(|| explicit_target.map(file_stem))
        .or_else(|| {
            step.target
                .as_ref()
                .and_then(crate::project_config::TargetSpec::name_template)
                .map(|template| render_name_template(template, item, person_name))
        })?;
    Some(ResolvedNametagTarget {
        library_file: explicit_target.unwrap_or(&playlist_name).to_string(),
        playlist_name,
    })
}

fn extract_speaker(title: &str) -> Option<String> {
    let start = title.rfind('(')?;
    let end = title.rfind(')')?;
    (end > start + 1).then(|| title[start + 1..end].trim().to_string())
}

/// Resolve the speaker for an item only when the person is configured.
/// Parentheticals and `Liturgist:` lines remain useful context, but an unknown
/// name requires review instead of inventing a nametag filename.
pub(super) fn resolve_speaker(
    title: &str,
    description: Option<&str>,
    mappings: &ProjectConfig,
) -> Option<String> {
    if let Some(candidate) = extract_speaker(title) {
        if let Some(name) = configured_person_name(&candidate, mappings) {
            return Some(name);
        }
    }
    if let Some(desc) = description {
        for line in desc.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed
                .strip_prefix("Liturgist:")
                .or_else(|| trimmed.strip_prefix("liturgist:"))
            {
                let name = rest.split(';').next().unwrap_or(rest).trim();
                if let Some(name) = configured_person_name(name, mappings) {
                    return Some(name);
                }
            }
        }
    }
    None
}

fn configured_person_name(candidate: &str, mappings: &ProjectConfig) -> Option<String> {
    let candidate = candidate.trim();
    mappings.people().iter().find_map(|(first, person)| {
        if candidate.eq_ignore_ascii_case(first) {
            return Some(first.clone());
        }
        let last = person.last.as_deref()?;
        let full = format!("{first} {last}");
        candidate.eq_ignore_ascii_case(&full).then_some(full)
    })
}

fn first_name(name: &str) -> String {
    name.split_whitespace().next().unwrap_or(name).to_string()
}

fn render_name_template(template: &str, item: &Item, speaker: &str) -> String {
    const SPEAKER_PLACEHOLDER: &str = "{speaker}";
    const FIRST_NAME_PLACEHOLDER: &str = "{first_name}";
    const TITLE_PLACEHOLDER: &str = "{title}";

    template
        .replace(SPEAKER_PLACEHOLDER, speaker)
        .replace(FIRST_NAME_PLACEHOLDER, &first_name(speaker))
        .replace(TITLE_PLACEHOLDER, &strip_speaker(&item.title))
}
