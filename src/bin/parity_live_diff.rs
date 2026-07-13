//! Compare a generated `ProPresenter` playlist package against a live playlist.
//!
//! Usage:
//!   `cargo run --bin parity_live_diff -- <ProPresenter root> <playlist name> <generated.proplaylist>`

#![allow(clippy::print_stdout)]

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use proflow::propresenter::live::{materialize_live_playlist, LivePlaylistMaterializeReport};
use proflow::propresenter::package::{
    compare_playlist_items_aligned, compare_playlist_packages, embedded_presentation_structures,
    infer_package_mode, presentation_items, read_playlist_package, EmbeddedPresentationStructure,
    PlaylistItemAlignedDiff, PlaylistItemSummary, PlaylistPackageComparison, PlaylistPackageMode,
};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Serialize)]
struct ManualExpectedPlaylistItem {
    policy: String,
    reason: String,
    item: PlaylistItemSummary,
}

#[derive(Debug, Serialize)]
struct ParityLiveDiffReport {
    materialized: LivePlaylistMaterializeReport,
    expected_path: String,
    actual_path: String,
    compatible: bool,
    expected_mode: PlaylistPackageMode,
    actual_mode: PlaylistPackageMode,
    package: PlaylistPackageComparison,
    expected_items: Vec<PlaylistItemSummary>,
    actual_items: Vec<PlaylistItemSummary>,
    manual_expected_items: Vec<ManualExpectedPlaylistItem>,
    aligned_item_diffs: Vec<PlaylistItemAlignedDiff>,
    actionable_item_compatible: bool,
    expected_presentations: Vec<EmbeddedPresentationStructure>,
    actual_presentations: Vec<EmbeddedPresentationStructure>,
}

fn main() -> ExitCode {
    match run() {
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

fn run() -> Result<ParityLiveDiffReport> {
    let mut args = std::env::args_os().skip(1);
    let root = args.next().map(PathBuf::from).context(
        "usage: parity_live_diff <ProPresenter root> <playlist name> <generated.proplaylist>",
    )?;
    let playlist_name = args
        .next()
        .map(|value| value.to_string_lossy().to_string())
        .context(
            "usage: parity_live_diff <ProPresenter root> <playlist name> <generated.proplaylist>",
        )?;
    let actual_path = args.next().map(PathBuf::from).context(
        "usage: parity_live_diff <ProPresenter root> <playlist name> <generated.proplaylist>",
    )?;
    if args.next().is_some() {
        anyhow::bail!(
            "usage: parity_live_diff <ProPresenter root> <playlist name> <generated.proplaylist>"
        );
    }

    let expected_path = std::env::temp_dir().join(format!(
        "proflow-live-groundtruth-{}.proplaylist",
        Uuid::new_v4()
    ));
    let materialized = materialize_live_playlist(&root, &playlist_name, &expected_path)
        .with_context(|| format!("materialize live playlist {playlist_name:?}"))?;

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
    let expected_items = presentation_items(&expected.document);
    let actual_items = presentation_items(&actual.document);
    let (manual_expected_items, expected_items_for_alignment) =
        split_manual_expected_items(&expected_items);
    let aligned_item_diffs =
        compare_playlist_items_aligned(&expected_items_for_alignment, &actual_items);
    let actionable_item_compatible = aligned_item_diffs.is_empty();

    Ok(ParityLiveDiffReport {
        expected_path: expected_path.display().to_string(),
        actual_path: actual_path.display().to_string(),
        compatible: package.compatible,
        expected_mode: infer_package_mode(&expected),
        actual_mode: infer_package_mode(&actual),
        expected_items,
        actual_items,
        manual_expected_items,
        actionable_item_compatible,
        aligned_item_diffs,
        expected_presentations: embedded_presentation_structures(&expected),
        actual_presentations: embedded_presentation_structures(&actual),
        package,
        materialized,
    })
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
