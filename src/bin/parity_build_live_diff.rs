//! Build a service from PCO, then compare it against a live `ProPresenter` playlist.
//!
//! Usage:
//!   `cargo run --features dev-tools --bin parity_build_live_diff -- <ProPresenter root> <plan id> <service name> <live playlist name> [work dir]`

#![allow(clippy::print_stdout)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{Context, Result};
use proflow::bible::BibleService;
use proflow::config::Config;
use proflow::paths::{BuildLocationInputs, BuildLocations};
use proflow::planning_center::api::PlanningCenterClient;
use proflow::project_config::load_project_config;
use proflow::propresenter::library::LibraryCatalog;
use proflow::propresenter::live::{materialize_live_playlist, LivePlaylistMaterializeReport};
use proflow::propresenter::package::{
    compare_playlist_items_aligned, compare_playlist_packages, embedded_presentation_structures,
    infer_archive_shape, presentation_items, read_playlist_package, EmbeddedPresentationStructure,
    PlaylistArchiveShape, PlaylistItemAlignedDiff, PlaylistItemSummary, PlaylistPackageComparison,
};
use proflow::propresenter::playlist::{PlaylistExportIntent, PlaylistMetadata};
use proflow::workflow::execute::{BuildRequest, RenderAssetSnapshot, ServiceBuildExecutor};
use proflow::workflow::report::BuildServiceResult;
use serde::Serialize;
use tokio::sync::Mutex;
use uuid::Uuid;
use walkdir::WalkDir;

#[derive(Debug, Serialize)]
struct ManualExpectedPlaylistItem {
    policy: String,
    reason: String,
    item: PlaylistItemSummary,
}

#[derive(Debug, Serialize)]
struct ParityBuildLiveDiffReport {
    work_dir: String,
    generated_library_dir: String,
    materialized: LivePlaylistMaterializeReport,
    build: BuildServiceResult,
    expected_path: String,
    actual_path: String,
    compatible: bool,
    expected_shape: PlaylistArchiveShape,
    actual_shape: PlaylistArchiveShape,
    package: PlaylistPackageComparison,
    expected_items: Vec<PlaylistItemSummary>,
    actual_items: Vec<PlaylistItemSummary>,
    manual_expected_items: Vec<ManualExpectedPlaylistItem>,
    aligned_item_diffs: Vec<PlaylistItemAlignedDiff>,
    actionable_item_compatible: bool,
    expected_presentations: Vec<EmbeddedPresentationStructure>,
    actual_presentations: Vec<EmbeddedPresentationStructure>,
}

struct ParityCliArgs {
    root: PathBuf,
    plan_id: String,
    service_name: String,
    live_playlist_name: String,
    work_dir: PathBuf,
}

struct ShadowLibraryPaths {
    generated_library: PathBuf,
    playlist_output: PathBuf,
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(report) => {
            match serde_json::to_string_pretty(&report) {
                Ok(json) => println!("{json}"),
                Err(err) => {
                    eprintln!("failed to render report: {err}");
                    return ExitCode::FAILURE;
                }
            }
            if report.compatible {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(err) => {
            eprintln!("{err:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<ParityBuildLiveDiffReport> {
    let ParityCliArgs {
        root,
        plan_id,
        service_name,
        live_playlist_name,
        work_dir,
    } = parse_args()?;
    let ShadowLibraryPaths {
        generated_library: generated_library_dir,
        playlist_output: playlist_output_dir,
    } = prepare_shadow_library(&root, &work_dir)?;

    let config = Config::load().context("load environment config")?;
    let locations = BuildLocations::from_inputs(BuildLocationInputs {
        project_data_root: BuildLocations::discover_project_data_root()?,
        presentation_library: generated_library_dir.clone(),
        playlist_output: playlist_output_dir.clone(),
        propresenter_root: root.clone(),
        themes: root.join("Themes"),
        macros: root.join("Configuration/Macros"),
    })?;
    let mappings = load_project_config(locations.project_config())
        .with_context(|| format!("load {}", locations.project_config().display()))?;

    let pco_client =
        PlanningCenterClient::new(&config).context("initialize Planning Center HTTP client")?;
    let bible_service = Arc::new(Mutex::new(BibleService::new(
        locations.project_data_root().join("bibles"),
    )));
    let file_index = Arc::new(Mutex::new(
        LibraryCatalog::build(&generated_library_dir)
            .with_context(|| format!("index shadow library {}", generated_library_dir.display()))?,
    ));
    let playlist_metadata = PlaylistMetadata::read_from_propresenter_root(&root)?;
    let render_assets = RenderAssetSnapshot::load(mappings, locations)?;
    let executor = ServiceBuildExecutor::new(
        &pco_client,
        &bible_service,
        &file_index,
        &render_assets,
        &playlist_metadata,
    );

    let build = executor
        .build_service(&BuildRequest {
            plan_id,
            service_name: Some(service_name),
            playlist_name: Some(live_playlist_name.clone()),
            skip_output_keys: Vec::new(),
            overrides: Vec::new(),
            playlist_export: PlaylistExportIntent::library_links(),
        })
        .await
        .context("build service from PCO plan")?;

    build_report(
        &root,
        &work_dir,
        &generated_library_dir,
        &live_playlist_name,
        build,
    )
}

fn parse_args() -> Result<ParityCliArgs> {
    let mut args = std::env::args_os().skip(1);
    let root = args.next().map(PathBuf::from).context(
        "usage: parity_build_live_diff <ProPresenter root> <plan id> <service name> <live playlist name> [work dir]",
    )?;
    let plan_id = args
        .next()
        .map(|value| value.to_string_lossy().to_string())
        .context(
            "usage: parity_build_live_diff <ProPresenter root> <plan id> <service name> <live playlist name> [work dir]",
        )?;
    let service_name = args
        .next()
        .map(|value| value.to_string_lossy().to_string())
        .context(
            "usage: parity_build_live_diff <ProPresenter root> <plan id> <service name> <live playlist name> [work dir]",
        )?;
    let live_playlist_name = args
        .next()
        .map(|value| value.to_string_lossy().to_string())
        .context(
            "usage: parity_build_live_diff <ProPresenter root> <plan id> <service name> <live playlist name> [work dir]",
        )?;
    let work_dir = args.next().map_or_else(
        || std::env::temp_dir().join(format!("proflow-parity-build-{}", Uuid::new_v4())),
        PathBuf::from,
    );
    if args.next().is_some() {
        anyhow::bail!(
            "usage: parity_build_live_diff <ProPresenter root> <plan id> <service name> <live playlist name> [work dir]"
        );
    }

    Ok(ParityCliArgs {
        root,
        plan_id,
        service_name,
        live_playlist_name,
        work_dir,
    })
}

fn prepare_shadow_library(root: &Path, work_dir: &Path) -> Result<ShadowLibraryPaths> {
    let live_library_dir = root.join("Libraries").join("Default");
    let generated_library_dir = work_dir.join("Libraries").join("Default");
    let playlist_output_dir = work_dir.join("Playlists");
    std::fs::create_dir_all(&generated_library_dir).with_context(|| {
        format!(
            "create generated library directory {}",
            generated_library_dir.display()
        )
    })?;
    std::fs::create_dir_all(&playlist_output_dir).with_context(|| {
        format!(
            "create playlist output directory {}",
            playlist_output_dir.display()
        )
    })?;
    copy_live_presentations(&live_library_dir, &generated_library_dir)?;

    Ok(ShadowLibraryPaths {
        generated_library: generated_library_dir,
        playlist_output: playlist_output_dir,
    })
}

fn build_report(
    root: &Path,
    work_dir: &Path,
    generated_library_dir: &Path,
    live_playlist_name: &str,
    build: BuildServiceResult,
) -> Result<ParityBuildLiveDiffReport> {
    let expected_path = work_dir.join("Ground Truth.proplaylist");
    let materialized = materialize_live_playlist(root, live_playlist_name, &expected_path)
        .with_context(|| format!("materialize live playlist {live_playlist_name:?}"))?;
    let actual_path = PathBuf::from(&build.playlist_path);

    let expected = read_playlist_package(&expected_path)
        .with_context(|| format!("read ground truth {}", expected_path.display()))?;
    let actual = read_playlist_package(&actual_path)
        .with_context(|| format!("read generated {}", actual_path.display()))?;
    let package = compare_playlist_packages(&expected_path, &actual_path).with_context(|| {
        format!(
            "compare {} to {}",
            expected_path.display(),
            actual_path.display()
        )
    })?;
    let expected_items = presentation_items(expected.document());
    let actual_items = presentation_items(actual.document());
    let (manual_expected_items, expected_items_for_alignment) =
        split_manual_expected_items(&expected_items);
    let aligned_item_diffs =
        compare_playlist_items_aligned(&expected_items_for_alignment, &actual_items);
    let actionable_item_compatible = aligned_item_diffs.is_empty();

    Ok(ParityBuildLiveDiffReport {
        work_dir: work_dir.display().to_string(),
        generated_library_dir: generated_library_dir.display().to_string(),
        expected_path: expected_path.display().to_string(),
        actual_path: actual_path.display().to_string(),
        compatible: package.compatible,
        expected_shape: infer_archive_shape(&expected),
        actual_shape: infer_archive_shape(&actual),
        expected_items,
        actual_items,
        manual_expected_items,
        actionable_item_compatible,
        aligned_item_diffs,
        expected_presentations: embedded_presentation_structures(&expected),
        actual_presentations: embedded_presentation_structures(&actual),
        package,
        materialized,
        build,
    })
}

fn copy_live_presentations(live_library_dir: &Path, generated_library_dir: &Path) -> Result<()> {
    for entry in WalkDir::new(live_library_dir)
        .follow_links(true)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "pro"))
    {
        let relative = entry
            .path()
            .strip_prefix(live_library_dir)
            .with_context(|| {
                format!(
                    "compute relative path for live presentation {}",
                    entry.path().display()
                )
            })?;
        let destination = generated_library_dir.join(relative);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create shadow library directory {}", parent.display()))?;
        }
        std::fs::copy(entry.path(), &destination).with_context(|| {
            format!(
                "copy live presentation {} to {}",
                entry.path().display(),
                destination.display()
            )
        })?;
    }

    Ok(())
}

fn split_manual_expected_items(
    items: &[PlaylistItemSummary],
) -> (Vec<ManualExpectedPlaylistItem>, Vec<PlaylistItemSummary>) {
    let mut manual = Vec::new();
    let mut actionable = Vec::new();
    for item in items {
        if is_manual_sermon_item(item) {
            manual.push(ManualExpectedPlaylistItem {
                policy: "sermon_manual_only".to_string(),
                reason:
                    "Sermon slides are provided after ProFlow builds and must be added manually"
                        .to_string(),
                item: item.clone(),
            });
        } else {
            actionable.push(item.clone());
        }
    }
    (manual, actionable)
}

fn is_manual_sermon_item(item: &PlaylistItemSummary) -> bool {
    is_sermon_label(&item.name)
        || [
            item.local_relative_path.as_deref(),
            item.storage_relative_path.as_deref(),
            item.external_relative_path.as_deref(),
            item.absolute_string.as_deref(),
        ]
        .into_iter()
        .flatten()
        .any(|value| is_sermon_label(&path_basename_stem(value)))
}

fn is_sermon_label(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase();
    let trimmed = normalized.trim();
    trimmed == "sermon"
        || trimmed.starts_with("sermon ")
        || trimmed.starts_with("sermon:")
        || trimmed.starts_with("sermon -")
        || (trimmed.ends_with("sermon") && trimmed.chars().any(|ch| ch.is_ascii_digit()))
}

fn path_basename_stem(value: &str) -> String {
    let decoded = value.replace("%20", " ");
    let basename = decoded.rsplit('/').next().unwrap_or(&decoded);
    basename
        .strip_suffix(".pro")
        .or_else(|| basename.strip_suffix(".PRO"))
        .unwrap_or(basename)
        .to_string()
}
