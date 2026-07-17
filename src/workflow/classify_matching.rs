//! Predicates for selecting configured classification and context rules.

use std::collections::BTreeMap;

use super::scripture::has_scripture_ref;
use crate::planning_center::types::{Category, Item};
use crate::project_config::{DecisionChoiceConfig, ItemRuleConfig, MatchSpec, ProjectConfig};

pub(super) fn find_matching_rule<'a>(
    item: &Item,
    normalized_title: &str,
    config: &'a ProjectConfig,
    service_name: Option<&str>,
) -> Option<&'a ItemRuleConfig> {
    config.item_rules().iter().find(|rule| {
        match_spec_matches_item(&rule.match_spec, item, normalized_title, service_name)
    })
}

fn match_spec_matches_item(
    match_spec: &MatchSpec,
    item: &Item,
    normalized_title: &str,
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
        if !category.eq_ignore_ascii_case(category_name(item.category)) {
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
            .any(|prefix| normalized_title.starts_with(&prefix.to_lowercase()))
    {
        return false;
    }

    if !match_spec.title_contains.is_empty()
        && !match_spec
            .title_contains
            .iter()
            .any(|needle| normalized_title.contains(&needle.to_lowercase()))
    {
        return false;
    }

    if !match_spec.description_contains.is_empty() {
        let normalized_description = normalize_apostrophes(
            &item
                .description
                .as_deref()
                .unwrap_or_default()
                .to_lowercase(),
        );
        if !match_spec
            .description_contains
            .iter()
            .any(|needle| normalized_description.contains(&needle.to_lowercase()))
        {
            return false;
        }
    }

    true
}

/// Replace curly apostrophes with ASCII apostrophes before matching text.
pub(super) fn normalize_apostrophes(text: &str) -> String {
    text.replace(['\u{2018}', '\u{2019}'], "'")
}

pub(super) fn strip_speaker(title: &str) -> String {
    title.rfind('(').map_or_else(
        || title.to_string(),
        |index| title[..index].trim().to_string(),
    )
}

pub(super) fn strip_title_prefix(title: &str) -> String {
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

const fn category_name(category: Category) -> &'static str {
    match category {
        Category::Text => "text",
        Category::Graphic => "graphic",
        Category::Title => "title",
        Category::Song => "song",
        Category::Other => "other",
    }
}

pub(super) fn decision_context_text(item: &Item, context_fields: &[String]) -> String {
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

pub(super) fn decision_choice_matches(choice: &DecisionChoiceConfig, context_text: &str) -> bool {
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
    let normalized_needle = normalize_apostrophes(&needle.to_lowercase());
    context_text.contains(&normalized_needle)
}

pub(super) fn decision_review_reason(
    rule: &ItemRuleConfig,
    instructions: Option<&str>,
    choices: &BTreeMap<String, DecisionChoiceConfig>,
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
