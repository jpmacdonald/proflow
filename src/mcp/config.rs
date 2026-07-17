//! Validation and activation of MCP-authored project configuration.

use std::path::{Path, PathBuf};

use chrono::Utc;

use crate::paths::{BuildLocationInputs, BuildLocations, BuildLocationsError};
use crate::project_config::{write_project_config, ProjectConfig};
use crate::workflow::execute::{RenderAssetSnapshot, RenderAssetSnapshotError};

use super::mcp_err;
use super::schema::ConfigValidationResponse;

#[derive(Debug, thiserror::Error)]
enum ConfigCandidateValidationError {
    #[error("failed to resolve candidate config locations: {0}")]
    Locations(#[from] BuildLocationsError),
    #[error("failed to load configured render assets: {0}")]
    Assets(#[from] RenderAssetSnapshotError),
}

pub(super) struct ConfigWriteOutcome {
    pub(super) path: PathBuf,
    pub(super) backup_path: Option<PathBuf>,
    pub(super) activated: bool,
}

const fn checked_config_validation() -> ConfigValidationResponse {
    ConfigValidationResponse {
        valid: true,
        issues: Vec::new(),
    }
}

fn validate_candidate(
    config: &ProjectConfig,
    current_locations: &BuildLocations,
) -> Result<BuildLocations, ConfigCandidateValidationError> {
    let locations = candidate_locations(config, current_locations)?;
    RenderAssetSnapshot::load(config.clone(), locations.clone())?;
    Ok(locations)
}

pub(super) fn candidate_locations(
    config: &ProjectConfig,
    current: &BuildLocations,
) -> Result<BuildLocations, BuildLocationsError> {
    BuildLocations::from_inputs(BuildLocationInputs {
        project_data_root: current.project_data_root().to_path_buf(),
        presentation_library: current
            .propresenter_root()
            .join("Libraries")
            .join(config.defaults().library.as_str()),
        playlist_output: current.playlist_output().to_path_buf(),
        propresenter_root: current.propresenter_root().to_path_buf(),
        themes: current.themes().to_path_buf(),
        macros: current.macros().to_path_buf(),
    })
}

pub(super) fn write_config_reviewed(
    config: &ProjectConfig,
    locations: &BuildLocations,
    activate: bool,
    name: Option<&str>,
) -> Result<(ConfigWriteOutcome, ConfigValidationResponse), rmcp::ErrorData> {
    let validation = checked_config_validation();
    let candidate_locations =
        validate_candidate(config, locations).map_err(|error| mcp_err(error.to_string()))?;
    let live_path = candidate_locations.project_config().to_path_buf();
    let write_path = if activate {
        live_path.clone()
    } else {
        candidate_config_path(candidate_locations.project_data_root(), name)
    };

    let backup_path = if activate && live_path.is_file() {
        let backup = backup_config_path(&live_path);
        let parent = backup.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent).map_err(|error| mcp_err(error.to_string()))?;
        std::fs::copy(&live_path, &backup).map_err(|error| mcp_err(error.to_string()))?;
        Some(backup)
    } else {
        None
    };

    write_project_config(&write_path, config.as_raw())
        .map_err(|error| mcp_err(error.to_string()))?;

    Ok((
        ConfigWriteOutcome {
            path: write_path,
            backup_path,
            activated: activate,
        },
        validation,
    ))
}

fn candidate_config_path(project_data_root: &Path, name: Option<&str>) -> PathBuf {
    let dir = project_data_root.join("config-candidates");
    let stamp = Utc::now().format("%Y%m%d-%H%M%S-%3f");
    let label = name
        .map(config_file_slug)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "candidate".to_string());
    dir.join(format!("{label}-{stamp}-{}.json", uuid::Uuid::new_v4()))
}

pub(super) fn backup_config_path(live_path: &Path) -> PathBuf {
    let base_dir = live_path
        .parent()
        .map_or_else(|| PathBuf::from("data"), Path::to_path_buf);
    let dir = base_dir.join("config-backups");
    let stem = live_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("proflow.config");
    let stamp = Utc::now().format("%Y%m%d-%H%M%S-%3f");
    dir.join(format!("{stem}-{stamp}-{}.json", uuid::Uuid::new_v4()))
}

fn config_file_slug(name: &str) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            slug.push('-');
            prev_dash = true;
        }
    }
    slug.trim_matches('-').to_string()
}
