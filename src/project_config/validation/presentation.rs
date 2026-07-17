//! Validation for presentation policies and their installed asset bindings.

use super::{
    identity::{validate_exact_identity, validate_map_keys},
    ConfigValidationIssue,
};
use crate::project_config::{
    ContentSourceKind, DisplayBindingConfig, ItemKind, OutputStrategy, PresentationTypeConfig,
    RawProjectConfig, RestyleMacroConfig, RestyleMacroSelectorConfig,
};
use std::collections::HashSet;

pub(super) fn validate_runtime_defaults(
    config: &RawProjectConfig,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    if let Some(theme) = config.defaults.theme.as_deref() {
        let path = "defaults.theme";
        if validate_exact_identity(theme, path, "theme", issues)
            && (matches!(theme, "." | "..") || theme.contains(['/', '\\']))
        {
            issues.push(ConfigValidationIssue {
                path: path.to_string(),
                message: "theme must be an installed theme name, not a path".to_string(),
            });
        }
    }
    if config
        .defaults
        .days_ahead
        .is_some_and(|days| !(1..=365).contains(&days))
    {
        issues.push(ConfigValidationIssue {
            path: "defaults.days_ahead".to_string(),
            message: "must be between 1 and 365".to_string(),
        });
    }
}

pub(super) fn validate_background_references(
    config: &RawProjectConfig,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    if let Some(background) = &config.defaults.background {
        if !config.backgrounds.contains_key(background) {
            issues.push(ConfigValidationIssue {
                path: "defaults.background".to_string(),
                message: format!("references unknown background '{background}'"),
            });
        }
    }
}

pub(super) fn validate_cue_roles(
    config: &RawProjectConfig,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    validate_map_keys(
        config.cue_roles.keys(),
        "cue_roles",
        "cue role name",
        issues,
    );
    for (role_key, role) in &config.cue_roles {
        let path = format!("cue_roles.{role_key}");
        validate_exact_identity(&role.slide, &format!("{path}.slide"), "slide", issues);
        validate_map_keys(
            role.text_slots.keys(),
            &format!("{path}.text_slots"),
            "semantic text-slot name",
            issues,
        );
        let mut native_slots = HashSet::new();
        for (semantic, native) in &role.text_slots {
            if validate_exact_identity(
                native,
                &format!("{path}.text_slots.{semantic}"),
                "native text-slot names",
                issues,
            ) && !native_slots.insert(native)
            {
                issues.push(ConfigValidationIssue {
                    path: format!("{path}.text_slots.{semantic}"),
                    message: format!("native text slot '{native}' is mapped more than once"),
                });
            }
        }
        for (field, value) in [
            ("enter_macro", role.enter_macro.as_deref()),
            ("leader_enter_macro", role.leader_enter_macro.as_deref()),
        ] {
            if let Some(value) = value {
                validate_exact_identity(value, &format!("{path}.{field}"), field, issues);
            }
        }
        if role.leader_enter_macro.is_some() && role.enter_macro.is_none() {
            issues.push(ConfigValidationIssue {
                path: format!("{path}.leader_enter_macro"),
                message: "leader_enter_macro requires enter_macro".to_string(),
            });
        }
        if role.leader_enter_macro.is_some() != role.speaker_colors.is_some() {
            issues.push(ConfigValidationIssue {
                path: format!("{path}.speaker_colors"),
                message: "leader_enter_macro and speaker_colors must be configured together"
                    .to_string(),
            });
        }
        if role
            .speaker_colors
            .is_some_and(|colors| colors.leader == colors.audience)
        {
            issues.push(ConfigValidationIssue {
                path: format!("{path}.speaker_colors"),
                message: "leader and audience colors must differ so mixed speaker styling remains observable"
                    .to_string(),
            });
        }
    }
}

pub(super) fn validate_presentation_types(
    config: &RawProjectConfig,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    validate_map_keys(
        config.presentation_types.keys(),
        "presentation_types",
        "presentation type key",
        issues,
    );
    for (type_key, ptype) in &config.presentation_types {
        validate_presentation_type(config, type_key, ptype, issues);
    }
}

fn validate_presentation_type(
    config: &RawProjectConfig,
    type_key: &str,
    ptype: &PresentationTypeConfig,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    match (ptype.content_source, ptype.description_parser) {
        (ContentSourceKind::Description, None) => issues.push(ConfigValidationIssue {
            path: format!("presentation_types.{type_key}.description_parser"),
            message: "description content requires an explicit description_parser".to_string(),
        }),
        (ContentSourceKind::Description, Some(_)) | (_, None) => {}
        (_, Some(_)) => issues.push(ConfigValidationIssue {
            path: format!("presentation_types.{type_key}.description_parser"),
            message: "description_parser is only valid for description content".to_string(),
        }),
    }
    if let Some(display) = &ptype.display {
        validate_display_binding(config, type_key, display, issues);
    }
    if let Some(background) = &ptype.background {
        if !config.backgrounds.contains_key(background) {
            issues.push(ConfigValidationIssue {
                path: format!("presentation_types.{type_key}.background"),
                message: format!("references unknown background '{background}'"),
            });
        }
    }
    if let Some(macros) = &ptype.macro_transitions {
        validate_macro_transitions(
            type_key,
            macros,
            ptype.operator_cue_limit.map(std::num::NonZeroUsize::get),
            issues,
        );
    }
    validate_presentation_kind_and_source(type_key, ptype, issues);
    validate_content_output_combination(type_key, ptype, issues);
    validate_existing_selection(type_key, ptype, issues);
    validate_output_strategy_fields(type_key, ptype, issues);
}

fn validate_macro_transitions(
    type_key: &str,
    macros: &RestyleMacroConfig,
    operator_cue_limit: Option<usize>,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    if macros.regions.is_empty() {
        issues.push(ConfigValidationIssue {
            path: format!("presentation_types.{type_key}.macro_transitions.regions"),
            message: "requires at least one macro region".to_string(),
        });
    }
    let mut operator_indexes = HashSet::new();
    let mut arrangement_indexes = HashSet::new();
    for (index, region) in macros.regions.iter().enumerate() {
        let path = format!("presentation_types.{type_key}.macro_transitions.regions.{index}");
        validate_exact_identity(
            &region.enter_macro,
            &format!("{path}.enter_macro"),
            "macro",
            issues,
        );
        match &region.selector {
            RestyleMacroSelectorConfig::OperatorCue { index } => {
                if !operator_indexes.insert(*index) {
                    issues.push(ConfigValidationIssue {
                        path: format!("{path}.selector.index"),
                        message: format!("duplicates operator cue index {index}"),
                    });
                }
                if let Some(limit) = operator_cue_limit.filter(|limit| *index >= *limit) {
                    issues.push(ConfigValidationIssue {
                        path: format!("{path}.selector.index"),
                        message: format!(
                            "operator cue index {index} is not retained by operator_cue_limit {limit}"
                        ),
                    });
                }
            }
            RestyleMacroSelectorConfig::ArrangementGroup { index, names } => {
                if !arrangement_indexes.insert(*index) {
                    issues.push(ConfigValidationIssue {
                        path: format!("{path}.selector.index"),
                        message: format!("duplicates arrangement group index {index}"),
                    });
                }
                if names.is_empty() {
                    issues.push(ConfigValidationIssue {
                        path: format!("{path}.selector.names"),
                        message: "requires at least one accepted group name".to_string(),
                    });
                }
                for (name_index, name) in names.iter().enumerate() {
                    validate_exact_identity(
                        name,
                        &format!("{path}.selector.names.{name_index}"),
                        "arrangement group",
                        issues,
                    );
                }
            }
        }
    }
}

fn validate_existing_selection(
    type_key: &str,
    ptype: &PresentationTypeConfig,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    if let Some(arrangement) = ptype.arrangement.as_deref() {
        validate_exact_identity(
            arrangement,
            &format!("presentation_types.{type_key}.arrangement"),
            "arrangement",
            issues,
        );
        if !matches!(
            ptype.output_strategy,
            OutputStrategy::PreserveExisting | OutputStrategy::RestyleExisting
        ) {
            issues.push(ConfigValidationIssue {
                path: format!("presentation_types.{type_key}.arrangement"),
                message:
                    "arrangement is only valid for preserve_existing or restyle_existing presentations"
                        .to_string(),
            });
        }
    }
    if ptype.operator_cue_limit.is_some()
        && ptype.output_strategy != OutputStrategy::RestyleExisting
    {
        issues.push(ConfigValidationIssue {
            path: format!("presentation_types.{type_key}.operator_cue_limit"),
            message: "operator_cue_limit is only valid for restyle_existing presentations"
                .to_string(),
        });
    }
}

fn validate_output_strategy_fields(
    type_key: &str,
    ptype: &PresentationTypeConfig,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    match ptype.output_strategy {
        OutputStrategy::PreserveExisting => {
            for (field, configured) in [
                ("display", ptype.display.is_some()),
                ("background", ptype.background.is_some()),
                ("max_lines_per_slide", ptype.max_lines_per_slide.is_some()),
                ("macro_transitions", ptype.macro_transitions.is_some()),
            ] {
                if configured {
                    issues.push(ConfigValidationIssue {
                        path: format!("presentation_types.{type_key}.{field}"),
                        message: format!(
                            "{field} is not valid for preserve_existing because exempt files are unchanged"
                        ),
                    });
                }
            }
        }
        OutputStrategy::RestyleExisting => {
            for (field, configured) in [
                ("display", ptype.display.is_some()),
                ("max_lines_per_slide", ptype.max_lines_per_slide.is_some()),
            ] {
                if configured {
                    issues.push(ConfigValidationIssue {
                        path: format!("presentation_types.{type_key}.{field}"),
                        message: format!(
                            "{field} is not valid for restyle_existing because slide content is preserved"
                        ),
                    });
                }
            }
            if ptype.background.is_none()
                && ptype.macro_transitions.is_none()
                && ptype.operator_cue_limit.is_none()
            {
                issues.push(ConfigValidationIssue {
                    path: format!("presentation_types.{type_key}.output_strategy"),
                    message:
                        "restyle_existing requires a background, macro_transitions, or operator_cue_limit"
                            .to_string(),
                });
            }
        }
        OutputStrategy::GenerateNew if ptype.display.is_none() => {
            issues.push(ConfigValidationIssue {
                path: format!("presentation_types.{type_key}.display"),
                message: "generate_new requires a display binding".to_string(),
            });
        }
        OutputStrategy::EditInPlace if ptype.display.is_none() => {
            issues.push(ConfigValidationIssue {
                path: format!("presentation_types.{type_key}.display"),
                message: "edit_in_place requires a display binding".to_string(),
            });
        }
        OutputStrategy::Skip
        | OutputStrategy::EditInPlace
        | OutputStrategy::GenerateNew
        | OutputStrategy::NeedsReview => {}
    }
}

fn validate_presentation_kind_and_source(
    type_key: &str,
    ptype: &PresentationTypeConfig,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    if ptype.content_source == ContentSourceKind::Song && ptype.kind != ItemKind::Song {
        issues.push(ConfigValidationIssue {
            path: format!("presentation_types.{type_key}.content_source"),
            message: "song content_source requires song kind; song kind may use static content for an existing presentation".to_string(),
        });
    }

    let valid_scripture_source = match ptype.kind {
        ItemKind::Scripture => matches!(
            ptype.content_source,
            ContentSourceKind::Static | ContentSourceKind::Scripture
        ),
        _ => ptype.content_source != ContentSourceKind::Scripture,
    };
    if !valid_scripture_source {
        issues.push(ConfigValidationIssue {
            path: format!("presentation_types.{type_key}.content_source"),
            message: "scripture content_source requires scripture kind; scripture kind may use static content for an existing presentation".to_string(),
        });
    }
}

fn validate_content_output_combination(
    type_key: &str,
    ptype: &PresentationTypeConfig,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    let supported = match ptype.output_strategy {
        OutputStrategy::PreserveExisting => matches!(
            ptype.content_source,
            ContentSourceKind::Static | ContentSourceKind::Song
        ),
        OutputStrategy::RestyleExisting => matches!(
            ptype.content_source,
            ContentSourceKind::Static | ContentSourceKind::Song
        ),
        OutputStrategy::EditInPlace => {
            matches!(ptype.content_source, ContentSourceKind::Description)
        }
        OutputStrategy::GenerateNew => matches!(
            ptype.content_source,
            ContentSourceKind::Description | ContentSourceKind::Scripture
        ),
        OutputStrategy::Skip | OutputStrategy::NeedsReview => true,
    };
    if !supported {
        let content_source = match ptype.content_source {
            ContentSourceKind::Static => "static",
            ContentSourceKind::Description => "description",
            ContentSourceKind::Scripture => "scripture",
            ContentSourceKind::Song => "song",
        };
        let output_strategy = match ptype.output_strategy {
            OutputStrategy::Skip => "skip",
            OutputStrategy::PreserveExisting => "preserve_existing",
            OutputStrategy::RestyleExisting => "restyle_existing",
            OutputStrategy::EditInPlace => "edit_in_place",
            OutputStrategy::GenerateNew => "generate_new",
            OutputStrategy::NeedsReview => "needs_review",
        };
        issues.push(ConfigValidationIssue {
            path: format!("presentation_types.{type_key}.output_strategy"),
            message: format!("{content_source} content is not supported by {output_strategy}"),
        });
    }
}

fn validate_display_binding(
    config: &RawProjectConfig,
    type_key: &str,
    display: &DisplayBindingConfig,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    match display {
        DisplayBindingConfig::Single { role } => {
            validate_cue_role_reference(config, type_key, "role", role, issues);
        }
        DisplayBindingConfig::Split { title, content } => {
            validate_cue_role_reference(config, type_key, "title", title, issues);
            validate_cue_role_reference(config, type_key, "content", content, issues);
            if title == content {
                issues.push(ConfigValidationIssue {
                    path: format!("presentation_types.{type_key}.display"),
                    message: format!(
                        "split display title and content must use different cue roles; both reference '{title}'"
                    ),
                });
            }
        }
    }
}

fn validate_cue_role_reference(
    config: &RawProjectConfig,
    type_key: &str,
    field: &str,
    role_key: &str,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    let path = format!("presentation_types.{type_key}.display.{field}");
    match config.cue_roles.get(role_key) {
        None => issues.push(ConfigValidationIssue {
            path,
            message: format!("references unknown cue role '{role_key}'"),
        }),
        Some(role) if !role.text_slots.is_empty() && !role.text_slots.contains_key("body") => {
            issues.push(ConfigValidationIssue {
                path,
                message: format!(
                    "cue role '{role_key}' uses explicit text slots but does not map the required 'body' field"
                ),
            });
        }
        Some(_) => {}
    }
}
