//! Project-level service build configuration.
//!
//! This module owns the config contract for headless runtime behavior.
//! Only the v4 schema is supported.

use serde::Serialize;
use std::collections::BTreeMap;

mod model;
mod runtime;
mod storage;
mod validation;

pub use model::{
    AmbiguousDecisionPolicy, BackgroundAssetPath, BackgroundId, ContentSourceKind, CueRoleConfig,
    DecisionChoiceConfig, DecisionChoiceMatch, DecisionConfig, DescriptionParserKind,
    DisplayBindingConfig, ExpansionRule, ExpansionStep, ItemKind, ItemRuleConfig, ItemRuleOutcome,
    LibraryName, MatchSpec, OutputStrategy, OverrideRuleConfig, OverrideWhen, PersonConfig,
    PresentationTypeConfig, ProjectDefaults, ProjectMetadata, RawProjectConfig,
    RequiredPlaylistItemConfig, RequiredPlaylistPlacement, RestyleMacroConfig,
    RestyleMacroRegionConfig, RestyleMacroSelectorConfig, RgbColor, RuleAction, ServiceGroupConfig,
    SpeakerColorConfig, SpeakerSource, TargetSpec,
};
pub use storage::{
    load_project_config, parse_project_config_str, parse_project_config_value,
    serialize_project_config, write_project_config, ProjectConfigLoadError,
};
pub use validation::{validate_project_config, ConfigValidationIssue};

pub(crate) use runtime::{ExistingSource, PresentationPolicy, ReviewPolicy};

/// Validated project configuration accepted by runtime planning.
///
/// The raw value is private and this type deliberately has no `Deserialize`,
/// `Default`, or mutable access. As a result, a caller cannot pass a forged or
/// subsequently-invalidated config to `classify::build_plan`.
#[derive(Debug, Clone)]
pub struct ProjectConfig {
    raw: RawProjectConfig,
    presentation_policies: BTreeMap<String, PresentationPolicy>,
}

impl ProjectConfig {
    /// Return the validated raw config for serialization or read-only tooling.
    #[must_use]
    pub const fn as_raw(&self) -> &RawProjectConfig {
        &self.raw
    }

    /// Consume this checked config and return its editable wire representation.
    #[must_use]
    pub fn into_raw(self) -> RawProjectConfig {
        self.raw
    }

    /// Runtime defaults that are independent of presentation-policy dispatch.
    #[must_use]
    pub const fn defaults(&self) -> &ProjectDefaults {
        &self.raw.defaults
    }

    /// Checked background assets available to operator overrides and setup.
    #[must_use]
    pub const fn backgrounds(&self) -> &BTreeMap<BackgroundId, BackgroundAssetPath> {
        &self.raw.backgrounds
    }

    /// Checked cue-role declarations used while loading native render assets.
    #[must_use]
    pub const fn cue_roles(&self) -> &BTreeMap<String, CueRoleConfig> {
        &self.raw.cue_roles
    }

    pub(crate) const fn service_groups(&self) -> &BTreeMap<String, ServiceGroupConfig> {
        &self.raw.service_groups
    }

    pub(crate) fn required_playlist_items(&self) -> &[RequiredPlaylistItemConfig] {
        &self.raw.required_playlist_items
    }

    pub(crate) fn item_rules(&self) -> &[ItemRuleConfig] {
        &self.raw.item_rules
    }

    pub(crate) const fn people(&self) -> &BTreeMap<String, PersonConfig> {
        &self.raw.people
    }

    /// Return the compiled presentation policy used by runtime planning.
    #[must_use]
    pub(crate) fn presentation_policy(&self, key: &str) -> Option<&PresentationPolicy> {
        self.presentation_policies.get(key)
    }
}

impl Serialize for ProjectConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.raw.serialize(serializer)
    }
}

/// Failure to compile an editable config into the checked runtime contract.
#[derive(Debug)]
pub struct ProjectConfigValidationError {
    issues: Vec<validation::ConfigValidationIssue>,
}

impl ProjectConfigValidationError {
    /// Return every violated config invariant.
    #[must_use]
    pub fn issues(&self) -> &[validation::ConfigValidationIssue] {
        &self.issues
    }
}

impl std::fmt::Display for ProjectConfigValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&validation::format_validation_issues(&self.issues))
    }
}

impl std::error::Error for ProjectConfigValidationError {}

impl TryFrom<RawProjectConfig> for ProjectConfig {
    type Error = ProjectConfigValidationError;

    fn try_from(raw: RawProjectConfig) -> Result<Self, Self::Error> {
        let issues = validation::validate_project_config(&raw);
        if !issues.is_empty() {
            return Err(ProjectConfigValidationError { issues });
        }
        let presentation_policies =
            runtime::compile_presentation_policies(&raw).map_err(|issue| {
                ProjectConfigValidationError {
                    issues: vec![issue],
                }
            })?;
        Ok(Self {
            raw,
            presentation_policies,
        })
    }
}

#[cfg(test)]
mod tests;
