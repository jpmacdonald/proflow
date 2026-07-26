//! Project-level service build configuration.
//!
//! This module owns the config contract for headless runtime behavior.
//! Only the v4 schema is supported.

use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

mod library_identity;
mod model;
mod runtime;
mod storage;
mod validation;

pub use library_identity::{LibraryIdentityConfig, LibraryIdentityMatch};
pub use model::{
    AmbiguousDecisionPolicy, BackgroundAssetPath, BackgroundId, ContentSourceKind, CueRoleConfig,
    DecisionChoiceConfig, DecisionChoiceMatch, DecisionConfig, DecisionContextField,
    DescriptionParserKind, DisplayBindingConfig, ExpansionRule, ExpansionStep, ItemKind,
    ItemRuleConfig, ItemRuleOutcome, LibraryName, MatchCategory, MatchSpec, OutputStrategy,
    OverrideRuleConfig, OverrideWhen, PersonConfig, PresentationTypeConfig, ProjectDefaults,
    ProjectMetadata, RawProjectConfig, RequiredPlaylistItemConfig, RequiredPlaylistPlacement,
    RestyleMacroConfig, RestyleMacroRegionConfig, RestyleMacroSelectorConfig, RgbColor, RuleAction,
    RuleTier, ServiceGroupConfig, SpeakerColorConfig, SpeakerSource, TargetSpec,
};
pub use runtime::ResolvedBackground;
pub use storage::{
    load_project_config, parse_project_config_str, parse_project_config_value,
    serialize_project_config, write_project_config, ProjectConfigLoadError,
};
pub use validation::{validate_project_config, ConfigValidationIssue};

pub(crate) use runtime::{
    BackgroundTransform, ClassificationTier, CompiledClassification, CompiledDecision,
    CompiledDirectTarget, CompiledExpansionStep, CompiledRequiredPlaylistItem, CompiledRuleOutcome,
    CompiledSpeakerTarget, CueTransform, ExistingSource, ExistingTransform, ItemMatchInput,
    MacroTransform, PresentationPolicy, RenderRole, RenderStyle, ResolvedPresentationType,
    ResolvedRequiredPresentation, RestyleMacroPolicy, RestyleMacroSelector, ReviewPolicy,
};
#[cfg(test)]
pub(crate) use runtime::{
    CueMacro, IdentifierProblem, RenderPlanError, RestyleMacroRegion, SpeakerPalette,
};

/// Validated project configuration accepted by runtime planning.
///
/// The raw value is private and this type deliberately has no `Deserialize`,
/// `Default`, or mutable access. As a result, a caller cannot pass a forged or
/// subsequently-invalidated config to `classify::build_plan`.
#[derive(Debug, Clone)]
pub struct ProjectConfig {
    raw: RawProjectConfig,
    plan_lookahead_days: crate::planning_center::PlanLookaheadDays,
    presentation_policies: BTreeMap<String, std::sync::Arc<PresentationPolicy>>,
    classifications: Vec<CompiledClassification>,
    required_playlist_items: Vec<CompiledRequiredPlaylistItem>,
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

    /// Checked Planning Center lookup window compiled from project defaults.
    #[must_use]
    pub const fn plan_lookahead_days(&self) -> crate::planning_center::PlanLookaheadDays {
        self.plan_lookahead_days
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

    /// Exact installed macro names that any compiled production policy can apply.
    ///
    /// This is the canonical inventory for native-asset validation. It covers
    /// both macro-bearing config surfaces: generated cue roles and
    /// existing-presentation macro regions.
    pub(crate) fn referenced_macro_names(&self) -> BTreeSet<&str> {
        let cue_role_names = self
            .raw
            .cue_roles
            .values()
            .flat_map(|role| {
                [
                    role.enter_macro.as_deref(),
                    role.leader_enter_macro.as_deref(),
                ]
            })
            .flatten();
        let restyle_names = self
            .raw
            .presentation_types
            .values()
            .filter_map(|presentation| presentation.macro_transitions.as_ref())
            .flat_map(|transitions| {
                transitions
                    .regions
                    .iter()
                    .map(|region| region.enter_macro.as_str())
            });
        cue_role_names.chain(restyle_names).collect()
    }

    /// Return compiled presentation-type keys in deterministic order.
    ///
    /// This is the same policy set referenced by compiled item rules, so config
    /// inspection never recompiles or interprets the wire model a second time.
    pub fn presentation_type_keys(&self) -> impl Iterator<Item = &str> {
        self.presentation_policies.keys().map(String::as_str)
    }

    pub(crate) fn compiled_classifications(&self) -> &[CompiledClassification] {
        &self.classifications
    }

    pub(crate) fn compiled_required_playlist_items(&self) -> &[CompiledRequiredPlaylistItem] {
        &self.required_playlist_items
    }

    pub(crate) const fn people(&self) -> &BTreeMap<String, PersonConfig> {
        &self.raw.people
    }

    /// Return the compiled presentation policy used by runtime planning.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn presentation_policy(&self, key: &str) -> Option<PresentationPolicy> {
        self.presentation_policies
            .get(key)
            .map(|policy| policy.as_ref().clone())
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
        let (plan_lookahead_days, mut issues) = match raw.defaults.days_ahead.map_or_else(
            || Ok(crate::planning_center::PlanLookaheadDays::DEFAULT),
            crate::planning_center::PlanLookaheadDays::new,
        ) {
            Ok(value) => (value, Vec::new()),
            Err(error) => (
                crate::planning_center::PlanLookaheadDays::DEFAULT,
                vec![validation::ConfigValidationIssue {
                    path: "defaults.days_ahead".to_string(),
                    message: error.to_string(),
                }],
            ),
        };
        issues.extend(validation::validate_project_config_structure(&raw));
        if !issues.is_empty() {
            return Err(ProjectConfigValidationError { issues });
        }

        let (render_roles, issues) = runtime::compile_render_roles(&raw);
        if !issues.is_empty() {
            return Err(ProjectConfigValidationError { issues });
        }

        let (presentation_policies, issues) =
            runtime::compile_presentation_policies(&raw, &render_roles);
        if !issues.is_empty() {
            return Err(ProjectConfigValidationError { issues });
        }

        let mut issues = Vec::new();
        let classifications = match runtime::compile_classifications(&raw, &presentation_policies) {
            Ok(classifications) => classifications,
            Err(classification_issues) => {
                issues.extend(classification_issues);
                Vec::new()
            }
        };
        let required_playlist_items =
            match runtime::compile_required_playlist_items(&raw, &presentation_policies) {
                Ok(items) => items,
                Err(required_issues) => {
                    issues.extend(required_issues);
                    Vec::new()
                }
            };

        if !issues.is_empty() {
            return Err(ProjectConfigValidationError { issues });
        }
        Ok(Self {
            raw,
            plan_lookahead_days,
            presentation_policies,
            classifications,
            required_playlist_items,
        })
    }
}

#[cfg(test)]
mod tests;
