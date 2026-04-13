//! Setup and onboarding helpers for project config authoring.
//!
//! These routines are intentionally diagnostic. They expose available
//! ProPresenter assets and summarize Planning Center item patterns so an LLM
//! can draft deterministic config, without leaking heuristics into the runtime
//! build path.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::planning_center::types::{Category, Item, Plan};
use crate::project_config::{
    ContentSourceKind, ExpansionStep, ItemKind, ItemRuleConfig, MatchSpec, OutputStrategy,
    PlanSort, PresentationTypeConfig, ProfileConfig, ProjectConfig, ProjectDefaults,
    ProjectMetadata, ReviewPolicy, ServiceGroupConfig, SpeakerSource, TargetSpec,
};
use crate::propresenter::macros::MacroCache;
use crate::propresenter::template::ThemeCache;
use crate::utils::file_index::{normalize_name, FileIndex};
use crate::workflow::classify::{self, PreviewStatus};

#[derive(Debug, Serialize)]
pub(crate) struct AssetCatalog {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library_path: Option<String>,
    pub templates: Vec<String>,
    pub macros: Vec<String>,
    pub library: LibraryCatalog,
    pub service_groups: Vec<ServiceGroupSummary>,
    pub profiles: Vec<ProfileSummary>,
    pub presentation_types: Vec<PresentationTypeSummary>,
}

#[derive(Debug, Serialize)]
pub(crate) struct LibraryCatalog {
    pub file_count: usize,
    pub top_level_folders: Vec<NamedCount>,
    pub sample_files: Vec<LibraryFileSummary>,
}

#[derive(Debug, Serialize)]
pub(crate) struct LibraryFileSummary {
    pub file_name: String,
    pub relative_path: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct NamedCount {
    pub name: String,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct ServiceGroupSummary {
    pub name: String,
    pub service_types: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ProfileSummary {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub service_groups: Vec<String>,
    pub service_types: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub days_ahead: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_policy: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PresentationTypeSummary {
    pub name: String,
    pub kind: String,
    pub content_source: String,
    pub output_strategy: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_template: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(rename = "macro", skip_serializing_if = "Option::is_none")]
    pub macro_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_macro: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arrangement: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct RecentPlanAnalysis {
    pub scope: AnalysisScope,
    pub service_breakdown: Vec<NamedCount>,
    pub category_breakdown: Vec<NamedCount>,
    pub analyzed_plans: Vec<AnalyzedPlanSummary>,
    pub recurring_titles: Vec<RecurringItemPattern>,
    pub recurring_patterns: Vec<RecurringItemPattern>,
    pub scripture_patterns: Vec<RecurringItemPattern>,
    pub speaker_candidates: Vec<NamedCount>,
    pub candidate_rules: Vec<CandidateRuleHint>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ConfigPatchSuggestion {
    pub summary: ConfigPatchSummary,
    pub patch: SuggestedConfigPatch,
    pub unresolved_patterns: Vec<SuggestedPatternPatch>,
    pub review_notes: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct DraftProjectConfigResponse {
    pub config: ProjectConfig,
    pub assumptions: Vec<String>,
    pub review_notes: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ConfigPatchSummary {
    pub analyzed_plans: usize,
    pub unresolved_patterns: usize,
    pub suggested_presentation_types: usize,
    pub suggested_item_rules: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SuggestedConfigPatch {
    pub presentation_types: BTreeMap<String, PresentationTypeConfig>,
    pub item_rules: Vec<ItemRuleConfig>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SuggestedPatternPatch {
    pub title: String,
    pub category: String,
    pub count: usize,
    pub reasons: Vec<String>,
    pub sample_titles: Vec<String>,
    pub sample_services: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_use_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_library_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_rule: Option<ItemRuleConfig>,
}

#[derive(Debug, Serialize)]
pub(crate) struct AnalysisScope {
    pub plan_count: usize,
    pub item_count: usize,
    pub service_types: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct AnalyzedPlanSummary {
    pub plan_id: String,
    pub service_name: String,
    pub plan_title: String,
    pub date: String,
    pub item_count: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct RecurringItemPattern {
    pub title: String,
    pub count: usize,
    pub categories: Vec<String>,
    pub sample_titles: Vec<String>,
    pub sample_services: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CandidateRuleHint {
    pub rationale: String,
    pub occurrence_count: usize,
    pub match_spec: CandidateMatchSpec,
    pub sample_titles: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CandidateMatchSpec {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_prefix: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_scripture_ref: Option<bool>,
}

#[derive(Debug, Default)]
struct PatternAccumulator {
    display: String,
    count: usize,
    categories: BTreeSet<String>,
    sample_titles: BTreeSet<String>,
    sample_services: BTreeSet<String>,
}

#[derive(Debug, Default)]
struct UnresolvedAccumulator {
    display: String,
    category: String,
    count: usize,
    reasons: BTreeSet<String>,
    sample_titles: BTreeSet<String>,
    sample_services: BTreeSet<String>,
    existing_item_types: BTreeSet<String>,
    suggested_library_files: BTreeSet<String>,
    speaker_candidates: BTreeSet<String>,
    has_description: bool,
    has_scripture_ref: bool,
    is_song: bool,
}

pub(crate) fn catalog_assets(
    config: &ProjectConfig,
    template_cache: &ThemeCache,
    macro_cache: &MacroCache,
    file_index: Option<&FileIndex>,
    library_path: Option<&Path>,
    sample_limit: usize,
) -> AssetCatalog {
    let mut service_groups: Vec<_> = config
        .service_groups
        .iter()
        .map(|(name, group)| summarize_service_group(name, group))
        .collect();
    service_groups.sort_by(|a, b| a.name.cmp(&b.name));

    let mut profiles: Vec<_> = config
        .profiles
        .iter()
        .map(|(name, profile)| summarize_profile(name, profile))
        .collect();
    profiles.sort_by(|a, b| a.name.cmp(&b.name));

    let mut presentation_types: Vec<_> = config
        .presentation_types
        .iter()
        .map(|(name, ptype)| summarize_presentation_type(name, ptype))
        .collect();
    presentation_types.sort_by(|a, b| a.name.cmp(&b.name));

    let templates = template_cache
        .theme_slide_names()
        .into_iter()
        .map(str::to_string)
        .collect();

    let macros = macro_cache
        .names()
        .into_iter()
        .map(str::to_string)
        .collect();

    let library = summarize_library(file_index, sample_limit);

    AssetCatalog {
        project_name: config.metadata.name.clone(),
        theme_name: template_cache.theme_name().map(str::to_string),
        library_path: library_path.map(|path| path.display().to_string()),
        templates,
        macros,
        library,
        service_groups,
        profiles,
        presentation_types,
    }
}

pub(crate) fn analyze_recent_plans(
    plans: &[Plan],
    item_sets: &[Vec<Item>],
    max_patterns: usize,
) -> RecentPlanAnalysis {
    let mut service_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut category_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut exact_titles: HashMap<String, PatternAccumulator> = HashMap::new();
    let mut normalized_patterns: HashMap<String, PatternAccumulator> = HashMap::new();
    let mut scripture_patterns: HashMap<String, PatternAccumulator> = HashMap::new();
    let mut speaker_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut analyzed_plans = Vec::new();
    let mut service_types = BTreeSet::new();
    let mut item_count = 0usize;

    for (plan, items) in plans.iter().zip(item_sets) {
        *service_counts.entry(plan.service_name.clone()).or_default() += 1;
        service_types.insert(plan.service_name.clone());
        analyzed_plans.push(AnalyzedPlanSummary {
            plan_id: plan.id.clone(),
            service_name: plan.service_name.clone(),
            plan_title: plan.title.clone(),
            date: plan.date.format("%Y-%m-%d").to_string(),
            item_count: items.len(),
        });

        for item in items {
            item_count += 1;
            let category = category_name(item.category).to_string();
            *category_counts.entry(category.clone()).or_default() += 1;

            let exact_key = item.title.to_lowercase();
            accumulate_pattern(
                exact_titles.entry(exact_key).or_default(),
                &item.title,
                &item.title,
                &category,
                &plan.service_name,
            );

            let normalized = normalized_rule_title(&item.title);
            let normalized_key = normalized.to_lowercase();
            accumulate_pattern(
                normalized_patterns.entry(normalized_key).or_default(),
                &normalized,
                &item.title,
                &category,
                &plan.service_name,
            );

            if is_scripture_item(item) {
                let scripture_title = scripture_pattern_title(&item.title);
                let scripture_key = scripture_title.to_lowercase();
                accumulate_pattern(
                    scripture_patterns.entry(scripture_key).or_default(),
                    &scripture_title,
                    &item.title,
                    &category,
                    &plan.service_name,
                );
            }

            if let Some(speaker) = infer_speaker_candidate(item) {
                *speaker_counts.entry(speaker).or_default() += 1;
            }
        }
    }

    let recurring_titles = top_patterns(exact_titles, max_patterns, 2);
    let recurring_patterns = top_patterns(normalized_patterns, max_patterns, 2);
    let scripture_patterns = top_patterns(scripture_patterns, max_patterns, 1);
    let service_breakdown = sort_named_counts(service_counts);
    let category_breakdown = sort_named_counts(category_counts);
    let speaker_candidates = sort_named_counts(speaker_counts);
    let candidate_rules = build_candidate_rules(
        &category_breakdown,
        &recurring_patterns,
        &scripture_patterns,
    );

    RecentPlanAnalysis {
        scope: AnalysisScope {
            plan_count: plans.len(),
            item_count,
            service_types: service_types.into_iter().collect(),
        },
        service_breakdown,
        category_breakdown,
        analyzed_plans,
        recurring_titles,
        recurring_patterns,
        scripture_patterns,
        speaker_candidates,
        candidate_rules,
    }
}

pub(crate) fn suggest_config_patch(
    config: &ProjectConfig,
    plans: &[Plan],
    item_sets: &[Vec<Item>],
    file_index: Option<&FileIndex>,
    template_cache: &ThemeCache,
    macro_cache: &MacroCache,
    max_suggestions: usize,
) -> ConfigPatchSuggestion {
    let mut unresolved: HashMap<String, UnresolvedAccumulator> = HashMap::new();

    for (plan, items) in plans.iter().zip(item_sets) {
        let previews = classify::build_preview(items, config, file_index, Some(&plan.service_name));

        for entry in previews {
            if !matches!(entry.status, PreviewStatus::Uncertain) {
                continue;
            }
            let Some(item) = items.iter().find(|item| item.position == entry.position) else {
                continue;
            };
            let key = format!(
                "{}|{}|{}",
                category_name(item.category),
                normalized_rule_title(&item.title).to_lowercase(),
                item.scripture.is_some()
            );
            let accumulator = unresolved.entry(key).or_default();
            accumulator.display = normalized_rule_title(&item.title);
            accumulator.category = category_name(item.category).to_string();
            accumulator.count += 1;
            accumulator.reasons.insert(entry.reason);
            if accumulator.sample_titles.len() < 5 {
                accumulator.sample_titles.insert(item.title.clone());
            }
            if accumulator.sample_services.len() < 5 {
                accumulator
                    .sample_services
                    .insert(plan.service_name.clone());
            }
            if let Some(item_type) = entry.item_type {
                accumulator.existing_item_types.insert(item_type);
            }
            if let Some(path) = entry.file_path {
                if let Some(file_name) = std::path::Path::new(&path)
                    .file_name()
                    .and_then(|name| name.to_str())
                {
                    accumulator
                        .suggested_library_files
                        .insert(file_name.to_string());
                }
            }
            accumulator.has_description |= item
                .description
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty());
            accumulator.has_scripture_ref |= item.scripture.is_some();
            accumulator.is_song |= matches!(item.category, Category::Song);
            if let Some(speaker) = infer_speaker_candidate(item) {
                accumulator.speaker_candidates.insert(speaker);
            }

            if accumulator.suggested_library_files.is_empty() {
                let query = item
                    .song
                    .as_ref()
                    .map_or(item.title.as_str(), |song| song.title.as_str());
                if let Some(file_name) = find_exact_library_file_name(file_index, query) {
                    accumulator.suggested_library_files.insert(file_name);
                }
            }
        }
    }

    let mut patch = SuggestedConfigPatch {
        presentation_types: BTreeMap::new(),
        item_rules: Vec::new(),
    };
    let mut review_notes = Vec::new();
    let mut used_rule_ids = existing_rule_ids(config);
    let mut unresolved_patterns = Vec::new();

    let template_names = template_cache
        .theme_slide_names()
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let macro_names = macro_cache
        .names()
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();

    let mut grouped: Vec<_> = unresolved.into_values().collect();
    grouped.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.display.cmp(&b.display))
            .then_with(|| a.category.cmp(&b.category))
    });

    for entry in grouped.into_iter().take(max_suggestions) {
        let suggested_library_file = entry.suggested_library_files.iter().next().cloned();
        let suggested_use_type = if is_speaker_driven_bundle_title(&entry.display)
            && !entry.speaker_candidates.is_empty()
        {
            infer_or_suggest_bundle_use_type(
                config,
                &mut patch.presentation_types,
                &entry,
                suggested_library_file.as_deref(),
                &template_names,
                &macro_names,
            )
        } else {
            infer_or_suggest_use_type(
                config,
                &mut patch.presentation_types,
                &entry,
                &template_names,
                &macro_names,
            )
        };

        let suggested_rule = if is_speaker_driven_bundle_title(&entry.display)
            && !entry.speaker_candidates.is_empty()
        {
            build_speaker_bundle_rule(
                config,
                &mut patch.presentation_types,
                &mut used_rule_ids,
                &entry,
                suggested_library_file.clone(),
                &template_names,
                &macro_names,
            )
        } else {
            suggested_use_type.as_deref().and_then(|use_type| {
                build_suggested_rule(
                    config,
                    &patch.presentation_types,
                    &mut used_rule_ids,
                    &entry,
                    use_type,
                    suggested_library_file.clone(),
                )
            })
        };

        if suggested_rule.is_none() {
            review_notes.push(format!(
                "Pattern '{}' needs manual review: no deterministic rule target/type could be inferred.",
                entry.display
            ));
        } else if let Some(rule) = &suggested_rule {
            patch.item_rules.push(rule.clone());
        }

        unresolved_patterns.push(SuggestedPatternPatch {
            title: entry.display,
            category: entry.category,
            count: entry.count,
            reasons: entry.reasons.into_iter().collect(),
            sample_titles: entry.sample_titles.into_iter().collect(),
            sample_services: entry.sample_services.into_iter().collect(),
            suggested_use_type,
            suggested_library_file,
            suggested_rule,
        });
    }

    ConfigPatchSuggestion {
        summary: ConfigPatchSummary {
            analyzed_plans: plans.len(),
            unresolved_patterns: unresolved_patterns.len(),
            suggested_presentation_types: patch.presentation_types.len(),
            suggested_item_rules: patch.item_rules.len(),
        },
        patch,
        unresolved_patterns,
        review_notes,
    }
}

pub(crate) fn draft_project_config(
    project_name: Option<&str>,
    plans: &[Plan],
    item_sets: &[Vec<Item>],
    file_index: Option<&FileIndex>,
    template_cache: &ThemeCache,
    macro_cache: &MacroCache,
    days_ahead: i64,
) -> DraftProjectConfigResponse {
    let service_counts = count_services(plans);
    let all_services: Vec<String> = service_counts.keys().cloned().collect();
    let recurring_services: Vec<String> = service_counts
        .iter()
        .filter(|(_, count)| **count > 1)
        .map(|(name, _)| name.clone())
        .collect();

    let template_names = template_cache
        .theme_slide_names()
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let macro_names = macro_cache
        .names()
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();

    let analysis = analyze_recent_plans(plans, item_sets, 20);
    let mut assumptions = Vec::new();
    let mut review_notes = Vec::new();
    let theme_name = template_cache.theme_name().map(str::to_string);
    let draft_config = ProjectConfig::default();

    let mut service_groups = BTreeMap::new();
    if !all_services.is_empty() {
        service_groups.insert(
            "all_services".to_string(),
            ServiceGroupConfig {
                service_types: all_services.clone(),
            },
        );
        assumptions.push("Created 'all_services' from analyzed service types.".to_string());
    }
    if !recurring_services.is_empty() && recurring_services.len() < all_services.len() {
        service_groups.insert(
            "recurring_services".to_string(),
            ServiceGroupConfig {
                service_types: recurring_services.clone(),
            },
        );
        assumptions.push(
            "Created 'recurring_services' from service types that appeared more than once."
                .to_string(),
        );
    }

    let mut profiles = BTreeMap::new();
    if service_groups.contains_key("all_services") {
        profiles.insert(
            "default".to_string(),
            ProfileConfig {
                description: Some(
                    "Starter profile covering all analyzed service types".to_string(),
                ),
                service_groups: vec!["all_services".to_string()],
                service_types: Vec::new(),
                days_ahead: Some(days_ahead),
                review_policy: Some(ReviewPolicy::Ask),
            },
        );
    }
    if service_groups.contains_key("recurring_services") {
        profiles.insert(
            "recurring".to_string(),
            ProfileConfig {
                description: Some(
                    "Starter profile for recurring service types observed in recent plans"
                        .to_string(),
                ),
                service_groups: vec!["recurring_services".to_string()],
                service_types: Vec::new(),
                days_ahead: Some(days_ahead),
                review_policy: Some(ReviewPolicy::Ask),
            },
        );
    }

    let recurring_groups = collect_recurring_groups(item_sets, file_index);
    let mut presentation_types = BTreeMap::new();
    if recurring_groups.iter().any(|group| {
        is_speaker_driven_bundle_title(&group.title) && !group.speaker_candidates.is_empty()
    }) {
        ensure_person_nametag_type(
            &draft_config,
            &mut presentation_types,
            &template_names,
            &macro_names,
        );
    }
    if analysis
        .category_breakdown
        .iter()
        .any(|entry| entry.name.eq_ignore_ascii_case("song"))
    {
        presentation_types.insert(
            "song".to_string(),
            PresentationTypeConfig {
                kind: ItemKind::Song,
                content_source: ContentSourceKind::Song,
                output_strategy: OutputStrategy::UseExisting,
                template: pick_template(&template_names, &["lyrics", "song"]),
                title_template: None,
                background: None,
                macro_name: pick_macro(&macro_names, &["song"]),
                content_macro: None,
                arrangement: Some("Default".to_string()),
                description: Some("Starter library-backed song type".to_string()),
            },
        );
    }

    if !analysis.scripture_patterns.is_empty() {
        presentation_types.insert(
            "scripture".to_string(),
            PresentationTypeConfig {
                kind: ItemKind::Scripture,
                content_source: ContentSourceKind::Scripture,
                output_strategy: OutputStrategy::GenerateNew,
                template: pick_template(&template_names, &["scripture"]),
                title_template: pick_template(&template_names, &["information", "title"]),
                background: None,
                macro_name: pick_macro(&macro_names, &["name tag", "title", "scripture"]),
                content_macro: pick_macro(&macro_names, &["scripture", "prayer"]),
                arrangement: None,
                description: Some("Starter scripture generation type".to_string()),
            },
        );
    }

    if has_group_matching(item_sets, |item| {
        !matches!(item.category, Category::Song)
            && item.scripture.is_none()
            && has_description(item)
    }) {
        presentation_types.insert(
            "description_presentation".to_string(),
            PresentationTypeConfig {
                kind: ItemKind::Other,
                content_source: ContentSourceKind::Description,
                output_strategy: OutputStrategy::GenerateNew,
                template: pick_template(
                    &template_names,
                    &["information", "responsive", "scripture", "text"],
                ),
                title_template: None,
                background: None,
                macro_name: pick_macro(&macro_names, &["scripture", "prayer", "title"]),
                content_macro: None,
                arrangement: None,
                description: Some(
                    "Starter generated type for description-driven items".to_string(),
                ),
            },
        );
    }

    if has_group_matching(item_sets, |item| {
        !matches!(item.category, Category::Song)
            && item.scripture.is_none()
            && !has_description(item)
    }) {
        presentation_types.insert(
            "static_presentation".to_string(),
            PresentationTypeConfig {
                kind: ItemKind::Other,
                content_source: ContentSourceKind::Static,
                output_strategy: OutputStrategy::UseExisting,
                template: pick_template(
                    &template_names,
                    &["information", "responsive", "scripture", "text"],
                ),
                title_template: None,
                background: None,
                macro_name: pick_macro(&macro_names, &["scripture", "prayer", "title"]),
                content_macro: None,
                arrangement: None,
                description: Some(
                    "Starter library-backed type for static recurring items".to_string(),
                ),
            },
        );
    }

    let mut item_rules = Vec::new();
    let mut used_rule_ids = HashSet::new();

    if presentation_types.contains_key("song") {
        item_rules.push(ItemRuleConfig {
            id: unique_rule_id(&mut used_rule_ids, "songs"),
            match_spec: MatchSpec {
                title_prefix: Vec::new(),
                title_contains: Vec::new(),
                category: Some("song".to_string()),
                has_scripture_ref: None,
                service_type: Vec::new(),
            },
            use_type: Some("song".to_string()),
            action: None,
            expand: Vec::new(),
            target: None,
            notes: Some("Starter category rule for song items".to_string()),
        });
    }

    if presentation_types.contains_key("scripture") {
        item_rules.push(ItemRuleConfig {
            id: unique_rule_id(&mut used_rule_ids, "scripture"),
            match_spec: MatchSpec {
                title_prefix: vec!["scripture".to_string()],
                title_contains: Vec::new(),
                category: None,
                has_scripture_ref: Some(true),
                service_type: Vec::new(),
            },
            use_type: Some("scripture".to_string()),
            action: None,
            expand: Vec::new(),
            target: None,
            notes: Some("Starter rule for scripture-bearing items".to_string()),
        });
    }

    for group in recurring_groups.into_iter().take(20) {
        if group.category.eq_ignore_ascii_case("song") || group.has_scripture_ref {
            continue;
        }

        if is_speaker_driven_bundle_title(&group.title) && !group.speaker_candidates.is_empty() {
            let speaker_type = ensure_person_nametag_type(
                &draft_config,
                &mut presentation_types,
                &template_names,
                &macro_names,
            );
            let content_type = ensure_bundle_content_type(
                &draft_config,
                &mut presentation_types,
                &template_names,
                &macro_names,
                group.library_file.is_some(),
            );
            item_rules.push(ItemRuleConfig {
                id: unique_rule_id(&mut used_rule_ids, &group.title),
                match_spec: MatchSpec {
                    title_prefix: vec![group.title.to_lowercase()],
                    title_contains: Vec::new(),
                    category: None,
                    has_scripture_ref: None,
                    service_type: Vec::new(),
                },
                use_type: None,
                action: None,
                expand: vec![
                    ExpansionStep {
                        use_type: speaker_type,
                        speaker: Some(SpeakerSource::Resolved),
                        target: None,
                    },
                    ExpansionStep {
                        use_type: content_type,
                        speaker: None,
                        target: group.library_file.map(|library_file| TargetSpec {
                            library_file: Some(library_file),
                            name_template: None,
                        }),
                    },
                ],
                target: None,
                notes: Some(
                    "Starter speaker-driven bundle rule derived from recent plans".to_string(),
                ),
            });
            continue;
        }

        let use_type = if group.has_description {
            "description_presentation"
        } else {
            "static_presentation"
        };

        if !presentation_types.contains_key(use_type) {
            continue;
        }

        let needs_target = presentation_types
            .get(use_type)
            .is_some_and(|ptype| matches!(ptype.output_strategy, OutputStrategy::UseExisting));
        if needs_target && group.library_file.is_none() {
            review_notes.push(format!(
                "Pattern '{}' looks static but no exact library file was found. Add a target or switch the type strategy.",
                group.title
            ));
            continue;
        }

        item_rules.push(ItemRuleConfig {
            id: unique_rule_id(&mut used_rule_ids, &group.title),
            match_spec: MatchSpec {
                title_prefix: vec![group.title.to_lowercase()],
                title_contains: Vec::new(),
                category: None,
                has_scripture_ref: None,
                service_type: Vec::new(),
            },
            use_type: Some(use_type.to_string()),
            action: None,
            expand: Vec::new(),
            target: group.library_file.map(|library_file| TargetSpec {
                library_file: Some(library_file),
                name_template: None,
            }),
            notes: Some("Starter recurring item rule derived from recent plans".to_string()),
        });
    }

    if presentation_types.is_empty() {
        review_notes.push(
            "No starter presentation types could be inferred from the analyzed plans and discovered assets."
                .to_string(),
        );
    }
    if item_rules.is_empty() {
        review_notes.push(
            "No starter item rules could be inferred. Inspect recent plans manually and add the first few rules by hand."
                .to_string(),
        );
    }

    assumptions.push(
        "Draft config favors conservative starter rules over exhaustive automation.".to_string(),
    );
    assumptions.push(
        "Recurring non-song items are split into generated vs library-backed types based on whether recent examples carried description content.".to_string(),
    );

    DraftProjectConfigResponse {
        config: ProjectConfig {
            version: 2,
            metadata: ProjectMetadata {
                name: project_name.map(str::to_string),
                timezone: Some("America/New_York".to_string()),
                notes: Some("Drafted by ProFlow setup tooling; review before use.".to_string()),
            },
            defaults: ProjectDefaults {
                theme: theme_name,
                days_ahead: Some(days_ahead),
                review_policy: Some(ReviewPolicy::Ask),
                plan_sort: Some(PlanSort::AscendingDate),
            },
            service_groups: service_groups.into_iter().collect(),
            profiles: profiles.into_iter().collect(),
            presentation_types: presentation_types.into_iter().collect(),
            item_rules,
            people: HashMap::new(),
            overrides: Vec::new(),
        },
        assumptions,
        review_notes,
    }
}

fn summarize_library(file_index: Option<&FileIndex>, sample_limit: usize) -> LibraryCatalog {
    let Some(index) = file_index else {
        return LibraryCatalog {
            file_count: 0,
            top_level_folders: Vec::new(),
            sample_files: Vec::new(),
        };
    };

    let mut folder_counts: BTreeMap<String, usize> = BTreeMap::new();
    for entry in &index.entries {
        let folder = entry
            .relative_path
            .split_once('/')
            .map_or_else(|| "(root)".to_string(), |(segment, _)| segment.to_string());
        *folder_counts.entry(folder).or_default() += 1;
    }

    let mut sample_files: Vec<_> = index
        .entries
        .iter()
        .map(|entry| LibraryFileSummary {
            file_name: entry.file_name.clone(),
            relative_path: entry.relative_path.clone(),
        })
        .collect();
    sample_files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    sample_files.truncate(sample_limit);

    LibraryCatalog {
        file_count: index.entries.len(),
        top_level_folders: sort_named_counts(folder_counts),
        sample_files,
    }
}

fn summarize_service_group(name: &str, group: &ServiceGroupConfig) -> ServiceGroupSummary {
    ServiceGroupSummary {
        name: name.to_string(),
        service_types: group.service_types.clone(),
    }
}

fn summarize_profile(name: &str, profile: &ProfileConfig) -> ProfileSummary {
    ProfileSummary {
        name: name.to_string(),
        description: profile.description.clone(),
        service_groups: profile.service_groups.clone(),
        service_types: profile.service_types.clone(),
        days_ahead: profile.days_ahead,
        review_policy: profile.review_policy.map(review_policy_name),
    }
}

fn summarize_presentation_type(
    name: &str,
    ptype: &PresentationTypeConfig,
) -> PresentationTypeSummary {
    PresentationTypeSummary {
        name: name.to_string(),
        kind: item_kind_name(ptype.kind).to_string(),
        content_source: content_source_name(ptype.content_source).to_string(),
        output_strategy: output_strategy_name(ptype.output_strategy).to_string(),
        template: ptype.template.clone(),
        title_template: ptype.title_template.clone(),
        background: ptype.background.clone(),
        macro_name: ptype.macro_name.clone(),
        content_macro: ptype.content_macro.clone(),
        arrangement: ptype.arrangement.clone(),
    }
}

fn sort_named_counts(counts: impl IntoIterator<Item = (String, usize)>) -> Vec<NamedCount> {
    let mut values: Vec<_> = counts
        .into_iter()
        .map(|(name, count)| NamedCount { name, count })
        .collect();
    values.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
    values
}

fn accumulate_pattern(
    accumulator: &mut PatternAccumulator,
    display: &str,
    sample_title: &str,
    category: &str,
    service_name: &str,
) {
    accumulator.display = display.to_string();
    accumulator.count += 1;
    accumulator.categories.insert(category.to_string());
    if accumulator.sample_titles.len() < 5 {
        accumulator.sample_titles.insert(sample_title.to_string());
    }
    if accumulator.sample_services.len() < 5 {
        accumulator.sample_services.insert(service_name.to_string());
    }
}

fn top_patterns(
    patterns: HashMap<String, PatternAccumulator>,
    max_patterns: usize,
    min_count: usize,
) -> Vec<RecurringItemPattern> {
    let mut values: Vec<_> = patterns
        .into_values()
        .filter(|entry| entry.count >= min_count)
        .map(|entry| RecurringItemPattern {
            title: entry.display,
            count: entry.count,
            categories: entry.categories.into_iter().collect(),
            sample_titles: entry.sample_titles.into_iter().collect(),
            sample_services: entry.sample_services.into_iter().collect(),
        })
        .collect();
    values.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.title.cmp(&b.title)));
    values.truncate(max_patterns);
    values
}

fn build_candidate_rules(
    category_breakdown: &[NamedCount],
    recurring_patterns: &[RecurringItemPattern],
    scripture_patterns: &[RecurringItemPattern],
) -> Vec<CandidateRuleHint> {
    let mut hints = Vec::new();

    if let Some(song_count) = category_breakdown
        .iter()
        .find(|entry| entry.name.eq_ignore_ascii_case("song"))
    {
        hints.push(CandidateRuleHint {
            rationale: "Song items recur across plans and usually map cleanly by category."
                .to_string(),
            occurrence_count: song_count.count,
            match_spec: CandidateMatchSpec {
                title_prefix: None,
                category: Some("song".to_string()),
                has_scripture_ref: None,
            },
            sample_titles: Vec::new(),
        });
    }

    if let Some(scripture) = scripture_patterns.first() {
        hints.push(CandidateRuleHint {
            rationale:
                "Scripture items usually need one dedicated rule using a scripture reference."
                    .to_string(),
            occurrence_count: scripture.count,
            match_spec: CandidateMatchSpec {
                title_prefix: Some(vec![scripture.title.to_lowercase()]),
                category: None,
                has_scripture_ref: Some(true),
            },
            sample_titles: scripture.sample_titles.clone(),
        });
    }

    for pattern in recurring_patterns.iter().take(12) {
        if pattern.title.eq_ignore_ascii_case("scripture") {
            continue;
        }
        hints.push(CandidateRuleHint {
            rationale: "Recurring normalized titles are good candidates for stable item_rules."
                .to_string(),
            occurrence_count: pattern.count,
            match_spec: CandidateMatchSpec {
                title_prefix: Some(vec![pattern.title.to_lowercase()]),
                category: None,
                has_scripture_ref: None,
            },
            sample_titles: pattern.sample_titles.clone(),
        });
    }

    hints
}

fn count_services(plans: &[Plan]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for plan in plans {
        *counts.entry(plan.service_name.clone()).or_default() += 1;
    }
    counts
}

fn has_group_matching(item_sets: &[Vec<Item>], predicate: impl Fn(&Item) -> bool) -> bool {
    item_sets.iter().flatten().any(predicate)
}

#[derive(Debug, Default)]
struct DraftRecurringGroup {
    title: String,
    category: String,
    count: usize,
    has_description: bool,
    has_scripture_ref: bool,
    library_file: Option<String>,
    speaker_candidates: BTreeSet<String>,
}

fn collect_recurring_groups(
    item_sets: &[Vec<Item>],
    file_index: Option<&FileIndex>,
) -> Vec<DraftRecurringGroup> {
    let mut groups: HashMap<String, DraftRecurringGroup> = HashMap::new();

    for item in item_sets.iter().flatten() {
        let title = normalized_rule_title(&item.title);
        let key = format!(
            "{}|{}|{}",
            category_name(item.category),
            title.to_lowercase(),
            item.scripture.is_some()
        );
        let group = groups.entry(key).or_default();
        group.title = title.clone();
        group.category = category_name(item.category).to_string();
        group.count += 1;
        group.has_description |= has_description(item);
        group.has_scripture_ref |= item.scripture.is_some();
        if let Some(speaker) = infer_speaker_candidate(item) {
            group.speaker_candidates.insert(speaker);
        }
        if group.library_file.is_none() {
            let query = item
                .song
                .as_ref()
                .map_or(item.title.as_str(), |song| song.title.as_str());
            group.library_file = find_exact_library_file_name(file_index, query);
        }
    }

    let mut values: Vec<_> = groups
        .into_values()
        .filter(|group| group.count >= 2)
        .collect();
    values.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.title.cmp(&b.title)));
    values
}

fn infer_or_suggest_use_type(
    config: &ProjectConfig,
    additions: &mut BTreeMap<String, PresentationTypeConfig>,
    entry: &UnresolvedAccumulator,
    template_names: &[String],
    macro_names: &[String],
) -> Option<String> {
    if let Some(type_key) = entry
        .existing_item_types
        .iter()
        .find(|type_key| config.presentation_types.contains_key(type_key.as_str()))
    {
        return Some(type_key.clone());
    }

    if entry.has_scripture_ref {
        if let Some(type_key) = find_existing_type_key(config, |ptype| {
            matches!(ptype.kind, ItemKind::Scripture)
                || matches!(ptype.content_source, ContentSourceKind::Scripture)
        }) {
            return Some(type_key);
        }

        let template = pick_template(template_names, &["scripture"])?;
        let type_key = unique_type_key(config, additions, "scripture");
        additions
            .entry(type_key.clone())
            .or_insert_with(|| PresentationTypeConfig {
                kind: ItemKind::Scripture,
                content_source: ContentSourceKind::Scripture,
                output_strategy: OutputStrategy::GenerateNew,
                template: Some(template),
                title_template: pick_template(template_names, &["information", "title"]),
                background: None,
                macro_name: pick_macro(macro_names, &["name tag", "title", "scripture"]),
                content_macro: pick_macro(macro_names, &["scripture", "prayer"]),
                arrangement: None,
                description: Some("Suggested scripture generation type".to_string()),
            });
        return Some(type_key);
    }

    if entry.is_song {
        if let Some(type_key) = find_existing_type_key(config, |ptype| {
            matches!(ptype.kind, ItemKind::Song)
                || matches!(ptype.content_source, ContentSourceKind::Song)
        }) {
            return Some(type_key);
        }

        let type_key = unique_type_key(config, additions, "song");
        additions
            .entry(type_key.clone())
            .or_insert_with(|| PresentationTypeConfig {
                kind: ItemKind::Song,
                content_source: ContentSourceKind::Song,
                output_strategy: OutputStrategy::UseExisting,
                template: pick_template(template_names, &["lyrics", "song"]),
                title_template: None,
                background: None,
                macro_name: pick_macro(macro_names, &["song"]),
                content_macro: None,
                arrangement: Some("Default".to_string()),
                description: Some("Suggested library-backed song type".to_string()),
            });
        return Some(type_key);
    }

    if entry.has_description {
        find_existing_type_key(config, |ptype| {
            matches!(ptype.content_source, ContentSourceKind::Description)
                && !matches!(ptype.kind, ItemKind::Song | ItemKind::Scripture)
        })
    } else {
        find_existing_type_key(config, |ptype| {
            matches!(ptype.output_strategy, OutputStrategy::UseExisting)
                && !matches!(ptype.kind, ItemKind::Song | ItemKind::Scripture)
        })
    }
}

fn build_suggested_rule(
    config: &ProjectConfig,
    added_types: &BTreeMap<String, PresentationTypeConfig>,
    used_rule_ids: &mut HashSet<String>,
    entry: &UnresolvedAccumulator,
    use_type: &str,
    suggested_library_file: Option<String>,
) -> Option<ItemRuleConfig> {
    let ptype = config
        .presentation_types
        .get(use_type)
        .or_else(|| added_types.get(use_type))?;

    let library_file = match ptype.output_strategy {
        OutputStrategy::UseExisting | OutputStrategy::EditInPlace => suggested_library_file,
        OutputStrategy::GenerateNew | OutputStrategy::Skip | OutputStrategy::NeedsReview => None,
    };

    if matches!(
        ptype.output_strategy,
        OutputStrategy::UseExisting | OutputStrategy::EditInPlace
    ) && library_file.is_none()
    {
        return None;
    }

    let mut match_spec = MatchSpec {
        title_prefix: vec![entry.display.to_lowercase()],
        title_contains: Vec::new(),
        category: entry.is_song.then(|| "song".to_string()),
        has_scripture_ref: entry.has_scripture_ref.then_some(true),
        service_type: Vec::new(),
    };

    if entry.display.eq_ignore_ascii_case("scripture") {
        match_spec.title_prefix = vec!["scripture".to_string()];
    }

    Some(ItemRuleConfig {
        id: unique_rule_id(used_rule_ids, &entry.display),
        match_spec,
        use_type: Some(use_type.to_string()),
        action: None,
        expand: Vec::new(),
        target: library_file.map(|library_file| TargetSpec {
            library_file: Some(library_file),
            name_template: None,
        }),
        notes: Some("Suggested from unresolved plan analysis".to_string()),
    })
}

fn build_speaker_bundle_rule(
    config: &ProjectConfig,
    added_types: &mut BTreeMap<String, PresentationTypeConfig>,
    used_rule_ids: &mut HashSet<String>,
    entry: &UnresolvedAccumulator,
    suggested_library_file: Option<String>,
    template_names: &[String],
    macro_names: &[String],
) -> Option<ItemRuleConfig> {
    let speaker_type = ensure_person_nametag_type(config, added_types, template_names, macro_names);
    let content_type = ensure_bundle_content_type(
        config,
        added_types,
        template_names,
        macro_names,
        suggested_library_file.is_some(),
    );
    let ptype = config
        .presentation_types
        .get(&content_type)
        .or_else(|| added_types.get(&content_type))?;
    let content_target = match ptype.output_strategy {
        OutputStrategy::UseExisting | OutputStrategy::EditInPlace => suggested_library_file,
        OutputStrategy::GenerateNew | OutputStrategy::Skip | OutputStrategy::NeedsReview => None,
    };

    if matches!(
        ptype.output_strategy,
        OutputStrategy::UseExisting | OutputStrategy::EditInPlace
    ) && content_target.is_none()
    {
        return None;
    }

    Some(ItemRuleConfig {
        id: unique_rule_id(used_rule_ids, &entry.display),
        match_spec: MatchSpec {
            title_prefix: vec![entry.display.to_lowercase()],
            title_contains: Vec::new(),
            category: None,
            has_scripture_ref: entry.has_scripture_ref.then_some(true),
            service_type: Vec::new(),
        },
        use_type: None,
        action: None,
        expand: vec![
            ExpansionStep {
                use_type: speaker_type,
                speaker: Some(SpeakerSource::Resolved),
                target: None,
            },
            ExpansionStep {
                use_type: content_type,
                speaker: None,
                target: content_target.map(|library_file| TargetSpec {
                    library_file: Some(library_file),
                    name_template: None,
                }),
            },
        ],
        target: None,
        notes: Some("Suggested speaker-driven bundle rule".to_string()),
    })
}

fn normalized_rule_title(title: &str) -> String {
    let stripped = strip_trailing_parenthetical(title).trim().to_string();
    if let Some((prefix, rest)) = stripped.split_once(':') {
        let prefix = prefix.trim();
        if !prefix.is_empty() && word_count(prefix) <= 5 && !rest.trim().is_empty() {
            return prefix.to_string();
        }
    }
    if let Some((prefix, rest)) = stripped.split_once(" - ") {
        let prefix = prefix.trim();
        if !prefix.is_empty() && word_count(prefix) <= 5 && !rest.trim().is_empty() {
            return prefix.to_string();
        }
    }
    stripped
}

fn is_speaker_driven_bundle_title(title: &str) -> bool {
    matches!(
        normalized_rule_title(title).to_lowercase().as_str(),
        "welcome" | "call to worship" | "moment for mission"
    )
}

fn infer_or_suggest_bundle_use_type(
    config: &ProjectConfig,
    additions: &mut BTreeMap<String, PresentationTypeConfig>,
    entry: &UnresolvedAccumulator,
    suggested_library_file: Option<&str>,
    template_names: &[String],
    macro_names: &[String],
) -> Option<String> {
    is_speaker_driven_bundle_title(&entry.display).then(|| {
        ensure_bundle_content_type(
            config,
            additions,
            template_names,
            macro_names,
            suggested_library_file.is_some(),
        )
    })
}

fn ensure_person_nametag_type(
    config: &ProjectConfig,
    additions: &mut BTreeMap<String, PresentationTypeConfig>,
    template_names: &[String],
    macro_names: &[String],
) -> String {
    if let Some(type_key) = find_existing_type_key(config, |ptype| {
        matches!(ptype.kind, ItemKind::Nametag)
            || (matches!(ptype.content_source, ContentSourceKind::Static)
                && ptype
                    .template
                    .as_deref()
                    .is_some_and(|template| template.to_lowercase().contains("name")))
    }) {
        return type_key;
    }
    if let Some(type_key) = find_existing_type_key_in_additions(additions, |ptype| {
        matches!(ptype.kind, ItemKind::Nametag)
            || (matches!(ptype.content_source, ContentSourceKind::Static)
                && ptype
                    .template
                    .as_deref()
                    .is_some_and(|template| template.to_lowercase().contains("name")))
    }) {
        return type_key;
    }

    let type_key = unique_type_key(config, additions, "person_nametag");
    additions
        .entry(type_key.clone())
        .or_insert_with(|| PresentationTypeConfig {
            kind: ItemKind::Nametag,
            content_source: ContentSourceKind::Static,
            output_strategy: OutputStrategy::UseExisting,
            template: pick_template(template_names, &["name tag", "nametag"]),
            title_template: None,
            background: None,
            macro_name: pick_macro(macro_names, &["name tag", "title"]),
            content_macro: None,
            arrangement: None,
            description: Some("Suggested speaker nametag type".to_string()),
        });
    type_key
}

fn ensure_bundle_content_type(
    config: &ProjectConfig,
    additions: &mut BTreeMap<String, PresentationTypeConfig>,
    template_names: &[String],
    macro_names: &[String],
    has_library_file: bool,
) -> String {
    if let Some(type_key) = find_existing_type_key(config, |ptype| {
        matches!(ptype.content_source, ContentSourceKind::Description)
            && !matches!(ptype.kind, ItemKind::Song | ItemKind::Scripture)
    }) {
        return type_key;
    }
    if let Some(type_key) = find_existing_type_key_in_additions(additions, |ptype| {
        matches!(ptype.content_source, ContentSourceKind::Description)
            && !matches!(ptype.kind, ItemKind::Song | ItemKind::Scripture)
    }) {
        return type_key;
    }

    let base = if has_library_file {
        "liturgy"
    } else {
        "description_presentation"
    };
    let type_key = unique_type_key(config, additions, base);
    additions
        .entry(type_key.clone())
        .or_insert_with(|| PresentationTypeConfig {
            kind: ItemKind::Liturgy,
            content_source: ContentSourceKind::Description,
            output_strategy: if has_library_file {
                OutputStrategy::EditInPlace
            } else {
                OutputStrategy::GenerateNew
            },
            template: pick_template(
                template_names,
                &["information", "responsive", "scripture", "text"],
            ),
            title_template: None,
            background: None,
            macro_name: pick_macro(macro_names, &["scripture", "prayer", "title"]),
            content_macro: None,
            arrangement: if has_library_file {
                Some("Standard".to_string())
            } else {
                None
            },
            description: Some("Suggested speaker-driven bundle content type".to_string()),
        });
    type_key
}

fn find_exact_library_file_name(file_index: Option<&FileIndex>, query: &str) -> Option<String> {
    let Some(index) = file_index else {
        return None;
    };

    let normalized_query = normalize_name(strip_trailing_parenthetical(query)).to_lowercase();
    let raw_query = strip_trailing_parenthetical(query).trim().to_lowercase();
    let mut matches = index
        .entries
        .iter()
        .filter(|entry| {
            entry.normalized_lower == normalized_query || entry.file_name_lower == raw_query
        })
        .map(|entry| format!("{}.pro", entry.file_name))
        .collect::<Vec<_>>();
    matches.sort();
    matches.dedup();
    (matches.len() == 1).then(|| matches.remove(0))
}

fn find_existing_type_key(
    config: &ProjectConfig,
    predicate: impl Fn(&PresentationTypeConfig) -> bool,
) -> Option<String> {
    let mut matches = config
        .presentation_types
        .iter()
        .filter(|(_, ptype)| predicate(ptype))
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    matches.sort();
    matches.into_iter().next()
}

fn find_existing_type_key_in_additions(
    additions: &BTreeMap<String, PresentationTypeConfig>,
    predicate: impl Fn(&PresentationTypeConfig) -> bool,
) -> Option<String> {
    let mut matches = additions
        .iter()
        .filter(|(_, ptype)| predicate(ptype))
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    matches.sort();
    matches.into_iter().next()
}

fn pick_template(template_names: &[String], needles: &[&str]) -> Option<String> {
    template_names.iter().find_map(|name| {
        let lower = name.to_lowercase();
        needles
            .iter()
            .any(|needle| lower.contains(&needle.to_lowercase()))
            .then(|| name.clone())
    })
}

fn pick_macro(macro_names: &[String], needles: &[&str]) -> Option<String> {
    macro_names.iter().find_map(|name| {
        let lower = name.to_lowercase();
        needles
            .iter()
            .any(|needle| lower.contains(&needle.to_lowercase()))
            .then(|| name.clone())
    })
}

fn unique_type_key(
    config: &ProjectConfig,
    additions: &BTreeMap<String, PresentationTypeConfig>,
    base: &str,
) -> String {
    if !config.presentation_types.contains_key(base) && !additions.contains_key(base) {
        return base.to_string();
    }

    let mut idx = 2usize;
    loop {
        let candidate = format!("{base}_{idx}");
        if !config.presentation_types.contains_key(&candidate)
            && !additions.contains_key(&candidate)
        {
            return candidate;
        }
        idx += 1;
    }
}

fn existing_rule_ids(config: &ProjectConfig) -> HashSet<String> {
    config
        .item_rules
        .iter()
        .map(|rule| rule.id.clone())
        .collect()
}

fn unique_rule_id(used_rule_ids: &mut HashSet<String>, title: &str) -> String {
    let slug = slugify(title);
    let base = if slug.is_empty() {
        "suggested_rule".to_string()
    } else {
        slug
    };

    if used_rule_ids.insert(base.clone()) {
        return base;
    }

    let mut idx = 2usize;
    loop {
        let candidate = format!("{base}_{idx}");
        if used_rule_ids.insert(candidate.clone()) {
            return candidate;
        }
        idx += 1;
    }
}

fn slugify(text: &str) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            slug.push('_');
            prev_dash = true;
        }
    }
    slug.trim_matches('_').to_string()
}

fn has_description(item: &Item) -> bool {
    item.description
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
}

fn scripture_pattern_title(title: &str) -> String {
    let normalized = normalized_rule_title(title);
    let lower = normalized.to_lowercase();
    if lower.starts_with("scripture") {
        "Scripture".to_string()
    } else {
        normalized
    }
}

fn strip_trailing_parenthetical(title: &str) -> &str {
    let trimmed = title.trim();
    let Some(end) = trimmed.rfind(')') else {
        return trimmed;
    };
    let Some(start) = trimmed[..end].rfind('(') else {
        return trimmed;
    };
    let candidate = trimmed[start + 1..end].trim();
    if candidate.is_empty() {
        return trimmed;
    }
    trimmed[..start].trim_end()
}

fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

fn is_scripture_item(item: &Item) -> bool {
    item.scripture.is_some() || normalized_rule_title(&item.title).eq_ignore_ascii_case("scripture")
}

fn infer_speaker_candidate(item: &Item) -> Option<String> {
    if let Some(candidate) = extract_parenthetical(&item.title) {
        if looks_like_person_name(&candidate) {
            return Some(candidate);
        }
    }

    item.description.as_deref().and_then(|description| {
        description.lines().find_map(|line| {
            let trimmed = line.trim();
            let rest = trimmed
                .strip_prefix("Leader:")
                .or_else(|| trimmed.strip_prefix("leader:"))
                .or_else(|| trimmed.strip_prefix("Speaker:"))
                .or_else(|| trimmed.strip_prefix("speaker:"))
                .or_else(|| trimmed.strip_prefix("Host:"))
                .or_else(|| trimmed.strip_prefix("host:"))
                .or_else(|| trimmed.strip_prefix("Liturgist:"))
                .or_else(|| trimmed.strip_prefix("liturgist:"))?;
            let name = rest.split(';').next().unwrap_or(rest).trim();
            (!name.is_empty()).then(|| name.to_string())
        })
    })
}

fn extract_parenthetical(title: &str) -> Option<String> {
    let end = title.rfind(')')?;
    let start = title[..end].rfind('(')?;
    let value = title[start + 1..end].trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn looks_like_person_name(candidate: &str) -> bool {
    candidate.chars().next().is_some_and(char::is_uppercase)
        && candidate
            .chars()
            .all(|c| c.is_alphabetic() || c.is_whitespace() || matches!(c, '-' | '\'' | '.'))
}

fn category_name(category: Category) -> &'static str {
    match category {
        Category::Text => "text",
        Category::Graphic => "graphic",
        Category::Title => "title",
        Category::Song => "song",
        Category::Other => "other",
    }
}

fn review_policy_name(policy: ReviewPolicy) -> String {
    match policy {
        ReviewPolicy::Ask => "ask".to_string(),
        ReviewPolicy::Fail => "fail".to_string(),
        ReviewPolicy::Skip => "skip".to_string(),
    }
}

fn item_kind_name(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Song => "song",
        ItemKind::Scripture => "scripture",
        ItemKind::Liturgy => "liturgy",
        ItemKind::Nametag => "nametag",
        ItemKind::Announcement => "announcement",
        ItemKind::Graphic => "graphic",
        ItemKind::Other => "other",
    }
}

fn content_source_name(kind: ContentSourceKind) -> &'static str {
    match kind {
        ContentSourceKind::Static => "static",
        ContentSourceKind::Description => "description",
        ContentSourceKind::Scripture => "scripture",
        ContentSourceKind::Song => "song",
    }
}

fn output_strategy_name(strategy: OutputStrategy) -> &'static str {
    match strategy {
        OutputStrategy::Skip => "skip",
        OutputStrategy::UseExisting => "use_existing",
        OutputStrategy::EditInPlace => "edit_in_place",
        OutputStrategy::GenerateNew => "generate_new",
        OutputStrategy::NeedsReview => "needs_review",
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use crate::planning_center::types::{Item, Plan, Scripture, Song};
    use crate::project_config::parse_project_config_str;
    use chrono::Utc;
    use std::path::{Path, PathBuf};

    fn fixture_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/workflow")
    }

    #[test]
    fn catalog_assets_reports_library_and_config_summaries() {
        let config =
            parse_project_config_str(include_str!("../../tests/fixtures/workflow/v2_config.json"))
                .expect("fixture config should parse");
        let index = FileIndex::build(&fixture_root().join("library"))
            .expect("fixture library should index");
        let theme_cache = ThemeCache::new(None, vec![]);
        let macro_cache = MacroCache::empty();

        let catalog = catalog_assets(
            &config,
            &theme_cache,
            &macro_cache,
            Some(&index),
            Some(Path::new("/tmp/library")),
            3,
        );

        assert_eq!(catalog.project_name.as_deref(), Some("Fixture Church"));
        assert_eq!(catalog.library.file_count, index.entries.len());
        assert_eq!(
            catalog.library.sample_files.len(),
            index.entries.len().min(3)
        );
        assert!(catalog
            .presentation_types
            .iter()
            .any(|entry| entry.name == "scripture"));
    }

    #[test]
    fn analyze_recent_plans_surfaces_recurring_patterns_and_rule_hints() {
        let plans = vec![
            Plan {
                id: "plan-1".to_string(),
                service_id: "svc-1".to_string(),
                service_name: "10:30am traditional".to_string(),
                date: Utc::now(),
                title: "April 12".to_string(),
                items: Vec::new(),
            },
            Plan {
                id: "plan-2".to_string(),
                service_id: "svc-1".to_string(),
                service_name: "10:30am traditional".to_string(),
                date: Utc::now(),
                title: "April 19".to_string(),
                items: Vec::new(),
            },
        ];
        let item_sets = vec![
            vec![
                item("1", 1, "Call to Worship (Hope)", Category::Text),
                item("2", 2, "Scripture: Isaiah 35:1-6", Category::Title),
                song_item("3", 3, "Amazing Grace"),
            ],
            vec![
                item("4", 1, "Call to Worship (Robert)", Category::Text),
                item("5", 2, "Scripture: Luke 2:1-7", Category::Title),
                song_item("6", 3, "Amazing Grace"),
            ],
        ];

        let analysis = analyze_recent_plans(&plans, &item_sets, 10);

        assert_eq!(analysis.scope.plan_count, 2);
        assert!(analysis
            .recurring_patterns
            .iter()
            .any(|entry| entry.title == "Call to Worship" && entry.count == 2));
        assert!(analysis
            .scripture_patterns
            .iter()
            .any(|entry| entry.title == "Scripture" && entry.count == 2));
        assert!(analysis
            .candidate_rules
            .iter()
            .any(|entry| entry.match_spec.category.as_deref() == Some("song")));
    }

    #[test]
    fn suggest_config_patch_proposes_rule_for_unmapped_recurring_liturgy() {
        let mut config =
            parse_project_config_str(include_str!("../../tests/fixtures/workflow/v2_config.json"))
                .expect("fixture config should parse");
        config
            .item_rules
            .retain(|rule| rule.id != "call_to_worship");

        let plans = vec![Plan {
            id: "plan-1".to_string(),
            service_id: "svc-1".to_string(),
            service_name: "Sunday Morning".to_string(),
            date: Utc::now(),
            title: "April 12".to_string(),
            items: Vec::new(),
        }];
        let item_sets = vec![vec![Item {
            id: "1".to_string(),
            position: 1,
            title: "Call to Worship (Hope)".to_string(),
            description: Some("Leader: Hello\nPeople: Welcome".to_string()),
            category: Category::Text,
            note: None,
            song: None,
            scripture: None,
        }]];
        let index = FileIndex::build(&fixture_root().join("library"))
            .expect("fixture library should index");
        let theme_cache = ThemeCache::new(None, vec![]);
        let macro_cache = MacroCache::empty();

        let suggestion = suggest_config_patch(
            &config,
            &plans,
            &item_sets,
            Some(&index),
            &theme_cache,
            &macro_cache,
            10,
        );

        assert_eq!(suggestion.summary.suggested_item_rules, 1);
        let rule = suggestion
            .patch
            .item_rules
            .first()
            .expect("should suggest a rule");
        assert!(rule.use_type.is_none());
        assert_eq!(rule.expand.len(), 2);
        assert_eq!(rule.expand[0].use_type, "person_nametag");
        assert!(matches!(
            rule.expand[0].speaker,
            Some(SpeakerSource::Resolved)
        ));
        assert_eq!(rule.expand[1].use_type, "liturgical_edited");
        assert_eq!(
            rule.expand[1]
                .target
                .as_ref()
                .and_then(|target| target.library_file.as_deref()),
            Some("Call to Worship.pro")
        );
    }

    #[test]
    fn suggest_config_patch_adds_song_type_when_missing() {
        let mut config =
            parse_project_config_str(include_str!("../../tests/fixtures/workflow/v2_config.json"))
                .expect("fixture config should parse");
        config.presentation_types.remove("song");
        config
            .item_rules
            .retain(|rule| rule.id != "song_amazing_grace");

        let plans = vec![Plan {
            id: "plan-1".to_string(),
            service_id: "svc-1".to_string(),
            service_name: "Sunday Morning".to_string(),
            date: Utc::now(),
            title: "April 12".to_string(),
            items: Vec::new(),
        }];
        let item_sets = vec![vec![song_item("3", 3, "Amazing Grace")]];
        let index = FileIndex::build(&fixture_root().join("library"))
            .expect("fixture library should index");
        let theme_cache = ThemeCache::new(None, vec![]);
        let macro_cache = MacroCache::empty();

        let suggestion = suggest_config_patch(
            &config,
            &plans,
            &item_sets,
            Some(&index),
            &theme_cache,
            &macro_cache,
            10,
        );

        assert!(suggestion.patch.presentation_types.contains_key("song"));
        assert_eq!(suggestion.summary.suggested_item_rules, 1);
    }

    #[test]
    fn draft_project_config_builds_starter_schema_from_recent_plans() {
        let plans = vec![
            Plan {
                id: "plan-1".to_string(),
                service_id: "svc-1".to_string(),
                service_name: "Sunday Morning".to_string(),
                date: Utc::now(),
                title: "April 12".to_string(),
                items: Vec::new(),
            },
            Plan {
                id: "plan-2".to_string(),
                service_id: "svc-1".to_string(),
                service_name: "Sunday Morning".to_string(),
                date: Utc::now(),
                title: "April 19".to_string(),
                items: Vec::new(),
            },
        ];
        let item_sets = vec![
            vec![
                Item {
                    id: "1".to_string(),
                    position: 1,
                    title: "Call to Worship (Hope)".to_string(),
                    description: Some("Leader: Hello\nPeople: Welcome".to_string()),
                    category: Category::Text,
                    note: None,
                    song: None,
                    scripture: None,
                },
                item("2", 2, "Scripture: Isaiah 35:1-6", Category::Title),
                song_item("3", 3, "Amazing Grace"),
            ],
            vec![
                Item {
                    id: "4".to_string(),
                    position: 1,
                    title: "Call to Worship (Robert)".to_string(),
                    description: Some("Leader: Again\nPeople: Response".to_string()),
                    category: Category::Text,
                    note: None,
                    song: None,
                    scripture: None,
                },
                item("5", 2, "Scripture: Luke 2:1-7", Category::Title),
                song_item("6", 3, "Amazing Grace"),
            ],
        ];
        let index = FileIndex::build(&fixture_root().join("library"))
            .expect("fixture library should index");
        let theme_cache = ThemeCache::new(None, vec![]);
        let macro_cache = MacroCache::empty();

        let draft = draft_project_config(
            Some("Starter Church"),
            &plans,
            &item_sets,
            Some(&index),
            &theme_cache,
            &macro_cache,
            21,
        );

        assert_eq!(
            draft.config.metadata.name.as_deref(),
            Some("Starter Church")
        );
        assert!(draft.config.service_groups.contains_key("all_services"));
        assert!(draft.config.profiles.contains_key("default"));
        assert!(draft.config.presentation_types.contains_key("song"));
        assert!(draft.config.presentation_types.contains_key("scripture"));
        assert!(draft
            .config
            .presentation_types
            .contains_key("person_nametag"));
        assert!(draft.config.item_rules.iter().any(|rule| {
            rule.use_type.is_none()
                && rule.expand.len() == 2
                && rule.expand[0].use_type == "person_nametag"
        }));
        assert!(draft
            .config
            .item_rules
            .iter()
            .any(|rule| rule.use_type.as_deref() == Some("song")));
        assert!(draft
            .config
            .item_rules
            .iter()
            .any(|rule| rule.use_type.as_deref() == Some("scripture")));
    }

    fn item(id: &str, position: usize, title: &str, category: Category) -> Item {
        Item {
            id: id.to_string(),
            position,
            title: title.to_string(),
            description: None,
            category,
            note: None,
            song: None,
            scripture: title.contains("Scripture").then(|| Scripture {
                reference: "Isaiah 35:1-6".to_string(),
                text: None,
                translation: None,
            }),
        }
    }

    #[test]
    fn infer_speaker_candidate_recognizes_common_bundle_roles() {
        let welcome = Item {
            id: "1".to_string(),
            position: 1,
            title: "Welcome".to_string(),
            description: Some("Leader: Hope".to_string()),
            category: Category::Text,
            note: None,
            song: None,
            scripture: None,
        };
        let call_to_worship = Item {
            id: "2".to_string(),
            position: 1,
            title: "Call to Worship".to_string(),
            description: Some("Host: Robert".to_string()),
            category: Category::Text,
            note: None,
            song: None,
            scripture: None,
        };

        assert_eq!(infer_speaker_candidate(&welcome).as_deref(), Some("Hope"));
        assert_eq!(
            infer_speaker_candidate(&call_to_worship).as_deref(),
            Some("Robert")
        );
    }

    fn song_item(id: &str, position: usize, title: &str) -> Item {
        Item {
            id: id.to_string(),
            position,
            title: title.to_string(),
            description: None,
            category: Category::Song,
            note: None,
            song: Some(Song {
                title: title.to_string(),
                author: None,
                copyright: None,
                ccli: None,
                themes: None,
                lyrics: None,
                arrangement: Some("Default".to_string()),
            }),
            scripture: None,
        }
    }
}
