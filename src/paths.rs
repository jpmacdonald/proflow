//! Shared path resolution helpers.
//!
//! This module contains filesystem lookup logic that should be available to
//! headless runtime layers like MCP without depending on the TUI app.

use std::path::PathBuf;

/// Project config filename stored under the data directory.
pub const PROJECT_CONFIG_FILE: &str = "proflow.config.json";

/// Locate a subdirectory under the app's bundled data folder.
///
/// Search order:
/// 1. `$PROFLOW_DATA/<subdir>` (explicit override)
/// 2. `<data_dir>/proflow/<subdir>` (installed location via `dirs::data_dir`)
/// 3. `<exe_dir>/data/<subdir>` (next to the binary)
/// 4. `data/<subdir>` (cwd fallback, works during `cargo run`)
#[must_use]
pub fn find_data_subdir(subdir: &str) -> PathBuf {
    // Explicit override
    if let Ok(base) = std::env::var("PROFLOW_DATA") {
        let p = PathBuf::from(base).join(subdir);
        if p.is_dir() {
            return p;
        }
    }

    // Platform data dir (~/Library/Application Support/proflow/ on macOS)
    if let Some(data) = dirs::data_dir() {
        let p = data.join("proflow").join(subdir);
        if p.is_dir() {
            return p;
        }
    }

    // Next to the executable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("data").join(subdir);
            if p.is_dir() {
                return p;
            }
        }
    }

    // Fallback: cwd (works during cargo run)
    PathBuf::from("data").join(subdir)
}

/// Locate the project config file path under the resolved data directory.
#[must_use]
pub(crate) fn project_config_path() -> PathBuf {
    let mut candidates = Vec::new();

    if let Ok(base) = std::env::var("PROFLOW_DATA") {
        candidates.push(PathBuf::from(base).join(PROJECT_CONFIG_FILE));
    }

    if let Some(data) = dirs::data_dir() {
        candidates.push(data.join("proflow").join(PROJECT_CONFIG_FILE));
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("data").join(PROJECT_CONFIG_FILE));
        }
    }

    candidates.push(PathBuf::from("data").join(PROJECT_CONFIG_FILE));

    candidates
        .into_iter()
        .find(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("data").join(PROJECT_CONFIG_FILE))
}
