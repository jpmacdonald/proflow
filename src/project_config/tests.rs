#![allow(clippy::expect_used, clippy::unwrap_used)]

mod invariants;
mod presentation_validation;
mod production_config;
mod serde_and_storage;
mod validation;

/// Parse the live project config from [`crate::paths::live_test_data_root`].
///
/// Callers must be `#[ignore]`d tests; `just live-data` runs them.
pub fn live_project_config() -> super::ProjectConfig {
    let path = crate::paths::live_test_data_root().join("proflow.config.json");
    let json = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read live project config {}: {error}", path.display()));
    super::parse_project_config_str(&json).expect("live project config should parse")
}
