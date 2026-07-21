//! Cross-reference and semantic validation for project config values.

mod identity;
mod overrides;
mod presentation;
mod rules;

use super::{ProjectConfig, RawProjectConfig};
use serde::Serialize;
use std::collections::HashSet;

type ServiceTypeScope = Option<HashSet<String>>;

/// Compile one service group into the case-insensitive service names it can
/// match. An unknown or invalid empty group becomes an empty, non-matching
/// scope while its own validation reports why.
fn service_group_scope(config: &RawProjectConfig, group: &str) -> HashSet<String> {
    config
        .service_groups
        .get(group)
        .into_iter()
        .flat_map(|group| &group.service_types)
        .map(|service_type| service_type.to_ascii_lowercase())
        .collect()
}

fn service_scopes_overlap(first: &ServiceTypeScope, second: &ServiceTypeScope) -> bool {
    match (first, second) {
        (None, None) => true,
        (Some(scope), None) | (None, Some(scope)) => !scope.is_empty(),
        (Some(first), Some(second)) => first.iter().any(|value| second.contains(value)),
    }
}

/// A validation issue found in the loaded project config.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigValidationIssue {
    /// Approximate config path where the issue was detected.
    pub path: String,
    /// Human-readable validation message.
    pub message: String,
}

/// Validate an editable config candidate before it enters runtime planning.
pub fn validate_project_config(config: &RawProjectConfig) -> Vec<ConfigValidationIssue> {
    ProjectConfig::try_from(config.clone())
        .err()
        .map_or_else(Vec::new, |error| error.issues().to_vec())
}

pub(super) fn validate_project_config_structure(
    config: &RawProjectConfig,
) -> Vec<ConfigValidationIssue> {
    let mut issues = Vec::new();

    if config.version != 4 {
        issues.push(ConfigValidationIssue {
            path: "version".to_string(),
            message: format!("unsupported version {}; expected 4", config.version),
        });
    }
    presentation::validate_runtime_defaults(config, &mut issues);
    rules::validate_service_groups(config, &mut issues);
    presentation::validate_background_references(config, &mut issues);
    presentation::validate_cue_roles(config, &mut issues);
    rules::validate_item_rules(config, &mut issues);
    rules::validate_required_playlist_items(config, &mut issues);
    presentation::validate_presentation_types(config, &mut issues);
    rules::validate_people(config, &mut issues);
    overrides::validate_overrides(config, &mut issues);

    issues
}

pub(super) fn format_validation_issues(issues: &[ConfigValidationIssue]) -> String {
    issues
        .iter()
        .map(|issue| format!("{}: {}", issue.path, issue.message))
        .collect::<Vec<_>>()
        .join("; ")
}
