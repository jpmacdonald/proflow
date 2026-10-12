//! Prepared state on disk: the `rendered/`, `verified/` and `committed/`
//! stages of a build directory.
//!
//! `rendered/` holds every artifact a build will commit, the bytes each one
//! replaces, the build report, and the fingerprints commit must find
//! unchanged. Nothing about a prepared build lives only in memory, so the
//! process that commits need not be the process that rendered.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::workflow::approval::SourceManifest;
use crate::workflow::build_dir::{hex, BuildDir, Classified, Committed, Rendered, Verified};
use crate::workflow::report::BuildServiceResult;
use crate::workflow::transaction::{ExportedOutput, PreparedFileTransaction};
use proflow_propresenter::text_fit::FontProgramSnapshot;

use super::BuildServiceError;

const MANIFEST_FILE: &str = "manifest.json";
const RESULT_FILE: &str = "result.json";
const CHECKS_FILE: &str = "checks.json";
const TARGETS_FILE: &str = "targets.json";
const MANIFEST_VERSION: u32 = 1;

/// Checks every build passes between rendering and the `verified/` stage.
const CHECKS: &[&str] = &[
    "every final presentation satisfies the contract of its reviewed plan",
    "every final presentation has unambiguous native references",
    "the receipt describes exactly the sealed artifacts",
    "the render assets did not change while rendering",
    "the library held the reviewed presentations when rendering finished",
];

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RenderedManifest {
    version: u32,
    /// Artifacts in commit order; the playlist is last.
    outputs: Vec<ExportedOutput>,
    /// Every source file the render read, as it was then.
    sources: Vec<RecordedFile>,
    /// Every font program native text layout measured against.
    font_programs: Vec<RecordedFile>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordedFile {
    path: PathBuf,
    sha256: String,
}

/// Everything a finished render hands to the build directory.
pub(super) struct RenderedArtifacts<'a> {
    pub(super) transaction: &'a PreparedFileTransaction,
    pub(super) sources: &'a SourceManifest,
    pub(super) font_programs: &'a FontProgramSnapshot,
    pub(super) result: &'a BuildServiceResult,
}

/// Write `rendered/` and then `verified/`.
pub(super) fn write_prepared(
    build: BuildDir<Classified>,
    artifacts: &RenderedArtifacts<'_>,
) -> Result<BuildDir<Verified>, BuildServiceError> {
    let rendered: BuildDir<Rendered> = build.write(|stage| {
        let manifest = RenderedManifest {
            version: MANIFEST_VERSION,
            outputs: artifacts.transaction.export(stage)?,
            sources: artifacts
                .sources
                .digests()
                .map(|(path, sha256)| RecordedFile {
                    path: path.to_path_buf(),
                    sha256: hex(&sha256),
                })
                .collect(),
            font_programs: artifacts
                .font_programs
                .programs()
                .map(|(path, sha256)| RecordedFile {
                    path: path.to_path_buf(),
                    sha256: sha256.to_string(),
                })
                .collect(),
        };
        write_json(&stage.join(MANIFEST_FILE), &manifest)?;
        write_json(&stage.join(RESULT_FILE), artifacts.result)
    })?;
    Ok(rendered.write(|stage| write_json(&stage.join(CHECKS_FILE), &CHECKS))?)
}

/// A prepared build read back from its `rendered/` stage.
pub(super) struct LoadedPrepared {
    pub(super) outputs: Vec<ExportedOutput>,
    pub(super) sources: SourceManifest,
    pub(super) font_programs: FontProgramSnapshot,
    pub(super) result: BuildServiceResult,
}

/// Read the manifest and report of a verified build.
pub(super) fn load_prepared(
    build: &BuildDir<Verified>,
) -> Result<LoadedPrepared, BuildServiceError> {
    let rendered = build.stage_directory::<Rendered>();
    let manifest: RenderedManifest = read_json(&rendered.join(MANIFEST_FILE))?;
    if manifest.version != MANIFEST_VERSION {
        return Err(io::Error::other(format!(
            "rendered build manifest has version {}, expected {MANIFEST_VERSION}",
            manifest.version
        ))
        .into());
    }
    let sources = manifest
        .sources
        .into_iter()
        .map(|source| Ok((source.path, sha256_bytes(&source.sha256)?)))
        .collect::<io::Result<Vec<_>>>()?;
    Ok(LoadedPrepared {
        outputs: manifest.outputs,
        sources: SourceManifest::recorded(sources),
        font_programs: FontProgramSnapshot::from_recorded(
            manifest
                .font_programs
                .into_iter()
                .map(|program| (program.path, program.sha256)),
        ),
        result: read_json(&rendered.join(RESULT_FILE))?,
    })
}

/// Record the targets a build committed.
pub(super) fn write_committed(
    build: BuildDir<Verified>,
    targets: &[PathBuf],
) -> Result<BuildDir<Committed>, BuildServiceError> {
    Ok(build.write(|stage| write_json(&stage.join(TARGETS_FILE), &targets))?)
}

/// Exact artifact bytes a verified build will commit to `target`.
#[cfg(test)]
pub(super) fn artifact_bytes(
    build: &BuildDir<Verified>,
    target: &Path,
) -> Result<Option<Vec<u8>>, BuildServiceError> {
    let rendered = build.stage_directory::<Rendered>();
    load_prepared(build)?
        .outputs
        .iter()
        .position(|output| output.target == target)
        .map(|index| fs::read(rendered.join(format!("artifacts/{index:03}"))))
        .transpose()
        .map_err(BuildServiceError::from)
}

fn write_json(path: &Path, value: &impl Serialize) -> io::Result<()> {
    fs::write(
        path,
        serde_json::to_vec_pretty(value).map_err(io::Error::other)?,
    )
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> io::Result<T> {
    serde_json::from_slice(&fs::read(path)?).map_err(|error| {
        io::Error::other(format!("{} is not a valid record: {error}", path.display()))
    })
}

fn sha256_bytes(text: &str) -> io::Result<[u8; 32]> {
    let malformed = || io::Error::other(format!("'{text}' is not a SHA-256 digest"));
    if text.len() != 64 || !text.is_ascii() {
        return Err(malformed());
    }
    let mut digest = [0_u8; 32];
    for (byte, pair) in digest.iter_mut().zip(text.as_bytes().chunks(2)) {
        let pair = std::str::from_utf8(pair).map_err(|_| malformed())?;
        *byte = u8::from_str_radix(pair, 16).map_err(|_| malformed())?;
    }
    Ok(digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recorded_digest_is_exactly_sixty_four_hex_digits() {
        let digest = [0xab_u8; 32];
        assert_eq!(sha256_bytes(&hex(&digest)).ok(), Some(digest));

        let short = "ab".repeat(31);
        let not_hex = "zz".repeat(32);
        let not_ascii = "é".repeat(32);
        assert_eq!(not_ascii.len(), 64);
        for malformed in [short.as_str(), not_hex.as_str(), not_ascii.as_str(), ""] {
            assert!(sha256_bytes(malformed).is_err(), "{malformed}");
        }
    }
}
