//! Service plan preview — analyzes a PCO plan and proposes playlist entries.
//!
//! Uses a declarative type system from `data/proflow.config.json` to classify
//! each PCO item, resolve library files, and produce a structured preview.

use std::collections::HashSet;

use serde::Serialize;

use super::description_parser::{self, ParsedContent};
use super::library_search::{
    resolve_song_library_match, search_index, search_index_strict, strip_hymn_number,
    SongLibraryMatch,
};
use super::plan::{
    ContentSource, ItemKind, PlanAction, PresentationStyle, ResolvedItemPlan, ScriptureContent,
    ScriptureRefInfo,
};
use super::scripture::{detect_version, has_scripture_ref, scripture_name, split_scripture_refs};
use crate::planning_center::types::Item;
use crate::project_config::{
    ContentSourceKind, ItemRuleConfig, OutputStrategy, OverrideRuleConfig, PresentationTypeConfig,
    ProjectConfig, RuleAction,
};
use crate::utils::file_index::FileIndex;

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
    pub background: Option<String>,
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
    /// Theme slide name to use for generation (from `presentation_types.template`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template_name: Option<String>,
    /// `ProPresenter` macro to trigger on the first slide.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub macro_name: Option<String>,
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

impl From<PreviewStatus> for PlanAction {
    fn from(status: PreviewStatus) -> Self {
        match status {
            PreviewStatus::Used => Self::UseExisting,
            PreviewStatus::Edited => Self::EditInPlace,
            PreviewStatus::Created => Self::GenerateNew,
            PreviewStatus::Skipped => Self::Skip,
            PreviewStatus::Uncertain => Self::NeedsReview,
        }
    }
}

impl From<ResolvedItemPlan> for PreviewEntry {
    fn from(plan: ResolvedItemPlan) -> Self {
        let (parsed_content, scripture_reference, bible_version, scripture_refs) =
            match plan.content_source {
                ContentSource::None => (None, None, None, None),
                ContentSource::Description { parsed_content } => (parsed_content, None, None, None),
                ContentSource::Scripture { scripture } => (
                    None,
                    scripture.reference,
                    scripture.bible_version,
                    (!scripture.references.is_empty()).then_some(scripture.references),
                ),
            };

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
            background: plan.style.background,
            arrangement: plan.style.arrangement,
            scripture_reference,
            bible_version,
            scripture_refs,
            template_name: plan.style.template_name,
            macro_name: plan.style.macro_name,
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
    build_resolved_plans(items, mappings, file_index, service_name)
}

/// Render typed plans back into preview rows for MCP output.
pub fn render_preview(plans: &[ResolvedItemPlan]) -> Vec<PreviewEntry> {
    plans.iter().cloned().map(PreviewEntry::from).collect()
}

/// Build a preview of the proposed playlist for a set of PCO items.
#[allow(clippy::too_many_lines)]
pub fn build_preview(
    items: &[Item],
    mappings: &ProjectConfig,
    file_index: Option<&FileIndex>,
    service_name: Option<&str>,
) -> Vec<PreviewEntry> {
    render_preview(&build_plan(items, mappings, file_index, service_name))
}

#[allow(clippy::too_many_lines)]
fn build_resolved_plans(
    items: &[Item],
    mappings: &ProjectConfig,
    file_index: Option<&FileIndex>,
    service_name: Option<&str>,
) -> Vec<ResolvedItemPlan> {
    let mut entries = Vec::new();
    let mut nametag_seen: HashSet<String> = HashSet::new();

    for item in items {
        let title_lower = item.title.to_lowercase();
        let speaker = resolve_speaker(&item.title, item.description.as_deref(), mappings);
        let Some(rule) = find_matching_rule(item, &title_lower, mappings, service_name) else {
            entries.push(ResolvedItemPlan {
                output_key: ResolvedItemPlan::primary_output_key(item.position),
                position: item.position,
                pco_title: item.title.clone(),
                playlist_name: strip_speaker(&item.title),
                action: PlanAction::NeedsReview,
                reason: "No matching item rule".to_string(),
                ..Default::default()
            });
            continue;
        };

        if !rule.expand.is_empty() {
            process_expansion(
                rule,
                item,
                speaker.as_deref(),
                mappings,
                &mut entries,
                &mut nametag_seen,
                file_index,
                service_name,
            );
            continue;
        }

        let plan = build_rule_plan(rule, item, mappings, file_index, service_name);
        entries.push(plan);
    }

    entries
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
    if !rule.match_spec.service_type.is_empty() {
        let Some(service_name) = service_name else {
            return false;
        };
        if !rule
            .match_spec
            .service_type
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(service_name))
        {
            return false;
        }
    }

    if let Some(category) = &rule.match_spec.category {
        if !category.eq_ignore_ascii_case(category_name(item)) {
            return false;
        }
    }

    if let Some(expected) = rule.match_spec.has_scripture_ref {
        let actual = item.scripture.is_some()
            || has_scripture_ref(&item.title)
            || has_scripture_ref(&strip_title_prefix(&item.title));
        if actual != expected {
            return false;
        }
    }

    if !rule.match_spec.title_prefix.is_empty()
        && !rule
            .match_spec
            .title_prefix
            .iter()
            .any(|prefix| title_lower.starts_with(&prefix.to_lowercase()))
    {
        return false;
    }

    if !rule.match_spec.title_contains.is_empty()
        && !rule
            .match_spec
            .title_contains
            .iter()
            .any(|needle| title_lower.contains(&needle.to_lowercase()))
    {
        return false;
    }

    true
}

fn category_name(item: &Item) -> &'static str {
    match item.category {
        crate::planning_center::types::Category::Text => "text",
        crate::planning_center::types::Category::Graphic => "graphic",
        crate::planning_center::types::Category::Title => "title",
        crate::planning_center::types::Category::Song => "song",
        crate::planning_center::types::Category::Other => "other",
    }
}

fn build_rule_plan(
    rule: &ItemRuleConfig,
    item: &Item,
    mappings: &ProjectConfig,
    file_index: Option<&FileIndex>,
    service_name: Option<&str>,
) -> ResolvedItemPlan {
    let output_key = ResolvedItemPlan::primary_output_key(item.position);
    if let Some(action) = &rule.action {
        return rule_action_plan(action, item, output_key);
    }

    let Some(type_key) = rule.use_type.as_deref() else {
        return ResolvedItemPlan {
            output_key,
            position: item.position,
            pco_title: item.title.clone(),
            playlist_name: strip_speaker(&item.title),
            action: PlanAction::NeedsReview,
            reason: format!("Rule '{}' has no use_type", rule.id),
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
            reason: format!("Unknown presentation type '{type_key}'"),
            item_type: Some(type_key.to_string()),
            ..Default::default()
        };
    };

    let mut plan = build_type_plan(
        type_key,
        ptype,
        item,
        rule.target
            .as_ref()
            .and_then(|target| target.library_file.as_deref()),
        mappings,
        file_index,
        service_name,
    );
    plan.output_key = ResolvedItemPlan::primary_output_key(item.position);
    plan
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
    match ptype.kind.into() {
        ItemKind::Song => build_song_plan(
            type_key,
            ptype,
            item,
            target_library_file,
            mappings,
            file_index,
            service_name,
        ),
        ItemKind::Scripture => build_scripture_plan(type_key, ptype, item, mappings, service_name),
        _ => build_generic_plan(
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
    let found = target_library_file
        .and_then(|name| search_index(file_index, name.trim_end_matches(".pro")));
    let style = resolve_style(ptype, type_key, service_name, mappings);
    let has_description_content = matches!(ptype.content_source, ContentSourceKind::Description);
    let has_scripture_content = matches!(ptype.content_source, ContentSourceKind::Scripture);
    let parsed_content = if has_description_content {
        item.description
            .as_deref()
            .and_then(|desc| description_parser::parse_description(desc, &item.title, type_key))
    } else {
        None
    };

    let content_source = if has_description_content {
        ContentSource::Description { parsed_content }
    } else if has_scripture_content {
        ContentSource::Scripture {
            scripture: ScriptureContent::default(),
        }
    } else {
        ContentSource::None
    };

    let (action, reason) = match ptype.output_strategy {
        OutputStrategy::Skip => (PlanAction::Skip, "Configured to skip".to_string()),
        OutputStrategy::NeedsReview => (
            PlanAction::NeedsReview,
            "Configured to require review".to_string(),
        ),
        OutputStrategy::UseExisting => {
            if found.is_some() {
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
            } else if found.is_some() {
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
            } else if has_scripture_content {
                (
                    PlanAction::NeedsReview,
                    "Scripture generation must use scripture item kind".to_string(),
                )
            } else {
                (
                    PlanAction::NeedsReview,
                    "Generate-new is not implemented for static content".to_string(),
                )
            }
        }
    };

    ResolvedItemPlan {
        output_key: String::new(),
        position: item.position,
        pco_title: item.title.clone(),
        playlist_name: found
            .as_deref()
            .map(file_stem)
            .unwrap_or_else(|| strip_speaker(&item.title)),
        file_path: found,
        action,
        reason,
        item_kind: ptype.kind.into(),
        item_type: Some(type_key.to_string()),
        content_source,
        style,
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
    let style = resolve_style(ptype, type_key, service_name, mappings);

    let song_match = target_library_file
        .and_then(|name| search_index_strict(file_index, name.trim_end_matches(".pro")))
        .map_or_else(
            || resolve_song_library_match(file_index, song_title, &stripped, &bare_title),
            SongLibraryMatch::exact,
        );

    let (action, reason) = match ptype.output_strategy {
        OutputStrategy::Skip => (PlanAction::Skip, "Configured to skip".to_string()),
        OutputStrategy::NeedsReview => (
            PlanAction::NeedsReview,
            "Configured to require review".to_string(),
        ),
        OutputStrategy::UseExisting => {
            if song_match.path.is_some() {
                if song_match.uncertain {
                    (
                        PlanAction::NeedsReview,
                        "Possible library match".to_string(),
                    )
                } else {
                    (PlanAction::UseExisting, "Library match".to_string())
                }
            } else {
                (
                    PlanAction::NeedsReview,
                    "Configured existing song not found".to_string(),
                )
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

    ResolvedItemPlan {
        position: item.position,
        pco_title: item.title.clone(),
        playlist_name: song_title.to_string(),
        file_path: song_match.path.clone(),
        action,
        reason,
        item_kind: ItemKind::Song,
        item_type: Some(type_key.to_string()),
        style,
        ..Default::default()
    }
}

fn build_scripture_plan(
    type_key: &str,
    ptype: &PresentationTypeConfig,
    item: &Item,
    mappings: &ProjectConfig,
    service_name: Option<&str>,
) -> ResolvedItemPlan {
    let style = resolve_style(ptype, type_key, service_name, mappings);
    let ref_parts = split_scripture_refs(&item.title);
    if !matches!(ptype.output_strategy, OutputStrategy::GenerateNew) {
        return ResolvedItemPlan {
            position: item.position,
            pco_title: item.title.clone(),
            playlist_name: strip_speaker(&item.title),
            action: match ptype.output_strategy {
                OutputStrategy::Skip => PlanAction::Skip,
                _ => PlanAction::NeedsReview,
            },
            reason: match ptype.output_strategy {
                OutputStrategy::Skip => "Configured to skip".to_string(),
                OutputStrategy::NeedsReview => "Configured to require review".to_string(),
                OutputStrategy::UseExisting => {
                    "Use-existing is not supported for scripture generation".to_string()
                }
                OutputStrategy::EditInPlace => {
                    "Edit-in-place is not supported for scripture generation".to_string()
                }
                OutputStrategy::GenerateNew => unreachable!(),
            },
            item_kind: ItemKind::Scripture,
            item_type: Some(type_key.to_string()),
            style,
            ..Default::default()
        };
    }

    if ref_parts.is_empty() {
        return ResolvedItemPlan {
            position: item.position,
            pco_title: item.title.clone(),
            playlist_name: strip_speaker(&item.title),
            action: PlanAction::NeedsReview,
            reason: "No scripture reference".to_string(),
            item_kind: ItemKind::Scripture,
            item_type: Some(type_key.to_string()),
            style,
            ..Default::default()
        };
    }

    let version = detect_version(&item.title);
    if ref_parts.len() > 1 {
        let ref_infos: Vec<ScriptureRefInfo> = ref_parts
            .iter()
            .filter_map(|part| {
                let v = detect_version(part);
                crate::bible::parse_scripture_ref(part).map(|r| {
                    let ref_str = r.end_verse.map_or_else(
                        || format!("{} {}:{}", r.book, r.chapter, r.start_verse),
                        |end| format!("{} {}:{}-{end}", r.book, r.chapter, r.start_verse),
                    );
                    ScriptureRefInfo {
                        reference: ref_str,
                        version: v.to_string(),
                    }
                })
            })
            .collect();

        let combined_name = ref_infos
            .iter()
            .map(|r| r.reference.replace(':', "v"))
            .collect::<Vec<_>>()
            .join(", ");

        return ResolvedItemPlan {
            position: item.position,
            pco_title: item.title.clone(),
            playlist_name: format!("{combined_name} {version}"),
            action: PlanAction::GenerateNew,
            reason: format!(
                "Generate combined scripture slides ({} refs, {version})",
                ref_infos.len()
            ),
            item_kind: ItemKind::Scripture,
            item_type: Some(type_key.to_string()),
            content_source: ContentSource::Scripture {
                scripture: ScriptureContent {
                    bible_version: Some(version.to_string()),
                    references: ref_infos,
                    ..ScriptureContent::default()
                },
            },
            style,
            ..Default::default()
        };
    }

    let ref_part = &ref_parts[0];
    let scripture_ref_str = crate::bible::parse_scripture_ref(ref_part).map(|r| {
        r.end_verse.map_or_else(
            || format!("{} {}:{}", r.book, r.chapter, r.start_verse),
            |end| format!("{} {}:{}-{end}", r.book, r.chapter, r.start_verse),
        )
    });

    ResolvedItemPlan {
        position: item.position,
        pco_title: item.title.clone(),
        playlist_name: scripture_name(ref_part, version),
        action: PlanAction::GenerateNew,
        reason: format!("Generate scripture slides ({version})"),
        item_kind: ItemKind::Scripture,
        item_type: Some(type_key.to_string()),
        content_source: ContentSource::Scripture {
            scripture: ScriptureContent {
                reference: scripture_ref_str,
                bible_version: Some(version.to_string()),
                ..ScriptureContent::default()
            },
        },
        style,
        ..Default::default()
    }
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
    let mut style = PresentationStyle {
        background: ptype.background.clone(),
        arrangement: ptype.arrangement.clone(),
        template_name: ptype.template.clone(),
        title_template: ptype.title_template.clone(),
        macro_name: ptype.macro_name.clone(),
        content_macro: ptype.content_macro.clone(),
    };

    for override_rule in &mappings.overrides {
        if override_applies(override_rule, type_key, service_name, mappings) {
            if override_rule.background.is_some() {
                style.background = override_rule.background.clone();
            }
            if override_rule.arrangement.is_some() {
                style.arrangement = override_rule.arrangement.clone();
            }
            if override_rule.template.is_some() {
                style.template_name = override_rule.template.clone();
            }
        }
    }

    style
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
    rule: &ItemRuleConfig,
    item: &Item,
    speaker: Option<&str>,
    mappings: &ProjectConfig,
    entries: &mut Vec<ResolvedItemPlan>,
    nametag_seen: &mut HashSet<String>,
    file_index: Option<&FileIndex>,
    service_name: Option<&str>,
) {
    for (step_index, step) in rule.expand.iter().enumerate() {
        let output_key =
            ResolvedItemPlan::expanded_output_key(item.position, step_index, &step.use_type);
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
                .and_then(|target| target.library_file.as_deref())
                .or_else(|| {
                    rule.target
                        .as_ref()
                        .and_then(|target| target.library_file.as_deref())
                }),
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
            .and_then(|target| target.library_file.clone())
            .or_else(|| {
                step.target
                    .as_ref()
                    .and_then(|target| target.name_template.as_deref())
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

    let nametag_name = mappings
        .people
        .get(&first_name(name))
        .and_then(|p| p.nametag.clone())
        .or_else(|| {
            step.target
                .as_ref()
                .and_then(|target| target.name_template.as_deref())
                .map(|template| render_name_template(template, item, name))
        })
        .unwrap_or_else(|| format!("{name} Nametag"));
    let found = step
        .target
        .as_ref()
        .and_then(|target| target.library_file.as_deref())
        .and_then(|library_file| search_index(file_index, library_file.trim_end_matches(".pro")))
        .or_else(|| search_index(file_index, nametag_name.trim_end_matches(".pro")));
    let style = resolve_style(ptype, &step.use_type, service_name, mappings);

    let (action, reason) = match ptype.output_strategy {
        OutputStrategy::Skip => (PlanAction::Skip, "Configured to skip".to_string()),
        OutputStrategy::NeedsReview => (
            PlanAction::NeedsReview,
            "Configured to require review".to_string(),
        ),
        OutputStrategy::UseExisting => {
            if found.is_some() {
                (PlanAction::UseExisting, format!("Nametag for {name}"))
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
    };

    ResolvedItemPlan {
        output_key,
        position: item.position,
        pco_title: item.title.clone(),
        playlist_name: nametag_name.clone(),
        file_path: found,
        action,
        reason,
        item_kind: ItemKind::PersonNametag,
        item_type: Some(step.use_type.clone()),
        style,
        ..Default::default()
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

/// Resolve the speaker for an item, validating against known staff and falling
/// back to "Liturgist:" in the description when the title parenthetical is not
/// a person name (e.g. "(sel. vv)", "(`NRSVue`)").
fn resolve_speaker(
    title: &str,
    description: Option<&str>,
    mappings: &ProjectConfig,
) -> Option<String> {
    if let Some(candidate) = extract_speaker(title) {
        let first = first_name(&candidate);
        if mappings.people.contains_key(&first) {
            return Some(candidate);
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
                if !name.is_empty() {
                    return Some(name.to_string());
                }
            }
        }
    }
    // Accept title parenthetical if it looks like a name (capitalized, no periods)
    if let Some(candidate) = extract_speaker(title) {
        let looks_like_name =
            candidate.chars().next().is_some_and(char::is_uppercase) && !candidate.contains('.');
        if looks_like_name {
            return Some(candidate);
        }
    }
    None
}

fn first_name(name: &str) -> String {
    name.split_whitespace().next().unwrap_or(name).to_string()
}

fn render_name_template(template: &str, item: &Item, speaker: &str) -> String {
    template
        .replace("{speaker}", speaker)
        .replace("{first_name}", &first_name(speaker))
        .replace("{title}", &strip_speaker(&item.title))
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
    use serde::Deserialize;
    use std::path::{Path, PathBuf};
    use tempfile::tempdir;

    fn fixture_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/workflow")
    }

    fn load_config() -> ProjectConfig {
        parse_project_config_str(include_str!("../../tests/fixtures/workflow/v2_config.json"))
            .expect("fixture config should parse")
    }

    fn load_items() -> Vec<Item> {
        let raw: Vec<FixtureItem> =
            serde_json::from_str(include_str!("../../tests/fixtures/workflow/items.json"))
                .expect("fixture items should parse");
        raw.into_iter().map(FixtureItem::into_item).collect()
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
        let library_dir = fixture_root().join("library");
        let index = FileIndex::build(&library_dir).expect("fixture library should index");

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
            Some(2)
        );
        assert_eq!(
            call_to_worship.template_name.as_deref(),
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
        assert_eq!(song.template_name.as_deref(), Some("Song"));

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
        assert_eq!(scripture.template_name.as_deref(), Some("Scripture"));

        let sermon = entries
            .iter()
            .find(|entry| entry.pco_title == "Sermon")
            .expect("sermon entry");
        assert!(matches!(sermon.status, PreviewStatus::Skipped));
        assert_eq!(sermon.reason, "Sermon is added day-of");
    }

    #[test]
    fn description_generate_new_uses_strategy_not_edited_fallback() {
        let config = parse_project_config_str(
            r#"
            {
              "version": 2,
              "presentation_types": {
                "liturgical_edited": {
                  "kind": "liturgy",
                  "content_source": "description",
                  "output_strategy": "generate_new",
                  "template": "Information (Projectors)"
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
              "version": 2,
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
        assert_eq!(plans[0].reason, "Configured existing file not found");
    }

    #[test]
    fn expansion_outputs_have_stable_keys_and_respect_declared_type() {
        let config = parse_project_config_str(
            r#"
            {
              "version": 2,
              "presentation_types": {
                "person_nametag": {
                  "kind": "nametag",
                  "content_source": "static",
                  "output_strategy": "use_existing",
                  "template": "Name Tag"
                },
                "liturgical_edited": {
                  "kind": "liturgy",
                  "content_source": "description",
                  "output_strategy": "edit_in_place",
                  "template": "Information (Projectors)"
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
        std::fs::write(library_dir.path().join("Hope Nametag.pro"), b"fixture").expect("nametag");
        std::fs::write(library_dir.path().join("Call to Worship.pro"), b"fixture")
            .expect("call to worship");
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
        assert_eq!(plans[0].output_key, "1:expand:0:person_nametag");
        assert_eq!(plans[0].item_type.as_deref(), Some("person_nametag"));
        assert_eq!(plans[0].action, PlanAction::UseExisting);
        assert!(plans[0]
            .file_path
            .as_deref()
            .is_some_and(|path| path.ends_with("Hope Nametag.pro")));

        assert_eq!(plans[1].output_key, "1:expand:1:liturgical_edited");
        assert_eq!(plans[1].item_type.as_deref(), Some("liturgical_edited"));
        assert_eq!(plans[1].action, PlanAction::EditInPlace);
        assert!(plans[1]
            .file_path
            .as_deref()
            .is_some_and(|path| path.ends_with("Call to Worship.pro")));
    }

    #[test]
    fn expanded_outputs_preserve_distinct_keys_even_when_file_matches_repeat() {
        let config = parse_project_config_str(
            r#"
            {
              "version": 2,
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
        std::fs::write(library_dir.path().join("Announcements.pro"), b"fixture").expect("slide");
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
        assert_eq!(plans[0].output_key, "1:expand:0:static_slide");
        assert_eq!(plans[1].output_key, "1:expand:1:static_slide");
        assert_eq!(plans[0].file_path, plans[1].file_path);
    }
}
