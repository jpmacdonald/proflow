//! Validation for scoped background and arrangement overrides.

use super::{
    identity::validate_exact_identity, service_group_scope, service_scopes_overlap,
    ConfigValidationIssue, ServiceTypeScope,
};
use crate::project_config::{OutputStrategy, OverrideWhen, RawProjectConfig};
use std::collections::HashSet;

pub(super) fn validate_overrides(
    config: &RawProjectConfig,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    for (idx, override_rule) in config.overrides.iter().enumerate() {
        if let Some(service_type) = override_rule.when.service_type.as_deref() {
            validate_exact_identity(
                service_type,
                &format!("overrides[{idx}].when.service_type"),
                "service type",
                issues,
            );
        }
        if let Some(arrangement) = override_rule.arrangement.as_deref() {
            validate_exact_identity(
                arrangement,
                &format!("overrides[{idx}].arrangement"),
                "arrangement",
                issues,
            );
        }
        if let Some(type_key) = &override_rule.when.presentation_type {
            match config.presentation_types.get(type_key) {
                None => issues.push(ConfigValidationIssue {
                    path: format!("overrides[{idx}].when.presentation_type"),
                    message: format!("references unknown presentation type '{type_key}'"),
                }),
                Some(ptype) => {
                    let read_only =
                        matches!(ptype.output_strategy, OutputStrategy::PreserveExisting);
                    let existing_source = matches!(
                        ptype.output_strategy,
                        OutputStrategy::PreserveExisting | OutputStrategy::RestyleExisting
                    );
                    if override_rule.arrangement.is_some() && !existing_source {
                        issues.push(ConfigValidationIssue {
                            path: format!("overrides[{idx}].arrangement"),
                            message: format!(
                                "arrangement cannot target non-preserve_existing/non-restyle_existing presentation type '{type_key}'"
                            ),
                        });
                    }
                    if override_rule.background.is_some() && read_only {
                        issues.push(ConfigValidationIssue {
                            path: format!("overrides[{idx}].background"),
                            message: format!(
                                "background cannot target preserve_existing presentation type '{type_key}' because exempt files are unchanged"
                            ),
                        });
                    }
                }
            }
        }

        if let Some(group) = &override_rule.when.service_group {
            match config.service_groups.get(group) {
                None => issues.push(ConfigValidationIssue {
                    path: format!("overrides[{idx}].when.service_group"),
                    message: format!("references unknown service group '{group}'"),
                }),
                Some(service_group) => {
                    if let Some(service_type) = override_rule.when.service_type.as_deref() {
                        if !service_group
                            .service_types
                            .iter()
                            .any(|candidate| candidate.eq_ignore_ascii_case(service_type))
                        {
                            issues.push(ConfigValidationIssue {
                                path: format!("overrides[{idx}].when"),
                                message: format!(
                                    "service type '{service_type}' is not a member of service group '{group}'"
                                ),
                            });
                        }
                    }
                }
            }
        }
        if let Some(background) = &override_rule.background {
            if !config.backgrounds.contains_key(background) {
                issues.push(ConfigValidationIssue {
                    path: format!("overrides[{idx}].background"),
                    message: format!("references unknown background '{background}'"),
                });
            }
        }
    }
    validate_override_conflicts(config, issues);
}

fn validate_override_conflicts(config: &RawProjectConfig, issues: &mut Vec<ConfigValidationIssue>) {
    for (later_index, later) in config.overrides.iter().enumerate() {
        for (earlier_index, earlier) in config.overrides[..later_index].iter().enumerate() {
            if !override_conditions_overlap(config, &earlier.when, &later.when) {
                continue;
            }
            if earlier
                .background
                .as_ref()
                .zip(later.background.as_ref())
                .is_some_and(|(first, second)| first != second)
            {
                issues.push(ConfigValidationIssue {
                    path: format!("overrides[{later_index}].background"),
                    message: format!(
                        "conflicts with overrides[{earlier_index}].background; both rules can match the same plan item"
                    ),
                });
            }
            if earlier
                .arrangement
                .as_ref()
                .zip(later.arrangement.as_ref())
                .is_some_and(|(first, second)| first != second)
            {
                issues.push(ConfigValidationIssue {
                    path: format!("overrides[{later_index}].arrangement"),
                    message: format!(
                        "conflicts with overrides[{earlier_index}].arrangement; both rules can match the same plan item"
                    ),
                });
            }
        }
    }
}

fn override_conditions_overlap(
    config: &RawProjectConfig,
    first: &OverrideWhen,
    second: &OverrideWhen,
) -> bool {
    let presentation_types_overlap = first
        .presentation_type
        .as_ref()
        .zip(second.presentation_type.as_ref())
        .is_none_or(|(first, second)| first == second);
    let first_scope = override_service_scope(config, first);
    let second_scope = override_service_scope(config, second);
    presentation_types_overlap && service_scopes_overlap(&first_scope, &second_scope)
}

fn override_service_scope(config: &RawProjectConfig, condition: &OverrideWhen) -> ServiceTypeScope {
    let mut scope = condition
        .service_group
        .as_deref()
        .map(|group| service_group_scope(config, group));
    if let Some(service_type) = condition.service_type.as_deref() {
        let service_type = service_type.to_ascii_lowercase();
        match &mut scope {
            Some(scope) => scope.retain(|candidate| candidate == &service_type),
            None => scope = Some(HashSet::from([service_type])),
        }
    }
    scope
}
