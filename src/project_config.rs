//! Project-level service build configuration.
//!
//! This module owns the config contract for headless runtime behavior.
//! Only the v2 schema is supported. Legacy v1 configs must be migrated.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Project config — the v2 schema is the only supported shape.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectConfig {
    /// Schema version — must be 2.
    #[serde(default = "default_version")]
    pub version: u16,
    /// Optional descriptive metadata.
    #[serde(default)]
    pub metadata: ProjectMetadata,
    /// Runtime defaults shared by headless entrypoints.
    #[serde(default)]
    pub defaults: ProjectDefaults,
    /// Reusable service groups.
    #[serde(default)]
    pub service_groups: HashMap<String, ServiceGroupConfig>,
    /// Optional named build profiles.
    #[serde(default)]
    pub profiles: HashMap<String, ProfileConfig>,
    /// Named presentation types.
    #[serde(default)]
    pub presentation_types: HashMap<String, PresentationTypeConfig>,
    /// Ordered item rules.
    #[serde(default)]
    pub item_rules: Vec<ItemRuleConfig>,
    /// Known people metadata.
    #[serde(default)]
    pub people: HashMap<String, PersonConfig>,
    /// Structured override rules.
    #[serde(default)]
    pub overrides: Vec<OverrideRuleConfig>,
}

/// Descriptive project metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectMetadata {
    /// Human-readable project name.
    pub name: Option<String>,
    /// Default timezone identifier.
    pub timezone: Option<String>,
    /// Free-form notes.
    pub notes: Option<String>,
}

/// Project-wide defaults for runtime behavior.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectDefaults {
    /// Default theme name.
    pub theme: Option<String>,
    /// Default lookahead window for builds.
    pub days_ahead: Option<i64>,
    /// Default review policy.
    pub review_policy: Option<ReviewPolicy>,
    /// Optional sort mode for plans.
    pub plan_sort: Option<PlanSort>,
}

/// Named set of service types.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServiceGroupConfig {
    /// Service type names belonging to the group.
    #[serde(default)]
    pub service_types: Vec<String>,
}

/// Named build preset.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProfileConfig {
    /// Human-readable description.
    pub description: Option<String>,
    /// Referenced service groups.
    #[serde(default)]
    pub service_groups: Vec<String>,
    /// Directly listed service types.
    #[serde(default)]
    pub service_types: Vec<String>,
    /// Days-ahead override.
    pub days_ahead: Option<i64>,
    /// Review policy override.
    pub review_policy: Option<ReviewPolicy>,
}

/// Presentation type behavior.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PresentationTypeConfig {
    /// Conceptual kind of output.
    #[serde(default)]
    pub kind: ItemKind,
    /// Source of content data.
    #[serde(default)]
    pub content_source: ContentSourceKind,
    /// Output behavior for the content.
    #[serde(default)]
    pub output_strategy: OutputStrategy,
    /// Theme slide / template name.
    pub template: Option<String>,
    /// Optional separate title slide template.
    pub title_template: Option<String>,
    /// Background category.
    pub background: Option<String>,
    /// `ProPresenter` macro to trigger on the first slide.
    #[serde(rename = "macro")]
    pub macro_name: Option<String>,
    /// `ProPresenter` macro to trigger on content slides (after the title).
    pub content_macro: Option<String>,
    /// Arrangement override.
    pub arrangement: Option<String>,
    /// Human-readable description.
    pub description: Option<String>,
}

/// Ordered matching rule for items.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ItemRuleConfig {
    /// Stable rule identifier.
    pub id: String,
    /// Match criteria.
    #[serde(rename = "match", default)]
    pub match_spec: MatchSpec,
    /// Presentation type to use when matched.
    pub use_type: Option<String>,
    /// Explicit action when matched.
    pub action: Option<RuleAction>,
    /// Expansion steps to produce multiple outputs.
    #[serde(default)]
    pub expand: Vec<ExpansionStep>,
    /// Optional target information.
    pub target: Option<TargetSpec>,
    /// Free-form notes.
    pub notes: Option<String>,
}

/// Match criteria for a rule.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MatchSpec {
    /// Lowercased title prefixes.
    #[serde(default)]
    pub title_prefix: Vec<String>,
    /// Title substrings.
    #[serde(default)]
    pub title_contains: Vec<String>,
    /// Optional category string.
    pub category: Option<String>,
    /// Whether the item contains a scripture reference.
    pub has_scripture_ref: Option<bool>,
    /// Restrict the rule to specific service types.
    #[serde(default)]
    pub service_type: Vec<String>,
}

/// Explicit rule action.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuleAction {
    /// Skip the item entirely.
    Skip {
        /// Human-readable reason for skipping the item.
        reason: String,
    },
    /// Mark the item as requiring review.
    Review {
        /// Human-readable reason the item needs review.
        reason: String,
    },
}

/// Expansion step used by multi-output rules.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExpansionStep {
    /// Presentation type to use for this step.
    pub use_type: String,
    /// Optional speaker source.
    pub speaker: Option<SpeakerSource>,
    /// Optional explicit target for this step.
    pub target: Option<TargetSpec>,
}

/// How a speaker value should be resolved.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeakerSource {
    /// Use the resolved speaker for the matched item.
    Resolved,
}

/// Output target hints.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TargetSpec {
    /// Explicit library file to read/write.
    pub library_file: Option<String>,
    /// Optional dynamic name template for generated files.
    pub name_template: Option<String>,
}

/// Known person metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PersonConfig {
    /// Last name.
    pub last: Option<String>,
    /// Role label.
    pub role: Option<String>,
    /// Preferred nametag filename.
    pub nametag: Option<String>,
}

/// Structured override rule.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OverrideRuleConfig {
    /// When this override applies.
    pub when: OverrideWhen,
    /// Arrangement override.
    pub arrangement: Option<String>,
    /// Background override.
    pub background: Option<String>,
    /// Template override.
    pub template: Option<String>,
}

/// Conditions under which an override applies.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OverrideWhen {
    /// Named service group.
    pub service_group: Option<String>,
    /// Exact service type.
    pub service_type: Option<String>,
    /// Presentation type key.
    pub presentation_type: Option<String>,
}

/// Review policy for uncertain items.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewPolicy {
    /// Ask for confirmation before proceeding.
    #[default]
    Ask,
    /// Fail the build.
    Fail,
    /// Skip uncertain items automatically.
    Skip,
}

/// Where content data comes from.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContentSourceKind {
    /// Reuse static content from an existing file.
    #[default]
    Static,
    /// Parse content from a Planning Center description.
    Description,
    /// Generate from Bible data.
    Scripture,
    /// Use linked song metadata / existing library content.
    Song,
}

/// What the runtime should do with the resolved content.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutputStrategy {
    /// Skip this item.
    Skip,
    /// Use an existing file unchanged.
    UseExisting,
    /// Overwrite an existing file in place.
    EditInPlace,
    /// Generate a new file.
    GenerateNew,
    /// Require review before choosing behavior.
    #[default]
    NeedsReview,
}

/// Conceptual kind of item/presentation.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    /// Song/lyrics item.
    Song,
    /// Scripture item.
    Scripture,
    /// Liturgical text.
    Liturgy,
    /// Speaker or content nametag.
    Nametag,
    /// Announcement/graphic item.
    Announcement,
    /// Graphic-only item.
    Graphic,
    /// Fallback kind.
    #[default]
    Other,
}

/// Plan ordering hint.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanSort {
    /// Earliest plans first.
    #[default]
    AscendingDate,
    /// Latest plans first.
    DescendingDate,
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

/// Error returned while reading project config.
#[derive(Debug, thiserror::Error)]
pub enum ProjectConfigLoadError {
    /// Failed to read the config file.
    #[error("failed to read project config: {0}")]
    Io(#[from] std::io::Error),
    /// Failed to parse the config file.
    #[error("failed to parse project config: {0}")]
    Parse(#[from] serde_json::Error),
    /// Encountered an unsupported or missing config version.
    #[error("unsupported project config version: {0} — migrate to v2")]
    UnsupportedVersion(u64),
    /// Config is missing a version field entirely.
    #[error("config has no version field — migrate to v2")]
    MissingVersion,
}

/// Load project config from a file path.
pub fn load_project_config(path: &Path) -> Result<ProjectConfig, ProjectConfigLoadError> {
    let text = std::fs::read_to_string(path)?;
    parse_project_config_str(&text)
}

/// Parse project config from a JSON value.
pub fn parse_project_config_value(
    value: serde_json::Value,
) -> Result<ProjectConfig, ProjectConfigLoadError> {
    match value.get("version").and_then(serde_json::Value::as_u64) {
        Some(2) => Ok(serde_json::from_value(value)?),
        Some(version) => Err(ProjectConfigLoadError::UnsupportedVersion(version)),
        None => Err(ProjectConfigLoadError::MissingVersion),
    }
}

/// Parse project config from a JSON string.
pub fn parse_project_config_str(json: &str) -> Result<ProjectConfig, ProjectConfigLoadError> {
    let value: serde_json::Value = serde_json::from_str(json)?;
    parse_project_config_value(value)
}

/// Serialize project config to pretty JSON with a trailing newline.
pub fn serialize_project_config(config: &ProjectConfig) -> Result<String, serde_json::Error> {
    let mut json = serde_json::to_string_pretty(config)?;
    json.push('\n');
    Ok(json)
}

/// Write project config atomically to disk.
pub fn write_project_config(path: &Path, config: &ProjectConfig) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;

    let json = serialize_project_config(config)
        .map_err(|err| std::io::Error::other(format!("serialize project config: {err}")))?;
    let temp_path = path.with_extension("json.tmp");
    std::fs::write(&temp_path, json)?;
    std::fs::rename(temp_path, path)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// A validation issue found in the loaded project config.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ConfigValidationIssue {
    /// Approximate config path where the issue was detected.
    pub path: String,
    /// Human-readable validation message.
    pub message: String,
}

/// Validate project config references and report issues.
pub(crate) fn validate_project_config(config: &ProjectConfig) -> Vec<ConfigValidationIssue> {
    let mut issues = Vec::new();

    for (profile_name, profile) in &config.profiles {
        for group in &profile.service_groups {
            if !config.service_groups.contains_key(group) {
                issues.push(ConfigValidationIssue {
                    path: format!("profiles.{profile_name}.service_groups"),
                    message: format!("references unknown service group '{group}'"),
                });
            }
        }
    }

    for (idx, rule) in config.item_rules.iter().enumerate() {
        if let Some(type_key) = &rule.use_type {
            if !config.presentation_types.contains_key(type_key) {
                issues.push(ConfigValidationIssue {
                    path: format!("item_rules[{idx}].use_type"),
                    message: format!("references unknown presentation type '{type_key}'"),
                });
            }
        }

        for (step_idx, step) in rule.expand.iter().enumerate() {
            if !config.presentation_types.contains_key(&step.use_type) {
                issues.push(ConfigValidationIssue {
                    path: format!("item_rules[{idx}].expand[{step_idx}].use_type"),
                    message: format!("references unknown presentation type '{}'", step.use_type),
                });
            }
        }
    }

    for (idx, override_rule) in config.overrides.iter().enumerate() {
        if let Some(type_key) = &override_rule.when.presentation_type {
            if !config.presentation_types.contains_key(type_key) {
                issues.push(ConfigValidationIssue {
                    path: format!("overrides[{idx}].when.presentation_type"),
                    message: format!("references unknown presentation type '{type_key}'"),
                });
            }
        }

        if let Some(group) = &override_rule.when.service_group {
            if !config.service_groups.contains_key(group) {
                issues.push(ConfigValidationIssue {
                    path: format!("overrides[{idx}].when.service_group"),
                    message: format!("references unknown service group '{group}'"),
                });
            }
        }
    }

    issues
}

const fn default_version() -> u16 {
    2
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parse_v2_config() {
        let json = r#"
        {
          "version": 2,
          "defaults": {
            "theme": "V2 Theme",
            "review_policy": "ask"
          },
          "service_groups": {
            "seasonal": {
              "service_types": ["Christmas Eve"]
            }
          },
          "profiles": {
            "seasonal": {
              "service_groups": ["seasonal"],
              "days_ahead": 60,
              "review_policy": "ask"
            }
          },
          "presentation_types": {
            "liturgical_weekly": {
              "kind": "liturgy",
              "content_source": "description",
              "output_strategy": "edit_in_place",
              "template": "Responsive",
              "background": "default",
              "macro": "Scripture/Prayer",
              "description": "Weekly liturgy"
            },
            "person_nametag": {
              "kind": "nametag",
              "content_source": "static",
              "output_strategy": "use_existing",
              "template": "Name Tag",
              "macro": "Name Tag/Title"
            }
          },
          "item_rules": [
            {
              "id": "call_to_worship",
              "match": {
                "title_prefix": ["call to worship"]
              },
              "use_type": "liturgical_weekly",
              "target": {
                "library_file": "Call to Worship.pro"
              }
            },
            {
              "id": "welcome_bundle",
              "match": {
                "title_prefix": ["welcome"]
              },
              "expand": [
                {
                  "use_type": "person_nametag",
                  "speaker": "resolved"
                },
                {
                  "use_type": "liturgical_weekly"
                }
              ]
            }
          ],
          "people": {
            "Robert": {
              "last": "Austell",
              "role": "pastor",
              "nametag": "Robert Nametag"
            }
          }
        }
        "#;

        let config = parse_project_config_str(json).expect("v2 config should parse");
        assert_eq!(config.version, 2);
        assert_eq!(config.defaults.theme.as_deref(), Some("V2 Theme"));
        assert!(config.profiles.contains_key("seasonal"));
        assert!(config.presentation_types.contains_key("liturgical_weekly"));
        assert!(config.people.contains_key("Robert"));
        assert_eq!(config.item_rules.len(), 2);
        assert_eq!(config.item_rules[0].id, "call_to_worship");
        assert_eq!(
            config.item_rules[0]
                .target
                .as_ref()
                .and_then(|t| t.library_file.as_deref()),
            Some("Call to Worship.pro")
        );
    }

    #[test]
    fn reject_legacy_config() {
        let json = r#"{ "theme": "Legacy", "item_types": {} }"#;
        let err = parse_project_config_str(json).unwrap_err();
        assert!(
            matches!(err, ProjectConfigLoadError::MissingVersion),
            "expected MissingVersion, got: {err}"
        );
    }

    #[test]
    fn reject_v1_config() {
        let json = r#"{ "version": 1, "theme": "Legacy" }"#;
        let err = parse_project_config_str(json).unwrap_err();
        assert!(
            matches!(err, ProjectConfigLoadError::UnsupportedVersion(1)),
            "expected UnsupportedVersion(1), got: {err}"
        );
    }

    #[test]
    fn validate_project_config_reports_unknown_refs() {
        let mut config = ProjectConfig::default();
        config.profiles.insert(
            "weekly".to_string(),
            ProfileConfig {
                service_groups: vec!["missing_group".to_string()],
                ..ProfileConfig::default()
            },
        );
        config.item_rules.push(ItemRuleConfig {
            id: "bad_rule".to_string(),
            use_type: Some("missing_type".to_string()),
            ..ItemRuleConfig::default()
        });
        config.overrides.push(OverrideRuleConfig {
            when: OverrideWhen {
                service_group: Some("missing_group".to_string()),
                presentation_type: Some("missing_type".to_string()),
                ..OverrideWhen::default()
            },
            ..OverrideRuleConfig::default()
        });

        let issues = validate_project_config(&config);
        assert_eq!(issues.len(), 4);
    }

    #[test]
    fn repo_project_config_is_v2_and_valid() {
        let config = parse_project_config_str(include_str!("../data/proflow.config.json"))
            .expect("repo config should parse");

        assert_eq!(config.version, 2);
        assert!(validate_project_config(&config).is_empty());
    }

    #[test]
    fn parse_project_config_value_accepts_v2() {
        let value = serde_json::json!({
            "version": 2,
            "metadata": {
                "name": "Example"
            }
        });

        let config = parse_project_config_value(value).expect("v2 config should parse");
        assert_eq!(config.metadata.name.as_deref(), Some("Example"));
    }

    #[test]
    fn write_project_config_round_trips() {
        let dir = tempdir().expect("tempdir should create");
        let path = dir.path().join("proflow.config.json");
        let mut config = ProjectConfig {
            version: 2,
            ..ProjectConfig::default()
        };
        config.metadata.name = Some("Round Trip".to_string());

        write_project_config(&path, &config).expect("config should write");
        let loaded = load_project_config(&path).expect("config should reload");

        assert_eq!(loaded.metadata.name.as_deref(), Some("Round Trip"));
    }
}
