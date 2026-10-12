//! The stages a review writes to its build directory before rendering.
//!
//! `snapshot/` holds the five build inputs exactly as they were hashed into
//! the directory name, one file each. `classified/` holds the effective plan
//! those inputs produce.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::planning_center::PlanSnapshot;
use crate::workflow::build_dir::{
    hex, BuildDir, BuildDirError, BuildInputs, BuildPlanId, Classified, Opened, Snapshotted,
    Verified,
};
use crate::workflow::plan::ResolvedItemPlan;
use proflow_propresenter::library::LibraryCandidates;
use proflow_propresenter::playlist::PlaylistExportMode;

use super::overrides::EntryOverride;
use super::request::BoundBuildRequest;
use super::{BuildServiceError, ServiceBuildExecutor};

const BUILDS_DIRECTORY: &str = "builds";
const PLAN_FILE: &str = "plan.json";
const RENDER_ASSETS_FILE: &str = "render-assets.json";
const BIBLE_CORPORA_FILE: &str = "bible-corpora.json";
const CATALOG_FILE: &str = "catalog.json";
const DECISIONS_FILE: &str = "decisions.json";
const PLANS_FILE: &str = "plans.json";

/// Everything the operator chose for one review, in one canonical order.
///
/// Two requests that differ only in the order of their skips or overrides are
/// the same decisions and reach the same build directory.
#[derive(Serialize)]
struct Decisions<'a> {
    playlist_name: &'a str,
    package_mode: PlaylistExportMode,
    media_assets: Vec<MediaAsset<'a>>,
    skip_output_keys: Vec<&'a str>,
    overrides: Vec<&'a EntryOverride>,
}

#[derive(Serialize)]
struct MediaAsset<'a> {
    path: &'a Path,
    archive_path: Option<&'a str>,
}

/// One `.pro` file the review could have matched, with its content hash.
#[derive(Serialize)]
struct CatalogEntry<'a> {
    path: &'a Path,
    sha256: String,
}

/// One catalog entry as read back from `snapshot/catalog.json`.
#[derive(Deserialize)]
struct RecordedCatalogEntry {
    path: PathBuf,
}

/// The decisions of one review, borrowed from the request they arrived in.
pub(super) struct ReviewedDecisions<'a> {
    pub(super) request: &'a BoundBuildRequest,
    pub(super) skip_output_keys: &'a [String],
    pub(super) overrides: &'a [EntryOverride],
}

impl ServiceBuildExecutor<'_> {
    /// Open the build directory these inputs name and write, or reopen, its
    /// `snapshot/` and `classified/` stages.
    pub(super) fn classified_build(
        &self,
        decisions: &ReviewedDecisions<'_>,
        planning_center: &PlanSnapshot,
        library_candidates: &LibraryCandidates,
        plans: &[ResolvedItemPlan],
    ) -> Result<BuildDir<Classified>, BuildServiceError> {
        let plan = json(planning_center)?;
        let render_assets = json(self.render_assets.fingerprint())?;
        let bible_corpora = json(&self.bible_corpora.fingerprint().to_json())?;
        let catalog = json(&catalog_entries(library_candidates)?)?;
        let decisions_bytes = json(&canonical_decisions(decisions))?;

        let build: BuildDir<Opened> = BuildDir::open(
            &self
                .render_assets
                .locations()
                .project_data_root()
                .join(BUILDS_DIRECTORY),
            &BuildPlanId::new(decisions.request.plan_id.as_str())?,
            &BuildInputs {
                plan_snapshot: &plan,
                render_assets: &render_assets,
                bible_corpora: &bible_corpora,
                library: &catalog,
                review_decisions: &decisions_bytes,
            },
        )?;
        let build: BuildDir<Snapshotted> = build.resume_or_write(|stage| {
            fs::write(stage.join(PLAN_FILE), &plan)?;
            fs::write(stage.join(RENDER_ASSETS_FILE), &render_assets)?;
            fs::write(stage.join(BIBLE_CORPORA_FILE), &bible_corpora)?;
            fs::write(stage.join(CATALOG_FILE), &catalog)?;
            fs::write(stage.join(DECISIONS_FILE), &decisions_bytes)
        })?;
        let plans = json(&plans)?;
        Ok(build.resume_or_write(|stage| fs::write(stage.join(PLANS_FILE), &plans))?)
    }
}

impl ServiceBuildExecutor<'_> {
    /// Prove a build's recorded inputs still describe this runtime, and
    /// return the library candidates it was reviewed against.
    ///
    /// The snapshot stage must match its seal and hash to the name of its
    /// directory, and the render assets and Bible corpora recorded in it must
    /// be the ones this executor holds.
    pub(super) fn verify_snapshot_stage(
        &self,
        build: &BuildDir<Verified>,
    ) -> Result<Vec<PathBuf>, BuildServiceError> {
        build.verify_stage::<Snapshotted>()?;
        let stage = build.stage_directory::<Snapshotted>();
        let read = |file: &str| fs::read(stage.join(file));
        let render_assets = read(RENDER_ASSETS_FILE)?;
        let bible_corpora = read(BIBLE_CORPORA_FILE)?;
        let catalog = read(CATALOG_FILE)?;
        if !build.is_named_by(&BuildInputs {
            plan_snapshot: &read(PLAN_FILE)?,
            render_assets: &render_assets,
            bible_corpora: &bible_corpora,
            library: &catalog,
            review_decisions: &read(DECISIONS_FILE)?,
        }) {
            return Err(BuildDirError::InputsMismatch {
                path: build.path().to_path_buf(),
            }
            .into());
        }
        if render_assets != json(self.render_assets.fingerprint())?
            || bible_corpora != json(&self.bible_corpora.fingerprint().to_json())?
        {
            return Err(BuildServiceError::PreparedRuntimeMismatch);
        }
        let entries: Vec<RecordedCatalogEntry> =
            serde_json::from_slice(&catalog).map_err(io::Error::other)?;
        Ok(entries.into_iter().map(|entry| entry.path).collect())
    }
}

fn canonical_decisions<'a>(decisions: &ReviewedDecisions<'a>) -> Decisions<'a> {
    let mut skip_output_keys = decisions
        .skip_output_keys
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    skip_output_keys.sort_unstable();
    let mut overrides = decisions.overrides.iter().collect::<Vec<_>>();
    overrides.sort_by(|left, right| left.output_key.cmp(&right.output_key));
    Decisions {
        playlist_name: &decisions.request.playlist_name,
        package_mode: decisions.request.playlist_export.mode(),
        media_assets: decisions
            .request
            .playlist_export
            .media_assets()
            .iter()
            .map(|asset| MediaAsset {
                path: &asset.source_path,
                archive_path: asset.archive_path.as_deref(),
            })
            .collect(),
        skip_output_keys,
        overrides,
    }
}

/// Hash every library candidate, so a presentation edited in place is a
/// different library even when no file was added or removed.
fn catalog_entries(candidates: &LibraryCandidates) -> io::Result<Vec<CatalogEntry<'_>>> {
    candidates
        .paths()
        .map(|path| {
            let digest = Sha256::digest(fs::read(path)?);
            Ok(CatalogEntry {
                path,
                sha256: hex(&digest),
            })
        })
        .collect()
}

fn json(value: &impl Serialize) -> io::Result<Vec<u8>> {
    serde_json::to_vec_pretty(value).map_err(io::Error::other)
}
