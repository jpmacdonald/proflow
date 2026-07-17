//! Checked operator decisions and their application to semantic plans.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use super::request::{canonical_presentation_source, validate_identity, validate_path_identity};
use super::BuildServiceError;
use crate::workflow::plan::{
    ItemKind, PlanDisposition, ReadyAction, RenderStyle, ResolvedBackground, ResolvedItemPlan,
};

/// Per-entry override applied during service build execution.
///
/// Public fields keep this type convenient at CLI and MCP translation
/// boundaries. `resolve_requested_plans` is the single checked transition
/// that rejects malformed or contradictory values before review capture.
#[derive(Debug, Clone)]
pub struct EntryOverride {
    /// Stable output identity of the plan entry being changed.
    pub output_key: String,
    /// Optional replacement playlist/presentation display name.
    pub playlist_name: Option<String>,
    /// Optional replacement playlist semantic type.
    pub slide_type: Option<OverrideSlideType>,
    /// Optional replacement operation.
    pub action: Option<OverrideAction>,
}

impl EntryOverride {
    fn validate(&self) -> Result<(), BuildServiceError> {
        validate_identity("override output_key", &self.output_key)?;
        if let Some(name) = &self.playlist_name {
            validate_identity("override playlist_name", name)?;
        }
        if let Some(action) = &self.action {
            action.validate()?;
        }
        if self.playlist_name.is_none() && self.slide_type.is_none() && self.action.is_none() {
            return Err(BuildServiceError::EmptyOverride {
                output_key: self.output_key.clone(),
            });
        }
        Ok(())
    }
}

/// One complete operator override for a planned presentation operation.
///
/// Each variant carries exactly the data required by that intent. In
/// particular, a read-only presentation cannot accidentally carry render
/// styling, and a rendered presentation cannot carry an arrangement.
#[derive(Debug, Clone)]
pub enum OverrideAction {
    /// Replace the proposed operation with one exact library presentation.
    UseExisting {
        /// Exact presentation file selected by the operator.
        file_path: PathBuf,
        /// Optional native arrangement name to select.
        arrangement: Option<String>,
    },
    /// Render the proposed content to a new presentation.
    GenerateNew {
        /// Optional replacement for the proposed render background.
        background: Option<ResolvedBackground>,
    },
    /// Render proposed description content back into an existing presentation.
    EditDescription {
        /// Exact presentation file selected by the operator.
        file_path: PathBuf,
        /// Optional replacement for the proposed render background.
        background: Option<ResolvedBackground>,
    },
    /// Keep the proposed render operation and replace only its background.
    SetBackground {
        /// Reviewed background asset to apply.
        background: ResolvedBackground,
    },
    /// Keep the proposed existing presentation and select one arrangement.
    SelectArrangement {
        /// Exact native arrangement name to select.
        arrangement: String,
    },
}

impl OverrideAction {
    fn validate(&self) -> Result<(), BuildServiceError> {
        match self {
            Self::UseExisting {
                file_path,
                arrangement,
            } => {
                validate_path_identity("override file_path", file_path)?;
                if let Some(arrangement) = arrangement {
                    validate_identity("override arrangement", arrangement)?;
                }
            }
            Self::EditDescription { file_path, .. } => {
                validate_path_identity("override file_path", file_path)?;
            }
            Self::SelectArrangement { arrangement } => {
                validate_identity("override arrangement", arrangement)?;
            }
            Self::GenerateNew { .. } | Self::SetBackground { .. } => {}
        }
        Ok(())
    }
}

/// Semantic slide role accepted at service-build boundaries.
#[derive(Debug, Clone, Copy, serde::Deserialize, schemars::JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OverrideSlideType {
    /// Generic text presentation.
    Text,
    /// Song lyric presentation.
    #[serde(alias = "song")]
    Lyrics,
    /// Scripture presentation.
    Scripture,
    /// Title presentation.
    Title,
    /// Graphic presentation.
    Graphic,
    /// Person or content nametag presentation.
    #[serde(alias = "person_nametag")]
    Nametag,
}

impl std::str::FromStr for OverrideSlideType {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.is_empty() || value.trim() != value || value.chars().any(char::is_control) {
            return Err(
                "slide type must be non-empty, unpadded, and contain no control characters"
                    .to_string(),
            );
        }
        match value.to_ascii_lowercase().as_str() {
            "text" => Ok(Self::Text),
            "lyrics" | "song" => Ok(Self::Lyrics),
            "scripture" => Ok(Self::Scripture),
            "title" => Ok(Self::Title),
            "graphic" => Ok(Self::Graphic),
            "nametag" | "person_nametag" => Ok(Self::Nametag),
            _ => Err(format!(
                "unknown slide type '{value}'; expected text, lyrics, scripture, title, graphic, or nametag"
            )),
        }
    }
}

pub(super) fn validate_request_edits(
    skip_output_keys: &[String],
    overrides: &[EntryOverride],
) -> Result<(), BuildServiceError> {
    let mut skip_keys = HashSet::new();
    for key in skip_output_keys {
        validate_identity("skip output_key", key)?;
        if !skip_keys.insert(key.as_str()) {
            return Err(BuildServiceError::message(format!(
                "duplicate skip_output_key '{key}'"
            )));
        }
    }

    let mut override_keys = HashSet::new();
    for entry in overrides {
        validate_identity("override output_key", &entry.output_key)?;
        if !override_keys.insert(entry.output_key.as_str()) {
            return Err(BuildServiceError::message(format!(
                "duplicate override for output_key '{}'",
                entry.output_key
            )));
        }
        if skip_keys.contains(entry.output_key.as_str()) {
            return Err(BuildServiceError::message(format!(
                "output_key '{}' cannot be both skipped and overridden",
                entry.output_key
            )));
        }
        entry.validate()?;
    }
    Ok(())
}

pub(super) fn resolve_requested_plans(
    plans: &[ResolvedItemPlan],
    skip_output_keys: &[String],
    overrides: &[EntryOverride],
) -> Result<Vec<ResolvedItemPlan>, BuildServiceError> {
    validate_request_edits(skip_output_keys, overrides)?;
    let skip_set = skip_output_keys
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let override_map = overrides
        .iter()
        .map(|entry| (entry.output_key.as_str(), entry))
        .collect::<HashMap<_, _>>();
    validate_requested_plan_keys(plans, &skip_set, &override_map)?;

    plans
        .iter()
        .map(|plan| {
            if skip_set.contains(plan.output_key.as_str()) {
                let mut skipped = plan.clone();
                skipped.disposition = PlanDisposition::Skip;
                skipped.reason = "Skipped by reviewed build request".to_string();
                Ok(skipped)
            } else {
                apply_override(plan, override_map.get(plan.output_key.as_str()).copied())
            }
        })
        .collect()
}

fn validate_requested_plan_keys(
    plans: &[ResolvedItemPlan],
    skip_set: &HashSet<&str>,
    override_map: &HashMap<&str, &EntryOverride>,
) -> Result<(), BuildServiceError> {
    let mut known_keys = HashSet::new();
    let mut duplicate_keys = Vec::new();
    for plan in plans {
        validate_plan_identity(plan)?;
        if !known_keys.insert(plan.output_key.as_str()) {
            duplicate_keys.push(plan.output_key.as_str());
        }
    }
    if !duplicate_keys.is_empty() {
        duplicate_keys.sort_unstable();
        duplicate_keys.dedup();
        return Err(BuildServiceError::message(format!(
            "duplicate plan output_keys: {}",
            duplicate_keys.join(", ")
        )));
    }
    let mut unknown_skips = skip_set
        .iter()
        .copied()
        .filter(|key| !known_keys.contains(key))
        .collect::<Vec<_>>();
    unknown_skips.sort_unstable();
    if !unknown_skips.is_empty() {
        return Err(BuildServiceError::message(format!(
            "unknown skip_output_keys: {}",
            unknown_skips.join(", ")
        )));
    }
    let mut unknown_overrides = override_map
        .keys()
        .copied()
        .filter(|key| !known_keys.contains(key))
        .collect::<Vec<_>>();
    unknown_overrides.sort_unstable();
    if !unknown_overrides.is_empty() {
        return Err(BuildServiceError::message(format!(
            "unknown override output_keys: {}",
            unknown_overrides.join(", ")
        )));
    }
    Ok(())
}

fn validate_plan_identity(plan: &ResolvedItemPlan) -> Result<(), BuildServiceError> {
    validate_identity("plan output_key", plan.output_key.as_str())?;
    validate_identity("plan playlist_name", &plan.playlist_name)?;
    let Some(action) = plan.preview_action() else {
        return Ok(());
    };
    match action {
        ReadyAction::UseExisting {
            file_path,
            arrangement,
        }
        | ReadyAction::RestyleExisting {
            file_path,
            arrangement,
            ..
        } => {
            validate_path_identity("plan file_path", file_path)?;
            if let Some(arrangement) = arrangement {
                validate_identity("plan arrangement", arrangement)?;
            }
        }
        ReadyAction::EditDescription { file_path, .. } => {
            validate_path_identity("plan file_path", file_path)?;
        }
        ReadyAction::GenerateDescription { .. }
        | ReadyAction::GenerateScripture { .. }
        | ReadyAction::GenerateTitle { .. } => {}
    }
    Ok(())
}

pub(super) fn apply_override(
    entry: &ResolvedItemPlan,
    override_entry: Option<&EntryOverride>,
) -> Result<ResolvedItemPlan, BuildServiceError> {
    let mut effective = entry.clone();
    if let Some(override_entry) = override_entry {
        override_entry.validate()?;
        if let Some(ref playlist_name) = override_entry.playlist_name {
            effective.playlist_name.clone_from(playlist_name);
        }
        if let Some(slide_type) = override_entry.slide_type {
            effective.item_kind = item_kind_from_override(slide_type);
            effective.item_type = item_type_from_override(slide_type);
        }
        if let Some(action) = &override_entry.action {
            effective.disposition = PlanDisposition::Ready(resolve_override_action(entry, action)?);
            effective.reason = "Build override action".to_string();
        }
    }
    Ok(effective)
}

fn resolve_override_action(
    plan: &ResolvedItemPlan,
    intent: &OverrideAction,
) -> Result<ReadyAction, BuildServiceError> {
    intent.validate()?;
    match intent {
        OverrideAction::UseExisting {
            file_path,
            arrangement,
        } => resolve_existing_source_override(plan, file_path, arrangement.clone()),
        OverrideAction::GenerateNew { background } => {
            let action = plan.preview_action().cloned().ok_or_else(|| {
                unsupported_override(plan, "generate content without a complete proposal")
            })?;
            let mut action = match action {
                ReadyAction::EditDescription {
                    parsed_content,
                    style,
                    ..
                } => ReadyAction::GenerateDescription {
                    parsed_content,
                    style,
                },
                action @ (ReadyAction::GenerateDescription { .. }
                | ReadyAction::GenerateScripture { .. }
                | ReadyAction::GenerateTitle { .. }) => action,
                ReadyAction::UseExisting { .. } | ReadyAction::RestyleExisting { .. } => {
                    return Err(unsupported_override(
                        plan,
                        "generate content from a use-existing proposal",
                    ));
                }
            };
            if let Some(background) = background {
                replace_render_background(plan, &mut action, background.clone())?;
            }
            Ok(action)
        }
        OverrideAction::EditDescription {
            file_path,
            background,
        } => {
            let action = plan.preview_action().cloned().ok_or_else(|| {
                unsupported_override(plan, "edit description without a complete proposal")
            })?;
            let (parsed_content, mut style) = match action {
                ReadyAction::EditDescription {
                    parsed_content,
                    style,
                    ..
                }
                | ReadyAction::GenerateDescription {
                    parsed_content,
                    style,
                } => (parsed_content, style),
                ReadyAction::UseExisting { .. }
                | ReadyAction::RestyleExisting { .. }
                | ReadyAction::GenerateScripture { .. }
                | ReadyAction::GenerateTitle { .. } => {
                    return Err(unsupported_override(
                        plan,
                        "edit an action that has no description content",
                    ));
                }
            };
            if let Some(background) = background {
                style = render_style_with_background(&style, background.clone())?;
            }
            Ok(ReadyAction::EditDescription {
                file_path: canonical_presentation_source(file_path)?,
                parsed_content,
                style,
            })
        }
        OverrideAction::SetBackground { background } => {
            let mut action = plan.preview_action().cloned().ok_or_else(|| {
                unsupported_override(plan, "set a background without a complete proposal")
            })?;
            replace_render_background(plan, &mut action, background.clone())?;
            Ok(action)
        }
        OverrideAction::SelectArrangement { arrangement } => match plan.preview_action() {
            Some(ReadyAction::UseExisting { file_path, .. }) => Ok(ReadyAction::UseExisting {
                file_path: file_path.clone(),
                arrangement: Some(arrangement.clone()),
            }),
            Some(ReadyAction::RestyleExisting {
                file_path,
                transform,
                ..
            }) => Ok(ReadyAction::RestyleExisting {
                file_path: file_path.clone(),
                arrangement: Some(arrangement.clone()),
                transform: transform.clone(),
            }),
            _ => Err(unsupported_override(
                plan,
                "select an arrangement for an action without an existing source",
            )),
        },
    }
}

fn resolve_existing_source_override(
    plan: &ResolvedItemPlan,
    file_path: &std::path::Path,
    arrangement: Option<String>,
) -> Result<ReadyAction, BuildServiceError> {
    let file_path = canonical_presentation_source(file_path)?;
    Ok(match plan.preview_action() {
        Some(ReadyAction::RestyleExisting { transform, .. }) => ReadyAction::RestyleExisting {
            file_path,
            arrangement,
            transform: transform.clone(),
        },
        _ => ReadyAction::UseExisting {
            file_path,
            arrangement,
        },
    })
}

fn replace_render_background(
    plan: &ResolvedItemPlan,
    action: &mut ReadyAction,
    background: ResolvedBackground,
) -> Result<(), BuildServiceError> {
    match action {
        ReadyAction::RestyleExisting { transform, .. } => {
            *transform = transform.clone().with_replacement_background(background);
            Ok(())
        }
        ReadyAction::EditDescription { style, .. }
        | ReadyAction::GenerateDescription { style, .. }
        | ReadyAction::GenerateScripture { style, .. }
        | ReadyAction::GenerateTitle { style, .. } => {
            *style = render_style_with_background(style, background)?;
            Ok(())
        }
        ReadyAction::UseExisting { .. } => Err(unsupported_override(
            plan,
            "apply a render background to a read-only presentation",
        )),
    }
}

fn render_style_with_background(
    style: &RenderStyle,
    background: ResolvedBackground,
) -> Result<RenderStyle, BuildServiceError> {
    RenderStyle::new(
        Some(background),
        style.content().clone(),
        style.title().cloned(),
        style.max_lines_per_slide(),
    )
    .map_err(|error| {
        BuildServiceError::message(format!(
            "failed to apply background to checked render style: {error}"
        ))
    })
}

fn unsupported_override(plan: &ResolvedItemPlan, intent: &str) -> BuildServiceError {
    BuildServiceError::message(format!(
        "override for '{}' cannot {intent}",
        plan.output_key
    ))
}

const fn item_kind_from_override(slide_type: OverrideSlideType) -> ItemKind {
    match slide_type {
        OverrideSlideType::Lyrics => ItemKind::Song,
        OverrideSlideType::Scripture => ItemKind::Scripture,
        OverrideSlideType::Title | OverrideSlideType::Nametag => ItemKind::Nametag,
        OverrideSlideType::Graphic => ItemKind::Graphic,
        OverrideSlideType::Text => ItemKind::Other,
    }
}

fn item_type_from_override(slide_type: OverrideSlideType) -> Option<String> {
    match slide_type {
        OverrideSlideType::Lyrics => Some("song".to_string()),
        OverrideSlideType::Scripture => Some("scripture".to_string()),
        OverrideSlideType::Title | OverrideSlideType::Nametag => Some("title".to_string()),
        OverrideSlideType::Text | OverrideSlideType::Graphic => None,
    }
}
