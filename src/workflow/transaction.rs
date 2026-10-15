//! Staged file transaction for service builds.
//!
//! Rendering writes only sibling staging files. Once sealed, a transaction is
//! exported to its build directory and its staging files are removed; commit
//! stages the exported bytes beside their targets again, through the same
//! reviewed-output checks, immediately before installing them. Commit
//! atomically replaces each target and writes the playlist last, so a failed
//! render never changes live library state and a reported commit failure can
//! restore already-replaced targets. The multi-file operation is process-transactional, not power-loss
//! atomic or durable: it has no fsync journal or startup recovery protocol.
//!
//! Each target is compared with its staged-time snapshot immediately before its
//! rename. Filesystems do not provide a portable compare-and-rename operation,
//! so an external writer can still race in the tiny interval between that check
//! and the rename. Environments that permit external writers during a build need
//! an advisory lock above this transaction boundary to eliminate that race.

mod commit;

mod file_io;
use file_io::exported_path;
#[cfg(test)]
use file_io::fingerprint_file;
use file_io::{
    concurrent_change_error, fingerprint_bytes, fingerprint_regular_file, read_optional,
    remove_pending_stages, rollback_file,
};

use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use super::approval::{OutputManifest, OutputReviewError, PhysicalPath, ReviewedOutput};

#[derive(Debug)]
struct StagedFile {
    reviewed_path: PhysicalPath,
    staged: PathBuf,
    original: Option<Vec<u8>>,
    prepared: FileFingerprint,
    installed: Option<FileFingerprint>,
}

#[derive(Debug)]
struct PendingReviewedOutput {
    output: ReviewedOutput,
    staged: Option<ReviewedStage>,
}

#[derive(Debug)]
struct ReviewedStage {
    path: PathBuf,
    ordinal: usize,
}

#[cfg(test)]
type BeforeRecheck<'a> = &'a mut dyn FnMut(usize, &Path) -> io::Result<()>;

/// Owns every filesystem change made by one service build.
#[derive(Debug)]
pub(crate) struct BuildFileTransaction {
    outputs: Vec<PendingReviewedOutput>,
}

/// A complete set of reviewed artifacts whose exact staged bytes are frozen.
///
/// Only this state can commit. Rendering code receives [`BuildFileTransaction`]
/// and must seal it after every target has been written successfully.
#[derive(Debug)]
pub(crate) struct PreparedFileTransaction {
    files: Vec<StagedFile>,
}

/// Immutable evidence for one currently staged reviewed output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StagedArtifact {
    target: PathBuf,
    length: u64,
    sha256: [u8; 32],
}

/// One artifact of a sealed transaction as recorded in a build directory.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExportedOutput {
    /// Final target the artifact is committed to.
    pub(crate) target: PathBuf,
    /// Exact artifact byte length.
    pub(crate) length: u64,
    /// SHA-256 of the exact artifact bytes.
    pub(crate) sha256: String,
    /// Whether the target held a file when it was reviewed.
    pub(crate) replaces_existing: bool,
}

const ARTIFACTS_DIRECTORY: &str = "artifacts";
const ORIGINALS_DIRECTORY: &str = "originals";

impl BuildFileTransaction {
    /// Consume the exact reviewed output state before rendering begins.
    pub(crate) fn from_reviewed(outputs: OutputManifest) -> Self {
        Self {
            outputs: outputs
                .into_outputs()
                .into_iter()
                .map(|output| PendingReviewedOutput {
                    output,
                    staged: None,
                })
                .collect(),
        }
    }

    /// Stage one path from the consumed review set.
    ///
    /// Unreviewed, duplicate, retargeted, and byte-changed paths cannot obtain
    /// a staging destination.
    pub(crate) fn stage_reviewed(&mut self, target: &Path) -> Result<PathBuf, OutputReviewError> {
        let physical = PhysicalPath::resolve_output(target)?;
        let ordinal = self
            .outputs
            .iter()
            .filter(|reviewed| reviewed.staged.is_some())
            .count();
        let reviewed = self
            .outputs
            .iter_mut()
            .find(|reviewed| reviewed.output.path() == &physical)
            .ok_or_else(|| OutputReviewError::UnreviewedTarget {
                path: target.to_path_buf(),
            })?;
        if reviewed.staged.is_some() {
            return Err(OutputReviewError::AlreadyStagedTarget {
                path: reviewed.output.path().requested_path().to_path_buf(),
            });
        }
        reviewed.output.verify()?;

        let reviewed_target = reviewed.output.path().requested_path();
        let parent = reviewed_target.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).map_err(|source| OutputReviewError::Stage {
            path: reviewed_target.to_path_buf(),
            source,
        })?;
        let filename = reviewed_target
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("artifact");
        let staged = parent.join(format!(
            ".{filename}.proflow-stage-{}",
            uuid::Uuid::new_v4()
        ));
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged)
            .map_err(|source| OutputReviewError::Stage {
                path: reviewed_target.to_path_buf(),
                source,
            })?;
        reviewed.staged = Some(ReviewedStage {
            path: staged.clone(),
            ordinal,
        });
        Ok(staged)
    }

    /// Snapshot exact staged bytes in reservation order without consuming or
    /// mutating the transaction.
    pub(crate) fn staged_artifacts(&self) -> io::Result<Vec<StagedArtifact>> {
        let mut artifacts = self
            .outputs
            .iter()
            .map(|reviewed| {
                let staged = reviewed.staged.as_ref().ok_or_else(|| {
                    io::Error::other(format!(
                        "reviewed output has not been staged: {}",
                        reviewed.output.path().requested_path().display()
                    ))
                })?;
                let fingerprint = fingerprint_regular_file(&staged.path)?;
                Ok((
                    staged.ordinal,
                    StagedArtifact {
                        target: reviewed.output.path().requested_path().to_path_buf(),
                        length: fingerprint.length,
                        sha256: fingerprint.sha256,
                    },
                ))
            })
            .collect::<io::Result<Vec<_>>>()?;
        artifacts.sort_by_key(|(ordinal, _)| *ordinal);
        Ok(artifacts
            .into_iter()
            .map(|(_, artifact)| artifact)
            .collect())
    }

    /// Freeze the exact staged bytes and transition into the only committable
    /// transaction state.
    pub(crate) fn seal(mut self) -> Result<PreparedFileTransaction, OutputReviewError> {
        for reviewed in &self.outputs {
            reviewed
                .staged
                .as_ref()
                .ok_or_else(|| OutputReviewError::UnstagedTarget {
                    path: reviewed.output.path().requested_path().to_path_buf(),
                })?;
            reviewed.output.verify()?;
        }
        let mut ordered_files = Vec::with_capacity(self.outputs.len());
        let mut remaining = std::mem::take(&mut self.outputs).into_iter();
        while let Some(reviewed) = remaining.next() {
            let Some(staged) = reviewed.staged else {
                remove_pending_stages(remaining);
                return Err(OutputReviewError::UnstagedTarget {
                    path: reviewed.output.path().requested_path().to_path_buf(),
                });
            };
            let prepared = match fingerprint_regular_file(&staged.path) {
                Ok(prepared) => prepared,
                Err(source) => {
                    let _ = fs::remove_file(&staged.path);
                    remove_pending_stages(remaining);
                    return Err(OutputReviewError::Stage {
                        path: reviewed.output.path().requested_path().to_path_buf(),
                        source,
                    });
                }
            };
            let (reviewed_path, original) = reviewed.output.into_parts();
            ordered_files.push((
                staged.ordinal,
                StagedFile {
                    reviewed_path,
                    staged: staged.path,
                    original,
                    prepared,
                    installed: None,
                },
            ));
        }
        ordered_files.sort_by_key(|(ordinal, _)| *ordinal);
        let files = ordered_files.into_iter().map(|(_, file)| file).collect();
        Ok(PreparedFileTransaction { files })
    }
}

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileFingerprint {
    length: u64,
    sha256: [u8; 32],
}

impl StagedArtifact {
    /// Final reviewed target represented by the staging file.
    pub(crate) fn target(&self) -> &Path {
        &self.target
    }

    /// Exact staged byte length.
    pub(crate) const fn length(&self) -> u64 {
        self.length
    }

    /// SHA-256 of the exact staged bytes.
    pub(crate) const fn sha256(&self) -> [u8; 32] {
        self.sha256
    }
}
