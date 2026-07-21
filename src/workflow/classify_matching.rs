//! Predicates for selecting checked classification rules.

use super::scripture::has_scripture_ref;
use crate::planning_center::types::{Category, Item};
use crate::project_config::{
    CompiledItemRule, ItemMatchInput, MatchCategory, ProjectConfig, RuleTier,
};

pub(super) enum RuleSelection<'a> {
    None,
    Selected(&'a CompiledItemRule),
    Ambiguous {
        tier: RuleTier,
        rules: Vec<&'a CompiledItemRule>,
    },
}

pub(super) fn select_matching_rule<'a>(
    item: &Item,
    config: &'a ProjectConfig,
    service_name: Option<&str>,
) -> RuleSelection<'a> {
    let has_scripture_ref = item.scripture.is_some()
        || has_scripture_ref(&item.title)
        || has_scripture_ref(&strip_title_prefix(&item.title));
    let input = ItemMatchInput::new(
        item.category.into(),
        &item.title,
        item.description.as_deref(),
        has_scripture_ref,
        service_name,
    );
    let mut winning_tier = None;
    let mut matching_rules = Vec::new();
    for rule in config.compiled_item_rules() {
        if !rule.matches(&input) {
            continue;
        }
        match winning_tier {
            None => {
                winning_tier = Some(rule.tier());
                matching_rules.push(rule);
            }
            Some(tier) if rule.tier().precedence() > tier.precedence() => {
                winning_tier = Some(rule.tier());
                matching_rules.clear();
                matching_rules.push(rule);
            }
            Some(tier) if rule.tier() == tier => matching_rules.push(rule),
            Some(_) => {}
        }
    }

    let Some(winning_tier) = winning_tier else {
        return RuleSelection::None;
    };
    match matching_rules.as_slice() {
        [] => RuleSelection::None,
        [rule] => RuleSelection::Selected(rule),
        _ => RuleSelection::Ambiguous {
            tier: winning_tier,
            rules: matching_rules,
        },
    }
}

impl From<Category> for MatchCategory {
    fn from(category: Category) -> Self {
        match category {
            Category::Text => Self::Text,
            Category::Graphic => Self::Graphic,
            Category::Title => Self::Title,
            Category::Song => Self::Song,
            Category::Other => Self::Other,
        }
    }
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
