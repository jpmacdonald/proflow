//! Compiled runtime presentation policy.
//!
//! The JSON contract deliberately remains a convenient editable structure.
//! This module performs its one semantic translation into variants that cannot
//! express unsupported source/output combinations.

use std::collections::{BTreeMap, BTreeSet};

use crate::workflow::plan::{
    BackgroundTransform, CueMacro, CueTransform, ExistingTransform, MacroTransform, RenderRole,
    RenderStyle, ResolvedBackground, RestyleMacroPolicy, RestyleMacroRegion, RestyleMacroSelector,
    SpeakerPalette,
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

pub(super) fn compile_presentation_policies(
    config: &RawProjectConfig,
) -> Result<BTreeMap<String, PresentationPolicy>, ConfigValidationIssue> {
    config
        .presentation_types
        .iter()
        .map(|(key, wire)| compile_policy(config, key, wire).map(|policy| (key.clone(), policy)))
        .collect()
}

fn compile_policy(
    config: &RawProjectConfig,
    key: &str,
    wire: &PresentationTypeConfig,
) -> Result<PresentationPolicy, ConfigValidationIssue> {
    let issue = |field: &str, message: &str| ConfigValidationIssue {
        path: format!("presentation_types.{key}.{field}"),
        message: message.to_string(),
    };
    match (wire.output_strategy, wire.content_source) {
        (OutputStrategy::Skip, _) => Ok(PresentationPolicy::Skip { kind: wire.kind }),
        (OutputStrategy::NeedsReview, ContentSourceKind::Static) => {
            Ok(PresentationPolicy::Review(ReviewPolicy::Static {
                kind: wire.kind,
            }))
        }
        (OutputStrategy::NeedsReview, ContentSourceKind::Description) => {
            let parser = wire.description_parser.ok_or_else(|| {
                issue(
                    "description_parser",
                    "description content requires an explicit description_parser",
                )
            })?;
            let render = wire
                .display
                .as_ref()
                .map(|_| compile_render_policy(config, key, wire))
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
            let parser = wire.description_parser.ok_or_else(|| {
                issue(
                    "description_parser",
                    "description content requires an explicit description_parser",
                )
            })?;
            Ok(PresentationPolicy::EditDescription {
                kind: wire.kind,
                parser,
                render: compile_render_policy(config, key, wire)?,
            })
        }
        (OutputStrategy::GenerateNew, ContentSourceKind::Description) => {
            let parser = wire.description_parser.ok_or_else(|| {
                issue(
                    "description_parser",
                    "description content requires an explicit description_parser",
                )
            })?;
            Ok(PresentationPolicy::GenerateDescription {
                kind: wire.kind,
                parser,
                render: compile_render_policy(config, key, wire)?,
            })
        }
        (OutputStrategy::GenerateNew, ContentSourceKind::Scripture) => {
            Ok(PresentationPolicy::GenerateScripture {
                render: compile_render_policy(config, key, wire)?,
            })
        }
        _ => Err(issue(
            "output_strategy",
            "content source and output strategy do not form a supported runtime policy",
        )),
    }
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
        .map(|macros| compile_macro_policy(key, macros).map(MacroTransform::Enforce))
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
) -> Result<RestyleMacroPolicy, ConfigValidationIssue> {
    let issue = |message: String| ConfigValidationIssue {
        path: format!("presentation_types.{key}.macro_transitions"),
        message,
    };
    let regions = macros
        .regions
        .iter()
        .map(|region| {
            let selector = match &region.selector {
                RestyleMacroSelectorConfig::OperatorCue { index } => {
                    RestyleMacroSelector::OperatorCue { index: *index }
                }
                RestyleMacroSelectorConfig::ArrangementGroup { index, names } => {
                    RestyleMacroSelector::arrangement_group(
                        *index,
                        names.iter().cloned().collect(),
                    )?
                }
            };
            RestyleMacroRegion::new(selector, region.enter_macro.clone())
        })
        .collect::<Result<Vec<_>, crate::workflow::plan::RenderPlanError>>()
        .map_err(|error| issue(error.to_string()))?;
    RestyleMacroPolicy::new(regions).map_err(|error| issue(error.to_string()))
}

fn compile_render_policy(
    config: &RawProjectConfig,
    key: &str,
    wire: &PresentationTypeConfig,
) -> Result<RenderPolicy, ConfigValidationIssue> {
    let display = wire.display.as_ref().ok_or_else(|| ConfigValidationIssue {
        path: format!("presentation_types.{key}.display"),
        message: "rendered presentation requires a display binding".to_string(),
    })?;
    let (title, content) = match display {
        DisplayBindingConfig::Single { role } => (None, compile_role(config, key, role)?),
        DisplayBindingConfig::Split { title, content } => (
            Some(compile_role(config, key, title)?),
            compile_role(config, key, content)?,
        ),
    };
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
            let style = checked_style(
                key,
                Some(background),
                base.content().clone(),
                base.title().cloned(),
                wire.max_lines_per_slide,
            )?;
            Ok(ServiceRenderStyle {
                scope: compile_scope(config, &override_rule.when),
                style,
            })
        })
        .collect::<Result<Vec<_>, ConfigValidationIssue>>()?;

    Ok(RenderPolicy { base, overrides })
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
    config: &RawProjectConfig,
    type_key: &str,
    role_key: &str,
) -> Result<RenderRole, ConfigValidationIssue> {
    let role = config
        .cue_roles
        .get(role_key)
        .ok_or_else(|| ConfigValidationIssue {
            path: format!("presentation_types.{type_key}.display"),
            message: format!("references unknown cue role '{role_key}'"),
        })?;
    let cue_macro = role
        .enter_macro
        .as_ref()
        .map(|enter| CueMacro::new(enter.clone(), role.leader_enter_macro.clone()))
        .transpose()
        .map_err(|error| ConfigValidationIssue {
            path: format!("cue_roles.{role_key}.enter_macro"),
            message: error.to_string(),
        })?;
    let speaker_palette = role.speaker_colors.map(|colors| {
        SpeakerPalette::new(colors.leader.components(), colors.audience.components())
    });
    RenderRole::new(
        role_key.to_string(),
        role.slide.clone(),
        role.text_slots.clone(),
        cue_macro,
        speaker_palette,
    )
    .map_err(|error| ConfigValidationIssue {
        path: format!("cue_roles.{role_key}"),
        message: error.to_string(),
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
            r#"{
              "version": 4,
              "service_groups": {"seasonal": {"service_types": ["Christmas Eve"]}},
              "backgrounds": {
                "ordinary": "backgrounds/ordinary.png",
                "seasonal": "backgrounds/seasonal.png"
              },
              "cue_roles": {"body": {"slide": "Body"}},
              "presentation_types": {
                "song": {"kind":"song", "content_source":"song", "output_strategy": "preserve_existing", "arrangement":"Default"},
                "text": {"kind":"liturgy", "content_source":"description", "description_parser":"liturgical", "output_strategy":"generate_new", "display":{"kind":"single", "role":"body"}, "background":"ordinary"}
              },
              "overrides": [{"when":{"service_group":"seasonal"}, "arrangement":"Seasonal", "background":"seasonal"}]
            }"#,
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
