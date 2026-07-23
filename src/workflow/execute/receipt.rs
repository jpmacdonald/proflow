//! Deterministic evidence sidecar for one reviewed service build.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use prost::Message;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::planning_center::{PlanRevisionError, PlanSnapshot};
use crate::propresenter::playlist::{PlaylistExportMode, PlaylistMetadata};
use crate::propresenter::text_fit::TextFitContractSummary;
use crate::workflow::approval::SourceManifest;
use crate::workflow::report::BuildServiceEntry;
use crate::workflow::transaction::{PreparedFileTransaction, StagedArtifact};

use super::RenderAssetFingerprint;
use crate::propresenter::playlist::PlaylistExportEvidence;

const RECEIPT_SCHEMA: &str = "proflow.build-receipt.v3";
const RECEIPT_SUFFIX: &str = ".proflow-build.json";

/// Exact JSON bytes and aggregate revision prepared for a reviewed receipt target.
pub(super) struct PreparedBuildReceipt {
    revision: String,
    bytes: Vec<u8>,
}

impl PreparedBuildReceipt {
    pub(super) fn revision(&self) -> &str {
        &self.revision
    }

    pub(super) fn write_to(&self, staged_path: &Path) -> Result<(), BuildReceiptError> {
        std::fs::write(staged_path, &self.bytes).map_err(|source| BuildReceiptError::Write {
            path: staged_path.to_path_buf(),
            source,
        })
    }
}

/// Prove that the only committable artifact set is exactly the set described by
/// the receipt plus the receipt's own deterministic bytes.
///
/// `staged` is the evidence snapshot used to construct the receipt. The receipt
/// stage is still empty in that snapshot, so its entry is replaced with the
/// fingerprint of [`PreparedBuildReceipt::bytes`]. Every other entry must match
/// in target, reservation order, length, and digest.
pub(super) fn verify_sealed_build_artifacts(
    receipt_target: &Path,
    receipt: &PreparedBuildReceipt,
    staged: &[StagedArtifact],
    transaction: &PreparedFileTransaction,
) -> Result<(), BuildReceiptError> {
    let sealed = transaction.sealed_artifacts();
    if sealed.len() != staged.len() {
        return Err(BuildReceiptError::SealedArtifactCount {
            expected: staged.len(),
            actual: sealed.len(),
        });
    }

    let receipt_length =
        u64::try_from(receipt.bytes.len()).map_err(|_| BuildReceiptError::ReceiptTooLarge)?;
    let receipt_sha256: [u8; 32] = Sha256::digest(&receipt.bytes).into();
    for (ordinal, (evidence, actual)) in staged.iter().zip(&sealed).enumerate() {
        let (expected_target, expected_length, expected_sha256) =
            if evidence.target() == receipt_target {
                (receipt_target, receipt_length, receipt_sha256)
            } else {
                (evidence.target(), evidence.length(), evidence.sha256())
            };
        if actual.target() != expected_target
            || actual.length() != expected_length
            || actual.sha256() != expected_sha256
        {
            return Err(BuildReceiptError::SealedArtifactDrift {
                ordinal,
                expected_path: expected_target.to_path_buf(),
                actual_path: actual.target().to_path_buf(),
                expected_length,
                actual_length: actual.length(),
                expected_sha256: digest_hex(&expected_sha256),
                actual_sha256: digest_hex(&actual.sha256()),
            });
        }
    }
    Ok(())
}

/// Derive `<playlist filename>.proflow-build.json` beside the playlist.
pub(super) fn receipt_path_for_playlist(
    playlist_path: &Path,
) -> Result<PathBuf, BuildReceiptError> {
    let filename =
        playlist_path
            .file_name()
            .ok_or_else(|| BuildReceiptError::PlaylistHasNoFilename {
                path: playlist_path.to_path_buf(),
            })?;
    let mut receipt_filename = OsString::from(filename);
    receipt_filename.push(RECEIPT_SUFFIX);
    Ok(playlist_path.with_file_name(receipt_filename))
}

/// Build the deterministic receipt after every native artifact, including the
/// playlist, has reached its final staged bytes.
pub(super) fn prepare_build_receipt(
    inputs: BuildReceiptInputs<'_>,
) -> Result<PreparedBuildReceipt, BuildReceiptError> {
    let BuildReceiptInputs {
        receipt_target,
        playlist_target,
        playlist_name,
        package_mode,
        planning_center,
        playlist_metadata,
        playlist_export,
        render_assets,
        text_fit_contract,
        sources,
        staged,
        entries,
    } = inputs;
    validate_staged_output_order(receipt_target, playlist_target, staged)?;

    let plan_revision = planning_center.revision()?.to_string();
    let planning_center = PlanningCenterEvidence {
        revision: plan_revision,
        normalized_snapshot: planning_center,
    };
    let application_info_bytes = playlist_metadata.application_info().encode_to_vec();
    let playlist_producer = PlaylistProducerEvidence {
        application_info_sha256: digest_hex(&Sha256::digest(&application_info_bytes).into()),
        application_info_protobuf_hex: hex_bytes(&application_info_bytes),
    };
    let reviewed_sources = sources
        .digests()
        .map(|(path, sha256)| {
            Ok(SourceEvidence {
                path: utf8_path(path)?,
                sha256: digest_hex(&sha256),
            })
        })
        .collect::<Result<Vec<_>, BuildReceiptError>>()?;
    let artifacts = staged
        .iter()
        .filter(|artifact| artifact.target() != receipt_target)
        .map(|artifact| artifact_evidence(artifact, playlist_target))
        .collect::<Result<Vec<_>, BuildReceiptError>>()?;
    let body = ReceiptBody {
        playlist_name,
        package_mode,
        planning_center,
        playlist_producer,
        playlist_export,
        render_assets,
        text_fit_contract,
        reviewed_sources,
        artifacts,
        entries,
    };
    let revision = hash_serialized(&RevisionMaterial {
        schema: RECEIPT_SCHEMA,
        body: &body,
    })?;
    let document = ReceiptDocument {
        schema: RECEIPT_SCHEMA,
        revision: &revision,
        body: &body,
    };
    let mut bytes = serde_json::to_vec_pretty(&document).map_err(BuildReceiptError::Serialize)?;
    bytes.push(b'\n');
    Ok(PreparedBuildReceipt { revision, bytes })
}

fn validate_staged_output_order(
    receipt_target: &Path,
    playlist_target: &Path,
    staged: &[StagedArtifact],
) -> Result<(), BuildReceiptError> {
    let receipt_index = staged
        .iter()
        .position(|artifact| artifact.target() == receipt_target)
        .ok_or_else(|| BuildReceiptError::MissingStagedReceipt {
            path: receipt_target.to_path_buf(),
        })?;
    let playlist_index = staged
        .iter()
        .position(|artifact| artifact.target() == playlist_target)
        .ok_or_else(|| BuildReceiptError::MissingStagedPlaylist {
            path: playlist_target.to_path_buf(),
        })?;
    if receipt_index >= playlist_index {
        return Err(BuildReceiptError::ReceiptNotBeforePlaylist {
            receipt: receipt_target.to_path_buf(),
            playlist: playlist_target.to_path_buf(),
        });
    }
    if playlist_index + 1 != staged.len() {
        return Err(BuildReceiptError::PlaylistNotLast {
            path: playlist_target.to_path_buf(),
        });
    }
    Ok(())
}

fn artifact_evidence(
    artifact: &StagedArtifact,
    playlist_target: &Path,
) -> Result<ArtifactEvidence, BuildReceiptError> {
    let kind = if artifact.target() == playlist_target {
        BuildArtifactKind::Playlist
    } else if artifact
        .target()
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pro"))
    {
        BuildArtifactKind::Presentation
    } else {
        return Err(BuildReceiptError::UnsupportedArtifactKind {
            path: artifact.target().to_path_buf(),
        });
    };
    Ok(ArtifactEvidence {
        kind,
        path: utf8_path(artifact.target())?,
        length: artifact.length(),
        sha256: digest_hex(&artifact.sha256()),
    })
}

/// Complete immutable evidence inputs for one receipt preparation phase.
#[derive(Clone, Copy)]
pub(super) struct BuildReceiptInputs<'a> {
    pub(super) receipt_target: &'a Path,
    pub(super) playlist_target: &'a Path,
    pub(super) playlist_name: &'a str,
    pub(super) package_mode: PlaylistExportMode,
    pub(super) planning_center: &'a PlanSnapshot,
    pub(super) playlist_metadata: &'a PlaylistMetadata,
    pub(super) playlist_export: &'a PlaylistExportEvidence,
    pub(super) render_assets: &'a RenderAssetFingerprint,
    pub(super) text_fit_contract: &'a TextFitContractSummary,
    pub(super) sources: &'a SourceManifest,
    pub(super) staged: &'a [StagedArtifact],
    pub(super) entries: &'a [BuildServiceEntry],
}

#[derive(Serialize)]
struct RevisionMaterial<'a> {
    schema: &'static str,
    #[serde(flatten)]
    body: &'a ReceiptBody<'a>,
}

#[derive(Serialize)]
struct ReceiptDocument<'a> {
    schema: &'static str,
    revision: &'a str,
    #[serde(flatten)]
    body: &'a ReceiptBody<'a>,
}

#[derive(Serialize)]
struct ReceiptBody<'a> {
    playlist_name: &'a str,
    package_mode: PlaylistExportMode,
    planning_center: PlanningCenterEvidence<'a>,
    playlist_producer: PlaylistProducerEvidence,
    playlist_export: &'a PlaylistExportEvidence,
    render_assets: &'a RenderAssetFingerprint,
    text_fit_contract: &'a TextFitContractSummary,
    reviewed_sources: Vec<SourceEvidence>,
    artifacts: Vec<ArtifactEvidence>,
    entries: &'a [BuildServiceEntry],
}

#[derive(Serialize)]
struct PlanningCenterEvidence<'a> {
    revision: String,
    #[serde(flatten)]
    normalized_snapshot: &'a PlanSnapshot,
}

#[derive(Serialize)]
struct PlaylistProducerEvidence {
    application_info_sha256: String,
    application_info_protobuf_hex: String,
}

#[derive(Serialize)]
struct SourceEvidence {
    path: String,
    sha256: String,
}

#[derive(Serialize)]
struct ArtifactEvidence {
    kind: BuildArtifactKind,
    path: String,
    length: u64,
    sha256: String,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum BuildArtifactKind {
    Presentation,
    Playlist,
}

/// A reviewed build receipt could not be constructed or written exactly.
#[derive(Debug, thiserror::Error)]
pub enum BuildReceiptError {
    /// The playlist target has no filename from which to derive a sidecar.
    #[error("playlist path has no filename for a build receipt: {}", path.display())]
    PlaylistHasNoFilename { path: PathBuf },
    /// A path cannot be represented exactly in the JSON evidence contract.
    #[error("build receipt path is not valid UTF-8: {}", path.display())]
    NonUtf8Path { path: PathBuf },
    /// The receipt target was reviewed but not staged.
    #[error("reviewed build receipt was not staged: {}", path.display())]
    MissingStagedReceipt { path: PathBuf },
    /// The playlist target was reviewed but not staged.
    #[error("reviewed playlist was not staged before receipt generation: {}", path.display())]
    MissingStagedPlaylist { path: PathBuf },
    /// Commit order would place the receipt at or after the playlist.
    #[error(
        "build receipt '{}' must be staged before playlist '{}'",
        receipt.display(),
        playlist.display()
    )]
    ReceiptNotBeforePlaylist { receipt: PathBuf, playlist: PathBuf },
    /// Another output was staged after the playlist.
    #[error("playlist must be the final staged build output: {}", path.display())]
    PlaylistNotLast { path: PathBuf },
    /// A staged output has no supported receipt classification.
    #[error("unsupported staged build artifact kind: {}", path.display())]
    UnsupportedArtifactKind { path: PathBuf },
    /// Exact staged artifact evidence could not be read.
    #[error("failed to inspect staged build artifacts for the receipt: {source}")]
    InspectStagedArtifacts {
        #[source]
        source: std::io::Error,
    },
    /// The receipt bytes cannot be represented by the native length contract.
    #[error("prepared build receipt is too large to fingerprint")]
    ReceiptTooLarge,
    /// Sealing produced a different number of committable targets than the
    /// artifact evidence used to serialize the receipt.
    #[error(
        "sealed artifact count differs from build receipt evidence (expected {expected}, found {actual})"
    )]
    SealedArtifactCount { expected: usize, actual: usize },
    /// A sealed target, its order, or its exact bytes differ from the receipt
    /// evidence. No transaction in this state is allowed to commit.
    #[error(
        "sealed artifact #{ordinal} differs from build receipt evidence: expected '{}' ({expected_length} bytes, SHA-256 {expected_sha256}), found '{}' ({actual_length} bytes, SHA-256 {actual_sha256})",
        expected_path.display(),
        actual_path.display()
    )]
    SealedArtifactDrift {
        ordinal: usize,
        expected_path: PathBuf,
        actual_path: PathBuf,
        expected_length: u64,
        actual_length: u64,
        expected_sha256: String,
        actual_sha256: String,
    },
    /// The normalized Planning Center snapshot could not be fingerprinted.
    #[error(transparent)]
    PlanRevision(#[from] PlanRevisionError),
    /// Deterministic receipt serialization failed.
    #[error("failed to serialize build receipt: {0}")]
    Serialize(serde_json::Error),
    /// Exact receipt bytes could not be written to the reviewed stage.
    #[error("failed to write staged build receipt '{}': {source}", path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

fn utf8_path(path: &Path) -> Result<String, BuildReceiptError> {
    path.to_str()
        .map(str::to_string)
        .ok_or_else(|| BuildReceiptError::NonUtf8Path {
            path: path.to_path_buf(),
        })
}

fn hash_serialized(value: &impl Serialize) -> Result<String, BuildReceiptError> {
    let bytes = serde_json::to_vec(value).map_err(BuildReceiptError::Serialize)?;
    Ok(digest_hex(&Sha256::digest(bytes).into()))
}

fn digest_hex(digest: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

#[cfg(test)]
#[path = "receipt/tests.rs"]
mod tests;
