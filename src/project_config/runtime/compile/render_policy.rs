//! Compile render styles, cue-role bindings, and backgrounds.

use super::{
    applicable_overrides, compile_scope, ConfigValidationIssue, DescriptionParserKind,
    DisplayBindingConfig, PresentationTypeConfig, RawProjectConfig, RenderPolicy, RenderRole,
    RenderStyle, ResolvedBackground, ServiceRenderStyle,
};
use std::collections::BTreeMap;

pub(super) fn compile_render_policy(
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

pub(super) fn compile_required_render_policy(
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

pub(super) fn compile_background(
    config: &RawProjectConfig,
    type_key: &str,
    id: &crate::project_config::BackgroundId,
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
