//! Commit, target rechecks, and rollback of sealed transactions.

use std::{fs, io};

#[cfg(test)]
use super::BeforeRecheck;

use super::{
    concurrent_change_error, exported_path, fingerprint_bytes, fingerprint_regular_file,
    read_optional, rollback_file, BuildFileTransaction, ExportedOutput, OutputManifest,
    OutputReviewError, PreparedFileTransaction, StagedArtifact, ARTIFACTS_DIRECTORY,
    ORIGINALS_DIRECTORY,
};
use std::path::Path;
use std::path::PathBuf;

impl PreparedFileTransaction {
    /// Exact target fingerprints captured by [`BuildFileTransaction::seal`].
    ///
    /// This deliberately reports the frozen `prepared` values instead of
    /// rereading staging paths. Receipt verification can therefore prove that
    /// the committable transaction is the same artifact set whose evidence was
    /// serialized, without opening another time-of-check/time-of-use window.
    pub(crate) fn sealed_artifacts(&self) -> Vec<StagedArtifact> {
        self.files
            .iter()
            .map(|file| StagedArtifact {
                target: file.reviewed_path.requested_path().to_path_buf(),
                length: file.prepared.length,
                sha256: file.prepared.sha256,
            })
            .collect()
    }

    /// Copy every sealed artifact, and the bytes it will replace, into
    /// `directory`, in commit order.
    pub(crate) fn export(&self, directory: &Path) -> io::Result<Vec<ExportedOutput>> {
        fs::create_dir(directory.join(ARTIFACTS_DIRECTORY))?;
        fs::create_dir(directory.join(ORIGINALS_DIRECTORY))?;
        self.files
            .iter()
            .enumerate()
            .map(|(index, file)| {
                let bytes = fs::read(&file.staged)?;
                if fingerprint_bytes(&bytes)? != file.prepared {
                    return Err(io::Error::other(format!(
                        "prepared build artifact changed after preview: {}",
                        file.reviewed_path.requested_path().display()
                    )));
                }
                fs::write(exported_path(directory, ARTIFACTS_DIRECTORY, index), bytes)?;
                if let Some(original) = &file.original {
                    fs::write(
                        exported_path(directory, ORIGINALS_DIRECTORY, index),
                        original,
                    )?;
                }
                Ok(ExportedOutput {
                    target: file.reviewed_path.requested_path().to_path_buf(),
                    length: file.prepared.length,
                    sha256: crate::workflow::build_dir::hex(&file.prepared.sha256),
                    replaces_existing: file.original.is_some(),
                })
            })
            .collect()
    }

    /// Stage an exported artifact set again, beside its live targets.
    ///
    /// This goes through the same reviewed-output checks as a first render:
    /// each target must still hold exactly the bytes recorded for it, and the
    /// restaged bytes must match the fingerprints in `outputs`.
    pub(crate) fn restage(
        directory: &Path,
        outputs: &[ExportedOutput],
    ) -> Result<Self, OutputReviewError> {
        let stage_error = |target: &Path, source: io::Error| OutputReviewError::Stage {
            path: target.to_path_buf(),
            source,
        };
        let recorded = outputs
            .iter()
            .enumerate()
            .map(|(index, output)| {
                let original = output
                    .replaces_existing
                    .then(|| fs::read(exported_path(directory, ORIGINALS_DIRECTORY, index)))
                    .transpose()
                    .map_err(|source| stage_error(&output.target, source))?;
                Ok((output.target.clone(), original))
            })
            .collect::<Result<Vec<_>, OutputReviewError>>()?;
        let mut transaction =
            BuildFileTransaction::from_reviewed(OutputManifest::recorded(recorded)?);
        for (index, output) in outputs.iter().enumerate() {
            let staged = transaction.stage_reviewed(&output.target)?;
            fs::copy(
                exported_path(directory, ARTIFACTS_DIRECTORY, index),
                &staged,
            )
            .map_err(|source| stage_error(&output.target, source))?;
        }
        let prepared = transaction.seal()?;
        for (file, output) in prepared.files.iter().zip(outputs) {
            if file.prepared.length != output.length
                || crate::workflow::build_dir::hex(&file.prepared.sha256) != output.sha256
            {
                return Err(stage_error(
                    &output.target,
                    io::Error::other("rendered artifact does not match its recorded fingerprint"),
                ));
            }
        }
        Ok(prepared)
    }

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
    pub(super) fn commit_with_before_recheck(
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

    pub(super) fn rollback_prefix(&self, committed: usize) -> io::Result<()> {
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
