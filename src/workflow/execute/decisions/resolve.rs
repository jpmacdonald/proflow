//! Resolve recorded operator choices into checked overrides.

use super::{
    expand_user_path, BackgroundId, DecisionError, EntryOverrideAction, OverrideAction,
    PlaylistExportIntent, PlaylistExportMode, PlaylistMediaAsset, ProjectConfig,
    ResolvedBackground,
};
use std::path::PathBuf;

/// Build the playlist export intent a request asks for.
///
/// Media assets only exist in a portable package; supplying them with
/// `LibraryLinks` would silently drop them, so that combination is rejected.
pub fn playlist_export_intent(
    mode: PlaylistExportMode,
    media_assets: Vec<PlaylistMediaAsset>,
) -> Result<PlaylistExportIntent, DecisionError> {
    match mode {
        PlaylistExportMode::LibraryLinks if media_assets.is_empty() => {
            Ok(PlaylistExportIntent::library_links())
        }
        PlaylistExportMode::LibraryLinks => Err(DecisionError::MediaWithoutPortablePackage),
        PlaylistExportMode::PortableImport => {
            Ok(PlaylistExportIntent::portable_import(media_assets))
        }
    }
}

/// Resolve one operator action against the project configuration.
pub fn resolve_override_action(
    config: &ProjectConfig,
    action: EntryOverrideAction,
) -> Result<OverrideAction, DecisionError> {
    resolve_action(action, |id| resolve_background(config, id))
}

/// Resolve an action that names no background.
pub(in crate::workflow::execute) fn resolve_without_background(
    action: EntryOverrideAction,
) -> Result<OverrideAction, DecisionError> {
    resolve_action(action, |id| {
        Err(DecisionError::UnknownBackground {
            id,
            available: String::new(),
        })
    })
}

fn resolve_action(
    action: EntryOverrideAction,
    background_for: impl Fn(BackgroundId) -> Result<ResolvedBackground, DecisionError>,
) -> Result<OverrideAction, DecisionError> {
    Ok(match action {
        EntryOverrideAction::UseExisting {
            file_path,
            arrangement,
        } => OverrideAction::UseExisting {
            file_path: operator_path(file_path)?,
            arrangement: arrangement
                .map(|arrangement| exact("arrangement", arrangement))
                .transpose()?,
        },
        EntryOverrideAction::GenerateNew { background } => OverrideAction::GenerateNew {
            background: background.map(&background_for).transpose()?,
        },
        EntryOverrideAction::EditDescription {
            file_path,
            background,
        } => OverrideAction::EditDescription {
            file_path: operator_path(file_path)?,
            background: background.map(&background_for).transpose()?,
        },
        EntryOverrideAction::SetBackground { background } => OverrideAction::SetBackground {
            background: background_for(background)?,
        },
        EntryOverrideAction::SelectArrangement { arrangement } => {
            OverrideAction::SelectArrangement {
                arrangement: exact("arrangement", arrangement)?,
            }
        }
    })
}

fn resolve_background(
    config: &ProjectConfig,
    id: BackgroundId,
) -> Result<ResolvedBackground, DecisionError> {
    match config.backgrounds().get(&id).cloned() {
        Some(file) => Ok(ResolvedBackground::new(id, file)),
        None => Err(unknown_background(config, id)),
    }
}

pub(super) fn unknown_background(config: &ProjectConfig, id: BackgroundId) -> DecisionError {
    DecisionError::UnknownBackground {
        id,
        available: config
            .backgrounds()
            .keys()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", "),
    }
}

fn operator_path(value: String) -> Result<PathBuf, DecisionError> {
    Ok(expand_user_path(exact("file_path", value)?))
}

pub(super) fn exact(field: &'static str, value: String) -> Result<String, DecisionError> {
    if value.is_empty() || value.trim() != value || value.chars().any(char::is_control) {
        Err(DecisionError::InvalidField { field })
    } else {
        Ok(value)
    }
}
