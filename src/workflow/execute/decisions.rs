//! The operator's recorded decisions for one plan: its `review.toml`.
//!
//! The file sits at `<data root>/builds/<plan id>/review.toml`. It carries
//! exactly what a transport would otherwise pass in a request: the playlist
//! name, the package mode, extra media, and one `[[decision]]` per plan item
//! the operator skipped or changed. An item with no decision is unanswered.
//!
//! ```toml
//! playlist_name = "May 24, 2026 - Sunday Morning"
//! package_mode = "library_local"
//!
//! [[decision]]
//! item = "pco:101:main"
//! question = "Two library presentations are titled 'Amazing Grace'"
//! decided_by = "operator"
//! at = "2026-05-23T19:30:00Z"
//!
//! [decision.action]
//! kind = "use_existing"
//! file_path = "/Users/me/Documents/ProPresenter/Libraries/Default/Amazing Grace.pro"
//!
//! [[decision]]
//! item = "pco:102:main"
//! skip = true
//! ```
//!
//! `question`, `options`, `decided_by` and `at` describe a decision for a
//! reader; they do not change what is built.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::paths::expand_user_path;
use crate::project_config::{BackgroundId, ProjectConfig};
use crate::workflow::plan::ResolvedBackground;
use proflow_propresenter::playlist::{
    PlaylistExportIntent, PlaylistExportMode, PlaylistMediaAsset,
};

use super::overrides::{EntryOverride, OverrideAction, OverrideSlideType};
use super::request::BuildRequest;

/// Complete operator intent for replacing or refining one preview action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EntryOverrideAction {
    /// Reuse one exact existing presentation and optional arrangement.
    UseExisting {
        /// Existing `.pro` file selected by the operator.
        file_path: String,
        /// Optional exact native arrangement name.
        arrangement: Option<String>,
    },
    /// Render the proposed content as a new presentation.
    GenerateNew {
        /// Optional replacement background ID from project config.
        #[schemars(with = "Option<String>")]
        background: Option<BackgroundId>,
    },
    /// Render proposed description content into one existing presentation.
    EditDescription {
        /// Existing `.pro` file whose owned envelope will be preserved.
        file_path: String,
        /// Optional replacement background ID from project config.
        #[schemars(with = "Option<String>")]
        background: Option<BackgroundId>,
    },
    /// Keep the proposed render action and replace only its background.
    SetBackground {
        /// Replacement background ID from project config.
        #[schemars(with = "String")]
        background: BackgroundId,
    },
    /// Keep the proposed existing presentation and select its arrangement.
    SelectArrangement {
        /// Exact native arrangement name.
        arrangement: String,
    },
}

/// Every recorded decision for one plan.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewDecisions {
    /// Playlist display and file name; derived from the plan date when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playlist_name: Option<String>,
    /// Linked-library or portable package.
    #[serde(default)]
    pub package_mode: PlaylistExportMode,
    /// Extra media files to carry in a portable package.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media: Vec<MediaDecision>,
    /// One entry per plan item the operator skipped or changed.
    #[serde(default, rename = "decision", skip_serializing_if = "Vec::is_empty")]
    pub decisions: Vec<ItemDecision>,
}

/// One extra media file for a portable package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaDecision {
    /// Absolute path to the media file.
    pub path: String,
    /// Confined path inside the package; the source path when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_path: Option<String>,
}

/// Who made a recorded decision.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecidedBy {
    /// A person reviewing the plan.
    #[default]
    Operator,
    /// A configured rule, recorded for the reader.
    Rule,
}

/// The decision for one plan item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemDecision {
    /// Stable output key of the plan item, as the review printed it.
    pub item: String,
    /// What the review asked, kept for the reader.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
    /// The choices the review offered, kept for the reader.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    /// Who decided.
    #[serde(default)]
    pub decided_by: DecidedBy,
    /// When the decision was made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<chrono::DateTime<chrono::Utc>>,
    /// Leave the item out of the playlist. Excludes every field below.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub skip: bool,
    /// Replacement playlist and presentation name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playlist_name: Option<String>,
    /// Replacement slide type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slide_type: Option<OverrideSlideType>,
    /// Replacement operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<EntryOverrideAction>,
}

/// Why recorded decisions could not become a build request.
#[derive(Debug, thiserror::Error)]
pub enum DecisionError {
    /// The decisions file exists but could not be read.
    #[error("failed to read review decisions '{}': {source}", path.display())]
    Read {
        /// Decisions file.
        path: PathBuf,
        /// Underlying filesystem failure.
        source: std::io::Error,
    },
    /// The decisions file is not valid TOML of the expected shape.
    #[error("review decisions '{}' are invalid: {message}", path.display())]
    Parse {
        /// Decisions file.
        path: PathBuf,
        /// Parser diagnostic with its line and column.
        message: String,
    },
    /// Two decisions name the same plan item.
    #[error("review decisions answer item '{item}' more than once")]
    DuplicateItem {
        /// Repeated output key.
        item: String,
    },
    /// A skipped item also carries a change.
    #[error("review decision for item '{item}' cannot both skip it and change it")]
    SkipWithChanges {
        /// Output key with the contradictory decision.
        item: String,
    },
    /// A background id is not defined by the project configuration.
    #[error("unknown background id '{id}'; configured backgrounds: {available}")]
    UnknownBackground {
        /// Requested id.
        id: BackgroundId,
        /// Sorted, comma-separated configured ids.
        available: String,
    },
    /// A path or arrangement name cannot be an exact lookup key.
    #[error("override {field} must be non-empty, unpadded, and contain no control characters")]
    InvalidField {
        /// Offending field.
        field: &'static str,
    },
    /// Extra media was supplied for a package that carries none.
    #[error(
        "media_assets require package_mode export_portable; library_local exports carry no media"
    )]
    MediaWithoutPortablePackage,
}

impl ReviewDecisions {
    /// Read a plan's decisions file. A plan nobody has decided anything about
    /// has no file, which is the empty set of decisions.
    pub fn read(path: &Path) -> Result<Self, DecisionError> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(source) => {
                return Err(DecisionError::Read {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };
        toml::from_str(&text).map_err(|error| DecisionError::Parse {
            path: path.to_path_buf(),
            message: error.to_string(),
        })
    }

    /// Render these decisions as the text of a decisions file.
    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string(self)
    }

    /// Turn the recorded decisions into the request a review consumes.
    pub fn into_request(
        self,
        plan_id: &str,
        config: &ProjectConfig,
    ) -> Result<BuildRequest, DecisionError> {
        let mut seen = BTreeSet::new();
        let mut skip_output_keys = Vec::new();
        let mut overrides = Vec::new();
        for decision in self.decisions {
            if !seen.insert(decision.item.clone()) {
                return Err(DecisionError::DuplicateItem {
                    item: decision.item,
                });
            }
            if !decision.skip {
                overrides.push(EntryOverride {
                    output_key: decision.item,
                    playlist_name: decision.playlist_name,
                    slide_type: decision.slide_type,
                    action: decision
                        .action
                        .map(|action| resolve_override_action(config, action))
                        .transpose()?,
                });
            } else if decision.playlist_name.is_some()
                || decision.slide_type.is_some()
                || decision.action.is_some()
            {
                return Err(DecisionError::SkipWithChanges {
                    item: decision.item,
                });
            } else {
                skip_output_keys.push(decision.item);
            }
        }
        Ok(BuildRequest {
            plan_id: plan_id.to_string(),
            service_name: None,
            playlist_name: self.playlist_name,
            skip_output_keys,
            overrides,
            playlist_export: playlist_export_intent(
                self.package_mode,
                self.media
                    .into_iter()
                    .map(|media| PlaylistMediaAsset {
                        source_path: PathBuf::from(media.path),
                        archive_path: media.archive_path,
                    })
                    .collect(),
            )?,
        })
    }
}

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
            background: background
                .map(|id| resolve_background(config, id))
                .transpose()?,
        },
        EntryOverrideAction::EditDescription {
            file_path,
            background,
        } => OverrideAction::EditDescription {
            file_path: operator_path(file_path)?,
            background: background
                .map(|id| resolve_background(config, id))
                .transpose()?,
        },
        EntryOverrideAction::SetBackground { background } => OverrideAction::SetBackground {
            background: resolve_background(config, background)?,
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
        None => Err(DecisionError::UnknownBackground {
            id,
            available: config
                .backgrounds()
                .keys()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", "),
        }),
    }
}

fn operator_path(value: String) -> Result<PathBuf, DecisionError> {
    Ok(expand_user_path(exact("file_path", value)?))
}

fn exact(field: &'static str, value: String) -> Result<String, DecisionError> {
    if value.is_empty() || value.trim() != value || value.chars().any(char::is_control) {
        Err(DecisionError::InvalidField { field })
    } else {
        Ok(value)
    }
}

#[cfg(test)]
mod tests;
