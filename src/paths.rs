//! Shared path resolution helpers.
//!
//! This module contains filesystem lookup logic that should be available to
//! headless runtime layers like MCP without depending on the TUI app.

use std::path::PathBuf;

/// Project config filename stored under the data directory.
pub const PROJECT_CONFIG_FILE: &str = "proflow.config.json";

/// Resolve the one data-bundle root used by this process.
///
/// An explicit `PROFLOW_DATA` value is authoritative even when it is missing;
/// callers then report the missing asset instead of silently mixing in files
/// from another installation. Without an override, the first existing bundle
/// root wins as a whole.
#[must_use]
pub fn data_root() -> PathBuf {
    if let Some(base) = std::env::var_os("PROFLOW_DATA") {
        return PathBuf::from(base);
    }

    // During repository-local development, keep the checked-in bundle
    // coherent even if an unrelated installed-state directory also exists.
    let workspace_data = PathBuf::from("data");
    if PathBuf::from("Cargo.toml").is_file() && workspace_data.is_dir() {
        return workspace_data;
    }

    let mut candidates = Vec::new();
    if let Some(data) = dirs::data_dir() {
        candidates.push(data.join("proflow"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("data"));
        }
    }
    candidates.push(workspace_data);

    candidates
        .into_iter()
        .find(|path| path.is_dir())
        .unwrap_or_else(|| PathBuf::from("data"))
}

/// Locate a subdirectory inside the process's resolved data bundle.
#[must_use]
pub fn find_data_subdir(subdir: &str) -> PathBuf {
    data_root().join(subdir)
}

/// Locate the project config file path under the resolved data directory.
#[must_use]
pub fn project_config_path() -> PathBuf {
    data_root().join(PROJECT_CONFIG_FILE)
}
