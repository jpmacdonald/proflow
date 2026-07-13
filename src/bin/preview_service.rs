//! Preview a `ProPresenter` service playlist from a Planning Center plan.
//!
//! Usage:
//! ```text
//! cargo run --bin preview_service -- <plan_id> <service_name>
//! ```

use std::path::PathBuf;

use anyhow::Context;
use proflow::config::Config;
use proflow::paths::project_config_path;
use proflow::planning_center::PlanningCenterClient;
use proflow::project_config::{load_project_config, validate_project_config};
use proflow::utils::file_index::FileIndex;
use proflow::workflow::classify;
use serde::Serialize;

#[derive(Debug, Serialize)]
struct PreviewOutput {
    plan_id: String,
    service_name: String,
    entries: Vec<classify::PreviewEntry>,
    summary: classify::PreviewSummary,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let plan_id = args
        .next()
        .context("usage: preview_service <plan_id> <service_name>")?;
    let service_name = args
        .next()
        .context("usage: preview_service <plan_id> <service_name>")?;

    let config = Config::load()?;
    let mappings = load_project_config(&project_config_path())?;
    let issues = validate_project_config(&mappings);
    if !issues.is_empty() {
        for issue in &issues {
            eprintln!("Config error at {}: {}", issue.path, issue.message);
        }
        anyhow::bail!("config validation failed");
    }

    let library_path = env_path("LIBRARY_DIR")
        .or_else(proflow::utils::file_index::get_default_library_path)
        .context("LIBRARY_DIR or the default ProPresenter library path is required")?;
    let file_index = FileIndex::build(&library_path)?;

    let client = PlanningCenterClient::new(&config);
    let items = client.get_service_items(&plan_id).await?;
    let entries = classify::build_preview(
        &items,
        &mappings,
        Some(&file_index),
        Some(service_name.as_str()),
    );
    let summary = classify::PreviewSummary::from_entries(&entries);

    let output = PreviewOutput {
        plan_id,
        service_name,
        entries,
        summary,
    };

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var(name)
        .ok()
        .map(|value| PathBuf::from(shellexpand::tilde(&value).to_string()))
}
