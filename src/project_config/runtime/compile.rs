//! Compile editable configuration into presentation policies.

mod render_policy;
use render_policy::{compile_background, compile_render_policy, compile_required_render_policy};

use super::{
    ArrangementPolicy, ConfigValidationIssue, ContentSourceKind, CueMacro, DescriptionParserKind,
    DisplayBindingConfig, ExistingSource, ExistingTransformPolicy, ItemKind, OutputStrategy,
    OverrideWhen, PresentationPolicy, PresentationTypeConfig, RawProjectConfig, RenderPolicy,
    RestyleMacroRegion, RestyleMacroSelectorConfig, ReviewPolicy, ServiceArrangement,
    ServiceBackground, ServiceRenderStyle, ServiceScope, SpeakerPalette,
};
use super::{
    BackgroundTransform, CueTransform, ExistingTransform, MacroTransform, PresentationTypeKey,
    RenderRole, RenderStyle, ResolvedBackground, RestyleMacroPolicy, RestyleMacroSelector,
};
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::sync::Arc;

/// Compile every cue-role declaration once, including roles not currently used
/// by a presentation. Presentation compilation consumes only these checked
/// values and never reinterprets the raw role contract.
pub(in crate::project_config) fn compile_render_roles(
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
    role: &super::super::CueRoleConfig,
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

pub(in crate::project_config) fn compile_presentation_policies(
    config: &RawProjectConfig,
    roles: &BTreeMap<String, RenderRole>,
) -> (
    BTreeMap<PresentationTypeKey, Arc<PresentationPolicy>>,
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
                policies.insert(PresentationTypeKey::compiled(key), Arc::new(policy));
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
    macros: &super::super::RestyleMacroConfig,
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
) -> impl Iterator<Item = &'a super::super::OverrideRuleConfig> {
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
