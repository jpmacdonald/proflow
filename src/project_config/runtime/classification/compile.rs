//! Semantic translation from editable classification JSON to checked runtime rules.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::super::{ExistingSource, PresentationPolicy, ServiceScope};
use super::{
    normalize_identity, normalize_phrases, ClassificationTier, CompiledClassification,
    CompiledDecision, CompiledDecisionChoice, CompiledDecisionMatcher, CompiledDirectTarget,
    CompiledExpansionStep, CompiledItemMatcher, CompiledRequiredPlaylistItem, CompiledRuleOutcome,
    CompiledSpeakerTarget, RequiredPresentation, ResolvedPresentationType,
};
use crate::project_config::{
    ConfigValidationIssue, DecisionChoiceConfig, DecisionChoiceMatch, DecisionConfig,
    DecisionContextField, ExpansionStep, ItemRuleOutcome, LibraryIdentityMatch, MatchSpec,
    RawProjectConfig, SpeakerSource, TargetSpec,
};

pub fn compile_classifications(
    config: &RawProjectConfig,
    presentations: &BTreeMap<String, Arc<PresentationPolicy>>,
) -> Result<Vec<CompiledClassification>, Vec<ConfigValidationIssue>> {
    let mut compiled =
        Vec::with_capacity(config.library_identities.len() + config.item_rules.len());
    let mut issues = Vec::new();
    for (identity_index, identity) in config.library_identities.iter().enumerate() {
        let path = format!("library_identities[{identity_index}]");
        let outcome = ItemRuleOutcome::UseType {
            type_key: identity.use_type.clone(),
            target: Some(TargetSpec::ExistingFile {
                library_file: identity.library_file.clone(),
            }),
        };
        match compile_rule_outcome(presentations, &outcome, &path) {
            Ok(outcome) => compiled.push(CompiledClassification {
                id: identity.id.clone(),
                matcher: compile_library_identity_matcher(&identity.match_spec),
                outcome,
                tier: ClassificationTier::LibraryIdentity,
            }),
            Err(issue) => issues.push(issue),
        }
    }
    for (rule_index, rule) in config.item_rules.iter().enumerate() {
        let path = format!("item_rules[{rule_index}]");
        match compile_rule_outcome(presentations, &rule.outcome, &path) {
            Ok(outcome) => compiled.push(CompiledClassification {
                id: rule.id.clone(),
                matcher: compile_item_matcher(&rule.match_spec),
                outcome,
                tier: ClassificationTier::ItemRule(rule.tier),
            }),
            Err(issue) => issues.push(issue),
        }
    }
    if issues.is_empty() {
        Ok(compiled)
    } else {
        Err(issues)
    }
}

fn compile_library_identity_matcher(matcher: &LibraryIdentityMatch) -> CompiledItemMatcher {
    let (title_prefix, title_contains) = match matcher {
        LibraryIdentityMatch::TitlePrefix { values } => (normalize_phrases(values), Vec::new()),
        LibraryIdentityMatch::TitleContains { values } => (Vec::new(), normalize_phrases(values)),
    };
    CompiledItemMatcher {
        title_prefix,
        title_contains,
        description_contains: Vec::new(),
        category: None,
        has_scripture_ref: None,
        service_types: None,
    }
}

pub fn compile_required_playlist_items(
    config: &RawProjectConfig,
    presentations: &BTreeMap<String, Arc<PresentationPolicy>>,
) -> Result<Vec<CompiledRequiredPlaylistItem>, Vec<ConfigValidationIssue>> {
    let mut compiled = Vec::with_capacity(config.required_playlist_items.len());
    let mut issues = Vec::new();
    for (index, required) in config.required_playlist_items.iter().enumerate() {
        let path = format!("required_playlist_items[{index}]");
        let result = (|| {
            let resolved = resolve_presentation(
                presentations,
                &required.use_type,
                &format!("{path}.use_type"),
            )?;
            let type_key = resolved.key().to_string();
            let presentation = match resolved.policy() {
                PresentationPolicy::PreserveExisting {
                    kind,
                    source: ExistingSource::Static,
                    arrangement,
                } => RequiredPresentation::Preserve {
                    kind: *kind,
                    arrangement: arrangement.clone(),
                },
                PresentationPolicy::RestyleExisting {
                    kind,
                    source: ExistingSource::Static,
                    arrangement,
                    transform,
                } => RequiredPresentation::Restyle {
                    kind: *kind,
                    arrangement: arrangement.clone(),
                    transform: transform.clone(),
                },
                _ => {
                    return Err(ConfigValidationIssue {
                        path: format!("{path}.use_type"),
                        message: "required playlist items must use a static preserve_existing or restyle_existing presentation type"
                            .to_string(),
                    });
                }
            };
            let scope = compile_required_scope(config, required.service_group.as_deref(), &path)?;
            Ok(CompiledRequiredPlaylistItem {
                id: required.id.clone(),
                type_key,
                library_file: required.library_file.clone(),
                placement: required.placement,
                scope,
                presentation,
            })
        })();
        match result {
            Ok(item) => compiled.push(item),
            Err(issue) => issues.push(issue),
        }
    }
    if issues.is_empty() {
        Ok(compiled)
    } else {
        Err(issues)
    }
}

fn compile_required_scope(
    config: &RawProjectConfig,
    service_group: Option<&str>,
    path: &str,
) -> Result<ServiceScope, ConfigValidationIssue> {
    let Some(group_key) = service_group else {
        return Ok(ServiceScope {
            service_types: None,
        });
    };
    let group = config
        .service_groups
        .get(group_key)
        .ok_or_else(|| ConfigValidationIssue {
            path: format!("{path}.service_group"),
            message: format!("references unknown service group '{group_key}'"),
        })?;
    Ok(ServiceScope {
        service_types: Some(
            group
                .service_types
                .iter()
                .map(|service_type| normalize_identity(service_type))
                .collect(),
        ),
    })
}

fn compile_rule_outcome(
    presentations: &BTreeMap<String, Arc<PresentationPolicy>>,
    outcome: &ItemRuleOutcome,
    path: &str,
) -> Result<CompiledRuleOutcome, ConfigValidationIssue> {
    match outcome {
        ItemRuleOutcome::UseType { type_key, target } => {
            let presentation =
                resolve_presentation(presentations, type_key, &format!("{path}.use_type"))?;
            let target =
                compile_direct_target(&presentation, target.as_ref(), &format!("{path}.target"))?;
            Ok(CompiledRuleOutcome::UseType {
                presentation,
                target,
            })
        }
        ItemRuleOutcome::Action(action) => Ok(CompiledRuleOutcome::Action(action.clone())),
        ItemRuleOutcome::Decision(decision) => Ok(CompiledRuleOutcome::Decision(compile_decision(
            presentations,
            decision,
            path,
        )?)),
        ItemRuleOutcome::Expand(expansion) => expansion
            .iter()
            .enumerate()
            .map(|(index, step)| compile_expansion_step(presentations, step, path, index))
            .collect::<Result<Vec<_>, _>>()
            .map(CompiledRuleOutcome::Expand),
    }
}

fn compile_expansion_step(
    presentations: &BTreeMap<String, Arc<PresentationPolicy>>,
    step: &ExpansionStep,
    path: &str,
    index: usize,
) -> Result<CompiledExpansionStep, ConfigValidationIssue> {
    let step_path = format!("{path}.expand[{index}]");
    let presentation = resolve_presentation(
        presentations,
        &step.use_type,
        &format!("{step_path}.use_type"),
    )?;
    match step.speaker {
        None => Ok(CompiledExpansionStep::Direct {
            target: compile_direct_target(
                &presentation,
                step.target.as_ref(),
                &format!("{step_path}.target"),
            )?,
            presentation,
        }),
        Some(SpeakerSource::Resolved) => Ok(CompiledExpansionStep::Speaker {
            target: compile_speaker_target(
                &presentation,
                step.target.as_ref(),
                &format!("{step_path}.target"),
            )?,
            presentation,
        }),
    }
}

fn compile_decision(
    presentations: &BTreeMap<String, Arc<PresentationPolicy>>,
    decision: &DecisionConfig,
    path: &str,
) -> Result<CompiledDecision, ConfigValidationIssue> {
    match decision {
        DecisionConfig::ChooseExistingFile {
            context_fields,
            instructions,
            choices,
            on_ambiguous,
        } => {
            let context_fields = if context_fields.is_empty() {
                vec![
                    DecisionContextField::Title,
                    DecisionContextField::Description,
                    DecisionContextField::Note,
                ]
            } else {
                context_fields.clone()
            };
            let choices = choices
                .iter()
                .map(|(key, choice)| {
                    let choice_path = format!("{path}.decision.choices.{key}");
                    let type_key =
                        choice
                            .use_type
                            .as_deref()
                            .ok_or_else(|| ConfigValidationIssue {
                                path: format!("{choice_path}.use_type"),
                                message: "decision choice must define use_type".to_string(),
                            })?;
                    let presentation = resolve_presentation(
                        presentations,
                        type_key,
                        &format!("{choice_path}.use_type"),
                    )?;
                    if !presentation.policy().accepts_existing_file_decision() {
                        return Err(ConfigValidationIssue {
                            path: format!("{choice_path}.use_type"),
                            message: "choose_existing_file requires a preserve_existing or restyle_existing presentation type"
                                .to_string(),
                        });
                    }
                    let library_file = compile_decision_library_file(choice, &choice_path)?;
                    Ok(CompiledDecisionChoice {
                        key: key.clone(),
                        presentation,
                        library_file,
                        matcher: compile_decision_matcher(&choice.match_spec),
                    })
                })
                .collect::<Result<Vec<_>, ConfigValidationIssue>>()?;
            Ok(CompiledDecision {
                context_fields,
                instructions: instructions.clone(),
                choices,
                on_ambiguous: on_ambiguous.unwrap_or_default(),
            })
        }
    }
}

fn compile_item_matcher(matcher: &MatchSpec) -> CompiledItemMatcher {
    CompiledItemMatcher {
        title_prefix: normalize_phrases(&matcher.title_prefix),
        title_contains: normalize_phrases(&matcher.title_contains),
        description_contains: normalize_phrases(&matcher.description_contains),
        category: matcher.category,
        has_scripture_ref: matcher.has_scripture_ref,
        service_types: (!matcher.service_type.is_empty()).then(|| {
            matcher
                .service_type
                .iter()
                .map(|service_type| normalize_identity(service_type))
                .collect()
        }),
    }
}

fn compile_direct_target(
    presentation: &ResolvedPresentationType,
    target: Option<&TargetSpec>,
    path: &str,
) -> Result<CompiledDirectTarget, ConfigValidationIssue> {
    match target {
        None => Ok(CompiledDirectTarget::Automatic),
        Some(TargetSpec::ExistingFile { library_file })
            if presentation.policy().accepts_library_file_target() =>
        {
            Ok(CompiledDirectTarget::LibraryFile(library_file.clone()))
        }
        Some(TargetSpec::ExistingFile { .. }) => Err(ConfigValidationIssue {
            path: path.to_string(),
            message: "library_file requires an existing-source output strategy".to_string(),
        }),
        Some(TargetSpec::GeneratedName { .. }) => Err(ConfigValidationIssue {
            path: path.to_string(),
            message: "name_template is supported only for a speaker expansion".to_string(),
        }),
    }
}

fn compile_speaker_target(
    presentation: &ResolvedPresentationType,
    target: Option<&TargetSpec>,
    path: &str,
) -> Result<CompiledSpeakerTarget, ConfigValidationIssue> {
    match target {
        None => Ok(CompiledSpeakerTarget::Automatic),
        Some(TargetSpec::ExistingFile { library_file })
            if presentation.policy().accepts_library_file_target() =>
        {
            Ok(CompiledSpeakerTarget::LibraryFile(library_file.clone()))
        }
        Some(TargetSpec::ExistingFile { .. }) => Err(ConfigValidationIssue {
            path: path.to_string(),
            message: "library_file requires an existing-source output strategy".to_string(),
        }),
        Some(TargetSpec::GeneratedName { name_template }) => {
            Ok(CompiledSpeakerTarget::NameTemplate(name_template.clone()))
        }
    }
}

fn compile_decision_library_file(
    choice: &DecisionChoiceConfig,
    path: &str,
) -> Result<String, ConfigValidationIssue> {
    match (choice.file.as_deref(), choice.target.as_ref()) {
        (Some(file), None) => Ok(file.to_string()),
        (None, Some(TargetSpec::ExistingFile { library_file })) => Ok(library_file.clone()),
        (_, Some(TargetSpec::GeneratedName { .. })) => Err(ConfigValidationIssue {
            path: format!("{path}.target"),
            message: "choose_existing_file target must define library_file, not name_template"
                .to_string(),
        }),
        _ => Err(ConfigValidationIssue {
            path: path.to_string(),
            message: "choice must define exactly one of file or target.library_file".to_string(),
        }),
    }
}

fn compile_decision_matcher(matcher: &DecisionChoiceMatch) -> CompiledDecisionMatcher {
    CompiledDecisionMatcher {
        any: normalize_phrases(&matcher.any),
        all: normalize_phrases(&matcher.all),
        none: normalize_phrases(&matcher.none),
    }
}

fn resolve_presentation(
    presentations: &BTreeMap<String, Arc<PresentationPolicy>>,
    key: &str,
    path: &str,
) -> Result<ResolvedPresentationType, ConfigValidationIssue> {
    presentations
        .get(key)
        .cloned()
        .map(|policy| ResolvedPresentationType {
            key: key.to_string(),
            policy,
        })
        .ok_or_else(|| ConfigValidationIssue {
            path: path.to_string(),
            message: format!("references unknown presentation type '{key}'"),
        })
}
