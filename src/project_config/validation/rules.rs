//! Validation for service scopes, classification rules, targets, and people.

use super::{
    identity::{
        canonical_presentation_filename, validate_exact_identity, validate_identity_values,
        validate_library_filename, validate_map_keys, validate_name_template,
        validate_presentation_filename,
    },
    service_group_scope, service_scopes_overlap, ConfigValidationIssue,
};
use crate::project_config::{
    DecisionChoiceConfig, DecisionChoiceMatch, DecisionConfig, ItemRuleOutcome, MatchSpec,
    RawProjectConfig, TargetSpec,
};
use std::collections::{HashMap, HashSet};

pub(super) fn validate_service_groups(
    config: &RawProjectConfig,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    validate_map_keys(
        config.service_groups.keys(),
        "service_groups",
        "service group key",
        issues,
    );
    for (group_key, group) in &config.service_groups {
        let path = format!("service_groups.{group_key}.service_types");
        if group.service_types.is_empty() {
            issues.push(ConfigValidationIssue {
                path,
                message: "service group must contain at least one service type".to_string(),
            });
            continue;
        }
        validate_identity_values(
            &group.service_types,
            &format!("service_groups.{group_key}.service_types"),
            "service type",
            issues,
        );
    }
}

pub(super) fn validate_item_rules(
    config: &RawProjectConfig,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    let mut rule_ids = HashSet::new();
    for (idx, rule) in config.item_rules.iter().enumerate() {
        validate_exact_identity(
            &rule.id,
            &format!("item_rules[{idx}].id"),
            "item rule id",
            issues,
        );
        if !rule_ids.insert(rule.id.to_ascii_lowercase()) {
            issues.push(ConfigValidationIssue {
                path: format!("item_rules[{idx}].id"),
                message: format!("duplicate item rule id '{}'", rule.id),
            });
        }
        validate_match_spec(
            &rule.match_spec,
            &format!("item_rules[{idx}].match"),
            issues,
        );

        match &rule.outcome {
            ItemRuleOutcome::UseType { type_key, target } => {
                validate_presentation_type_reference(
                    config,
                    type_key,
                    &format!("item_rules[{idx}].use_type"),
                    issues,
                );
                if let Some(target) = target {
                    let path = format!("item_rules[{idx}].target");
                    validate_target_spec(target, &path, issues);
                }
            }
            ItemRuleOutcome::Action(_) => {}
            ItemRuleOutcome::Decision(decision) => {
                validate_decision(config, idx, decision, issues);
            }
            ItemRuleOutcome::Expand(expansion) => {
                for (step_idx, step) in expansion.iter().enumerate() {
                    validate_presentation_type_reference(
                        config,
                        &step.use_type,
                        &format!("item_rules[{idx}].expand[{step_idx}].use_type"),
                        issues,
                    );
                    if let Some(target) = &step.target {
                        let path = format!("item_rules[{idx}].expand[{step_idx}].target");
                        validate_target_spec(target, &path, issues);
                    }
                }
            }
        }
    }

    if let Some(rule_id) = config.defaults.speaker_fallback_rule.as_deref() {
        let path = "defaults.speaker_fallback_rule";
        validate_exact_identity(rule_id, path, "speaker fallback rule ID", issues);
        if !config.item_rules.iter().any(|rule| rule.id == rule_id) {
            issues.push(ConfigValidationIssue {
                path: path.to_string(),
                message: format!("unknown item rule ID '{rule_id}'"),
            });
        }
    }
}

pub(super) fn validate_required_playlist_items(
    config: &RawProjectConfig,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    let mut ids = HashSet::new();
    for (index, item) in config.required_playlist_items.iter().enumerate() {
        let path = format!("required_playlist_items[{index}]");
        validate_exact_identity(
            &item.id,
            &format!("{path}.id"),
            "required playlist item id",
            issues,
        );
        if !ids.insert(item.id.to_ascii_lowercase()) {
            issues.push(ConfigValidationIssue {
                path: format!("{path}.id"),
                message: format!("duplicate required playlist item id '{}'", item.id),
            });
        }
        validate_library_filename(&item.library_file, &format!("{path}.library_file"), issues);
        validate_presentation_type_reference(
            config,
            &item.use_type,
            &format!("{path}.use_type"),
            issues,
        );
        if let Some(group) = item.service_group.as_deref() {
            if !config.service_groups.contains_key(group) {
                issues.push(ConfigValidationIssue {
                    path: format!("{path}.service_group"),
                    message: format!("references unknown service group '{group}'"),
                });
            }
        }
    }

    validate_required_playlist_item_ownership(config, issues);
}

/// Reject two policies that would both own the same native presentation in any
/// one service. Reusing a presentation across disjoint service scopes remains
/// valid because at most one policy can apply to a concrete service name.
fn validate_required_playlist_item_ownership(
    config: &RawProjectConfig,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    for (index, item) in config.required_playlist_items.iter().enumerate() {
        let Some(identity) = canonical_presentation_filename(&item.library_file) else {
            continue;
        };
        let duplicate = config.required_playlist_items[..index]
            .iter()
            .enumerate()
            .find(|(_, previous)| {
                canonical_presentation_filename(&previous.library_file).as_deref()
                    == Some(identity.as_str())
                    && required_service_scopes_intersect(
                        config,
                        previous.service_group.as_deref(),
                        item.service_group.as_deref(),
                    )
            });
        let Some((previous_index, previous)) = duplicate else {
            continue;
        };
        issues.push(ConfigValidationIssue {
            path: format!("required_playlist_items[{index}].library_file"),
            message: format!(
                "library file '{}' is already required by required_playlist_items[{previous_index}] ('{}') in an overlapping service scope",
                item.library_file, previous.id
            ),
        });
    }
}

fn required_service_scopes_intersect(
    config: &RawProjectConfig,
    left_group: Option<&str>,
    right_group: Option<&str>,
) -> bool {
    let left_scope = left_group.map(|group| service_group_scope(config, group));
    let right_scope = right_group.map(|group| service_group_scope(config, group));
    service_scopes_overlap(&left_scope, &right_scope)
}

pub(super) fn validate_people(config: &RawProjectConfig, issues: &mut Vec<ConfigValidationIssue>) {
    validate_map_keys(config.people.keys(), "people", "person key", issues);
    for (first_name, person) in &config.people {
        let path = format!("people.{first_name}");
        if first_name.split_whitespace().count() != 1 {
            issues.push(ConfigValidationIssue {
                path: "people".to_string(),
                message: format!(
                    "person key '{first_name}' must identify one first name; put the surname in 'last'"
                ),
            });
        }
        if let Some(last_name) = person.last.as_deref() {
            validate_exact_identity(last_name, &format!("{path}.last"), "last name", issues);
        }
        if let Some(nametag) = person.nametag.as_deref() {
            validate_presentation_filename(nametag, &format!("{path}.nametag"), "nametag", issues);
        }
    }
}

fn validate_target_spec(target: &TargetSpec, path: &str, issues: &mut Vec<ConfigValidationIssue>) {
    match target {
        TargetSpec::ExistingFile { library_file } => {
            validate_library_filename(library_file, &format!("{path}.library_file"), issues);
        }
        TargetSpec::GeneratedName { name_template } => {
            validate_name_template(name_template, &format!("{path}.name_template"), issues);
        }
    }
}

fn validate_match_spec(
    match_spec: &MatchSpec,
    path: &str,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    if match_spec.is_empty() {
        issues.push(ConfigValidationIssue {
            path: path.to_string(),
            message: "match must contain at least one criterion".to_string(),
        });
    }
    for (field, values) in [
        ("title_prefix", &match_spec.title_prefix),
        ("title_contains", &match_spec.title_contains),
        ("description_contains", &match_spec.description_contains),
        ("service_type", &match_spec.service_type),
    ] {
        validate_identity_values(values, &format!("{path}.{field}"), "match value", issues);
    }
}

fn validate_decision(
    config: &RawProjectConfig,
    rule_idx: usize,
    decision: &DecisionConfig,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    match decision {
        DecisionConfig::ChooseExistingFile {
            context_fields,
            choices,
            ..
        } => {
            let mut seen_context_fields = HashSet::new();
            for (field_idx, field) in context_fields.iter().enumerate() {
                if !seen_context_fields.insert(*field) {
                    issues.push(ConfigValidationIssue {
                        path: format!(
                            "item_rules[{rule_idx}].decision.context_fields[{field_idx}]"
                        ),
                        message: format!("duplicate context field '{}'", field.as_str()),
                    });
                }
            }
            if choices.is_empty() {
                issues.push(ConfigValidationIssue {
                    path: format!("item_rules[{rule_idx}].decision.choices"),
                    message: "decision has no choices".to_string(),
                });
            }
            validate_map_keys(
                choices.keys(),
                &format!("item_rules[{rule_idx}].decision.choices"),
                "decision choice key",
                issues,
            );
            for (choice_key, choice) in choices {
                let choice_path = format!("item_rules[{rule_idx}].decision.choices.{choice_key}");
                if let Some(type_key) = choice.use_type.as_deref() {
                    validate_presentation_type_reference(
                        config,
                        type_key,
                        &format!("{choice_path}.use_type"),
                        issues,
                    );
                }
                validate_decision_target(choice, &choice_path, issues);
                if choice.match_spec.any.is_empty()
                    && choice.match_spec.all.is_empty()
                    && choice.match_spec.none.is_empty()
                {
                    issues.push(ConfigValidationIssue {
                        path: format!("{choice_path}.match"),
                        message: "decision choice match must contain at least one criterion"
                            .to_string(),
                    });
                }
                validate_decision_phrases(&choice.match_spec, &choice_path, issues);
            }
        }
    }
}

fn validate_presentation_type_reference(
    config: &RawProjectConfig,
    type_key: &str,
    path: &str,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    if !config.presentation_types.contains_key(type_key) {
        issues.push(ConfigValidationIssue {
            path: path.to_string(),
            message: format!("references unknown presentation type '{type_key}'"),
        });
    }
}

fn validate_decision_target(
    choice: &DecisionChoiceConfig,
    choice_path: &str,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    if let Some(file) = &choice.file {
        validate_library_filename(file, &format!("{choice_path}.file"), issues);
    }
    if let Some(target) = &choice.target {
        validate_target_spec(target, &format!("{choice_path}.target"), issues);
    }
}

fn validate_decision_phrases(
    match_spec: &DecisionChoiceMatch,
    choice_path: &str,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    let path = format!("{choice_path}.match");
    let mut criteria_by_phrase = HashMap::new();
    for (criterion, phrases) in [
        ("any", &match_spec.any),
        ("all", &match_spec.all),
        ("none", &match_spec.none),
    ] {
        for (index, phrase) in phrases.iter().enumerate() {
            let phrase_path = format!("{path}.{criterion}[{index}]");
            // Edge spaces are intentionally meaningful here: the current
            // substring matcher uses them as crude word boundaries. They must
            // remain allowed until that matcher owns explicit token semantics.
            if phrase.trim().is_empty() {
                issues.push(ConfigValidationIssue {
                    path: phrase_path,
                    message: "decision match phrase must not be blank".to_string(),
                });
                continue;
            }
            if phrase.chars().any(char::is_control) {
                issues.push(ConfigValidationIssue {
                    path: phrase_path,
                    message: "decision match phrase must not contain control characters"
                        .to_string(),
                });
                continue;
            }
            let canonical = phrase.to_lowercase().replace(['\u{2018}', '\u{2019}'], "'");
            if let Some(previous) = criteria_by_phrase.insert(canonical, criterion) {
                let message = if previous == criterion {
                    format!("duplicate decision match phrase in '{criterion}'")
                } else {
                    format!("decision match phrase appears in both '{previous}' and '{criterion}'")
                };
                issues.push(ConfigValidationIssue {
                    path: phrase_path,
                    message,
                });
            }
        }
    }
}
