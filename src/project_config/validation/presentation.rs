//! Validation for presentation policies and their installed asset bindings.

use super::{
    identity::{validate_exact_identity, validate_map_keys},
    ConfigValidationIssue,
};
use crate::project_config::{
    DisplayBindingConfig, RawProjectConfig, RestyleMacroConfig, RestyleMacroSelectorConfig,
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
    for (type_key, presentation) in &config.presentation_types {
        if let Some(background) = &presentation.background {
            if !config.backgrounds.contains_key(background) {
                issues.push(ConfigValidationIssue {
                    path: format!("presentation_types.{type_key}.background"),
                    message: format!("references unknown background '{background}'"),
                });
            }
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
        if let Some(display) = &ptype.display {
            validate_display_references(config, type_key, display, issues);
        }
        if let Some(arrangement) = ptype.arrangement.as_deref() {
            validate_exact_identity(
                arrangement,
                &format!("presentation_types.{type_key}.arrangement"),
                "arrangement",
                issues,
            );
        }
        if let Some(macros) = &ptype.macro_transitions {
            validate_macro_transition_names(type_key, macros, issues);
        }
    }
}

fn validate_display_references(
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
        }
    }
}

fn validate_macro_transition_names(
    type_key: &str,
    macros: &RestyleMacroConfig,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    for (index, region) in macros.regions.iter().enumerate() {
        let path = format!("presentation_types.{type_key}.macro_transitions.regions.{index}");
        validate_exact_identity(
            &region.enter_macro,
            &format!("{path}.enter_macro"),
            "macro",
            issues,
        );
        if let RestyleMacroSelectorConfig::ArrangementGroup { names, .. } = &region.selector {
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

fn validate_cue_role_reference(
    config: &RawProjectConfig,
    type_key: &str,
    field: &str,
    role_key: &str,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    let path = format!("presentation_types.{type_key}.display.{field}");
    if !config.cue_roles.contains_key(role_key) {
        issues.push(ConfigValidationIssue {
            path,
            message: format!("references unknown cue role '{role_key}'"),
        });
    }
}
