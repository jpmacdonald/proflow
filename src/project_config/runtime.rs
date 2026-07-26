//! Compiled runtime presentation policy.
//!
//! The JSON contract deliberately remains a convenient editable structure.
//! This module performs its one semantic translation into variants that cannot
//! express unsupported source/output combinations.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

mod classification;
mod existing;
mod render;

pub use classification::{
    compile_classifications, compile_required_playlist_items, ClassificationTier,
    CompiledClassification, CompiledDecision, CompiledDirectTarget, CompiledExpansionStep,
    CompiledRequiredPlaylistItem, CompiledRuleOutcome, CompiledSpeakerTarget, ItemMatchInput,
    ResolvedPresentationType, ResolvedRequiredPresentation,
};
pub use existing::{BackgroundTransform, CueTransform, ExistingTransform, MacroTransform};
#[cfg(test)]
pub use render::{
    CueMacro, IdentifierProblem, RenderPlanError, RestyleMacroRegion, SpeakerPalette,
};
#[cfg(not(test))]
use render::{CueMacro, RestyleMacroRegion, SpeakerPalette};
pub use render::{
    RenderRole, RenderStyle, ResolvedBackground, RestyleMacroPolicy, RestyleMacroSelector,
};

use super::{
    ConfigValidationIssue, ContentSourceKind, DescriptionParserKind, DisplayBindingConfig,
    ItemKind, OutputStrategy, OverrideWhen, PresentationTypeConfig, RawProjectConfig,
    RestyleMacroSelectorConfig,
};

/// One checked presentation behavior used by classification.
#[derive(Debug, Clone)]
pub enum PresentationPolicy {
    /// Always omit a matching item.
    Skip { kind: ItemKind },
    /// Preserve an explicit human decision boundary.
    Review(ReviewPolicy),
    /// Preserve one explicitly exempt native presentation without modifying it.
    PreserveExisting {
        kind: ItemKind,
        source: ExistingSource,
        arrangement: ArrangementPolicy,
    },
    /// Reuse native content and structure while enforcing the reviewed background.
    RestyleExisting {
        kind: ItemKind,
        source: ExistingSource,
        arrangement: ArrangementPolicy,
        transform: ExistingTransformPolicy,
    },
    /// Parse description content and replace an existing presentation.
    EditDescription {
        kind: ItemKind,
        parser: DescriptionParserKind,
        render: RenderPolicy,
    },
    /// Parse description content and create a presentation.
    GenerateDescription {
        kind: ItemKind,
        parser: DescriptionParserKind,
        render: RenderPolicy,
    },
    /// Fetch scripture content and create a presentation.
    GenerateScripture { render: RenderPolicy },
}

impl PresentationPolicy {
    /// Conceptual item kind reported to the operator.
    pub(crate) const fn kind(&self) -> ItemKind {
        match self {
            Self::Skip { kind }
            | Self::PreserveExisting { kind, .. }
            | Self::RestyleExisting { kind, .. }
            | Self::EditDescription { kind, .. }
            | Self::GenerateDescription { kind, .. } => *kind,
            Self::Review(review) => review.kind(),
            Self::GenerateScripture { .. } => ItemKind::Scripture,
        }
    }

    /// Whether a rule may select one exact existing library file for this policy.
    const fn accepts_library_file_target(&self) -> bool {
        matches!(
            self,
            Self::PreserveExisting { .. }
                | Self::RestyleExisting { .. }
                | Self::EditDescription { .. }
        )
    }

    /// Whether a contextual decision may choose an existing presentation.
    const fn accepts_existing_file_decision(&self) -> bool {
        matches!(
            self,
            Self::PreserveExisting { .. } | Self::RestyleExisting { .. }
        )
    }
}

/// Content family accepted by the use-existing operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExistingSource {
    Static,
    Song,
}

/// Source-specific context retained when configuration requires review.
#[derive(Debug, Clone)]
pub enum ReviewPolicy {
    Static {
        kind: ItemKind,
    },
    Description {
        kind: ItemKind,
        parser: DescriptionParserKind,
        render: Option<Box<RenderPolicy>>,
    },
    Scripture,
    Song,
}

impl ReviewPolicy {
    pub(crate) const fn kind(&self) -> ItemKind {
        match self {
            Self::Static { kind } | Self::Description { kind, .. } => *kind,
            Self::Scripture => ItemKind::Scripture,
            Self::Song => ItemKind::Song,
        }
    }
}

/// A complete checked style plus the only service-scoped substitutions it can
/// accept: alternative backgrounds.
#[derive(Debug, Clone)]
pub struct RenderPolicy {
    base: RenderStyle,
    overrides: Vec<ServiceRenderStyle>,
}

/// A checked native transform plus service-scoped background substitutions.
#[derive(Debug, Clone)]
pub struct ExistingTransformPolicy {
    base: ExistingTransform,
    background_overrides: Vec<ServiceBackground>,
}

impl ExistingTransformPolicy {
    /// Resolve the complete transform without consulting the raw configuration.
    pub(crate) fn for_service(&self, service_name: Option<&str>) -> ExistingTransform {
        self.background_overrides
            .iter()
            .rev()
            .find(|entry| entry.scope.matches(service_name))
            .map_or_else(
                || self.base.clone(),
                |entry| {
                    self.base
                        .clone()
                        .with_replacement_background(entry.background.clone())
                },
            )
    }
}

#[derive(Debug, Clone)]
struct ServiceBackground {
    scope: ServiceScope,
    background: ResolvedBackground,
}

impl RenderPolicy {
    /// Resolve a complete style without consulting raw config or revalidating
    /// cue-role relationships.
    pub(crate) fn for_service(&self, service_name: Option<&str>) -> RenderStyle {
        self.overrides
            .iter()
            .rev()
            .find(|entry| entry.scope.matches(service_name))
            .map_or_else(|| self.base.clone(), |entry| entry.style.clone())
    }
}

#[derive(Debug, Clone)]
struct ServiceRenderStyle {
    scope: ServiceScope,
    style: RenderStyle,
}

/// Base arrangement and scoped alternatives for an existing presentation.
#[derive(Debug, Clone)]
pub struct ArrangementPolicy {
    base: Option<String>,
    overrides: Vec<ServiceArrangement>,
}

impl ArrangementPolicy {
    pub(crate) fn for_service(&self, service_name: Option<&str>) -> Option<String> {
        self.overrides
            .iter()
            .rev()
            .find(|entry| entry.scope.matches(service_name))
            .map_or(self.base.as_ref(), |entry| Some(&entry.arrangement))
            .cloned()
    }
}

#[derive(Debug, Clone)]
struct ServiceArrangement {
    scope: ServiceScope,
    arrangement: String,
}

/// A precomputed service-name predicate. `None` means every service, including
/// planning calls where no service name is available.
#[derive(Debug, Clone)]
struct ServiceScope {
    service_types: Option<BTreeSet<String>>,
}

impl ServiceScope {
    fn matches(&self, service_name: Option<&str>) -> bool {
        match (&self.service_types, service_name) {
            (None, _) => true,
            (Some(service_types), Some(service_name)) => {
                service_types.contains(&service_name.to_ascii_lowercase())
            }
            (Some(_), None) => false,
        }
    }
}

/// Compile every cue-role declaration once, including roles not currently used
/// by a presentation. Presentation compilation consumes only these checked
/// values and never reinterprets the raw role contract.
pub(super) fn compile_render_roles(
    config: &RawProjectConfig,
) -> (BTreeMap<String, RenderRole>, Vec<ConfigValidationIssue>) {
    let mut roles = BTreeMap::new();
    let mut issues = Vec::new();
    for (key, role) in &config.cue_roles {
        match compile_render_role(key, role) {
            Ok(role) => {
                roles.insert(key.clone(), role);
            }
            Err(issue) => issues.push(issue),
        }
    }
    (roles, issues)
}

fn compile_render_role(
    key: &str,
    role: &super::CueRoleConfig,
) -> Result<RenderRole, ConfigValidationIssue> {
    let cue_macro = match (&role.enter_macro, &role.leader_enter_macro) {
        (None, Some(_)) => {
            return Err(ConfigValidationIssue {
                path: format!("cue_roles.{key}.leader_enter_macro"),
                message: "leader_enter_macro requires enter_macro".to_string(),
            });
        }
        (Some(enter), leader_enter) => Some(
            CueMacro::new(enter.clone(), leader_enter.clone()).map_err(|error| {
                ConfigValidationIssue {
                    path: format!("cue_roles.{key}.enter_macro"),
                    message: error.to_string(),
                }
            })?,
        ),
        (None, None) => None,
    };
    let speaker_palette = role.speaker_colors.map(|colors| {
        SpeakerPalette::new(colors.leader.components(), colors.audience.components())
    });
    RenderRole::new(
        key.to_string(),
        role.slide.clone(),
        role.text_slots.clone(),
        cue_macro,
        speaker_palette,
    )
    .map_err(|error| ConfigValidationIssue {
        path: format!("cue_roles.{key}"),
        message: error.to_string(),
    })
}

/// Semantic presentation-shape checks owned by runtime compilation.
///
/// Lexical identities and global references are checked before this phase. The
/// checks here correspond exactly to the variants that `compile_policy` can
/// construct, so validation cannot drift into a second policy matrix.
fn presentation_policy_issues(
    key: &str,
    wire: &PresentationTypeConfig,
) -> Vec<ConfigValidationIssue> {
    let mut issues = Vec::new();
    let issue = |field: &str, message: String| ConfigValidationIssue {
        path: format!("presentation_types.{key}.{field}"),
        message,
    };

    match (wire.content_source, wire.description_parser) {
        (ContentSourceKind::Description, _) | (_, None) => {}
        (_, Some(_)) => issues.push(issue(
            "description_parser",
            "description_parser is only valid for description content".to_string(),
        )),
    }

    if wire.content_source == ContentSourceKind::Song && wire.kind != ItemKind::Song {
        issues.push(issue(
            "content_source",
            "song content_source requires song kind; song kind may use static content for an existing presentation".to_string(),
        ));
    }
    let valid_scripture_source = match wire.kind {
        ItemKind::Scripture => matches!(
            wire.content_source,
            ContentSourceKind::Static | ContentSourceKind::Scripture
        ),
        _ => wire.content_source != ContentSourceKind::Scripture,
    };
    if !valid_scripture_source {
        issues.push(issue(
            "content_source",
            "scripture content_source requires scripture kind; scripture kind may use static content for an existing presentation".to_string(),
        ));
    }

    if wire.arrangement.is_some()
        && !matches!(
            wire.output_strategy,
            OutputStrategy::PreserveExisting | OutputStrategy::RestyleExisting
        )
    {
        issues.push(issue(
            "arrangement",
            "arrangement is only valid for preserve_existing or restyle_existing presentations"
                .to_string(),
        ));
    }
    if wire.operator_cue_limit.is_some() && wire.output_strategy != OutputStrategy::RestyleExisting
    {
        issues.push(issue(
            "operator_cue_limit",
            "operator_cue_limit is only valid for restyle_existing presentations".to_string(),
        ));
    }

    match wire.output_strategy {
        OutputStrategy::PreserveExisting => {
            for (field, configured) in [
                ("display", wire.display.is_some()),
                ("background", wire.background.is_some()),
                ("max_lines_per_slide", wire.max_lines_per_slide.is_some()),
                ("macro_transitions", wire.macro_transitions.is_some()),
            ] {
                if configured {
                    issues.push(issue(
                        field,
                        format!(
                            "{field} is not valid for preserve_existing because exempt files are unchanged"
                        ),
                    ));
                }
            }
        }
        OutputStrategy::RestyleExisting => {
            for (field, configured) in [
                ("display", wire.display.is_some()),
                ("max_lines_per_slide", wire.max_lines_per_slide.is_some()),
            ] {
                if configured {
                    issues.push(issue(
                        field,
                        format!(
                            "{field} is not valid for restyle_existing because slide content is preserved"
                        ),
                    ));
                }
            }
        }
        OutputStrategy::Skip
        | OutputStrategy::EditInPlace
        | OutputStrategy::GenerateNew
        | OutputStrategy::NeedsReview => {}
    }

    issues
}

const fn content_source_name(source: ContentSourceKind) -> &'static str {
    match source {
        ContentSourceKind::Static => "static",
        ContentSourceKind::Description => "description",
        ContentSourceKind::Scripture => "scripture",
        ContentSourceKind::Song => "song",
    }
}

const fn output_strategy_name(strategy: OutputStrategy) -> &'static str {
    match strategy {
        OutputStrategy::Skip => "skip",
        OutputStrategy::PreserveExisting => "preserve_existing",
        OutputStrategy::RestyleExisting => "restyle_existing",
        OutputStrategy::EditInPlace => "edit_in_place",
        OutputStrategy::GenerateNew => "generate_new",
        OutputStrategy::NeedsReview => "needs_review",
    }
}

pub(super) fn compile_presentation_policies(
    config: &RawProjectConfig,
    roles: &BTreeMap<String, RenderRole>,
) -> (
    BTreeMap<String, Arc<PresentationPolicy>>,
    Vec<ConfigValidationIssue>,
) {
    let mut policies = BTreeMap::new();
    let mut issues = Vec::new();
    for (key, wire) in &config.presentation_types {
        let policy_issues = presentation_policy_issues(key, wire);
        if !policy_issues.is_empty() {
            issues.extend(policy_issues);
            continue;
        }
        match compile_policy(config, roles, key, wire) {
            Ok(policy) => {
                policies.insert(key.clone(), Arc::new(policy));
            }
            Err(issue) => issues.push(issue),
        }
    }
    (policies, issues)
}

fn compile_policy(
    config: &RawProjectConfig,
    roles: &BTreeMap<String, RenderRole>,
    key: &str,
    wire: &PresentationTypeConfig,
) -> Result<PresentationPolicy, ConfigValidationIssue> {
    match (wire.output_strategy, wire.content_source) {
        (OutputStrategy::Skip, _) => Ok(PresentationPolicy::Skip { kind: wire.kind }),
        (OutputStrategy::NeedsReview, ContentSourceKind::Static) => {
            Ok(PresentationPolicy::Review(ReviewPolicy::Static {
                kind: wire.kind,
            }))
        }
        (OutputStrategy::NeedsReview, ContentSourceKind::Description) => {
            let parser = required_description_parser(key, wire)?;
            let render = wire
                .display
                .as_ref()
                .map(|display| compile_render_policy(config, roles, key, wire, display))
                .transpose()?;
            Ok(PresentationPolicy::Review(ReviewPolicy::Description {
                kind: wire.kind,
                parser,
                render: render.map(Box::new),
            }))
        }
        (OutputStrategy::NeedsReview, ContentSourceKind::Scripture) => {
            Ok(PresentationPolicy::Review(ReviewPolicy::Scripture))
        }
        (OutputStrategy::NeedsReview, ContentSourceKind::Song) => {
            Ok(PresentationPolicy::Review(ReviewPolicy::Song))
        }
        (
            OutputStrategy::PreserveExisting,
            source @ (ContentSourceKind::Static | ContentSourceKind::Song),
        ) => {
            let (kind, source) = existing_identity(wire.kind, source);
            Ok(PresentationPolicy::PreserveExisting {
                kind,
                source,
                arrangement: compile_arrangement_policy(config, key, wire),
            })
        }
        (
            OutputStrategy::RestyleExisting,
            source @ (ContentSourceKind::Static | ContentSourceKind::Song),
        ) => {
            let (kind, source) = existing_identity(wire.kind, source);
            Ok(PresentationPolicy::RestyleExisting {
                kind,
                source,
                arrangement: compile_arrangement_policy(config, key, wire),
                transform: compile_existing_transform_policy(config, key, wire)?,
            })
        }
        (OutputStrategy::EditInPlace, ContentSourceKind::Description) => {
            let parser = required_description_parser(key, wire)?;
            Ok(PresentationPolicy::EditDescription {
                kind: wire.kind,
                parser,
                render: compile_required_render_policy(config, roles, key, wire, "edit_in_place")?,
            })
        }
        (OutputStrategy::GenerateNew, ContentSourceKind::Description) => {
            let parser = required_description_parser(key, wire)?;
            Ok(PresentationPolicy::GenerateDescription {
                kind: wire.kind,
                parser,
                render: compile_required_render_policy(config, roles, key, wire, "generate_new")?,
            })
        }
        (OutputStrategy::GenerateNew, ContentSourceKind::Scripture) => {
            Ok(PresentationPolicy::GenerateScripture {
                render: compile_required_render_policy(config, roles, key, wire, "generate_new")?,
            })
        }
        _ => Err(ConfigValidationIssue {
            path: format!("presentation_types.{key}.output_strategy"),
            message: format!(
                "{} content is not supported by {}",
                content_source_name(wire.content_source),
                output_strategy_name(wire.output_strategy)
            ),
        }),
    }
}

fn required_description_parser(
    key: &str,
    wire: &PresentationTypeConfig,
) -> Result<DescriptionParserKind, ConfigValidationIssue> {
    wire.description_parser
        .ok_or_else(|| ConfigValidationIssue {
            path: format!("presentation_types.{key}.description_parser"),
            message: "description content requires an explicit description_parser".to_string(),
        })
}

fn existing_identity(
    configured_kind: ItemKind,
    source: ContentSourceKind,
) -> (ItemKind, ExistingSource) {
    if source == ContentSourceKind::Song {
        (ItemKind::Song, ExistingSource::Song)
    } else {
        (configured_kind, ExistingSource::Static)
    }
}

fn compile_existing_transform_policy(
    config: &RawProjectConfig,
    key: &str,
    wire: &PresentationTypeConfig,
) -> Result<ExistingTransformPolicy, ConfigValidationIssue> {
    let background = wire
        .background
        .as_ref()
        .map(|id| compile_background(config, key, id).map(BackgroundTransform::Replace))
        .transpose()?
        .unwrap_or(BackgroundTransform::Preserve);
    let macros = wire
        .macro_transitions
        .as_ref()
        .map(|macros| {
            compile_macro_policy(key, macros, wire.operator_cue_limit).map(MacroTransform::Enforce)
        })
        .transpose()?
        .unwrap_or(MacroTransform::Preserve);
    let cues = wire
        .operator_cue_limit
        .map_or(CueTransform::Preserve, |limit| {
            CueTransform::RetainOperatorPrefix(limit)
        });
    let base = ExistingTransform::new(background, macros, cues).map_err(|error| {
        ConfigValidationIssue {
            path: format!("presentation_types.{key}.output_strategy"),
            message: error.to_string(),
        }
    })?;
    let background_overrides = applicable_overrides(config, key)
        .filter_map(|override_rule| {
            override_rule
                .background
                .as_ref()
                .map(|background| (override_rule, background))
        })
        .map(|(override_rule, background)| {
            Ok(ServiceBackground {
                scope: compile_scope(config, &override_rule.when),
                background: compile_background(config, key, background)?,
            })
        })
        .collect::<Result<Vec<_>, ConfigValidationIssue>>()?;
    Ok(ExistingTransformPolicy {
        base,
        background_overrides,
    })
}

fn compile_macro_policy(
    key: &str,
    macros: &super::RestyleMacroConfig,
    operator_cue_limit: Option<std::num::NonZeroUsize>,
) -> Result<RestyleMacroPolicy, ConfigValidationIssue> {
    let issue = |message: String| ConfigValidationIssue {
        path: format!("presentation_types.{key}.macro_transitions"),
        message,
    };
    let regions = macros
        .regions
        .iter()
        .enumerate()
        .map(|(region_index, region)| {
            let selector = match &region.selector {
                RestyleMacroSelectorConfig::OperatorCue { index } => {
                    if let Some(limit) = operator_cue_limit.filter(|limit| *index >= limit.get()) {
                        return Err(ConfigValidationIssue {
                            path: format!(
                                "presentation_types.{key}.macro_transitions.regions.{region_index}.selector.index"
                            ),
                            message: format!(
                                "operator cue index {index} is not retained by operator_cue_limit {limit}"
                            ),
                        });
                    }
                    RestyleMacroSelector::OperatorCue { index: *index }
                }
                RestyleMacroSelectorConfig::ArrangementGroup { index, names } => {
                    RestyleMacroSelector::arrangement_group(
                        *index,
                        names.iter().cloned().collect(),
                    )
                    .map_err(|error| ConfigValidationIssue {
                        path: format!(
                            "presentation_types.{key}.macro_transitions.regions.{region_index}.selector"
                        ),
                        message: error.to_string(),
                    })?
                }
            };
            RestyleMacroRegion::new(selector, region.enter_macro.clone()).map_err(|error| {
                ConfigValidationIssue {
                    path: format!(
                        "presentation_types.{key}.macro_transitions.regions.{region_index}.enter_macro"
                    ),
                    message: error.to_string(),
                }
            })
        })
        .collect::<Result<Vec<_>, ConfigValidationIssue>>()?;
    RestyleMacroPolicy::new(regions).map_err(|error| issue(error.to_string()))
}

fn compile_render_policy(
    config: &RawProjectConfig,
    roles: &BTreeMap<String, RenderRole>,
    key: &str,
    wire: &PresentationTypeConfig,
    display: &DisplayBindingConfig,
) -> Result<RenderPolicy, ConfigValidationIssue> {
    let (title, content) = match display {
        DisplayBindingConfig::Single { role } => (None, compile_role(roles, key, role)?),
        DisplayBindingConfig::Split { title, content } => (
            Some(compile_role(roles, key, title)?),
            compile_role(roles, key, content)?,
        ),
    };
    if matches!(
        wire.description_parser,
        Some(DescriptionParserKind::Liturgical | DescriptionParserKind::LiturgicalAudience)
    ) && content.speaker_palette().is_none()
    {
        return Err(ConfigValidationIssue {
            path: format!("presentation_types.{key}.display"),
            message: format!(
                "liturgical rendering requires content cue role '{}' to define speaker_colors and its paired leader_enter_macro",
                content.id()
            ),
        });
    }
    let background = wire
        .background
        .as_ref()
        .or(config.defaults.background.as_ref())
        .map(|id| compile_background(config, key, id))
        .transpose()?;
    let base = checked_style(key, background, content, title, wire.max_lines_per_slide)?;

    let overrides = applicable_overrides(config, key)
        .filter_map(|override_rule| {
            override_rule
                .background
                .as_ref()
                .map(|background| (override_rule, background))
        })
        .map(|(override_rule, background)| {
            let background = compile_background(config, key, background)?;
            let style = base.clone().with_background(background);
            Ok(ServiceRenderStyle {
                scope: compile_scope(config, &override_rule.when),
                style,
            })
        })
        .collect::<Result<Vec<_>, ConfigValidationIssue>>()?;

    Ok(RenderPolicy { base, overrides })
}

fn compile_required_render_policy(
    config: &RawProjectConfig,
    roles: &BTreeMap<String, RenderRole>,
    key: &str,
    wire: &PresentationTypeConfig,
    strategy: &str,
) -> Result<RenderPolicy, ConfigValidationIssue> {
    let display = wire.display.as_ref().ok_or_else(|| ConfigValidationIssue {
        path: format!("presentation_types.{key}.display"),
        message: format!("{strategy} requires a display binding"),
    })?;
    compile_render_policy(config, roles, key, wire, display)
}

fn checked_style(
    key: &str,
    background: Option<ResolvedBackground>,
    content: RenderRole,
    title: Option<RenderRole>,
    max_lines: Option<std::num::NonZeroUsize>,
) -> Result<RenderStyle, ConfigValidationIssue> {
    RenderStyle::new(
        background,
        content,
        title,
        max_lines.map(std::num::NonZeroUsize::get),
    )
    .map_err(|error| ConfigValidationIssue {
        path: format!("presentation_types.{key}.display"),
        message: error.to_string(),
    })
}

fn compile_role(
    roles: &BTreeMap<String, RenderRole>,
    type_key: &str,
    role_key: &str,
) -> Result<RenderRole, ConfigValidationIssue> {
    roles
        .get(role_key)
        .cloned()
        .ok_or_else(|| ConfigValidationIssue {
            path: format!("presentation_types.{type_key}.display"),
            message: format!("references unknown cue role '{role_key}'"),
        })
}

fn compile_background(
    config: &RawProjectConfig,
    type_key: &str,
    id: &super::BackgroundId,
) -> Result<ResolvedBackground, ConfigValidationIssue> {
    config
        .backgrounds
        .get(id)
        .cloned()
        .map(|file| ResolvedBackground::new(id.clone(), file))
        .ok_or_else(|| ConfigValidationIssue {
            path: format!("presentation_types.{type_key}.background"),
            message: format!("references unknown background '{id}'"),
        })
}

fn compile_arrangement_policy(
    config: &RawProjectConfig,
    key: &str,
    wire: &PresentationTypeConfig,
) -> ArrangementPolicy {
    let overrides = applicable_overrides(config, key)
        .filter_map(|override_rule| {
            override_rule
                .arrangement
                .as_ref()
                .map(|arrangement| ServiceArrangement {
                    scope: compile_scope(config, &override_rule.when),
                    arrangement: arrangement.clone(),
                })
        })
        .collect();
    ArrangementPolicy {
        base: wire.arrangement.clone(),
        overrides,
    }
}

fn applicable_overrides<'a>(
    config: &'a RawProjectConfig,
    key: &'a str,
) -> impl Iterator<Item = &'a super::OverrideRuleConfig> {
    config.overrides.iter().filter(move |override_rule| {
        override_rule
            .when
            .presentation_type
            .as_deref()
            .is_none_or(|configured| configured == key)
    })
}

fn compile_scope(config: &RawProjectConfig, when: &OverrideWhen) -> ServiceScope {
    let mut service_types = when.service_group.as_ref().map(|group| {
        config
            .service_groups
            .get(group)
            .into_iter()
            .flat_map(|group| &group.service_types)
            .map(|service_type| service_type.to_ascii_lowercase())
            .collect::<BTreeSet<_>>()
    });
    if let Some(service_type) = when.service_type.as_deref() {
        let service_type = service_type.to_ascii_lowercase();
        match &mut service_types {
            Some(service_types) => service_types.retain(|candidate| candidate == &service_type),
            None => service_types = Some(BTreeSet::from([service_type])),
        }
    }
    ServiceScope { service_types }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic)]

    use super::*;
    use crate::project_config::{parse_project_config_str, RawProjectConfig};

    #[test]
    fn compiled_policy_preserves_wire_json() {
        let json = include_str!("../../tests/fixtures/workflow/v4_config.json");
        let checked = parse_project_config_str(json).expect("fixture should compile");
        let raw: RawProjectConfig = serde_json::from_str(json).expect("fixture json");
        let original = serde_json::to_value(raw).expect("raw config should serialize");
        let serialized = serde_json::to_value(&checked).expect("checked config should serialize");
        assert_eq!(serialized, original);
    }

    #[test]
    fn service_background_and_arrangement_are_compiled_by_capability() {
        let checked = parse_project_config_str(
            r##"{
              "version": 4,
              "service_groups": {"seasonal": {"service_types": ["Christmas Eve"]}},
              "backgrounds": {
                "ordinary": "backgrounds/ordinary.png",
                "seasonal": "backgrounds/seasonal.png"
              },
              "cue_roles": {"body": {
                "slide": "Body",
                "enter_macro": "Scripture/Prayer",
                "leader_enter_macro": "Scripture/Prayer (Highlighted)",
                "speaker_colors": {"leader": "#FEDB4F", "audience": "#FFFFFF"}
              }},
              "presentation_types": {
                "song": {"kind":"song", "content_source":"song", "output_strategy": "preserve_existing", "arrangement":"Default"},
                "text": {"kind":"liturgy", "content_source":"description", "description_parser":"liturgical", "output_strategy":"generate_new", "display":{"kind":"single", "role":"body"}, "background":"ordinary"}
              },
              "overrides": [{"when":{"service_group":"seasonal"}, "arrangement":"Seasonal", "background":"seasonal"}]
            }"##,
        )
        .expect("config should compile");

        let song = checked.presentation_policy("song").expect("song policy");
        let PresentationPolicy::PreserveExisting { arrangement, .. } = song else {
            panic!("song should compile as use-existing");
        };
        assert_eq!(arrangement.for_service(None).as_deref(), Some("Default"));
        assert_eq!(
            arrangement.for_service(Some("Christmas Eve")).as_deref(),
            Some("Seasonal")
        );

        let text = checked.presentation_policy("text").expect("text policy");
        let PresentationPolicy::GenerateDescription { render, .. } = text else {
            panic!("text should compile as generated description");
        };
        assert_eq!(
            render
                .for_service(None)
                .background()
                .map(|background| background.id().as_str()),
            Some("ordinary")
        );
        assert_eq!(
            render
                .for_service(Some("Christmas Eve"))
                .background()
                .map(|background| background.id().as_str()),
            Some("seasonal")
        );
    }
}
