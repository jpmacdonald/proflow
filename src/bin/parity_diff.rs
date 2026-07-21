//! Compare two `ProPresenter` playlist packages for parity work.
//!
//! Usage:
//!   `cargo run --features dev-tools --bin parity_diff -- <ground-truth.proplaylist> <generated.proplaylist>`

#![allow(clippy::print_stdout)]

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use proflow::propresenter::package::{
    compare_playlist_packages, embedded_presentation_structures, infer_archive_shape,
    presentation_items, read_playlist_package, EmbeddedPresentationStructure, PlaylistArchiveShape,
    PlaylistItemSummary, PlaylistPackageComparison,
};
use serde::Serialize;

#[derive(Debug, Serialize)]
struct ParityDiffReport {
    expected_path: String,
    actual_path: String,
    compatible: bool,
    expected_shape: PlaylistArchiveShape,
    actual_shape: PlaylistArchiveShape,
    package: PlaylistPackageComparison,
    expected_items: Vec<PlaylistItemSummary>,
    actual_items: Vec<PlaylistItemSummary>,
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

fn run() -> Result<ParityDiffReport> {
    let mut args = std::env::args_os().skip(1);
    let expected_path = args
        .next()
        .map(PathBuf::from)
        .context("usage: parity_diff <ground-truth.proplaylist> <generated.proplaylist>")?;
    let actual_path = args
        .next()
        .map(PathBuf::from)
        .context("usage: parity_diff <ground-truth.proplaylist> <generated.proplaylist>")?;
    if args.next().is_some() {
        anyhow::bail!("usage: parity_diff <ground-truth.proplaylist> <generated.proplaylist>");
    }

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

    Ok(ParityDiffReport {
        expected_path: expected_path.display().to_string(),
        actual_path: actual_path.display().to_string(),
        compatible: package.compatible,
        expected_shape: infer_archive_shape(&expected),
        actual_shape: infer_archive_shape(&actual),
        expected_items: presentation_items(expected.document()),
        actual_items: presentation_items(actual.document()),
        expected_presentations: embedded_presentation_structures(&expected),
        actual_presentations: embedded_presentation_structures(&actual),
        package,
    })
}
