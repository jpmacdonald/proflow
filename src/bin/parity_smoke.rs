//! Offline `ProPresenter` parity smoke harness.
//!
//! This inspects the real fixture corpus, re-emits presentation-only playlists
//! through the diagnostic preservation writer, and prints a JSON report.
//! Media-bearing native packages are audited in place; the parity gate runs a
//! separate dependency/link integrity test for those fixtures.

#[path = "parity_smoke/inspection.rs"]
mod inspection;
use inspection::{inspect_playlist_fixture, inspect_presentation_fixture};

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use propresenter::media::presentation_media_dependencies;
use propresenter::package::{
    compare_playlist_packages, embedded_presentation_summaries, infer_archive_shape,
    presentation_items, read_playlist_package, PlaylistArchiveShape, PlaylistPackageIssue,
};
use propresenter::playlist::{
    build_playlist, linked_presentation_filename, write_playlist_document_for_fidelity,
    PlaylistEntry, PlaylistMetadata, SelectedArrangement,
};
use propresenter::unstable_native::rv_data::playlist;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct Manifest {
    playlists: Vec<PlaylistFixture>,
    presentations: Vec<PresentationFixture>,
}

#[derive(Debug, Deserialize)]
struct PlaylistFixture {
    path: String,
    provenance: String,
    producer_version: String,
    operating_system: String,
    export_mode: String,
    covered_native_capabilities: Vec<String>,
    independent_native_export: bool,
    mode: PlaylistArchiveShape,
    item_count: usize,
    embedded_file_count: usize,
    media_file_count: usize,
    required_embedded_files: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct PresentationFixture {
    path: String,
    provenance: String,
    producer_version: String,
    operating_system: String,
    export_mode: String,
    covered_native_capabilities: Vec<String>,
    independent_native_export: bool,
    name: String,
    cue_count: usize,
    cue_group_count: usize,
    arrangement_count: usize,
    media_dependency_count: usize,
}

#[derive(Debug, Serialize)]
struct SmokeReport {
    fixture_root: String,
    compatible: bool,
    playlists: Vec<PlaylistSmokeReport>,
    presentations: Vec<PresentationSmokeReport>,
}

#[derive(Debug, Serialize)]
struct SmokeIssue {
    kind: String,
    message: String,
}

#[derive(Debug, Serialize)]
struct PlaylistSmokeReport {
    path: String,
    provenance: String,
    producer_version: String,
    operating_system: String,
    export_mode: String,
    covered_native_capabilities: Vec<String>,
    independent_native_export: bool,
    mode: PlaylistArchiveShape,
    item_count: usize,
    embedded_file_count: usize,
    media_file_count: usize,
    embedded_presentation_count: usize,
    round_trip_compatible: Option<bool>,
    issues: Vec<PlaylistPackageIssue>,
}

#[derive(Debug, Serialize)]
struct PresentationSmokeReport {
    path: String,
    provenance: String,
    producer_version: String,
    operating_system: String,
    export_mode: String,
    covered_native_capabilities: Vec<String>,
    independent_native_export: bool,
    name: String,
    cue_count: usize,
    cue_group_count: usize,
    arrangement_count: usize,
    media_dependency_count: usize,
    matches_manifest: bool,
    issues: Vec<SmokeIssue>,
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

fn run() -> Result<SmokeReport> {
    let fixture_root = std::env::args()
        .nth(1)
        .map_or_else(default_fixture_root, PathBuf::from);
    let manifest = read_manifest(&fixture_root)?;

    let playlists = manifest
        .playlists
        .iter()
        .map(|fixture| inspect_playlist_fixture(&fixture_root, fixture))
        .collect::<Result<Vec<_>>>()?;
    let presentations = manifest
        .presentations
        .iter()
        .map(|fixture| inspect_presentation_fixture(&fixture_root, fixture))
        .collect::<Result<Vec<_>>>()?;

    let compatible = playlists
        .iter()
        .filter(|playlist| playlist.independent_native_export)
        .all(|playlist| {
            playlist.issues.is_empty() && playlist.round_trip_compatible != Some(false)
        })
        && presentations
            .iter()
            .all(|presentation| presentation.matches_manifest);

    Ok(SmokeReport {
        fixture_root: fixture_root.display().to_string(),
        compatible,
        playlists,
        presentations,
    })
}

fn default_fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../propresenter/tests/fixtures/native/corpus")
}

fn read_manifest(fixture_root: &Path) -> Result<Manifest> {
    let path = fixture_root.join("manifest.json");
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("read manifest {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parse manifest {}", path.display()))
}
