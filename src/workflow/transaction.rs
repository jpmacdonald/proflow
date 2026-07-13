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

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

struct StagedFile {
    target: PathBuf,
    staged: PathBuf,
    original: Option<Vec<u8>>,
    installed: Option<Vec<u8>>,
}

#[cfg(test)]
type BeforeRecheck<'a> = &'a mut dyn FnMut(usize, &Path) -> io::Result<()>;

/// Owns every filesystem change made by one service build.
pub(crate) struct BuildFileTransaction {
    files: Vec<StagedFile>,
    targets: HashSet<PathBuf>,
}

impl BuildFileTransaction {
    pub(crate) fn new() -> Self {
        Self {
            files: Vec::new(),
            targets: HashSet::new(),
        }
    }

    /// Reserve a target and return a unique sibling path for rendering.
    pub(crate) fn stage_for(&mut self, target: &Path) -> io::Result<PathBuf> {
        if !self.targets.insert(target.to_path_buf()) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("duplicate build output target: {}", target.display()),
            ));
        }

        let parent = target.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let original = read_optional(target)?;
        let filename = target
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("artifact");
        let staged = parent.join(format!(
            ".{filename}.proflow-stage-{}",
            uuid::Uuid::new_v4()
        ));
        self.files.push(StagedFile {
            target: target.to_path_buf(),
            staged: staged.clone(),
            original,
            installed: None,
        });
        Ok(staged)
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

        for committed in 0..self.files.len() {
            let replacement = match fs::read(&self.files[committed].staged) {
                Ok(bytes) => bytes,
                Err(error) => return Err(self.commit_failure(committed, error)),
            };
            #[cfg(test)]
            {
                let target = &self.files[committed].target;
                if let Some(before_recheck) = before_recheck.as_mut() {
                    if let Err(error) = before_recheck(committed, target) {
                        return Err(self.commit_failure(committed, error));
                    }
                }
            }
            if let Err(error) = self.verify_target_unchanged(committed) {
                return Err(self.commit_failure(committed, error));
            }
            if let Err(error) =
                fs::rename(&self.files[committed].staged, &self.files[committed].target)
            {
                return Err(self.commit_failure(committed, error));
            }
            self.files[committed].installed = Some(replacement);
        }

        let targets = self.files.iter().map(|file| file.target.clone()).collect();
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

    fn verify_target_unchanged(&self, index: usize) -> io::Result<()> {
        let file = &self.files[index];
        let current = read_optional(&file.target)?;
        if current != file.original {
            return Err(concurrent_change_error(&file.target));
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
        for file in &self.files {
            let _ = fs::remove_file(&file.staged);
        }
    }
}

fn rollback_file(file: &StagedFile) -> io::Result<()> {
    let installed = file.installed.as_deref().ok_or_else(|| {
        io::Error::other(format!(
            "cannot roll back '{}' without installed-byte state",
            file.target.display()
        ))
    })?;
    let current = read_optional(&file.target)?;
    if current.as_deref() != Some(installed) {
        return Err(concurrent_change_error(&file.target));
    }

    file.original.as_ref().map_or_else(
        || fs::remove_file(&file.target),
        |bytes| atomic_restore(&file.target, bytes),
    )
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
mod tests {
    #![allow(clippy::expect_used)]

    use std::cell::Cell;

    use super::*;

    #[test]
    fn duplicate_target_is_rejected_before_rendering() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("same.pro");
        let mut transaction = BuildFileTransaction::new();
        transaction.stage_for(&target).expect("first target");
        let error = transaction
            .stage_for(&target)
            .expect_err("duplicate target");
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    }

    #[test]
    fn dropping_transaction_preserves_existing_target() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("service.pro");
        fs::write(&target, b"old").expect("old target");
        let staged;
        {
            let mut transaction = BuildFileTransaction::new();
            staged = transaction.stage_for(&target).expect("stage target");
            fs::write(&staged, b"new").expect("stage bytes");
        }
        assert_eq!(fs::read(target).expect("target"), b"old");
        assert!(!staged.exists(), "drop must remove staged output");
    }

    #[test]
    fn commit_replaces_target() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("service.pro");
        fs::write(&target, b"old").expect("old target");
        let mut transaction = BuildFileTransaction::new();
        let staged = transaction.stage_for(&target).expect("stage target");
        fs::write(staged, b"new").expect("stage bytes");
        transaction.commit().expect("commit");
        assert_eq!(fs::read(target).expect("target"), b"new");
    }

    #[test]
    fn commit_aborts_before_any_rename_when_a_target_changes_after_staging() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first_target = dir.path().join("first.pro");
        let second_target = dir.path().join("second.pro");
        fs::write(&first_target, b"old first").expect("old first target");
        fs::write(&second_target, b"old second").expect("old second target");
        let mut transaction = BuildFileTransaction::new();
        let first_stage = transaction.stage_for(&first_target).expect("first stage");
        let second_stage = transaction.stage_for(&second_target).expect("second stage");
        fs::write(first_stage, b"new first").expect("new first bytes");
        fs::write(second_stage, b"new second").expect("new second bytes");

        fs::write(&second_target, b"concurrent second").expect("concurrent target change");
        let entered_commit_loop = Cell::new(false);
        let error = transaction
            .commit_with_before_recheck(|_, _| {
                entered_commit_loop.set(true);
                Ok(())
            })
            .expect_err("concurrent target change must abort commit");

        assert!(error.to_string().contains("changed concurrently"));
        assert!(
            !entered_commit_loop.get(),
            "initial validation must stop before the first rename-loop iteration"
        );
        assert_eq!(
            fs::read(first_target).expect("unchanged first target"),
            b"old first"
        );
        assert_eq!(
            fs::read(second_target).expect("concurrent second target"),
            b"concurrent second"
        );
    }

    #[test]
    fn commit_preserves_a_concurrently_created_target() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("new.pro");
        let mut transaction = BuildFileTransaction::new();
        let staged = transaction.stage_for(&target).expect("stage absent target");
        fs::write(staged, b"generated").expect("generated bytes");

        fs::write(&target, b"concurrent").expect("concurrently create target");
        let error = transaction
            .commit()
            .expect_err("concurrently created target must abort commit");

        assert!(error.to_string().contains("changed concurrently"));
        assert_eq!(fs::read(target).expect("concurrent target"), b"concurrent");
    }

    #[test]
    fn per_target_recheck_rolls_back_without_overwriting_post_install_change() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first_target = dir.path().join("first.pro");
        let second_target = dir.path().join("second.pro");
        let third_target = dir.path().join("third.pro");
        fs::write(&first_target, b"old first").expect("old first target");
        fs::write(&second_target, b"old second").expect("old second target");
        fs::write(&third_target, b"old third").expect("old third target");

        let mut transaction = BuildFileTransaction::new();
        let first_stage = transaction.stage_for(&first_target).expect("first stage");
        let second_stage = transaction.stage_for(&second_target).expect("second stage");
        let third_stage = transaction.stage_for(&third_target).expect("third stage");
        fs::write(first_stage, b"new first").expect("new first bytes");
        fs::write(second_stage, b"new second").expect("new second bytes");
        fs::write(third_stage, b"new third").expect("new third bytes");

        let error = transaction
            .commit_with_before_recheck(|index, target| {
                if index == 2 {
                    fs::write(&second_target, b"concurrent second")?;
                    fs::write(target, b"concurrent third")?;
                }
                Ok(())
            })
            .expect_err("third target changed after preflight must abort commit");

        assert!(error.to_string().contains("changed concurrently"));
        assert_eq!(
            fs::read(first_target).expect("restored first target"),
            b"old first"
        );
        assert_eq!(
            fs::read(second_target).expect("concurrent second target"),
            b"concurrent second"
        );
        assert_eq!(
            fs::read(third_target).expect("concurrent third target"),
            b"concurrent third"
        );
    }

    #[test]
    fn staging_propagates_non_not_found_read_errors() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("directory-target.pro");
        fs::create_dir(&target).expect("directory target");
        let mut transaction = BuildFileTransaction::new();

        assert!(transaction.stage_for(&target).is_err());
    }

    #[test]
    fn later_commit_failure_restores_an_earlier_target() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first_target = dir.path().join("first.pro");
        let second_target = dir.path().join("second.pro");
        fs::write(&first_target, b"old").expect("old first target");
        let mut transaction = BuildFileTransaction::new();
        let first_stage = transaction.stage_for(&first_target).expect("first stage");
        fs::write(first_stage, b"new").expect("new first target");
        let second_stage = transaction.stage_for(&second_target).expect("second stage");
        fs::write(&second_stage, b"second").expect("second bytes");
        fs::remove_file(second_stage).expect("force second commit failure");

        assert!(transaction.commit().is_err());
        assert_eq!(
            fs::read(first_target).expect("restored first target"),
            b"old"
        );
        assert!(!second_target.exists());
    }

    #[test]
    fn later_commit_failure_removes_an_earlier_new_target() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first_target = dir.path().join("new-first.pro");
        let second_target = dir.path().join("second.pro");
        let mut transaction = BuildFileTransaction::new();
        let first_stage = transaction.stage_for(&first_target).expect("first stage");
        fs::write(first_stage, b"new").expect("new first target");
        let second_stage = transaction.stage_for(&second_target).expect("second stage");
        fs::write(&second_stage, b"second").expect("second bytes");
        fs::remove_file(second_stage).expect("force second commit failure");

        assert!(transaction.commit().is_err());
        assert!(!first_target.exists());
        assert!(!second_target.exists());
    }

    #[test]
    fn rollback_does_not_overwrite_a_concurrently_changed_committed_target() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("service.pro");
        fs::write(&target, b"old").expect("old target");
        let mut transaction = BuildFileTransaction::new();
        let staged = transaction.stage_for(&target).expect("stage target");
        fs::write(&staged, b"installed").expect("installed bytes");

        let installed = fs::read(&staged).expect("read staged bytes before rename");
        fs::rename(&staged, &target).expect("simulate committed rename");
        transaction.files[0].installed = Some(installed);
        fs::write(&target, b"concurrent").expect("concurrent post-commit change");

        let error = transaction
            .rollback_prefix(1)
            .expect_err("rollback must reject changed installed bytes");

        assert!(error.to_string().contains("changed concurrently"));
        assert_eq!(fs::read(target).expect("concurrent target"), b"concurrent");
    }
}
