//! Classification for multi-output rules and speaker/title presentations.

use std::collections::HashSet;

use super::{build_type_plan, file_stem};
use crate::planning_center::types::Item;
use crate::project_config::{
    CompiledDirectTarget, CompiledExpansionStep, CompiledSpeakerTarget, ProjectConfig,
    ResolvedPresentationType,
};
use crate::propresenter::library::LibraryCatalog;
use crate::workflow::classify_matching::strip_speaker;
use crate::workflow::plan::{
    ItemKind, OutputKey, PlanDisposition, ResolvedItemPlan, ReviewContext,
};

#[allow(clippy::too_many_arguments)]
pub(super) fn process_expansion(
    expansion: &[CompiledExpansionStep],
    item: &Item,
    speaker: Option<&str>,
    mappings: &ProjectConfig,
    entries: &mut Vec<ResolvedItemPlan>,
    nametag_seen: &mut HashSet<String>,
    file_index: Option<&LibraryCatalog>,
    service_name: Option<&str>,
) {
    for (step_index, step) in expansion.iter().enumerate() {
        let presentation = step.presentation();
        let output_key = OutputKey::expanded(&item.id, step_index, presentation.key());
        match step {
            CompiledExpansionStep::Direct {
                presentation,
                target,
            } => entries.push(build_direct_expansion_plan(
                presentation,
                target,
                item,
                mappings,
                file_index,
                service_name,
                output_key,
            )),
            CompiledExpansionStep::Speaker {
                presentation,
                target,
            } => {
                let Some(name) = speaker else {
                    entries.push(ResolvedItemPlan::new(
                        output_key,
                        item.position,
                        item.title.clone(),
                        strip_speaker(&item.title),
                        "Speaker could not be resolved for expansion".to_string(),
                        presentation.kind(),
                        Some(presentation.key().to_string()),
                        PlanDisposition::NeedsReview(ReviewContext::new(None)),
                    ));
                    continue;
                };
                if presentation.kind() == crate::project_config::ItemKind::Nametag {
                    let first = first_name(name);
                    if !nametag_seen.insert(first) {
                        continue;
                    }
                }
                entries.push(build_speaker_expansion_plan(
                    presentation,
                    target,
                    item,
                    name,
                    mappings,
                    file_index,
                    service_name,
                    output_key,
                ));
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn build_direct_expansion_plan(
    presentation: &ResolvedPresentationType,
    target: &CompiledDirectTarget,
    item: &Item,
    mappings: &ProjectConfig,
    file_index: Option<&LibraryCatalog>,
    service_name: Option<&str>,
    output_key: OutputKey,
) -> ResolvedItemPlan {
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

#[allow(clippy::too_many_arguments)]
fn build_speaker_expansion_plan(
    presentation: &ResolvedPresentationType,
    target: &CompiledSpeakerTarget,
    item: &Item,
    name: &str,
    mappings: &ProjectConfig,
    file_index: Option<&LibraryCatalog>,
    service_name: Option<&str>,
    output_key: OutputKey,
) -> ResolvedItemPlan {
    if presentation.kind() == crate::project_config::ItemKind::Nametag {
        let Some(target) = resolve_nametag_target(target, item, name, mappings) else {
            return ResolvedItemPlan::new(
                output_key,
                item.position,
                item.title.clone(),
                strip_speaker(&item.title),
                format!("Configured person '{name}' has no nametag target"),
                ItemKind::Nametag,
                Some(presentation.key().to_string()),
                PlanDisposition::NeedsReview(ReviewContext::new(None)),
            );
        };
        let mut plan = build_type_plan(
            output_key,
            presentation.key(),
            presentation.policy(),
            item,
            Some(&target.library_file),
            mappings,
            file_index,
            service_name,
        );
        plan.playlist_name = target.playlist_name;
        return plan;
    }

    let resolved_target = match target {
        CompiledSpeakerTarget::Automatic => None,
        CompiledSpeakerTarget::LibraryFile(file) => Some(file.clone()),
        CompiledSpeakerTarget::NameTemplate(template) => {
            Some(render_name_template(template, item, name))
        }
    };
    let mut plan = build_type_plan(
        output_key,
        presentation.key(),
        presentation.policy(),
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
    target: &CompiledSpeakerTarget,
    item: &Item,
    person_name: &str,
    mappings: &ProjectConfig,
) -> Option<ResolvedNametagTarget> {
    let explicit_target = match target {
        CompiledSpeakerTarget::LibraryFile(file) => Some(file.as_str()),
        CompiledSpeakerTarget::Automatic | CompiledSpeakerTarget::NameTemplate(_) => None,
    };
    let playlist_name = mappings
        .people()
        .get(&first_name(person_name))
        .and_then(|person| person.nametag.clone())
        .or_else(|| explicit_target.map(file_stem))
        .or_else(|| {
            if let CompiledSpeakerTarget::NameTemplate(template) = target {
                Some(render_name_template(template, item, person_name))
            } else {
                None
            }
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SpeakerResolution {
    Resolved(String),
    Missing,
    Unrecognized,
}

impl SpeakerResolution {
    pub(super) fn with_fallback(self, fallback: Option<&str>) -> Option<String> {
        match self {
            Self::Resolved(name) => Some(name),
            Self::Missing => fallback.map(str::to_string),
            Self::Unrecognized => None,
        }
    }
}

/// Resolve the speaker for an item only when the person is configured.
/// Parentheticals and `Liturgist:` lines remain useful context, but an unknown
/// name requires review instead of inventing a nametag filename.
pub(super) fn resolve_speaker(
    title: &str,
    description: Option<&str>,
    mappings: &ProjectConfig,
) -> SpeakerResolution {
    let mut speaker_was_supplied = false;
    if let Some(candidate) = extract_speaker(title) {
        speaker_was_supplied = true;
        if let Some(name) = configured_person_name(&candidate, mappings) {
            return SpeakerResolution::Resolved(name);
        }
    }
    if let Some(desc) = description {
        for line in desc.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed
                .strip_prefix("Liturgist:")
                .or_else(|| trimmed.strip_prefix("liturgist:"))
            {
                speaker_was_supplied = true;
                let name = rest.split(';').next().unwrap_or(rest).trim();
                if let Some(name) = configured_person_name(name, mappings) {
                    return SpeakerResolution::Resolved(name);
                }
            }
        }
    }
    if speaker_was_supplied {
        SpeakerResolution::Unrecognized
    } else {
        SpeakerResolution::Missing
    }
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
