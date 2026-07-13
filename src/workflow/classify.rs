//! Service plan preview — analyzes a PCO plan and proposes playlist entries.
//!
//! Uses a declarative type system from `data/proflow.config.json` to classify
//! each PCO item, resolve library files, and produce a structured preview.

use std::collections::{BTreeMap, HashSet};

use serde::Serialize;

use super::description_parser::{self, ParsedContent};
use super::library_search::{
    resolve_exact_library_file, resolve_song_library_match, strip_hymn_number,
    ExactLibraryFileMatch, SongLibraryMatch,
};
use super::plan::{
    ContentSource, CueMacro, ItemKind, PlanAction, PresentationStyle, ResolvedBackground,
    ResolvedItemPlan, ScriptureContent, ScriptureRefInfo, ScriptureRequest,
};
use super::scripture::{has_scripture_ref, parse_scripture_refs, ParsedScriptureRef};
use crate::bible::BibleVersion;
use crate::planning_center::types::Item;
use crate::project_config::{
    AmbiguousDecisionPolicy, ContentSourceKind, CueRoleConfig, DecisionChoiceConfig,
    DecisionConfig, DisplayBindingConfig, ExpansionRule, ItemRuleConfig, ItemRuleOutcome,
    MatchSpec, OutputStrategy, OverrideRuleConfig, PresentationTypeConfig, ProjectConfig,
    RequiredPlaylistItemConfig, RequiredPlaylistPlacement, RuleAction,
};
use crate::utils::file_index::{FileIndex, IndexedArrangement};

// ---------------------------------------------------------------------------
// Preview output
// ---------------------------------------------------------------------------

/// Status of a proposed playlist entry.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewStatus {
    /// Existing library file, no changes needed.
    Used,
    /// New file generated from scratch (scripture, etc.).
    Created,
    /// Library file whose content is refreshed from this week's description.
    Edited,
    /// Not included in the playlist.
    #[default]
    Skipped,
    /// Needs user confirmation.
    Uncertain,
}

/// A single row in the preview table.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PreviewEntry {
    pub output_key: String,
    pub position: usize,
    pub pco_title: String,
    pub playlist_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    pub status: PreviewStatus,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parsed_content: Option<ParsedContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<crate::project_config::BackgroundId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arrangement: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scripture_reference: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bible_version: Option<String>,
    /// Individual scripture references for multi-reference items.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub scripture_refs: Option<Vec<ScriptureRefInfo>>,
    /// Cue-role slide name used for generated content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_slide: Option<String>,
    /// Cue-role slide name used for a leading title cue.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_slide: Option<String>,
    /// `ProPresenter` macro triggered on the first operator-visible cue.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_cue_macro: Option<String>,
    /// `ProPresenter` macro triggered on the first content cue after the title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_content_cue_macro: Option<String>,
}

/// Full preview result.
#[derive(Debug, Serialize)]
pub struct PreviewResult {
    pub plan_title: String,
    pub service_name: String,
    pub date: String,
    pub entries: Vec<PreviewEntry>,
    pub summary: PreviewSummary,
}

/// Summary counts for the preview.
#[derive(Debug, Serialize)]
pub struct PreviewSummary {
    pub used_count: usize,
    pub created_count: usize,
    pub edited_count: usize,
    pub skip_count: usize,
    pub uncertain_count: usize,
    pub total_playlist_items: usize,
}

impl PreviewSummary {
    /// Count one preview using the same definition at every operator boundary.
    #[must_use]
    pub fn from_entries(entries: &[PreviewEntry]) -> Self {
        let mut summary = Self {
            used_count: 0,
            created_count: 0,
            edited_count: 0,
            skip_count: 0,
            uncertain_count: 0,
            total_playlist_items: 0,
        };
        for entry in entries {
            match &entry.status {
                PreviewStatus::Used => summary.used_count += 1,
                PreviewStatus::Created => summary.created_count += 1,
                PreviewStatus::Edited => summary.edited_count += 1,
                PreviewStatus::Skipped => summary.skip_count += 1,
                PreviewStatus::Uncertain => summary.uncertain_count += 1,
            }
        }
        summary.total_playlist_items =
            summary.used_count + summary.created_count + summary.edited_count;
        summary
    }
}

impl From<PlanAction> for PreviewStatus {
    fn from(action: PlanAction) -> Self {
        match action {
            PlanAction::UseExisting => Self::Used,
            PlanAction::EditInPlace => Self::Edited,
            PlanAction::GenerateNew => Self::Created,
            PlanAction::Skip => Self::Skipped,
            PlanAction::NeedsReview => Self::Uncertain,
        }
    }
}

impl From<ResolvedItemPlan> for PreviewEntry {
    fn from(plan: ResolvedItemPlan) -> Self {
        let (parsed_content, scripture_reference, bible_version, scripture_refs) =
            match plan.content_source {
                ContentSource::None => (None, None, None, None),
                ContentSource::Description { parsed_content } => (parsed_content, None, None, None),
                ContentSource::Scripture { scripture } => match scripture.request() {
                    ScriptureRequest::Single {
                        reference,
                        bible_version,
                    } => (
                        None,
                        Some(reference.to_string()),
                        Some(bible_version.to_string()),
                        None,
                    ),
                    ScriptureRequest::Combined(references) => {
                        (None, None, None, Some(references.to_vec()))
                    }
                },
            };
        let all_content_colored = parsed_content.as_ref().is_some_and(|content| {
            let visible = content
                .segments
                .iter()
                .filter(|segment| !segment.text.is_empty())
                .collect::<Vec<_>>();
            !visible.is_empty() && visible.iter().all(|segment| segment.color.is_some())
        });
        let first_cue_is_content = plan.style.first_content_cue_macro.is_none();
        let first_cue_macro = plan.style.first_cue_macro.as_ref().map(|binding| {
            binding
                .select(first_cue_is_content && all_content_colored)
                .to_string()
        });
        let first_content_cue_macro = plan
            .style
            .first_content_cue_macro
            .as_ref()
            .map(|binding| binding.select(all_content_colored).to_string());

        Self {
            output_key: plan.output_key,
            position: plan.position,
            pco_title: plan.pco_title,
            playlist_name: plan.playlist_name,
            file_path: plan.file_path,
            status: PreviewStatus::from(plan.action),
            reason: plan.reason,
            item_type: plan.item_type,
            parsed_content,
            background: plan
                .style
                .background
                .as_ref()
                .map(|background| background.id().clone()),
            arrangement: plan.style.arrangement,
            scripture_reference,
            bible_version,
            scripture_refs,
            content_slide: plan.style.content_slide,
            title_slide: plan.style.title_slide,
            first_cue_macro,
            first_content_cue_macro,
        }
    }
}

// ---------------------------------------------------------------------------
// Preview builder
// ---------------------------------------------------------------------------

/// Build typed workflow plans for a set of PCO items.
pub fn build_plan(
    items: &[Item],
    mappings: &ProjectConfig,
    file_index: Option<&FileIndex>,
    service_name: Option<&str>,
) -> Vec<ResolvedItemPlan> {
    let mut entries = Vec::new();
    let mut nametag_seen: HashSet<String> = HashSet::new();

    for item in items {
        let title_lower = normalize_apostrophes(&item.title.to_lowercase());
        let speaker = resolve_speaker(&item.title, item.description.as_deref(), mappings);
        let Some(rule) = find_matching_rule(item, &title_lower, mappings, service_name) else {
            entries.push(ResolvedItemPlan {
                output_key: ResolvedItemPlan::primary_output_key(&item.id),
                position: item.position,
                pco_title: item.title.clone(),
                playlist_name: strip_speaker(&item.title),
                action: PlanAction::NeedsReview,
                reason: "No matching item rule".to_string(),
                ..Default::default()
            });
            continue;
        };

        let output_key = ResolvedItemPlan::primary_output_key(&item.id);
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
        mappings.defaults.presentation_size,
        file_index,
    );
    audit_mutable_presentation_target_collisions(&mut entries);
    entries
}

/// Render typed plans back into preview rows for MCP output.
pub fn render_preview(plans: &[ResolvedItemPlan]) -> Vec<PreviewEntry> {
    plans.iter().cloned().map(PreviewEntry::from).collect()
}

/// Build a preview of the proposed playlist for a set of PCO items.
pub fn build_preview(
    items: &[Item],
    mappings: &ProjectConfig,
    file_index: Option<&FileIndex>,
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

    for (index, plan) in entries.iter().enumerate() {
        let (key, description, display_name) = match plan.action {
            PlanAction::EditInPlace => {
                let Some(path) = plan.file_path.as_deref() else {
                    continue;
                };
                (
                    format!("edit_in_place:{path}"),
                    "edit-in-place file",
                    path.to_string(),
                )
            }
            PlanAction::GenerateNew => {
                let name = crate::propresenter::playlist::canonical_presentation_name(
                    &plan.playlist_name,
                    plan.slide_type(),
                );
                let filename = format!("{name}.pro");
                (
                    format!("generate_new:{}", filename.to_lowercase()),
                    "generated file",
                    filename,
                )
            }
            PlanAction::UseExisting | PlanAction::Skip | PlanAction::NeedsReview => continue,
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
            entries[*index].action = PlanAction::NeedsReview;
            entries[*index].reason.clone_from(&reason);
        }
    }
}

fn ensure_required_playlist_items(
    entries: &mut Vec<ResolvedItemPlan>,
    mappings: &ProjectConfig,
    file_index: Option<&FileIndex>,
    service_name: Option<&str>,
) {
    let mut start = Vec::new();
    let mut end = Vec::new();
    for required in &mappings.required_playlist_items {
        if !required_playlist_item_applies(required, mappings, service_name) {
            continue;
        }
        let target = resolve_exact_library_file(file_index, &required.library_file);
        if let ExactLibraryFileMatch::Unique(path) = &target {
            entries.retain(|entry| {
                entry.action == PlanAction::Skip || entry.file_path.as_deref() != Some(path)
            });
        }
        let plan = build_required_playlist_item(required, &target, mappings, service_name, entries);
        match required.placement {
            RequiredPlaylistPlacement::Start => start.push(plan),
            RequiredPlaylistPlacement::End => end.push(plan),
        }
    }

    if start.is_empty() && end.is_empty() {
        return;
    }
    let mut combined = Vec::with_capacity(start.len() + entries.len() + end.len());
    combined.extend(start);
    combined.append(entries);
    combined.extend(end);
    *entries = combined;
}

fn required_playlist_item_applies(
    required: &RequiredPlaylistItemConfig,
    mappings: &ProjectConfig,
    service_name: Option<&str>,
) -> bool {
    let Some(group_name) = required.service_group.as_deref() else {
        return true;
    };
    let Some(service_name) = service_name else {
        return false;
    };
    mappings
        .service_groups
        .get(group_name)
        .is_some_and(|group| {
            group
                .service_types
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(service_name))
        })
}

fn build_required_playlist_item(
    required: &RequiredPlaylistItemConfig,
    target: &ExactLibraryFileMatch,
    mappings: &ProjectConfig,
    service_name: Option<&str>,
    existing: &[ResolvedItemPlan],
) -> ResolvedItemPlan {
    let file_path = match &target {
        ExactLibraryFileMatch::Unique(path) => Some(path.clone()),
        ExactLibraryFileMatch::Missing | ExactLibraryFileMatch::Ambiguous => None,
    };

    let position = match required.placement {
        RequiredPlaylistPlacement::Start => existing
            .first()
            .map_or(0, |entry| entry.position.saturating_sub(1)),
        RequiredPlaylistPlacement::End => existing
            .last()
            .map_or(0, |entry| entry.position.saturating_add(1)),
    };
    let Some(presentation_type) = mappings.presentation_types.get(&required.use_type) else {
        return ResolvedItemPlan {
            output_key: ResolvedItemPlan::required_output_key(&required.id),
            position,
            pco_title: file_stem(&required.library_file),
            playlist_name: file_stem(&required.library_file),
            action: PlanAction::NeedsReview,
            reason: format!("Unknown presentation type '{}'", required.use_type),
            item_type: Some(required.use_type.clone()),
            ..ResolvedItemPlan::default()
        };
    };

    let (action, reason) = match target {
        ExactLibraryFileMatch::Unique(_) => (
            PlanAction::UseExisting,
            format!(
                "Required playlist item inserted at {}",
                required_placement_name(required.placement)
            ),
        ),
        ExactLibraryFileMatch::Missing => (
            PlanAction::NeedsReview,
            format!(
                "Required playlist file not found: {}",
                required.library_file
            ),
        ),
        ExactLibraryFileMatch::Ambiguous => (
            PlanAction::NeedsReview,
            format!(
                "Required playlist file is ambiguous: {}",
                required.library_file
            ),
        ),
    };
    ResolvedItemPlan {
        output_key: ResolvedItemPlan::required_output_key(&required.id),
        position,
        pco_title: file_stem(&required.library_file),
        playlist_name: file_path
            .as_deref()
            .map_or_else(|| file_stem(&required.library_file), file_stem),
        file_path,
        action,
        reason,
        item_kind: presentation_type.kind,
        item_type: Some(required.use_type.clone()),
        content_source: ContentSource::None,
        style: resolve_style(
            presentation_type,
            &required.use_type,
            service_name,
            mappings,
        ),
    }
}

const fn required_placement_name(placement: RequiredPlaylistPlacement) -> &'static str {
    match placement {
        RequiredPlaylistPlacement::Start => "start",
        RequiredPlaylistPlacement::End => "end",
    }
}

fn audit_selected_presentation_sizes(
    entries: &mut [ResolvedItemPlan],
    expected: crate::propresenter::PresentationSize,
    file_index: Option<&FileIndex>,
) {
    let Some(file_index) = file_index else {
        return;
    };
    for plan in entries {
        if plan.action != PlanAction::UseExisting {
            continue;
        }
        let Some(path) = plan.file_path.as_deref() else {
            continue;
        };
        let Some(indexed) = file_index.entry_at(std::path::Path::new(path)) else {
            continue;
        };
        if indexed.presentation_size.matches(expected) {
            continue;
        }
        plan.action = PlanAction::NeedsReview;
        plan.reason = format!(
            "Presentation size {} does not match project {}; set the expected output first, then reapply the theme",
            indexed.presentation_size.describe(),
            expected
        );
    }
}

// ---------------------------------------------------------------------------
// Rule evaluation
// ---------------------------------------------------------------------------

fn find_matching_rule<'a>(
    item: &Item,
    title_lower: &str,
    mappings: &'a ProjectConfig,
    service_name: Option<&str>,
) -> Option<&'a ItemRuleConfig> {
    mappings
        .item_rules
        .iter()
        .find(|rule| rule_matches_item(rule, item, title_lower, service_name))
}

fn rule_matches_item(
    rule: &ItemRuleConfig,
    item: &Item,
    title_lower: &str,
    service_name: Option<&str>,
) -> bool {
    match_spec_matches_item(&rule.match_spec, item, title_lower, service_name)
}

fn match_spec_matches_item(
    match_spec: &MatchSpec,
    item: &Item,
    title_lower: &str,
    service_name: Option<&str>,
) -> bool {
    if !match_spec.service_type.is_empty() {
        let Some(service_name) = service_name else {
            return false;
        };
        if !match_spec
            .service_type
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(service_name))
        {
            return false;
        }
    }

    if let Some(category) = &match_spec.category {
        if !category.eq_ignore_ascii_case(category_name(item)) {
            return false;
        }
    }

    if let Some(expected) = match_spec.has_scripture_ref {
        let actual = item.scripture.is_some()
            || has_scripture_ref(&item.title)
            || has_scripture_ref(&strip_title_prefix(&item.title));
        if actual != expected {
            return false;
        }
    }

    if !match_spec.title_prefix.is_empty()
        && !match_spec
            .title_prefix
            .iter()
            .any(|prefix| title_lower.starts_with(&prefix.to_lowercase()))
    {
        return false;
    }

    if !match_spec.title_contains.is_empty()
        && !match_spec
            .title_contains
            .iter()
            .any(|needle| title_lower.contains(&needle.to_lowercase()))
    {
        return false;
    }

    if !match_spec.description_contains.is_empty() {
        let description_lower = normalize_apostrophes(
            &item
                .description
                .as_deref()
                .unwrap_or_default()
                .to_lowercase(),
        );
        if !match_spec
            .description_contains
            .iter()
            .any(|needle| description_lower.contains(&needle.to_lowercase()))
        {
            return false;
        }
    }

    true
}

/// Replace curly apostrophes (U+2018, U+2019) with ASCII straight apostrophe.
/// PCO occasionally uses smart quotes while config rules use straight ones.
fn normalize_apostrophes(s: &str) -> String {
    s.replace(['\u{2018}', '\u{2019}'], "'")
}

#[allow(clippy::missing_const_for_fn)]
fn category_name(item: &Item) -> &'static str {
    match item.category {
        crate::planning_center::types::Category::Text => "text",
        crate::planning_center::types::Category::Graphic => "graphic",
        crate::planning_center::types::Category::Title => "title",
        crate::planning_center::types::Category::Song => "song",
        crate::planning_center::types::Category::Other => "other",
    }
}

fn build_use_type_rule_plan(
    type_key: &str,
    target: Option<&crate::project_config::TargetSpec>,
    item: &Item,
    mappings: &ProjectConfig,
    file_index: Option<&FileIndex>,
    service_name: Option<&str>,
) -> ResolvedItemPlan {
    let output_key = ResolvedItemPlan::primary_output_key(&item.id);
    let Some(ptype) = mappings.presentation_types.get(type_key) else {
        return ResolvedItemPlan {
            output_key,
            position: item.position,
            pco_title: item.title.clone(),
            playlist_name: strip_speaker(&item.title),
            action: PlanAction::NeedsReview,
            reason: format!("Unknown presentation type '{type_key}'"),
            item_type: Some(type_key.to_string()),
            ..Default::default()
        };
    };

    let mut plan = build_type_plan(
        type_key,
        ptype,
        item,
        target.and_then(crate::project_config::TargetSpec::library_file),
        mappings,
        file_index,
        service_name,
    );
    plan.output_key = output_key;
    plan
}

fn build_decision_plan(
    decision: &DecisionConfig,
    rule: &ItemRuleConfig,
    item: &Item,
    output_key: String,
    mappings: &ProjectConfig,
    file_index: Option<&FileIndex>,
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
                let action = match on_ambiguous.unwrap_or_default() {
                    AmbiguousDecisionPolicy::Ask => PlanAction::NeedsReview,
                    AmbiguousDecisionPolicy::Skip => PlanAction::Skip,
                };
                let reason =
                    decision_review_reason(rule, instructions.as_deref(), choices, &matched);
                return ResolvedItemPlan {
                    output_key,
                    position: item.position,
                    pco_title: item.title.clone(),
                    playlist_name: strip_speaker(&item.title),
                    action,
                    reason,
                    ..Default::default()
                };
            }

            let (choice_key, choice) = matched[0];
            let Some(type_key) = choice.use_type.as_deref() else {
                return ResolvedItemPlan {
                    output_key,
                    position: item.position,
                    pco_title: item.title.clone(),
                    playlist_name: strip_speaker(&item.title),
                    action: PlanAction::NeedsReview,
                    reason: format!("Decision choice '{choice_key}' has no use_type"),
                    ..Default::default()
                };
            };
            let Some(ptype) = mappings.presentation_types.get(type_key) else {
                return ResolvedItemPlan {
                    output_key,
                    position: item.position,
                    pco_title: item.title.clone(),
                    playlist_name: strip_speaker(&item.title),
                    action: PlanAction::NeedsReview,
                    reason: format!(
                        "Decision choice '{choice_key}' uses unknown type '{type_key}'"
                    ),
                    item_type: Some(type_key.to_string()),
                    ..Default::default()
                };
            };

            let target_library_file = choice_library_file(choice);
            let mut plan = build_type_plan(
                type_key,
                ptype,
                item,
                target_library_file.as_deref(),
                mappings,
                file_index,
                service_name,
            );
            plan.output_key = output_key;
            plan.reason = format!("Context choice '{choice_key}': {}", plan.reason);
            plan
        }
    }
}

fn decision_context_text(item: &Item, context_fields: &[String]) -> String {
    let fields: Vec<&str> = if context_fields.is_empty() {
        vec!["title", "description", "note"]
    } else {
        context_fields.iter().map(String::as_str).collect()
    };

    let mut values = Vec::new();
    for field in fields {
        match field {
            "title" => values.push(item.title.as_str()),
            "description" => {
                if let Some(description) = item.description.as_deref() {
                    values.push(description);
                }
            }
            "note" => {
                if let Some(note) = item.note.as_deref() {
                    values.push(note);
                }
            }
            _ => {}
        }
    }

    normalize_apostrophes(&values.join("\n").to_lowercase())
}

fn decision_choice_matches(choice: &DecisionChoiceConfig, context_text: &str) -> bool {
    let spec = &choice.match_spec;
    if spec
        .none
        .iter()
        .any(|needle| context_contains(context_text, needle))
    {
        return false;
    }
    if !spec.all.is_empty()
        && !spec
            .all
            .iter()
            .all(|needle| context_contains(context_text, needle))
    {
        return false;
    }
    spec.any.is_empty()
        || spec
            .any
            .iter()
            .any(|needle| context_contains(context_text, needle))
}

fn context_contains(context_text: &str, needle: &str) -> bool {
    let needle = normalize_apostrophes(&needle.to_lowercase());
    context_text.contains(&needle)
}

fn decision_review_reason(
    rule: &ItemRuleConfig,
    instructions: Option<&str>,
    choices: &std::collections::HashMap<String, DecisionChoiceConfig>,
    matched: &[(&String, &DecisionChoiceConfig)],
) -> String {
    let choices_list = {
        let mut keys: Vec<&str> = choices.keys().map(String::as_str).collect();
        keys.sort_unstable();
        keys.join(", ")
    };
    let base = if matched.is_empty() {
        format!(
            "Rule '{}' needs contextual choice; no choice matched. Choices: {choices_list}",
            rule.id
        )
    } else {
        let mut keys: Vec<&str> = matched.iter().map(|(key, _)| key.as_str()).collect();
        keys.sort_unstable();
        format!(
            "Rule '{}' needs contextual choice; multiple choices matched: {}",
            rule.id,
            keys.join(", ")
        )
    };

    instructions.map_or_else(|| base.clone(), |text| format!("{base}. {text}"))
}

fn choice_library_file(choice: &DecisionChoiceConfig) -> Option<String> {
    choice
        .target
        .as_ref()
        .and_then(crate::project_config::TargetSpec::library_file)
        .map(str::to_string)
        .or_else(|| choice.file.clone())
}

fn build_type_plan(
    type_key: &str,
    ptype: &PresentationTypeConfig,
    item: &Item,
    target_library_file: Option<&str>,
    mappings: &ProjectConfig,
    file_index: Option<&FileIndex>,
    service_name: Option<&str>,
) -> ResolvedItemPlan {
    match ptype.content_source {
        ContentSourceKind::Song => build_song_plan(
            type_key,
            ptype,
            item,
            target_library_file,
            mappings,
            file_index,
            service_name,
        ),
        ContentSourceKind::Scripture => {
            build_scripture_plan(type_key, ptype, item, mappings, service_name)
        }
        ContentSourceKind::Static | ContentSourceKind::Description => build_generic_plan(
            type_key,
            ptype,
            item,
            target_library_file,
            mappings,
            file_index,
            service_name,
        ),
    }
}

fn build_generic_plan(
    type_key: &str,
    ptype: &PresentationTypeConfig,
    item: &Item,
    target_library_file: Option<&str>,
    mappings: &ProjectConfig,
    file_index: Option<&FileIndex>,
    service_name: Option<&str>,
) -> ResolvedItemPlan {
    let target_match = target_library_file.map(|name| resolve_exact_library_file(file_index, name));
    let found = match &target_match {
        Some(ExactLibraryFileMatch::Unique(path)) => Some(path.clone()),
        Some(ExactLibraryFileMatch::Missing | ExactLibraryFileMatch::Ambiguous) | None => None,
    };
    let style = resolve_style(ptype, type_key, service_name, mappings);
    let has_description_content = matches!(ptype.content_source, ContentSourceKind::Description);
    let parse_result = if has_description_content {
        item.description
            .as_deref()
            .zip(ptype.description_parser)
            .map(|(description, parser)| {
                description_parser::parse_description(description, &item.title, parser)
            })
    } else {
        None
    };
    let (parsed_content, description_error) = match parse_result {
        Some(Ok(content)) => (content, None),
        Some(Err(error)) => (None, Some(error)),
        None => (None, None),
    };

    let content_source = if has_description_content {
        ContentSource::Description { parsed_content }
    } else {
        ContentSource::None
    };

    let (action, mut reason) = resolve_generic_action(
        ptype.output_strategy,
        &content_source,
        target_match.as_ref(),
    );
    if matches!(action, PlanAction::NeedsReview) {
        if let Some(error) = description_error {
            reason = error.to_string();
        }
    }
    if let (Some(target), Some(ExactLibraryFileMatch::Missing | ExactLibraryFileMatch::Ambiguous)) =
        (target_library_file, target_match.as_ref())
    {
        reason = format!("{reason}: {target}");
    }

    ResolvedItemPlan {
        output_key: String::new(),
        position: item.position,
        pco_title: item.title.clone(),
        playlist_name: found.as_deref().map_or_else(
            || target_library_file.map_or_else(|| strip_speaker(&item.title), file_stem),
            file_stem,
        ),
        file_path: found,
        action,
        reason,
        item_kind: ptype.kind,
        item_type: Some(type_key.to_string()),
        content_source,
        style,
    }
}

fn resolve_generic_action(
    output_strategy: OutputStrategy,
    content_source: &ContentSource,
    target_match: Option<&ExactLibraryFileMatch>,
) -> (PlanAction, String) {
    let has_description_content = matches!(content_source, ContentSource::Description { .. });
    let target_found = matches!(target_match, Some(ExactLibraryFileMatch::Unique(_)));
    let target_ambiguous = matches!(target_match, Some(ExactLibraryFileMatch::Ambiguous));
    match output_strategy {
        OutputStrategy::Skip => (PlanAction::Skip, "Configured to skip".to_string()),
        OutputStrategy::NeedsReview => (
            PlanAction::NeedsReview,
            "Configured to require review".to_string(),
        ),
        OutputStrategy::UseExisting => {
            if target_ambiguous {
                (
                    PlanAction::NeedsReview,
                    "Configured existing file is ambiguous".to_string(),
                )
            } else if target_found {
                (PlanAction::UseExisting, "Library match".to_string())
            } else {
                (
                    PlanAction::NeedsReview,
                    "Configured existing file not found".to_string(),
                )
            }
        }
        OutputStrategy::EditInPlace => {
            if !has_description_content {
                (
                    PlanAction::NeedsReview,
                    "Edit-in-place requires description content".to_string(),
                )
            } else if !matches!(
                &content_source,
                ContentSource::Description {
                    parsed_content: Some(_)
                }
            ) {
                (
                    PlanAction::NeedsReview,
                    "No description content to edit".to_string(),
                )
            } else if target_ambiguous {
                (
                    PlanAction::NeedsReview,
                    "Edit-in-place target is ambiguous".to_string(),
                )
            } else if target_found {
                (
                    PlanAction::EditInPlace,
                    "Content updated from description".to_string(),
                )
            } else {
                (
                    PlanAction::NeedsReview,
                    "Edit-in-place target not found".to_string(),
                )
            }
        }
        OutputStrategy::GenerateNew => {
            if has_description_content {
                match &content_source {
                    ContentSource::Description {
                        parsed_content: Some(_),
                    } => (
                        PlanAction::GenerateNew,
                        "Generate from description content".to_string(),
                    ),
                    _ => (
                        PlanAction::NeedsReview,
                        "No description content to generate".to_string(),
                    ),
                }
            } else {
                (
                    PlanAction::NeedsReview,
                    "Generate-new is not implemented for static content".to_string(),
                )
            }
        }
    }
}

fn build_song_plan(
    type_key: &str,
    ptype: &PresentationTypeConfig,
    item: &Item,
    target_library_file: Option<&str>,
    mappings: &ProjectConfig,
    file_index: Option<&FileIndex>,
    service_name: Option<&str>,
) -> ResolvedItemPlan {
    let song_title = item
        .song
        .as_ref()
        .map_or(item.title.as_str(), |s| s.title.as_str());
    let stripped = strip_title_prefix(&item.title);
    let bare_title = strip_hymn_number(song_title);
    let mut style = resolve_style(ptype, type_key, service_name, mappings);
    if matches!(ptype.output_strategy, OutputStrategy::UseExisting) && style.arrangement.is_none() {
        style.arrangement = item.song.as_ref().and_then(|song| {
            let arrangement = song.arrangement.as_deref()?.trim();
            (!arrangement.is_empty()).then(|| arrangement.to_string())
        });
    }

    let explicit_target_match =
        target_library_file.map(|name| (name, resolve_exact_library_file(file_index, name)));
    let song_match = match &explicit_target_match {
        Some((_, ExactLibraryFileMatch::Unique(path))) => SongLibraryMatch::Resolved(path.clone()),
        Some((_, ExactLibraryFileMatch::Missing | ExactLibraryFileMatch::Ambiguous)) => {
            SongLibraryMatch::Missing
        }
        None => {
            resolve_song_library_match(file_index, song_title, &item.title, &stripped, &bare_title)
        }
    };

    let (mut action, mut reason) = match ptype.output_strategy {
        OutputStrategy::Skip => (PlanAction::Skip, "Configured to skip".to_string()),
        OutputStrategy::NeedsReview => (
            PlanAction::NeedsReview,
            "Configured to require review".to_string(),
        ),
        OutputStrategy::UseExisting => {
            if let Some((target, ExactLibraryFileMatch::Ambiguous)) = &explicit_target_match {
                (
                    PlanAction::NeedsReview,
                    format!("Configured existing song target is ambiguous: {target}"),
                )
            } else {
                match &song_match {
                    SongLibraryMatch::Resolved(_) => {
                        (PlanAction::UseExisting, "Library match".to_string())
                    }
                    SongLibraryMatch::Candidate(_) => (
                        PlanAction::NeedsReview,
                        "Possible library match".to_string(),
                    ),
                    SongLibraryMatch::Missing => (
                        PlanAction::NeedsReview,
                        target_library_file.map_or_else(
                            || "No song library match".to_string(),
                            |target| format!("Configured existing song not found: {target}"),
                        ),
                    ),
                }
            }
        }
        OutputStrategy::EditInPlace => (
            PlanAction::NeedsReview,
            "Edit-in-place is not supported for song content".to_string(),
        ),
        OutputStrategy::GenerateNew => (
            PlanAction::NeedsReview,
            "Generate-new is not supported for song content".to_string(),
        ),
    };

    if action == PlanAction::UseExisting {
        if let (SongLibraryMatch::Resolved(path), Some(requested)) =
            (&song_match, style.arrangement.clone())
        {
            match resolve_song_arrangement(file_index, path, &requested) {
                Ok(SongArrangementResolution::Selected(canonical_name)) => {
                    style.arrangement = Some(canonical_name);
                }
                Ok(SongArrangementResolution::NoSelection) => style.arrangement = None,
                Err(review_reason) => {
                    action = PlanAction::NeedsReview;
                    reason = review_reason;
                }
            }
        }
    }

    let playlist_name = match &song_match {
        SongLibraryMatch::Resolved(path) | SongLibraryMatch::Candidate(path) => file_stem(path),
        SongLibraryMatch::Missing => song_title.to_string(),
    };

    ResolvedItemPlan {
        position: item.position,
        pco_title: item.title.clone(),
        playlist_name,
        file_path: song_match.into_path(),
        action,
        reason,
        item_kind: ItemKind::Song,
        item_type: Some(type_key.to_string()),
        style,
        ..Default::default()
    }
}

const PCO_DEFAULT_ARRANGEMENT: &str = "Default Arrangement";
const NATIVE_DEFAULT_ARRANGEMENT: &str = "Default";

#[derive(Debug, PartialEq, Eq)]
enum SongArrangementResolution {
    Selected(String),
    NoSelection,
}

fn resolve_song_arrangement(
    file_index: Option<&FileIndex>,
    presentation_path: &str,
    requested: &str,
) -> Result<SongArrangementResolution, String> {
    let Some(entry) = file_index.and_then(|index| {
        index
            .entries
            .iter()
            .find(|entry| entry.full_path.to_string_lossy() == presentation_path)
    }) else {
        return Err(format!(
            "Could not verify arrangement '{requested}' because the resolved library file is not indexed"
        ));
    };

    let exact_matches = entry
        .arrangements
        .iter()
        .filter(|arrangement| arrangement.name().eq_ignore_ascii_case(requested))
        .collect::<Vec<_>>();
    if exact_matches.is_empty()
        && requested == PCO_DEFAULT_ARRANGEMENT
        && entry.arrangements.is_empty()
    {
        return Ok(SongArrangementResolution::NoSelection);
    }
    let matches = if exact_matches.is_empty() && requested == PCO_DEFAULT_ARRANGEMENT {
        entry
            .arrangements
            .iter()
            .filter(|arrangement| {
                arrangement
                    .name()
                    .eq_ignore_ascii_case(NATIVE_DEFAULT_ARRANGEMENT)
            })
            .collect::<Vec<_>>()
    } else {
        exact_matches
    };
    let available = arrangement_names(&entry.arrangements);

    match matches.as_slice() {
        [IndexedArrangement::Complete { name }] => {
            Ok(SongArrangementResolution::Selected(name.clone()))
        }
        [IndexedArrangement::Incomplete { .. }] => Err(format!(
            "Arrangement '{requested}' in '{}' has a missing or invalid UUID; available arrangements: {available}",
            entry.file_name
        )),
        [] => Err(format!(
            "Arrangement '{requested}' is unavailable in '{}'; available arrangements: {available}",
            entry.file_name
        )),
        _ => Err(format!(
            "Arrangement '{requested}' is ambiguous in '{}'; available arrangements: {available}",
            entry.file_name
        )),
    }
}

fn arrangement_names(arrangements: &[IndexedArrangement]) -> String {
    let mut names = arrangements
        .iter()
        .map(IndexedArrangement::name)
        .filter(|name| !name.trim().is_empty())
        .collect::<Vec<_>>();
    names.sort_unstable_by(|left, right| {
        left.to_ascii_lowercase()
            .cmp(&right.to_ascii_lowercase())
            .then_with(|| left.cmp(right))
    });
    if names.is_empty() {
        "none".to_string()
    } else {
        names.join(", ")
    }
}

#[allow(clippy::too_many_lines)]
fn build_scripture_plan(
    type_key: &str,
    ptype: &PresentationTypeConfig,
    item: &Item,
    mappings: &ProjectConfig,
    service_name: Option<&str>,
) -> ResolvedItemPlan {
    let style = resolve_style(ptype, type_key, service_name, mappings);
    let unsupported_strategy = match ptype.output_strategy {
        OutputStrategy::Skip => Some((PlanAction::Skip, "Configured to skip")),
        OutputStrategy::NeedsReview => {
            Some((PlanAction::NeedsReview, "Configured to require review"))
        }
        OutputStrategy::UseExisting => Some((
            PlanAction::NeedsReview,
            "Use-existing is not supported for scripture generation",
        )),
        OutputStrategy::EditInPlace => Some((
            PlanAction::NeedsReview,
            "Edit-in-place is not supported for scripture generation",
        )),
        OutputStrategy::GenerateNew => None,
    };
    if let Some((action, reason)) = unsupported_strategy {
        return ResolvedItemPlan {
            position: item.position,
            pco_title: item.title.clone(),
            playlist_name: strip_speaker(&item.title),
            action,
            reason: reason.to_string(),
            item_kind: ItemKind::Scripture,
            item_type: Some(type_key.to_string()),
            style,
            ..Default::default()
        };
    }

    let parsed_refs = match item_scripture_refs(item, mappings.defaults.bible_version) {
        Ok(references) => references,
        Err(error) => {
            return ResolvedItemPlan {
                position: item.position,
                pco_title: item.title.clone(),
                playlist_name: strip_speaker(&item.title),
                action: PlanAction::NeedsReview,
                reason: error,
                item_kind: ItemKind::Scripture,
                item_type: Some(type_key.to_string()),
                style,
                ..Default::default()
            };
        }
    };

    if parsed_refs.len() > 1 {
        let ref_infos: Vec<ScriptureRefInfo> = parsed_refs
            .iter()
            .map(|reference| ScriptureRefInfo {
                reference: reference.reference.clone(),
                version: reference.version.clone(),
            })
            .collect();
        let first_version = &ref_infos[0].version;
        let same_version = ref_infos
            .iter()
            .all(|reference| reference.version == *first_version);
        let combined_name = if same_version {
            format!(
                "{} {first_version}",
                ref_infos
                    .iter()
                    .map(|reference| reference.reference.replace(':', "v"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        } else {
            ref_infos
                .iter()
                .map(|reference| {
                    format!(
                        "{} {}",
                        reference.reference.replace(':', "v"),
                        reference.version
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        let version_summary = if same_version {
            first_version.clone()
        } else {
            "mixed versions".to_string()
        };
        let reference_count = ref_infos.len();
        let Some(scripture) = ScriptureContent::combined(ref_infos) else {
            return ResolvedItemPlan {
                position: item.position,
                pco_title: item.title.clone(),
                playlist_name: combined_name,
                action: PlanAction::NeedsReview,
                reason: "Combined scripture source requires at least two references".to_string(),
                item_kind: ItemKind::Scripture,
                item_type: Some(type_key.to_string()),
                style,
                ..Default::default()
            };
        };

        return ResolvedItemPlan {
            position: item.position,
            pco_title: item.title.clone(),
            playlist_name: combined_name,
            action: PlanAction::GenerateNew,
            reason: format!(
                "Generate combined scripture slides ({reference_count} refs, {version_summary})"
            ),
            item_kind: ItemKind::Scripture,
            item_type: Some(type_key.to_string()),
            content_source: ContentSource::Scripture { scripture },
            style,
            ..Default::default()
        };
    }

    let parsed_ref = &parsed_refs[0];

    ResolvedItemPlan {
        position: item.position,
        pco_title: item.title.clone(),
        playlist_name: format!("{} {}", parsed_ref.reference, parsed_ref.version),
        action: PlanAction::GenerateNew,
        reason: format!("Generate scripture slides ({})", parsed_ref.version),
        item_kind: ItemKind::Scripture,
        item_type: Some(type_key.to_string()),
        content_source: ContentSource::Scripture {
            scripture: ScriptureContent::single(
                parsed_ref.reference.clone(),
                parsed_ref.version.clone(),
            ),
        },
        style,
        ..Default::default()
    }
}

fn item_scripture_refs(
    item: &Item,
    configured_default: Option<BibleVersion>,
) -> Result<Vec<ParsedScriptureRef>, String> {
    let Some(structured) = item.scripture.as_ref() else {
        return parse_scripture_refs(&item.title, configured_default)
            .map_err(|error| error.to_string());
    };
    let Some(translation) = structured.translation.as_deref() else {
        return parse_scripture_refs(&item.title, configured_default)
            .map_err(|error| error.to_string());
    };
    let translation = translation.trim();
    let Some(version) = BibleVersion::all()
        .iter()
        .copied()
        .find(|version| version.name().eq_ignore_ascii_case(translation))
    else {
        return Err(format!("Unsupported Bible version '{translation}'"));
    };
    let reference = structured.reference.trim();
    if reference.is_empty() {
        return Err("No scripture reference".to_string());
    }
    parse_scripture_refs(&format!("{reference} {}", version.name()), None)
        .map_err(|error| error.to_string())
}

fn rule_action_plan(action: &RuleAction, item: &Item, output_key: String) -> ResolvedItemPlan {
    let (plan_action, reason) = match action {
        RuleAction::Skip { reason } => (PlanAction::Skip, reason.clone()),
        RuleAction::Review { reason } => (PlanAction::NeedsReview, reason.clone()),
    };

    ResolvedItemPlan {
        output_key,
        position: item.position,
        pco_title: item.title.clone(),
        playlist_name: strip_speaker(&item.title),
        action: plan_action,
        reason,
        ..Default::default()
    }
}

fn resolve_style(
    ptype: &PresentationTypeConfig,
    type_key: &str,
    service_name: Option<&str>,
    mappings: &ProjectConfig,
) -> PresentationStyle {
    let mut style = PresentationStyle::default();

    if matches!(ptype.output_strategy, OutputStrategy::UseExisting) {
        style.arrangement.clone_from(&ptype.arrangement);
        apply_arrangement_overrides(&mut style, type_key, service_name, mappings);
        return style;
    }

    if let Some(display) = &ptype.display {
        match display {
            DisplayBindingConfig::Single { role } => {
                if let Some(role) = mappings.cue_roles.get(role) {
                    apply_single_cue_role(&mut style, role);
                }
            }
            DisplayBindingConfig::Split { title, content } => {
                if let Some(role) = mappings.cue_roles.get(title) {
                    apply_title_cue_role(&mut style, role);
                }
                if let Some(role) = mappings.cue_roles.get(content) {
                    apply_content_cue_role(&mut style, role);
                }
            }
        }
    }

    style.max_lines_per_slide = ptype.max_lines_per_slide.map(std::num::NonZeroUsize::get);
    let mut background_id = ptype
        .background
        .as_ref()
        .or(mappings.defaults.background.as_ref());

    for override_rule in &mappings.overrides {
        if override_applies(override_rule, type_key, service_name, mappings) {
            if let Some(background) = &override_rule.background {
                background_id = Some(background);
            }
        }
    }
    style.background = background_id.and_then(|id| {
        mappings
            .backgrounds
            .get(id)
            .cloned()
            .map(|file| ResolvedBackground::new(id.clone(), file))
    });

    style
}

fn apply_arrangement_overrides(
    style: &mut PresentationStyle,
    type_key: &str,
    service_name: Option<&str>,
    mappings: &ProjectConfig,
) {
    for override_rule in &mappings.overrides {
        if override_applies(override_rule, type_key, service_name, mappings)
            && override_rule.arrangement.is_some()
        {
            style.arrangement.clone_from(&override_rule.arrangement);
        }
    }
}

fn cue_macro(role: &CueRoleConfig) -> Option<CueMacro> {
    role.enter_macro
        .as_ref()
        .map(|enter| CueMacro::new(enter.clone(), role.all_content_colored_macro.clone()))
}

fn apply_single_cue_role(style: &mut PresentationStyle, role: &CueRoleConfig) {
    style.content_slide = Some(role.slide.clone());
    style.first_cue_macro = cue_macro(role);
}

fn apply_title_cue_role(style: &mut PresentationStyle, role: &CueRoleConfig) {
    style.title_slide = Some(role.slide.clone());
    style.first_cue_macro = cue_macro(role);
}

fn apply_content_cue_role(style: &mut PresentationStyle, role: &CueRoleConfig) {
    style.content_slide = Some(role.slide.clone());
    style.first_content_cue_macro = cue_macro(role);
}

fn override_applies(
    override_rule: &OverrideRuleConfig,
    type_key: &str,
    service_name: Option<&str>,
    mappings: &ProjectConfig,
) -> bool {
    if override_rule
        .when
        .presentation_type
        .as_deref()
        .is_some_and(|value| value != type_key)
    {
        return false;
    }

    if let Some(service_type) = &override_rule.when.service_type {
        let Some(service_name) = service_name else {
            return false;
        };
        if !service_type.eq_ignore_ascii_case(service_name) {
            return false;
        }
    }

    if let Some(group_name) = &override_rule.when.service_group {
        let Some(service_name) = service_name else {
            return false;
        };
        let Some(group) = mappings.service_groups.get(group_name) else {
            return false;
        };
        if !group
            .service_types
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(service_name))
        {
            return false;
        }
    }

    true
}

// ---------------------------------------------------------------------------
// Expansion processing
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn process_expansion(
    expansion: &ExpansionRule,
    item: &Item,
    speaker: Option<&str>,
    mappings: &ProjectConfig,
    entries: &mut Vec<ResolvedItemPlan>,
    nametag_seen: &mut HashSet<String>,
    file_index: Option<&FileIndex>,
    service_name: Option<&str>,
) {
    for (step_index, step) in expansion.iter().enumerate() {
        let output_key =
            ResolvedItemPlan::expanded_output_key(&item.id, step_index, &step.use_type);
        if step.speaker.is_some() {
            if let Some(name) = speaker {
                if matches!(
                    mappings
                        .presentation_types
                        .get(&step.use_type)
                        .map(|ptype| ptype.kind),
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
                entries.push(ResolvedItemPlan {
                    output_key,
                    position: item.position,
                    pco_title: item.title.clone(),
                    playlist_name: strip_speaker(&item.title),
                    action: PlanAction::NeedsReview,
                    reason: "Speaker could not be resolved for expansion".to_string(),
                    item_type: Some(step.use_type.clone()),
                    ..Default::default()
                });
            }
            continue;
        }

        let Some(ptype) = mappings.presentation_types.get(&step.use_type) else {
            entries.push(ResolvedItemPlan {
                output_key,
                position: item.position,
                pco_title: item.title.clone(),
                playlist_name: strip_speaker(&item.title),
                action: PlanAction::NeedsReview,
                reason: format!("Unknown presentation type '{}'", step.use_type),
                item_type: Some(step.use_type.clone()),
                ..Default::default()
            });
            continue;
        };

        let mut plan = build_type_plan(
            &step.use_type,
            ptype,
            item,
            step.target
                .as_ref()
                .and_then(crate::project_config::TargetSpec::library_file),
            mappings,
            file_index,
            service_name,
        );
        plan.output_key = output_key;
        entries.push(plan);
    }
}

fn build_speaker_expansion_plan(
    step: &crate::project_config::ExpansionStep,
    item: &Item,
    name: &str,
    mappings: &ProjectConfig,
    file_index: Option<&FileIndex>,
    service_name: Option<&str>,
    output_key: String,
) -> ResolvedItemPlan {
    let Some(ptype) = mappings.presentation_types.get(&step.use_type) else {
        return ResolvedItemPlan {
            output_key,
            position: item.position,
            pco_title: item.title.clone(),
            playlist_name: strip_speaker(&item.title),
            action: PlanAction::NeedsReview,
            reason: format!("Unknown presentation type '{}'", step.use_type),
            item_type: Some(step.use_type.clone()),
            ..Default::default()
        };
    };

    if !matches!(ptype.kind, crate::project_config::ItemKind::Nametag) {
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
            &step.use_type,
            ptype,
            item,
            resolved_target.as_deref(),
            mappings,
            file_index,
            service_name,
        );
        plan.output_key = output_key;
        if let Some(target_name) = resolved_target.as_deref() {
            if plan.file_path.is_none() || plan.playlist_name == strip_speaker(&item.title) {
                plan.playlist_name = file_stem(target_name);
            }
        }
        return plan;
    }

    let explicit_target = step
        .target
        .as_ref()
        .and_then(crate::project_config::TargetSpec::library_file);
    let Some(nametag_name) = mappings
        .people
        .get(&first_name(name))
        .and_then(|p| p.nametag.clone())
        .or_else(|| explicit_target.map(file_stem))
        .or_else(|| {
            step.target
                .as_ref()
                .and_then(crate::project_config::TargetSpec::name_template)
                .map(|template| render_name_template(template, item, name))
        })
    else {
        return ResolvedItemPlan {
            output_key,
            position: item.position,
            pco_title: item.title.clone(),
            playlist_name: strip_speaker(&item.title),
            action: PlanAction::NeedsReview,
            reason: format!("Configured person '{name}' has no nametag target"),
            item_kind: ItemKind::Nametag,
            item_type: Some(step.use_type.clone()),
            ..Default::default()
        };
    };
    let target_file = explicit_target.unwrap_or(&nametag_name);
    let (found, target_ambiguous) = match resolve_exact_library_file(file_index, target_file) {
        ExactLibraryFileMatch::Unique(path) => (Some(path), false),
        ExactLibraryFileMatch::Missing => (None, false),
        ExactLibraryFileMatch::Ambiguous => (None, true),
    };
    let style = resolve_style(ptype, &step.use_type, service_name, mappings);

    let (action, reason) = resolve_nametag_action(
        ptype.output_strategy,
        found.is_some(),
        target_ambiguous,
        &nametag_name,
        name,
    );

    ResolvedItemPlan {
        output_key,
        position: item.position,
        pco_title: item.title.clone(),
        playlist_name: nametag_name,
        file_path: found,
        action,
        reason,
        item_kind: ItemKind::Nametag,
        item_type: Some(step.use_type.clone()),
        style,
        ..Default::default()
    }
}

fn resolve_nametag_action(
    output_strategy: OutputStrategy,
    target_found: bool,
    target_ambiguous: bool,
    nametag_name: &str,
    person_name: &str,
) -> (PlanAction, String) {
    match output_strategy {
        OutputStrategy::Skip => (PlanAction::Skip, "Configured to skip".to_string()),
        OutputStrategy::NeedsReview => (
            PlanAction::NeedsReview,
            "Configured to require review".to_string(),
        ),
        OutputStrategy::UseExisting => {
            if target_ambiguous {
                (
                    PlanAction::NeedsReview,
                    format!("{nametag_name} — configured target is ambiguous"),
                )
            } else if target_found {
                (
                    PlanAction::UseExisting,
                    format!("Nametag for {person_name}"),
                )
            } else {
                (
                    PlanAction::NeedsReview,
                    format!("{nametag_name} — not found"),
                )
            }
        }
        OutputStrategy::EditInPlace => (
            PlanAction::NeedsReview,
            "Edit-in-place is not supported for speaker nametags".to_string(),
        ),
        OutputStrategy::GenerateNew => (
            PlanAction::NeedsReview,
            "Generate-new is not supported for speaker nametags".to_string(),
        ),
    }
}

fn file_stem(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .and_then(|segment| segment.to_str())
        .unwrap_or(path)
        .to_string()
}

fn extract_speaker(title: &str) -> Option<String> {
    let start = title.rfind('(')?;
    let end = title.rfind(')')?;
    (end > start + 1).then(|| title[start + 1..end].trim().to_string())
}

/// Resolve the speaker for an item only when the person is configured.
/// Parentheticals and `Liturgist:` lines remain useful context, but an unknown
/// name requires review instead of inventing a nametag filename.
fn resolve_speaker(
    title: &str,
    description: Option<&str>,
    mappings: &ProjectConfig,
) -> Option<String> {
    if let Some(candidate) = extract_speaker(title) {
        if let Some(name) = configured_person_name(&candidate, mappings) {
            return Some(name);
        }
    }
    // Fall back to description "Liturgist:" line
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
    mappings.people.iter().find_map(|(first, person)| {
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

pub(super) fn strip_speaker(title: &str) -> String {
    title
        .rfind('(')
        .map_or_else(|| title.to_string(), |i| title[..i].trim().to_string())
}

fn strip_title_prefix(title: &str) -> String {
    const PREFIXES: &[&str] = &[
        "Organ Prelude:",
        "Organ Postlude:",
        "Offertory:",
        "Youth Choir:",
        "Scripture:",
        "Scripture -",
        "Scripture Reading:",
        "Moment for Mission:",
    ];
    for prefix in PREFIXES {
        if let Some(rest) = title.strip_prefix(prefix) {
            return strip_speaker(rest.trim());
        }
    }
    strip_speaker(title)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

    use super::*;
    use crate::planning_center::types::{Category, Item, Scripture, Song};
    use crate::project_config::{parse_project_config_str, validate_project_config};
    use crate::propresenter::generated::rv_data;
    use crate::workflow::description_parser::{ParsedContent, ParsedSegment};
    use prost::Message;
    use serde::Deserialize;
    use std::path::Path;
    use tempfile::tempdir;

    fn scripture_request(plan: &ResolvedItemPlan) -> ScriptureRequest<'_> {
        let ContentSource::Scripture { scripture } = &plan.content_source else {
            panic!("expected scripture content");
        };
        scripture.request()
    }

    #[test]
    fn preview_summary_counts_each_status_once() {
        let entries = [
            PreviewStatus::Used,
            PreviewStatus::Created,
            PreviewStatus::Edited,
            PreviewStatus::Skipped,
            PreviewStatus::Uncertain,
        ]
        .into_iter()
        .map(|status| PreviewEntry {
            status,
            ..PreviewEntry::default()
        })
        .collect::<Vec<_>>();

        let summary = PreviewSummary::from_entries(&entries);

        assert_eq!(summary.used_count, 1);
        assert_eq!(summary.created_count, 1);
        assert_eq!(summary.edited_count, 1);
        assert_eq!(summary.skip_count, 1);
        assert_eq!(summary.uncertain_count, 1);
        assert_eq!(summary.total_playlist_items, 3);
    }

    fn write_library_presentation(path: &Path) {
        write_library_presentation_with_size(path, 1920.0, 1080.0);
    }

    fn presentation_cue_with_size(width: f64, height: f64) -> rv_data::Cue {
        rv_data::Cue {
            actions: vec![rv_data::Action {
                action_type_data: Some(rv_data::action::ActionTypeData::Slide(
                    rv_data::action::SlideType {
                        slide: Some(rv_data::action::slide_type::Slide::Presentation(
                            rv_data::PresentationSlide {
                                base_slide: Some(rv_data::Slide {
                                    size: Some(rv_data::graphics::Size { width, height }),
                                    ..rv_data::Slide::default()
                                }),
                                ..rv_data::PresentationSlide::default()
                            },
                        )),
                    },
                )),
                ..rv_data::Action::default()
            }],
            ..rv_data::Cue::default()
        }
    }

    fn write_library_presentation_with_size(path: &Path, width: f64, height: f64) {
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .expect("fixture path has a UTF-8 stem");
        let presentation = rv_data::Presentation {
            uuid: Some(rv_data::Uuid {
                string: format!("{name}-id"),
            }),
            name: name.to_string(),
            cues: vec![presentation_cue_with_size(width, height)],
            ..rv_data::Presentation::default()
        };
        std::fs::write(path, presentation.encode_to_vec())
            .expect("write sized presentation fixture");
    }

    fn write_song_with_arrangements(path: &Path, arrangements: &[(&str, Option<&str>)]) {
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .expect("fixture path has a UTF-8 stem");
        let presentation = rv_data::Presentation {
            uuid: Some(rv_data::Uuid {
                string: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            }),
            name: name.to_string(),
            arrangements: arrangements
                .iter()
                .map(|(name, uuid)| rv_data::presentation::Arrangement {
                    uuid: uuid.map(|uuid| rv_data::Uuid {
                        string: uuid.to_string(),
                    }),
                    name: (*name).to_string(),
                    ..Default::default()
                })
                .collect(),
            cues: vec![presentation_cue_with_size(1920.0, 1080.0)],
            ..Default::default()
        };
        std::fs::write(path, presentation.encode_to_vec()).expect("write song fixture");
    }

    fn song_config(
        configured_arrangement: Option<&str>,
        override_arrangement: Option<&str>,
    ) -> ProjectConfig {
        let mut config = serde_json::json!({
            "version": 4,
            "presentation_types": {
                "song": {
                    "kind": "song",
                    "content_source": "song",
                    "output_strategy": "use_existing"
                }
            },
            "item_rules": [{
                "id": "song",
                "match": { "category": "song" },
                "use_type": "song",
                "target": { "library_file": "Amazing Grace.pro" }
            }]
        });
        if let Some(arrangement) = configured_arrangement {
            config["presentation_types"]["song"]["arrangement"] =
                serde_json::Value::String(arrangement.to_string());
        }
        if let Some(arrangement) = override_arrangement {
            config["overrides"] = serde_json::json!([{
                "when": { "service_type": "Christmas Eve" },
                "arrangement": arrangement
            }]);
        }
        parse_project_config_str(&config.to_string()).expect("song config should parse")
    }

    fn song_item(arrangement: Option<&str>) -> Item {
        Item {
            id: "song".to_string(),
            position: 1,
            title: "Amazing Grace".to_string(),
            description: None,
            category: Category::Song,
            note: None,
            song: Some(Song {
                title: "Amazing Grace".to_string(),
                author: None,
                copyright: None,
                ccli: None,
                themes: None,
                lyrics: None,
                arrangement: arrangement.map(str::to_string),
            }),
            scripture: None,
        }
    }

    fn song_index(arrangements: &[(&str, Option<&str>)]) -> (tempfile::TempDir, FileIndex) {
        let directory = tempdir().expect("fixture library directory");
        write_song_with_arrangements(&directory.path().join("Amazing Grace.pro"), arrangements);
        let index = FileIndex::build(directory.path()).expect("fixture library should index");
        (directory, index)
    }

    fn fixture_library() -> (tempfile::TempDir, FileIndex) {
        let directory = tempdir().expect("fixture library directory");
        for name in ["Amazing Grace.pro", "Call to Worship.pro"] {
            write_library_presentation(&directory.path().join(name));
        }
        let index = FileIndex::build(directory.path()).expect("fixture library should index");
        (directory, index)
    }

    fn load_config() -> ProjectConfig {
        parse_project_config_str(include_str!("../../tests/fixtures/workflow/v4_config.json"))
            .expect("fixture config should parse")
    }

    fn load_items() -> Vec<Item> {
        let raw: Vec<FixtureItem> =
            serde_json::from_str(include_str!("../../tests/fixtures/workflow/items.json"))
                .expect("fixture items should parse");
        raw.into_iter().map(FixtureItem::into_item).collect()
    }

    fn scripture_config() -> ProjectConfig {
        parse_project_config_str(
            r#"
            {
              "version": 4,
              "cue_roles": { "scripture": { "slide": "Scripture" } },
              "presentation_types": {
                "scripture": {
                  "kind": "scripture",
                  "content_source": "scripture",
                  "output_strategy": "generate_new",
                  "display": { "kind": "single", "role": "scripture" }
                }
              },
              "item_rules": [{
                "id": "scripture",
                "match": { "title_prefix": ["scripture"] },
                "use_type": "scripture"
              }]
            }
            "#,
        )
        .expect("scripture config should parse")
    }

    fn explicit_library_target_config() -> ProjectConfig {
        parse_project_config_str(
            r#"
            {
              "version": 4,
              "presentation_types": {
                "static_slide": {
                  "kind": "graphic",
                  "content_source": "static",
                  "output_strategy": "use_existing"
                },
                "song": {
                  "kind": "song",
                  "content_source": "song",
                  "output_strategy": "use_existing"
                }
              },
              "item_rules": [
                {
                  "id": "announcements",
                  "match": { "title_prefix": ["announcements"] },
                  "use_type": "static_slide",
                  "target": { "library_file": "Announcements.pro" }
                },
                {
                  "id": "song",
                  "match": { "title_prefix": ["song"] },
                  "use_type": "song",
                  "target": { "library_file": "Amazing Grace.pro" }
                }
              ]
            }
            "#,
        )
        .expect("explicit target config should parse")
    }

    fn explicit_library_target_items() -> Vec<Item> {
        vec![
            Item {
                id: "announcements".to_string(),
                position: 1,
                title: "Announcements".to_string(),
                description: None,
                category: Category::Graphic,
                note: None,
                song: None,
                scripture: None,
            },
            Item {
                id: "song".to_string(),
                position: 2,
                title: "Song".to_string(),
                description: None,
                category: Category::Song,
                note: None,
                song: Some(Song {
                    title: "Amazing Grace".to_string(),
                    author: None,
                    copyright: None,
                    ccli: None,
                    themes: None,
                    lyrics: None,
                    arrangement: None,
                }),
                scripture: None,
            },
        ]
    }

    fn mutable_target_collision_config() -> ProjectConfig {
        parse_project_config_str(
            r#"
            {
              "version": 4,
              "cue_roles": { "text": { "slide": "Text" } },
              "presentation_types": {
                "edited": {
                  "kind": "liturgy",
                  "content_source": "description",
                  "description_parser": "liturgical",
                  "output_strategy": "edit_in_place",
                  "display": { "kind": "single", "role": "text" }
                },
                "generated": {
                  "kind": "liturgy",
                  "content_source": "description",
                  "description_parser": "liturgical",
                  "output_strategy": "generate_new",
                  "display": { "kind": "single", "role": "text" }
                },
                "existing": {
                  "kind": "graphic",
                  "content_source": "static",
                  "output_strategy": "use_existing"
                }
              },
              "item_rules": [
                {
                  "id": "edited",
                  "match": { "title_prefix": ["edited"] },
                  "use_type": "edited",
                  "target": { "library_file": "Weekly Slot.pro" }
                },
                {
                  "id": "generated",
                  "match": { "title_prefix": ["generated"] },
                  "use_type": "generated"
                },
                {
                  "id": "existing",
                  "match": { "title_prefix": ["existing"] },
                  "use_type": "existing",
                  "target": { "library_file": "Reusable.pro" }
                }
              ]
            }
            "#,
        )
        .expect("collision config should parse")
    }

    fn test_text_item(id: &str, position: usize, title: &str, description: Option<&str>) -> Item {
        Item {
            id: id.to_string(),
            position,
            title: title.to_string(),
            description: description.map(str::to_string),
            category: Category::Text,
            note: None,
            song: None,
            scripture: None,
        }
    }

    #[derive(Debug, Deserialize)]
    struct FixtureItem {
        id: String,
        position: usize,
        title: String,
        description: Option<String>,
        category: FixtureCategory,
        note: Option<String>,
        song: Option<FixtureSong>,
        scripture: Option<FixtureScripture>,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "PascalCase")]
    enum FixtureCategory {
        Text,
        Graphic,
        Title,
        Song,
        Other,
    }

    #[derive(Debug, Deserialize)]
    struct FixtureSong {
        title: String,
        author: Option<String>,
        copyright: Option<String>,
        ccli: Option<String>,
        themes: Option<Vec<String>>,
        lyrics: Option<String>,
        arrangement: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    struct FixtureScripture {
        reference: String,
        text: Option<String>,
        translation: Option<String>,
    }

    impl FixtureItem {
        fn into_item(self) -> Item {
            Item {
                id: self.id,
                position: self.position,
                title: self.title,
                description: self.description,
                category: match self.category {
                    FixtureCategory::Text => Category::Text,
                    FixtureCategory::Graphic => Category::Graphic,
                    FixtureCategory::Title => Category::Title,
                    FixtureCategory::Song => Category::Song,
                    FixtureCategory::Other => Category::Other,
                },
                note: self.note,
                song: self.song.map(|song| Song {
                    title: song.title,
                    author: song.author,
                    copyright: song.copyright,
                    ccli: song.ccli,
                    themes: song.themes,
                    lyrics: song.lyrics,
                    arrangement: song.arrangement,
                }),
                scripture: self.scripture.map(|scripture| Scripture {
                    reference: scripture.reference,
                    text: scripture.text,
                    translation: scripture.translation,
                }),
            }
        }
    }

    #[test]
    fn build_preview_uses_fixture_rules_for_library_scripture_and_skip() {
        let config = load_config();
        let items = load_items();
        let (_library_dir, index) = fixture_library();

        let entries = build_preview(&items, &config, Some(&index), Some("Sunday Morning"));

        assert!(validate_project_config(&config).is_empty());
        assert_eq!(entries.len(), 4);

        let call_to_worship = entries
            .iter()
            .find(|entry| entry.pco_title == "Call to Worship")
            .expect("call to worship entry");
        assert!(matches!(call_to_worship.status, PreviewStatus::Edited));
        assert!(call_to_worship
            .file_path
            .as_deref()
            .is_some_and(|path| path.ends_with("Call to Worship.pro")));
        assert_eq!(
            call_to_worship
                .parsed_content
                .as_ref()
                .map(|c| c.segments.len()),
            Some(3)
        );
        assert_eq!(
            call_to_worship.content_slide.as_deref(),
            Some("Call to Worship")
        );

        let song = entries
            .iter()
            .find(|entry| entry.pco_title == "Amazing Grace")
            .expect("song entry");
        assert!(matches!(song.status, PreviewStatus::Used));
        assert!(song
            .file_path
            .as_deref()
            .is_some_and(|path| path.ends_with("Amazing Grace.pro")));
        assert_eq!(song.item_type.as_deref(), Some("song"));
        assert_eq!(song.content_slide, None);

        let scripture = entries
            .iter()
            .find(|entry| entry.pco_title == "Scripture: John 3:16-17 NRSVue")
            .expect("scripture entry");
        assert!(matches!(scripture.status, PreviewStatus::Created));
        assert_eq!(
            scripture.scripture_reference.as_deref(),
            Some("John 3:16-17")
        );
        assert_eq!(scripture.bible_version.as_deref(), Some("NRSVue"));
        assert_eq!(scripture.content_slide.as_deref(), Some("Scripture"));

        let sermon = entries
            .iter()
            .find(|entry| entry.pco_title == "Sermon")
            .expect("sermon entry");
        assert!(matches!(sermon.status, PreviewStatus::Skipped));
        assert_eq!(sermon.reason, "Sermon is added day-of");
    }

    #[test]
    fn output_keys_follow_pco_item_ids_when_plan_positions_change() {
        let config = load_config();
        let items = load_items();
        let (_library_dir, index) = fixture_library();
        let original = build_plan(&items, &config, Some(&index), Some("Sunday Morning"));

        let mut reordered = items;
        reordered.reverse();
        for (index, item) in reordered.iter_mut().enumerate() {
            item.position = index + 1;
        }
        let reordered = build_plan(&reordered, &config, Some(&index), Some("Sunday Morning"));

        let keys_by_title = |plans: &[ResolvedItemPlan]| {
            plans
                .iter()
                .map(|plan| (plan.pco_title.clone(), plan.output_key.clone()))
                .collect::<std::collections::BTreeMap<_, _>>()
        };
        assert_eq!(keys_by_title(&original), keys_by_title(&reordered));
        assert_eq!(
            keys_by_title(&original)
                .get("Call to Worship")
                .map(String::as_str),
            Some("pco:item-1:main")
        );
    }

    #[test]
    fn scripture_plan_rejects_a_partially_valid_reference_list() {
        let config = load_config();
        let item = Item {
            id: "partial-scripture".to_string(),
            position: 1,
            title: "Scripture - Luke 8:26-39; not a reference NRSVue".to_string(),
            description: None,
            category: Category::Text,
            note: None,
            song: None,
            scripture: None,
        };

        let plans = build_plan(&[item], &config, None, None);

        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].action, PlanAction::NeedsReview);
        assert_eq!(
            plans[0].reason,
            "Invalid scripture reference 'not a reference NRSVue'"
        );
    }

    #[test]
    fn partial_verse_reference_routes_to_review_with_a_precise_reason() {
        let mut config = scripture_config();
        config.defaults.bible_version = Some(BibleVersion::NRSVue);
        let item = Item {
            id: "partial-verse".to_string(),
            position: 1,
            title: "Scripture (Robert) - Exodus 16:1-4a".to_string(),
            description: Some("[INSERT TRANSLATION]".to_string()),
            category: Category::Title,
            note: None,
            song: None,
            scripture: None,
        };

        let plans = build_plan(&[item], &config, None, None);

        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].action, PlanAction::NeedsReview);
        assert_eq!(
            plans[0].reason,
            "Partial-verse reference 'Exodus 16:1-4a' cannot be generated from whole-verse Bible data"
        );
    }

    #[test]
    fn scripture_without_a_translation_uses_only_the_configured_default() {
        let mut config = scripture_config();
        let item = Item {
            id: "implicit-scripture-version".to_string(),
            position: 1,
            title: "Scripture - John 3:16".to_string(),
            description: None,
            category: Category::Text,
            note: None,
            song: None,
            scripture: None,
        };

        let unresolved = build_plan(std::slice::from_ref(&item), &config, None, None);
        assert_eq!(unresolved[0].action, PlanAction::NeedsReview);
        assert_eq!(
            unresolved[0].reason,
            "No Bible version was supplied and no project default is configured"
        );

        config.defaults.bible_version = Some(BibleVersion::NIV);
        let resolved = build_plan(&[item], &config, None, None);
        assert_eq!(resolved[0].action, PlanAction::GenerateNew);
        assert!(matches!(
            scripture_request(&resolved[0]),
            ScriptureRequest::Single {
                bible_version: "NIV",
                ..
            }
        ));
    }

    #[test]
    fn scripture_plan_preserves_mixed_explicit_versions() {
        let config = load_config();
        let item = Item {
            id: "mixed-scripture".to_string(),
            position: 1,
            title: "Scripture - Psalm 23:1-6 NIV; John 3:16 NRSVue".to_string(),
            description: None,
            category: Category::Text,
            note: None,
            song: None,
            scripture: None,
        };

        let plans = build_plan(&[item], &config, None, None);

        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].action, PlanAction::GenerateNew);
        let ScriptureRequest::Combined(references) = scripture_request(&plans[0]) else {
            panic!("expected combined scripture content");
        };
        assert_eq!(references.len(), 2);
        assert_eq!(references[0].version, "NIV");
        assert_eq!(references[1].version, "NRSVue");
    }

    #[test]
    fn scripture_plan_prefers_supported_structured_reference_and_translation() {
        let config = scripture_config();
        let item = Item {
            id: "structured-scripture".to_string(),
            position: 1,
            title: "Scripture: Malachi 1:1 NRSVue".to_string(),
            description: None,
            category: Category::Text,
            note: None,
            song: None,
            scripture: Some(Scripture {
                reference: "John 3:16-17".to_string(),
                text: None,
                translation: Some("niv".to_string()),
            }),
        };

        let plans = build_plan(&[item], &config, None, None);

        assert_eq!(plans[0].action, PlanAction::GenerateNew);
        assert!(matches!(
            scripture_request(&plans[0]),
            ScriptureRequest::Single {
                reference: "John 3:16-17",
                bible_version: "NIV"
            }
        ));
    }

    #[test]
    fn scripture_plan_rejects_unsupported_structured_translation() {
        let config = scripture_config();
        let item = Item {
            id: "unsupported-translation".to_string(),
            position: 1,
            title: "Scripture: John 3:16 NRSVue".to_string(),
            description: None,
            category: Category::Text,
            note: None,
            song: None,
            scripture: Some(Scripture {
                reference: "John 3:16".to_string(),
                text: None,
                translation: Some("ESV".to_string()),
            }),
        };

        let plans = build_plan(&[item], &config, None, None);

        assert_eq!(plans[0].action, PlanAction::NeedsReview);
        assert_eq!(plans[0].reason, "Unsupported Bible version 'ESV'");
    }

    #[test]
    fn scripture_plan_falls_back_to_title_without_structured_translation() {
        let config = scripture_config();
        let item = Item {
            id: "title-fallback".to_string(),
            position: 1,
            title: "Scripture: Luke 2:1-3 NRSV".to_string(),
            description: None,
            category: Category::Text,
            note: None,
            song: None,
            scripture: Some(Scripture {
                reference: "John 3:16".to_string(),
                text: None,
                translation: None,
            }),
        };

        let plans = build_plan(&[item], &config, None, None);

        assert_eq!(plans[0].action, PlanAction::GenerateNew);
        assert!(matches!(
            scripture_request(&plans[0]),
            ScriptureRequest::Single {
                reference: "Luke 2:1-3",
                bible_version: "NRSV"
            }
        ));
    }

    #[test]
    fn edit_in_place_without_parsed_content_requires_review() {
        let config = parse_project_config_str(
            r#"
            {
              "version": 4,
              "cue_roles": {
                "content": { "slide": "Content" }
              },
              "presentation_types": {
                "weekly": {
                  "kind": "liturgy",
                  "content_source": "description",
                  "description_parser": "liturgical",
                  "output_strategy": "edit_in_place",
                  "display": { "kind": "single", "role": "content" }
                }
              },
              "item_rules": [{
                "id": "weekly",
                "match": { "title_prefix": ["weekly"] },
                "use_type": "weekly",
                "target": { "library_file": "Weekly.pro" }
              }]
            }
            "#,
        )
        .expect("edit config should parse");
        let library_dir = tempdir().expect("tempdir");
        write_library_presentation(&library_dir.path().join("Weekly.pro"));
        let index = FileIndex::build(library_dir.path()).expect("fixture library should index");
        let item = Item {
            id: "weekly".to_string(),
            position: 1,
            title: "Weekly Text".to_string(),
            description: None,
            category: Category::Text,
            note: None,
            song: None,
            scripture: None,
        };

        let plans = build_plan(&[item], &config, Some(&index), None);

        assert_eq!(plans[0].action, PlanAction::NeedsReview);
        assert_eq!(plans[0].reason, "No description content to edit");
    }

    #[test]
    fn description_placeholders_block_edit_and_generation_plans() {
        let config = parse_project_config_str(
            r#"
            {
              "version": 4,
              "cue_roles": {
                "content": { "slide": "Content" }
              },
              "presentation_types": {
                "weekly_edit": {
                  "kind": "liturgy",
                  "content_source": "description",
                  "description_parser": "liturgical",
                  "output_strategy": "edit_in_place",
                  "display": { "kind": "single", "role": "content" }
                },
                "weekly_generate": {
                  "kind": "liturgy",
                  "content_source": "description",
                  "description_parser": "liturgical",
                  "output_strategy": "generate_new",
                  "display": { "kind": "single", "role": "content" }
                }
              },
              "item_rules": [
                {
                  "id": "weekly_edit",
                  "match": { "title_prefix": ["weekly edit"] },
                  "use_type": "weekly_edit",
                  "target": { "library_file": "Weekly.pro" }
                },
                {
                  "id": "weekly_generate",
                  "match": { "title_prefix": ["weekly generate"] },
                  "use_type": "weekly_generate"
                }
              ]
            }
            "#,
        )
        .expect("placeholder config should parse");
        let library_dir = tempdir().expect("tempdir");
        write_library_presentation(&library_dir.path().join("Weekly.pro"));
        let index = FileIndex::build(library_dir.path()).expect("fixture library should index");
        let description = "[CONFESSION no slide] - introduction\n[SLIDE/ALL] - [insert prayer]\n[SILENT CONFESSION]";
        let items = [
            Item {
                id: "weekly-edit".to_string(),
                position: 1,
                title: "Weekly Edit".to_string(),
                description: Some(description.to_string()),
                category: Category::Text,
                note: None,
                song: None,
                scripture: None,
            },
            Item {
                id: "weekly-generate".to_string(),
                position: 2,
                title: "Weekly Generate".to_string(),
                description: Some(description.to_string()),
                category: Category::Text,
                note: None,
                song: None,
                scripture: None,
            },
        ];

        let plans = build_plan(&items, &config, Some(&index), None);

        assert_eq!(plans.len(), 2);
        for plan in plans {
            assert_eq!(plan.action, PlanAction::NeedsReview);
            assert_eq!(
                plan.reason,
                "Unresolved description placeholder 'insert prayer'"
            );
            assert!(plan.parsed_content().is_none());
        }
    }

    #[test]
    fn preview_selects_colored_macro_only_for_the_colored_content_region() {
        let parsed_content = ParsedContent {
            segments: vec![ParsedSegment {
                text: "Congregational response".to_string(),
                color: Some("#FEDB4F".to_string()),
                bold: None,
                italic: None,
            }],
            title_text: Some("Title".to_string()),
        };
        let split = PreviewEntry::from(ResolvedItemPlan {
            content_source: ContentSource::Description {
                parsed_content: Some(parsed_content.clone()),
            },
            style: PresentationStyle {
                title_slide: Some("Title".to_string()),
                first_cue_macro: Some(CueMacro::new(
                    "Title".to_string(),
                    Some("Title Highlighted".to_string()),
                )),
                first_content_cue_macro: Some(CueMacro::new(
                    "Content".to_string(),
                    Some("Content Highlighted".to_string()),
                )),
                ..PresentationStyle::default()
            },
            ..ResolvedItemPlan::default()
        });
        let single = PreviewEntry::from(ResolvedItemPlan {
            content_source: ContentSource::Description {
                parsed_content: Some(parsed_content),
            },
            style: PresentationStyle {
                first_cue_macro: Some(CueMacro::new(
                    "Content".to_string(),
                    Some("Content Highlighted".to_string()),
                )),
                ..PresentationStyle::default()
            },
            ..ResolvedItemPlan::default()
        });

        assert_eq!(split.first_cue_macro.as_deref(), Some("Title"));
        assert_eq!(
            split.first_content_cue_macro.as_deref(),
            Some("Content Highlighted")
        );
        assert_eq!(
            single.first_cue_macro.as_deref(),
            Some("Content Highlighted")
        );
        assert_eq!(single.first_content_cue_macro, None);
    }

    #[test]
    fn broad_arrangement_override_only_changes_use_existing_style() {
        let config = parse_project_config_str(
            r#"
            {
              "version": 4,
              "cue_roles": { "content": { "slide": "Content" } },
              "presentation_types": {
                "existing": {
                  "kind": "graphic",
                  "content_source": "static",
                  "output_strategy": "use_existing",
                  "arrangement": "Default"
                },
                "rendered": {
                  "kind": "liturgy",
                  "content_source": "description",
                  "description_parser": "liturgical",
                  "output_strategy": "generate_new",
                  "display": { "kind": "single", "role": "content" }
                }
              },
              "overrides": [{
                "when": { "service_type": "Christmas Eve" },
                "arrangement": "Seasonal"
              }]
            }
            "#,
        )
        .expect("arrangement config should parse");

        let existing = resolve_style(
            &config.presentation_types["existing"],
            "existing",
            Some("Christmas Eve"),
            &config,
        );
        let rendered = resolve_style(
            &config.presentation_types["rendered"],
            "rendered",
            Some("Christmas Eve"),
            &config,
        );

        assert_eq!(existing.arrangement.as_deref(), Some("Seasonal"));
        assert_eq!(rendered.arrangement, None);
    }

    #[test]
    fn use_existing_song_uses_the_planning_center_arrangement() {
        let config = song_config(None, None);
        let (_directory, index) = song_index(&[(
            "PCO Verse Order",
            Some("550e8400-e29b-41d4-a716-446655440001"),
        )]);

        let plans = build_plan(
            &[song_item(Some("PCO Verse Order"))],
            &config,
            Some(&index),
            None,
        );

        assert_eq!(plans[0].action, PlanAction::UseExisting);
        assert_eq!(
            plans[0].style.arrangement.as_deref(),
            Some("PCO Verse Order")
        );
    }

    #[test]
    fn planning_center_default_arrangement_aliases_a_unique_native_default() {
        let config = song_config(None, None);
        let (_directory, index) =
            song_index(&[("Default", Some("550e8400-e29b-41d4-a716-446655440001"))]);

        let plans = build_plan(
            &[song_item(Some("Default Arrangement"))],
            &config,
            Some(&index),
            None,
        );

        assert_eq!(plans[0].action, PlanAction::UseExisting);
        assert_eq!(plans[0].style.arrangement.as_deref(), Some("Default"));
    }

    #[test]
    fn planning_center_default_arrangement_selects_none_when_native_has_no_arrangements() {
        let config = song_config(None, None);
        let (_directory, index) = song_index(&[]);

        let plans = build_plan(
            &[song_item(Some("Default Arrangement"))],
            &config,
            Some(&index),
            None,
        );

        assert_eq!(plans[0].action, PlanAction::UseExisting);
        assert_eq!(plans[0].style.arrangement, None);
    }

    #[test]
    fn exact_native_arrangement_precedes_the_default_alias() {
        let config = song_config(None, None);
        let (_directory, index) = song_index(&[
            (
                "Default Arrangement",
                Some("550e8400-e29b-41d4-a716-446655440001"),
            ),
            ("Default", Some("550e8400-e29b-41d4-a716-446655440002")),
        ]);

        let plans = build_plan(
            &[song_item(Some("Default Arrangement"))],
            &config,
            Some(&index),
            None,
        );

        assert_eq!(plans[0].action, PlanAction::UseExisting);
        assert_eq!(
            plans[0].style.arrangement.as_deref(),
            Some("Default Arrangement")
        );
    }

    #[test]
    fn default_arrangement_alias_requires_one_complete_native_default() {
        let config = song_config(None, None);
        let (_ambiguous_directory, ambiguous_index) = song_index(&[
            ("Default", Some("550e8400-e29b-41d4-a716-446655440001")),
            ("default", Some("550e8400-e29b-41d4-a716-446655440002")),
        ]);

        let ambiguous = build_plan(
            &[song_item(Some("Default Arrangement"))],
            &config,
            Some(&ambiguous_index),
            None,
        );

        assert_eq!(ambiguous[0].action, PlanAction::NeedsReview);
        assert!(ambiguous[0].reason.contains("is ambiguous"));

        let (_incomplete_directory, incomplete_index) = song_index(&[("Default", None)]);
        let incomplete = build_plan(
            &[song_item(Some("Default Arrangement"))],
            &config,
            Some(&incomplete_index),
            None,
        );

        assert_eq!(incomplete[0].action, PlanAction::NeedsReview);
        assert!(incomplete[0]
            .reason
            .contains("has a missing or invalid UUID"));

        let (_empty_record_directory, empty_record_index) = song_index(&[("", None)]);
        let empty_record = build_plan(
            &[song_item(Some("Default Arrangement"))],
            &config,
            Some(&empty_record_index),
            None,
        );

        assert_eq!(empty_record[0].action, PlanAction::NeedsReview);
        assert!(empty_record[0].reason.contains("is unavailable"));
    }

    #[test]
    fn default_arrangement_alias_does_not_generalize_other_labels() {
        let config = song_config(None, None);
        let (_directory, index) =
            song_index(&[("Default", Some("550e8400-e29b-41d4-a716-446655440001"))]);

        let plans = build_plan(
            &[song_item(Some("default arrangement"))],
            &config,
            Some(&index),
            None,
        );

        assert_eq!(plans[0].action, PlanAction::NeedsReview);
        assert!(plans[0].reason.contains("is unavailable"));
    }

    #[test]
    fn configured_and_service_override_arrangements_precede_planning_center() {
        let config = song_config(Some("Configured Order"), Some("Christmas Order"));
        let (_directory, index) = song_index(&[
            (
                "Configured Order",
                Some("550e8400-e29b-41d4-a716-446655440001"),
            ),
            (
                "Christmas Order",
                Some("550e8400-e29b-41d4-a716-446655440002"),
            ),
            ("PCO Order", Some("550e8400-e29b-41d4-a716-446655440003")),
        ]);
        let item = song_item(Some("PCO Order"));

        let ordinary = build_plan(std::slice::from_ref(&item), &config, Some(&index), None);
        let christmas = build_plan(&[item], &config, Some(&index), Some("Christmas Eve"));

        assert_eq!(ordinary[0].action, PlanAction::UseExisting);
        assert_eq!(
            ordinary[0].style.arrangement.as_deref(),
            Some("Configured Order")
        );
        assert_eq!(christmas[0].action, PlanAction::UseExisting);
        assert_eq!(
            christmas[0].style.arrangement.as_deref(),
            Some("Christmas Order")
        );
    }

    #[test]
    fn use_existing_song_without_an_arrangement_selects_none() {
        let config = song_config(None, None);
        let (_directory, index) = song_index(&[]);

        let plans = build_plan(&[song_item(None)], &config, Some(&index), None);

        assert_eq!(plans[0].action, PlanAction::UseExisting);
        assert_eq!(plans[0].style.arrangement, None);
    }

    #[test]
    fn existing_song_uses_the_canonical_library_name() {
        let directory = tempdir().expect("fixture library directory");
        let path = directory
            .path()
            .join("[Hymn] Come, Thou Fount of Every Blessing.pro");
        write_song_with_arrangements(&path, &[]);
        let index = FileIndex::build(directory.path()).expect("fixture library should index");
        let config = parse_project_config_str(
            r#"
            {
              "version": 4,
              "cue_roles": {
                "content": { "slide": "Content" }
              },
              "presentation_types": {
                "song": {
                  "kind": "song",
                  "content_source": "song",
                  "output_strategy": "use_existing"
                }
              },
              "item_rules": [{
                "id": "come_thou_fount",
                "match": { "category": "song" },
                "use_type": "song",
                "target": {
                  "library_file": "[Hymn] Come, Thou Fount of Every Blessing.pro"
                }
              }]
            }
            "#,
        )
        .expect("song config should parse");
        let mut item = song_item(Some("Default Arrangement"));
        item.title = "#356 Come, Thou Fount of Every Blessing".to_string();
        if let Some(song) = &mut item.song {
            song.title.clone_from(&item.title);
        }

        let plans = build_plan(&[item], &config, Some(&index), None);

        assert_eq!(plans[0].action, PlanAction::UseExisting);
        assert_eq!(
            plans[0].playlist_name,
            "[Hymn] Come, Thou Fount of Every Blessing"
        );
        assert_eq!(plans[0].style.arrangement, None);
    }

    #[test]
    fn static_scripture_type_reuses_an_existing_presentation() {
        let directory = tempdir().expect("fixture library directory");
        let path = directory.path().join("Jonah 4.pro");
        write_library_presentation(&path);
        let index = FileIndex::build(directory.path()).expect("fixture library should index");
        let config = parse_project_config_str(
            r#"
            {
              "version": 4,
              "presentation_types": {
                "scripture_existing": {
                  "kind": "scripture",
                  "content_source": "static",
                  "output_strategy": "use_existing"
                }
              },
              "item_rules": [{
                "id": "jonah_4",
                "match": { "title_contains": ["jonah 4"] },
                "use_type": "scripture_existing",
                "target": { "library_file": "Jonah 4.pro" }
              }]
            }
            "#,
        )
        .expect("existing scripture config should parse");
        let item = Item {
            id: "jonah".to_string(),
            position: 1,
            title: "Scripture: Jonah 4".to_string(),
            description: None,
            category: Category::Title,
            note: None,
            song: None,
            scripture: None,
        };

        let plans = build_plan(&[item], &config, Some(&index), None);

        assert_eq!(plans[0].action, PlanAction::UseExisting);
        assert_eq!(plans[0].item_kind, ItemKind::Scripture);
        assert!(matches!(plans[0].content_source, ContentSource::None));
        assert_eq!(plans[0].playlist_name, "Jonah 4");
        assert_eq!(plans[0].file_path.as_deref(), path.to_str());
    }

    #[test]
    fn unavailable_ambiguous_and_incomplete_arrangements_require_review() {
        let config = song_config(None, None);

        let (_missing_directory, missing_index) = song_index(&[
            ("Default", Some("550e8400-e29b-41d4-a716-446655440001")),
            ("Seasonal", Some("550e8400-e29b-41d4-a716-446655440002")),
        ]);
        let missing = build_plan(
            &[song_item(Some("Missing"))],
            &config,
            Some(&missing_index),
            None,
        );
        assert_eq!(missing[0].action, PlanAction::NeedsReview);
        assert!(missing[0].reason.contains("is unavailable"));
        assert!(missing[0].reason.contains("Default, Seasonal"));

        let (_ambiguous_directory, ambiguous_index) = song_index(&[
            ("Default", Some("550e8400-e29b-41d4-a716-446655440001")),
            ("default", Some("550e8400-e29b-41d4-a716-446655440002")),
        ]);
        let ambiguous = build_plan(
            &[song_item(Some("DEFAULT"))],
            &config,
            Some(&ambiguous_index),
            None,
        );
        assert_eq!(ambiguous[0].action, PlanAction::NeedsReview);
        assert!(ambiguous[0].reason.contains("is ambiguous"));
        assert!(ambiguous[0].reason.contains("Default, default"));

        let (_incomplete_directory, incomplete_index) = song_index(&[("Broken", None)]);
        let incomplete = build_plan(
            &[song_item(Some("Broken"))],
            &config,
            Some(&incomplete_index),
            None,
        );
        assert_eq!(incomplete[0].action, PlanAction::NeedsReview);
        assert!(incomplete[0]
            .reason
            .contains("has a missing or invalid UUID"));
        assert!(incomplete[0]
            .reason
            .contains("available arrangements: Broken"));
    }

    #[test]
    fn requested_arrangement_uses_the_canonical_native_casing() {
        let config = song_config(None, None);
        let (_directory, index) =
            song_index(&[("Verse Order", Some("550e8400-e29b-41d4-a716-446655440001"))]);

        let plans = build_plan(
            &[song_item(Some("verse order"))],
            &config,
            Some(&index),
            None,
        );

        assert_eq!(plans[0].action, PlanAction::UseExisting);
        assert_eq!(plans[0].style.arrangement.as_deref(), Some("Verse Order"));
    }

    #[test]
    fn ordered_skip_rule_precedes_a_broader_fallback() {
        let config = parse_project_config_str(
            r#"
            {
              "version": 4,
              "presentation_types": {
                "song": {
                  "kind": "song",
                  "content_source": "song",
                  "output_strategy": "use_existing"
                }
              },
              "item_rules": [
                {
                  "id": "sermon_manual_only",
                  "match": { "title_prefix": ["sermon"] },
                  "action": {
                    "kind": "skip",
                    "reason": "Sermon slides are added manually after ProFlow builds"
                  }
                },
                {
                  "id": "wrong_sermon_fallback",
                  "match": { "title_prefix": ["sermon"] },
                  "use_type": "song"
                }
              ]
            }
            "#,
        )
        .expect("config should parse");
        let item = Item {
            id: "1".to_string(),
            position: 1,
            title: "Sermon".to_string(),
            description: None,
            category: Category::Text,
            note: None,
            song: None,
            scripture: None,
        };

        let entries = build_preview(&[item], &config, None, Some("Sunday Morning"));

        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0].status, PreviewStatus::Skipped));
        assert_eq!(
            entries[0].reason,
            "Sermon slides are added manually after ProFlow builds"
        );
        assert!(entries[0].item_type.is_none());
    }

    #[test]
    fn description_generate_new_uses_strategy_not_edited_fallback() {
        let config = parse_project_config_str(
            r#"
            {
              "version": 4,
              "cue_roles": {
                "information": {
                  "slide": "Information (Projectors)"
                }
              },
              "presentation_types": {
                "liturgical_edited": {
                  "kind": "liturgy",
                  "content_source": "description",
                  "description_parser": "liturgical",
                  "output_strategy": "generate_new",
                  "display": { "kind": "single", "role": "information" }
                }
              },
              "item_rules": [
                {
                  "id": "weekly_text",
                  "match": { "title_prefix": ["weekly text"] },
                  "use_type": "liturgical_edited"
                }
              ]
            }
            "#,
        )
        .expect("config should parse");

        let items = vec![Item {
            id: "1".to_string(),
            position: 1,
            title: "Weekly Text".to_string(),
            description: Some("Leader: Grace and peace".to_string()),
            category: Category::Text,
            note: None,
            song: None,
            scripture: None,
        }];

        let plans = build_plan(&items, &config, None, None);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].action, PlanAction::GenerateNew);
        assert!(matches!(
            plans[0].content_source,
            ContentSource::Description {
                parsed_content: Some(_)
            }
        ));
    }

    #[test]
    fn use_existing_missing_target_requires_review_instead_of_skip() {
        let config = parse_project_config_str(
            r#"
            {
              "version": 4,
              "presentation_types": {
                "static_slide": {
                  "kind": "graphic",
                  "content_source": "static",
                  "output_strategy": "use_existing"
                }
              },
              "item_rules": [
                {
                  "id": "static_slide",
                  "match": { "title_prefix": ["welcome bumper"] },
                  "use_type": "static_slide",
                  "target": { "library_file": "Missing Slide.pro" }
                }
              ]
            }
            "#,
        )
        .expect("config should parse");

        let items = vec![Item {
            id: "1".to_string(),
            position: 1,
            title: "Welcome Bumper".to_string(),
            description: None,
            category: Category::Graphic,
            note: None,
            song: None,
            scripture: None,
        }];

        let plans = build_plan(&items, &config, None, None);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].action, PlanAction::NeedsReview);
        assert_eq!(
            plans[0].reason,
            "Configured existing file not found: Missing Slide.pro"
        );
    }

    #[test]
    fn explicit_generic_and_song_targets_never_use_fuzzy_matches() {
        let config = explicit_library_target_config();
        let library_dir = tempdir().expect("tempdir");
        write_library_presentation(&library_dir.path().join("Weekly Announcements.pro"));
        write_library_presentation(&library_dir.path().join("[Hymn] Amazing Grace.pro"));
        let index = FileIndex::build(library_dir.path()).expect("fixture library should index");

        let plans = build_plan(
            &explicit_library_target_items(),
            &config,
            Some(&index),
            None,
        );

        assert_eq!(plans[0].action, PlanAction::NeedsReview);
        assert_eq!(
            plans[0].reason,
            "Configured existing file not found: Announcements.pro"
        );
        assert_eq!(plans[0].file_path, None);
        assert_eq!(plans[1].action, PlanAction::NeedsReview);
        assert_eq!(
            plans[1].reason,
            "Configured existing song not found: Amazing Grace.pro"
        );
        assert_eq!(plans[1].file_path, None);
    }

    #[test]
    fn explicit_generic_and_song_targets_reject_duplicate_filenames() {
        let config = explicit_library_target_config();
        let library_dir = tempdir().expect("tempdir");
        let nested = library_dir.path().join("nested");
        std::fs::create_dir(&nested).expect("nested fixture directory");
        for (root_name, nested_name) in [
            ("Announcements.pro", "announcements.pro"),
            ("Amazing Grace.pro", "AMAZING GRACE.pro"),
        ] {
            write_library_presentation(&library_dir.path().join(root_name));
            write_library_presentation(&nested.join(nested_name));
        }
        let index = FileIndex::build(library_dir.path()).expect("fixture library should index");

        let plans = build_plan(
            &explicit_library_target_items(),
            &config,
            Some(&index),
            None,
        );

        assert_eq!(plans[0].action, PlanAction::NeedsReview);
        assert_eq!(
            plans[0].reason,
            "Configured existing file is ambiguous: Announcements.pro"
        );
        assert_eq!(plans[0].file_path, None);
        assert_eq!(plans[1].action, PlanAction::NeedsReview);
        assert_eq!(
            plans[1].reason,
            "Configured existing song target is ambiguous: Amazing Grace.pro"
        );
        assert_eq!(plans[1].file_path, None);
    }

    #[test]
    fn v4_contextual_baptism_decision_selects_allowed_file() {
        let config = parse_project_config_str(
            r#"
            {
              "version": 4,
              "presentation_types": {
                "liturgical_static": {
                  "kind": "liturgy",
                  "content_source": "static",
                  "output_strategy": "use_existing"
                }
              },
              "item_rules": [
                {
                  "id": "baptism_contextual",
                  "match": { "title_prefix": ["baptism"] },
                  "decision": {
                    "kind": "choose_existing_file",
                    "context_fields": ["title", "description"],
                    "instructions": "Use Him for a boy, Her for a girl, Them for multiple candidates.",
                    "on_ambiguous": "ask",
                    "choices": {
                      "him": {
                        "use_type": "liturgical_static",
                        "file": "Baptism Him.pro",
                        "match": { "any": ["son of"] }
                      },
                      "her": {
                        "use_type": "liturgical_static",
                        "file": "Baptism Her.pro",
                        "match": { "any": ["daughter of"] }
                      },
                      "them": {
                        "use_type": "liturgical_static",
                        "file": "Baptism Them.pro",
                        "match": { "any": ["children of"] }
                      }
                    }
                  }
                }
              ]
            }
            "#,
        )
        .expect("config should parse");

        let library_dir = tempdir().expect("tempdir");
        write_library_presentation(&library_dir.path().join("Baptism Him.pro"));
        let index = FileIndex::build(library_dir.path()).expect("fixture library should index");

        let plans = build_plan(
            &[Item {
                id: "1".to_string(),
                position: 1,
                title: "Baptism".to_string(),
                description: Some("James, son of Jane and John".to_string()),
                category: Category::Text,
                note: None,
                song: None,
                scripture: None,
            }],
            &config,
            Some(&index),
            None,
        );

        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].action, PlanAction::UseExisting);
        assert_eq!(plans[0].playlist_name, "Baptism Him");
        assert!(plans[0]
            .file_path
            .as_deref()
            .is_some_and(|path| path.ends_with("Baptism Him.pro")));
        assert_eq!(plans[0].style.content_slide, None);
        assert_eq!(plans[0].style.first_cue_macro, None);
    }

    #[test]
    fn contextual_choice_supports_all_only_and_none_only_matchers() {
        let all_only = DecisionChoiceConfig {
            match_spec: crate::project_config::DecisionChoiceMatch {
                all: vec!["child".to_string(), "baptism".to_string()],
                ..Default::default()
            },
            ..Default::default()
        };
        let none_only = DecisionChoiceConfig {
            match_spec: crate::project_config::DecisionChoiceMatch {
                none: vec!["private".to_string()],
                ..Default::default()
            },
            ..Default::default()
        };

        assert!(decision_choice_matches(
            &all_only,
            "child baptism during worship"
        ));
        assert!(!decision_choice_matches(&all_only, "child dedication"));
        assert!(decision_choice_matches(&none_only, "public baptism"));
        assert!(!decision_choice_matches(&none_only, "private baptism"));
    }

    #[test]
    fn v4_contextual_decision_requires_review_when_ambiguous() {
        let config = parse_project_config_str(
            r#"
            {
              "version": 4,
              "presentation_types": {
                "liturgical_static": {
                  "kind": "liturgy",
                  "content_source": "static",
                  "output_strategy": "use_existing"
                }
              },
              "item_rules": [
                {
                  "id": "baptism_contextual",
                  "match": { "title_prefix": ["baptism"] },
                  "decision": {
                    "kind": "choose_existing_file",
                    "instructions": "Ask if unclear.",
                    "choices": {
                      "him": {
                        "use_type": "liturgical_static",
                        "file": "Baptism Him.pro",
                        "match": { "any": ["son of"] }
                      },
                      "her": {
                        "use_type": "liturgical_static",
                        "file": "Baptism Her.pro",
                        "match": { "any": ["daughter of"] }
                      }
                    }
                  }
                }
              ]
            }
            "#,
        )
        .expect("config should parse");

        let plans = build_plan(
            &[Item {
                id: "1".to_string(),
                position: 1,
                title: "Baptism".to_string(),
                description: Some("Baptism during worship".to_string()),
                category: Category::Text,
                note: None,
                song: None,
                scripture: None,
            }],
            &config,
            None,
            None,
        );

        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].action, PlanAction::NeedsReview);
        assert!(plans[0].reason.contains("no choice matched"));
        assert!(plans[0].reason.contains("Ask if unclear."));
    }

    #[test]
    fn v4_split_display_resolves_cue_roles() {
        let config = parse_project_config_str(
            r#"
            {
              "version": 4,
              "cue_roles": {
                "title": {
                  "slide": "Information (Projectors)",
                  "enter_macro": "Name Tag/Title"
                },
                "scripture_prayer": {
                  "slide": "Scripture (Projectors)",
                  "enter_macro": "Scripture/Prayer"
                }
              },
              "presentation_types": {
                "scripture": {
                  "kind": "scripture",
                  "content_source": "scripture",
                  "output_strategy": "generate_new",
                  "display": {
                    "kind": "split",
                    "title": "title",
                    "content": "scripture_prayer"
                  }
                }
              },
              "item_rules": [
                {
                  "id": "scripture",
                  "match": {
                    "title_prefix": ["scripture"],
                    "has_scripture_ref": true
                  },
                  "use_type": "scripture"
                }
              ]
            }
            "#,
        )
        .expect("config should parse");

        let plans = build_plan(
            &[Item {
                id: "1".to_string(),
                position: 1,
                title: "Scripture: John 3:16 NRSVue".to_string(),
                description: None,
                category: Category::Text,
                note: None,
                song: None,
                scripture: None,
            }],
            &config,
            None,
            None,
        );

        assert_eq!(plans.len(), 1);
        assert_eq!(
            plans[0].style.title_slide.as_deref(),
            Some("Information (Projectors)")
        );
        assert_eq!(
            plans[0].style.content_slide.as_deref(),
            Some("Scripture (Projectors)")
        );
        assert_eq!(
            plans[0].style.first_cue_macro.as_ref().map(CueMacro::enter),
            Some("Name Tag/Title")
        );
        assert_eq!(
            plans[0]
                .style
                .first_content_cue_macro
                .as_ref()
                .map(CueMacro::enter),
            Some("Scripture/Prayer")
        );
    }

    #[test]
    fn expansion_outputs_have_stable_keys_and_respect_declared_type() {
        let config = parse_project_config_str(
            r#"
            {
              "version": 4,
              "cue_roles": {
                "content": { "slide": "Content" }
              },
              "presentation_types": {
                "person_nametag": {
                  "kind": "nametag",
                  "content_source": "static",
                  "output_strategy": "use_existing"
                },
                "liturgical_edited": {
                  "kind": "liturgy",
                  "content_source": "description",
                  "description_parser": "liturgical",
                  "output_strategy": "edit_in_place",
                  "display": { "kind": "single", "role": "content" }
                }
              },
              "people": {
                "Hope": {
                  "nametag": "Hope Nametag"
                }
              },
              "item_rules": [
                {
                  "id": "call_to_worship_bundle",
                  "match": { "title_prefix": ["call to worship"] },
                  "expand": [
                    { "use_type": "person_nametag", "speaker": "resolved" },
                    {
                      "use_type": "liturgical_edited",
                      "speaker": "resolved",
                      "target": { "library_file": "Call to Worship.pro" }
                    }
                  ]
                }
              ]
            }
            "#,
        )
        .expect("config should parse");

        let library_dir = tempdir().expect("tempdir");
        write_library_presentation(&library_dir.path().join("Hope Nametag.pro"));
        write_library_presentation(&library_dir.path().join("Call to Worship.pro"));
        let index = FileIndex::build(library_dir.path()).expect("fixture library should index");

        let items = vec![Item {
            id: "1".to_string(),
            position: 1,
            title: "Call to Worship (Hope)".to_string(),
            description: Some("Leader: Grace and peace".to_string()),
            category: Category::Text,
            note: None,
            song: None,
            scripture: None,
        }];

        let plans = build_plan(&items, &config, Some(&index), None);
        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].output_key, "pco:1:expand:0:person_nametag");
        assert_eq!(plans[0].item_type.as_deref(), Some("person_nametag"));
        assert_eq!(plans[0].action, PlanAction::UseExisting);
        assert!(plans[0]
            .file_path
            .as_deref()
            .is_some_and(|path| path.ends_with("Hope Nametag.pro")));

        assert_eq!(plans[1].output_key, "pco:1:expand:1:liturgical_edited");
        assert_eq!(plans[1].item_type.as_deref(), Some("liturgical_edited"));
        assert_eq!(plans[1].action, PlanAction::EditInPlace);
        assert!(plans[1]
            .file_path
            .as_deref()
            .is_some_and(|path| path.ends_with("Call to Worship.pro")));
    }

    #[test]
    fn speaker_resolution_requires_a_configured_person() {
        let config = parse_project_config_str(
            r#"
            {
              "version": 4,
              "people": {
                "Hope": { "last": "Lee", "nametag": "Hope Nametag" }
              }
            }
            "#,
        )
        .expect("config should parse");

        assert_eq!(
            resolve_speaker("Call to Worship (Hope)", None, &config).as_deref(),
            Some("Hope")
        );
        assert_eq!(
            resolve_speaker("Call to Worship (Jordan)", None, &config),
            None
        );
        assert_eq!(
            resolve_speaker("Call to Worship", Some("Liturgist: Jordan"), &config),
            None
        );
        assert_eq!(
            resolve_speaker("Call to Worship", Some("Liturgist: Hope Lee"), &config).as_deref(),
            Some("Hope Lee")
        );
        assert_eq!(
            resolve_speaker("Call to Worship (Hope & Robert)", None, &config),
            None
        );
        assert_eq!(
            resolve_speaker("Call to Worship", Some("Liturgist: Hope leads"), &config),
            None
        );
    }

    #[test]
    fn expanded_outputs_preserve_distinct_keys_even_when_file_matches_repeat() {
        let config = parse_project_config_str(
            r#"
            {
              "version": 4,
              "presentation_types": {
                "static_slide": {
                  "kind": "graphic",
                  "content_source": "static",
                  "output_strategy": "use_existing"
                }
              },
              "item_rules": [
                {
                  "id": "bundle",
                  "match": { "title_prefix": ["welcome"] },
                  "expand": [
                    {
                      "use_type": "static_slide",
                      "target": { "library_file": "Announcements.pro" }
                    },
                    {
                      "use_type": "static_slide",
                      "target": { "library_file": "Announcements.pro" }
                    }
                  ]
                }
              ]
            }
            "#,
        )
        .expect("config should parse");

        let library_dir = tempdir().expect("tempdir");
        write_library_presentation(&library_dir.path().join("Announcements.pro"));
        let index = FileIndex::build(library_dir.path()).expect("fixture library should index");

        let items = vec![Item {
            id: "1".to_string(),
            position: 1,
            title: "Welcome".to_string(),
            description: None,
            category: Category::Graphic,
            note: None,
            song: None,
            scripture: None,
        }];

        let plans = build_plan(&items, &config, Some(&index), None);
        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].output_key, "pco:1:expand:0:static_slide");
        assert_eq!(plans[1].output_key, "pco:1:expand:1:static_slide");
        assert_eq!(plans[0].file_path, plans[1].file_path);
    }

    #[test]
    fn required_playlist_items_insert_at_edges_without_duplicating_pco_matches() {
        let config = parse_project_config_str(
            r#"
            {
              "version": 4,
              "defaults": {
                "presentation_size": { "width": 1920, "height": 1080 }
              },
              "service_groups": {
                "weekly": { "service_types": ["Sunday Morning"] }
              },
              "presentation_types": {
                "static_graphic": {
                  "kind": "graphic",
                  "content_source": "static",
                  "output_strategy": "use_existing"
                }
              },
              "required_playlist_items": [
                {
                  "id": "pre_service",
                  "use_type": "static_graphic",
                  "library_file": "Pre-Service.pro",
                  "placement": "start",
                  "service_group": "weekly"
                },
                {
                  "id": "closing",
                  "use_type": "static_graphic",
                  "library_file": "Closing.pro",
                  "placement": "end",
                  "service_group": "weekly"
                }
              ],
              "item_rules": [{
                "id": "pre_service",
                "match": { "title_prefix": ["pre-service"] },
                "use_type": "static_graphic",
                "target": { "library_file": "Pre-Service.pro" }
              }]
            }
            "#,
        )
        .expect("required playlist config should parse");
        let library = tempdir().expect("temporary library");
        for name in ["Pre-Service.pro", "Closing.pro"] {
            write_library_presentation_with_size(&library.path().join(name), 1920.0, 1080.0);
        }
        let index = FileIndex::build(library.path()).expect("fixture library should index");
        let items = vec![
            test_text_item("ordinary-before", 5, "Ordinary Before", None),
            test_text_item("pre", 10, "Pre-Service Slides", None),
            test_text_item("ordinary-after", 15, "Ordinary After", None),
        ];

        let plans = build_plan(&items, &config, Some(&index), Some("Sunday Morning"));

        assert_eq!(
            plans.len(),
            4,
            "the PCO pre-service item must be replaced by one required item"
        );
        assert_eq!(plans[0].output_key, "required:pre_service");
        assert_eq!(plans[1].output_key, "pco:ordinary-before:main");
        assert_eq!(plans[2].output_key, "pco:ordinary-after:main");
        assert_eq!(plans[3].output_key, "required:closing");
        assert!(!plans.iter().any(|plan| plan.output_key == "pco:pre:main"));
        assert_eq!(plans[3].action, PlanAction::UseExisting);
        assert!(plans[3]
            .file_path
            .as_deref()
            .is_some_and(|path| path.ends_with("Closing.pro")));

        let inserted = build_plan(&[], &config, Some(&index), Some("Sunday Morning"));
        assert_eq!(
            inserted
                .iter()
                .map(|plan| plan.output_key.as_str())
                .collect::<Vec<_>>(),
            vec!["required:pre_service", "required:closing"]
        );

        let other_service = build_plan(&items, &config, Some(&index), Some("Christmas Eve"));
        assert_eq!(other_service.len(), 3);
        assert_eq!(other_service[1].output_key, "pco:pre:main");
    }

    #[test]
    fn duplicate_edit_in_place_targets_require_review_during_classification() {
        let config = mutable_target_collision_config();
        let library = tempdir().expect("temporary library");
        write_library_presentation(&library.path().join("Weekly Slot.pro"));
        let index = FileIndex::build(library.path()).expect("fixture library should index");
        let items = vec![
            test_text_item("first", 1, "Edited First", Some("Leader: First text")),
            test_text_item("second", 2, "Edited Second", Some("Leader: Second text")),
        ];

        let plans = build_plan(&items, &config, Some(&index), None);

        assert_eq!(plans.len(), 2);
        assert!(plans
            .iter()
            .all(|plan| plan.action == PlanAction::NeedsReview));
        assert_eq!(plans[0].reason, plans[1].reason);
        assert!(plans[0].reason.contains("edit-in-place file"));
        assert!(plans[0].reason.contains("Weekly Slot.pro"));
        assert!(plans[0].reason.contains("pco:first:main"));
        assert!(plans[0].reason.contains("pco:second:main"));
    }

    #[test]
    fn canonical_generated_filename_collisions_require_review_in_preview() {
        let config = mutable_target_collision_config();
        let items = vec![
            test_text_item(
                "first",
                1,
                "Generated: Weekly (Hope)",
                Some("Leader: First text"),
            ),
            test_text_item(
                "second",
                2,
                "Generated - Weekly (Robert)",
                Some("Leader: Second text"),
            ),
        ];

        let plans = build_plan(&items, &config, None, None);
        let preview = render_preview(&plans);

        assert!(plans
            .iter()
            .all(|plan| plan.action == PlanAction::NeedsReview));
        assert!(preview
            .iter()
            .all(|entry| matches!(entry.status, PreviewStatus::Uncertain)));
        assert_eq!(plans[0].reason, plans[1].reason);
        assert!(plans[0].reason.contains("generated file"));
        assert!(plans[0].reason.contains("Generated - Weekly.pro"));
        assert!(plans[0].reason.contains("pco:first:main"));
        assert!(plans[0].reason.contains("pco:second:main"));
    }

    #[test]
    fn repeated_use_existing_targets_remain_valid_playlist_references() {
        let config = mutable_target_collision_config();
        let library = tempdir().expect("temporary library");
        write_library_presentation(&library.path().join("Reusable.pro"));
        let index = FileIndex::build(library.path()).expect("fixture library should index");
        let items = vec![
            test_text_item("first", 1, "Existing First", None),
            test_text_item("second", 2, "Existing Second", None),
        ];

        let plans = build_plan(&items, &config, Some(&index), None);

        assert_eq!(plans.len(), 2);
        assert!(plans
            .iter()
            .all(|plan| plan.action == PlanAction::UseExisting));
        assert_eq!(plans[0].file_path, plans[1].file_path);
    }

    #[test]
    fn selected_existing_presentation_with_wrong_size_requires_review() {
        let config = parse_project_config_str(
            r#"
            {
              "version": 4,
              "defaults": {
                "presentation_size": { "width": 1920, "height": 1080 }
              },
              "presentation_types": {
                "static_graphic": {
                  "kind": "graphic",
                  "content_source": "static",
                  "output_strategy": "use_existing"
                }
              },
              "item_rules": [{
                "id": "graphic",
                "match": { "title_prefix": ["graphic"] },
                "use_type": "static_graphic",
                "target": { "library_file": "Legacy.pro" }
              }]
            }
            "#,
        )
        .expect("size-audited config should parse");
        let library = tempdir().expect("temporary library");
        write_library_presentation_with_size(&library.path().join("Legacy.pro"), 1280.0, 720.0);
        let index = FileIndex::build(library.path()).expect("fixture library should index");
        let items = vec![Item {
            id: "legacy".to_string(),
            position: 1,
            title: "Graphic".to_string(),
            description: None,
            category: Category::Graphic,
            note: None,
            song: None,
            scripture: None,
        }];

        let plans = build_plan(&items, &config, Some(&index), None);

        assert_eq!(plans[0].action, PlanAction::NeedsReview);
        assert!(plans[0].reason.contains("1280x720"));
        assert!(plans[0].reason.contains("1920x1080"));
        assert!(plans[0].reason.contains("then reapply the theme"));
    }
}
