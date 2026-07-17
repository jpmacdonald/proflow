//! Project config parsing, serialization, and persistence.

use super::{
    validation::{format_validation_issues, validate_project_config},
    ProjectConfig, RawProjectConfig,
};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

/// Error returned while reading project config.
#[derive(Debug, thiserror::Error)]
pub enum ProjectConfigLoadError {
    /// Failed to read the config file.
    #[error("failed to read project config: {0}")]
    Io(#[from] std::io::Error),
    /// Failed to parse the config file.
    #[error("failed to parse project config: {0}")]
    Parse(#[from] serde_json::Error),
    /// Encountered an unsupported or missing config version.
    #[error("unsupported project config version: {0} — migrate to v4")]
    UnsupportedVersion(u64),
    /// Config is missing a version field entirely.
    #[error("config has no version field — migrate to v4")]
    MissingVersion,
    /// Config parsed successfully but violates its domain contract.
    #[error("invalid project config: {0}")]
    Invalid(String),
}

/// Load project config from a file path.
pub fn load_project_config(path: &Path) -> Result<ProjectConfig, ProjectConfigLoadError> {
    let text = std::fs::read_to_string(path)?;
    parse_project_config_str(&text)
}

/// Parse project config from a JSON value.
pub fn parse_project_config_value(
    value: serde_json::Value,
) -> Result<ProjectConfig, ProjectConfigLoadError> {
    match value.get("version").and_then(serde_json::Value::as_u64) {
        Some(4) => {
            let raw = serde_json::from_value::<RawProjectConfig>(value)?;
            ProjectConfig::try_from(raw)
                .map_err(|error| ProjectConfigLoadError::Invalid(error.to_string()))
        }
        Some(version) => Err(ProjectConfigLoadError::UnsupportedVersion(version)),
        None => Err(ProjectConfigLoadError::MissingVersion),
    }
}

/// Parse project config from a JSON string.
pub fn parse_project_config_str(json: &str) -> Result<ProjectConfig, ProjectConfigLoadError> {
    let value: serde_json::Value = serde_json::from_str(json)?;
    parse_project_config_value(value)
}

/// Serialize project config to pretty JSON with a trailing newline.
pub fn serialize_project_config(config: &RawProjectConfig) -> Result<String, serde_json::Error> {
    let mut json = serde_json::to_string_pretty(config)?;
    json.push('\n');
    Ok(json)
}

/// Write project config atomically to disk.
pub fn write_project_config(path: &Path, config: &RawProjectConfig) -> std::io::Result<()> {
    let issues = validate_project_config(config);
    if !issues.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "invalid project config: {}",
                format_validation_issues(&issues)
            ),
        ));
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if path.file_name().is_none() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("project config path has no filename: {}", path.display()),
        ));
    }
    std::fs::create_dir_all(parent)?;

    let json = serialize_project_config(config)
        .map_err(|err| std::io::Error::other(format!("serialize project config: {err}")))?;
    let temp_path = parent.join(format!(".proflow-config.{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp_path, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    result
}
