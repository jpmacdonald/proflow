//! Staged file transaction for service builds.
//!
//! Rendering writes only sibling staging files. Commit atomically replaces each
//! target and writes the playlist last, so a failed render never changes live
//! library state and a commit failure can restore already-replaced targets.
//!
//! Each target is compared with its staged-time snapshot immediately before its
//! rename. Filesystems do not provide a portable compare-and-rename operation,
//! so an external writer can still race in the tiny interval between that check
//! and the rename. Environments that permit external writers during a build need
//! an advisory lock above this transaction boundary to eliminate that race.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::approval::{OutputManifest, OutputReviewError, PhysicalPath, ReviewedOutput};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileFingerprint {
    length: u64,
    sha256: [u8; 32],
}

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

impl PreparedFileTransaction {
    /// Read exact sealed bytes for native presentation targets.
    ///
    /// The returned bytes are fingerprint-checked against the sealed state.
    /// Commit repeats that check, so any later staging-file edit aborts before
    /// replacing a live target.
    pub(crate) fn presentation_artifacts(&self) -> io::Result<Vec<(PathBuf, Vec<u8>)>> {
        self.files
            .iter()
            .filter(|file| {
                file.reviewed_path
                    .requested_path()
                    .extension()
                    .and_then(std::ffi::OsStr::to_str)
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("pro"))
            })
            .map(|file| {
                let bytes = fs::read(&file.staged)?;
                if fingerprint_bytes(&bytes)? != file.prepared {
                    return Err(io::Error::other(format!(
                        "prepared build artifact changed after preview: {}",
                        file.reviewed_path.requested_path().display()
                    )));
                }
                Ok((file.reviewed_path.requested_path().to_path_buf(), bytes))
            })
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn staged_bytes_for(&self, target: &Path) -> io::Result<Option<Vec<u8>>> {
        self.files
            .iter()
            .find(|file| file.reviewed_path.requested_path() == target)
            .map(|file| fs::read(&file.staged))
            .transpose()
    }

    /// Commit in reservation order. Callers reserve the playlist last.
    pub(crate) fn commit(self) -> io::Result<Vec<PathBuf>> {
        #[cfg(test)]
        {
            self.commit_inner(None)
        }
        #[cfg(not(test))]
        {
            self.commit_inner()
        }
    }

    fn commit_inner(
        mut self,
        #[cfg(test)] mut before_recheck: Option<BeforeRecheck<'_>>,
    ) -> io::Result<Vec<PathBuf>> {
        self.verify_targets_unchanged()?;
        self.verify_staged_unchanged()?;

        for committed in 0..self.files.len() {
            #[cfg(test)]
            {
                let target = self.files[committed].reviewed_path.requested_path();
                if let Some(before_recheck) = before_recheck.as_mut() {
                    if let Err(error) = before_recheck(committed, target) {
                        return Err(self.commit_failure(committed, error));
                    }
                }
            }
            if let Err(error) = self.verify_target_unchanged(committed) {
                return Err(self.commit_failure(committed, error));
            }
            if let Err(error) = self.verify_staged_file_unchanged(committed) {
                return Err(self.commit_failure(committed, error));
            }
            if let Err(error) = fs::rename(
                &self.files[committed].staged,
                self.files[committed].reviewed_path.requested_path(),
            ) {
                return Err(self.commit_failure(committed, error));
            }
            self.files[committed].installed = Some(self.files[committed].prepared);
        }

        let targets = self
            .files
            .iter()
            .map(|file| file.reviewed_path.requested_path().to_path_buf())
            .collect();
        self.files.clear();
        Ok(targets)
    }

    #[cfg(test)]
    fn commit_with_before_recheck(
        self,
        mut before_recheck: impl FnMut(usize, &Path) -> io::Result<()>,
    ) -> io::Result<Vec<PathBuf>> {
        self.commit_inner(Some(&mut before_recheck))
    }

    fn verify_targets_unchanged(&self) -> io::Result<()> {
        for index in 0..self.files.len() {
            self.verify_target_unchanged(index)?;
        }
        Ok(())
    }

    fn verify_staged_unchanged(&self) -> io::Result<()> {
        for index in 0..self.files.len() {
            self.verify_staged_file_unchanged(index)?;
        }
        Ok(())
    }

    fn verify_staged_file_unchanged(&self, index: usize) -> io::Result<()> {
        let file = &self.files[index];
        let actual = fingerprint_regular_file(&file.staged)?;
        if actual != file.prepared {
            return Err(io::Error::other(format!(
                "prepared build artifact changed after preview: {}",
                file.reviewed_path.requested_path().display()
            )));
        }
        Ok(())
    }

    fn verify_target_unchanged(&self, index: usize) -> io::Result<()> {
        let file = &self.files[index];
        if file.reviewed_path.verify_identity().is_err() {
            return Err(concurrent_change_error(file.reviewed_path.requested_path()));
        }
        let current = read_optional(file.reviewed_path.as_path())?;
        if current != file.original {
            return Err(concurrent_change_error(file.reviewed_path.requested_path()));
        }
        Ok(())
    }

    fn commit_failure(&self, committed: usize, error: io::Error) -> io::Error {
        match self.rollback_prefix(committed) {
            Ok(()) => error,
            Err(rollback) => io::Error::other(format!(
                "commit failed: {error}; rollback also failed: {rollback}"
            )),
        }
    }

    fn rollback_prefix(&self, committed: usize) -> io::Result<()> {
        let mut first_error = None;
        for file in self.files[..committed].iter().rev() {
            if let Err(error) = rollback_file(file) {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

impl Drop for BuildFileTransaction {
    fn drop(&mut self) {
        for reviewed in &self.outputs {
            if let Some(staged) = &reviewed.staged {
                let _ = fs::remove_file(&staged.path);
            }
        }
    }
}

impl Drop for StagedFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.staged);
    }
}

fn remove_pending_stages(outputs: impl IntoIterator<Item = PendingReviewedOutput>) {
    for reviewed in outputs {
        if let Some(staged) = reviewed.staged {
            let _ = fs::remove_file(staged.path);
        }
    }
}

fn rollback_file(file: &StagedFile) -> io::Result<()> {
    let installed = file.installed.ok_or_else(|| {
        io::Error::other(format!(
            "cannot roll back '{}' without installed-byte state",
            file.reviewed_path.requested_path().display()
        ))
    })?;
    let current = fingerprint_file(file.reviewed_path.requested_path())?;
    if current != installed {
        return Err(concurrent_change_error(file.reviewed_path.requested_path()));
    }

    file.original.as_ref().map_or_else(
        || fs::remove_file(file.reviewed_path.requested_path()),
        |bytes| atomic_restore(file.reviewed_path.requested_path(), bytes),
    )
}

fn fingerprint_file(path: &Path) -> io::Result<FileFingerprint> {
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 16_384];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(FileFingerprint {
        length,
        sha256: hasher.finalize().into(),
    })
}

fn fingerprint_regular_file(path: &Path) -> io::Result<FileFingerprint> {
    ensure_regular_file(path)?;
    let fingerprint = fingerprint_file(path)?;
    ensure_regular_file(path)?;
    Ok(fingerprint)
}

fn ensure_regular_file(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(io::Error::other(format!(
            "staged build artifact is a symlink: {}",
            path.display()
        )));
    }
    if !metadata.is_file() {
        return Err(io::Error::other(format!(
            "staged build artifact is not a regular file: {}",
            path.display()
        )));
    }
    Ok(())
}

fn fingerprint_bytes(bytes: &[u8]) -> io::Result<FileFingerprint> {
    let length = u64::try_from(bytes.len())
        .map_err(|_| io::Error::other("build artifact is too large to fingerprint"))?;
    Ok(FileFingerprint {
        length,
        sha256: Sha256::digest(bytes).into(),
    })
}

fn read_optional(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn concurrent_change_error(target: &Path) -> io::Error {
    io::Error::other(format!(
        "build output target changed concurrently: {}",
        target.display()
    ))
}

fn atomic_restore(target: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let filename = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("artifact");
    let staged = parent.join(format!(
        ".{filename}.proflow-restore-{}",
        uuid::Uuid::new_v4()
    ));
    let result = (|| {
        let mut file = File::create(&staged)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&staged, target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(staged);
    }
    result
}

#[cfg(test)]
mod tests;
