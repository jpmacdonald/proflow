//! Regular-file fingerprints, rollback, cleanup, and atomic restoration.

use super::{BuildFileTransaction, FileFingerprint, PendingReviewedOutput, StagedFile};
use sha2::{Digest, Sha256};
use std::fs;
use std::fs::File;
use std::io;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

pub(super) fn remove_pending_stages(outputs: impl IntoIterator<Item = PendingReviewedOutput>) {
    for reviewed in outputs {
        if let Some(staged) = reviewed.staged {
            let _ = fs::remove_file(staged.path);
        }
    }
}

pub(super) fn rollback_file(file: &StagedFile) -> io::Result<()> {
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

pub(super) fn fingerprint_file(path: &Path) -> io::Result<FileFingerprint> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let length = io::copy(&mut file, &mut hasher)?;
    Ok(FileFingerprint {
        length,
        sha256: hasher.finalize().into(),
    })
}

pub(super) fn fingerprint_regular_file(path: &Path) -> io::Result<FileFingerprint> {
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

pub(super) fn fingerprint_bytes(bytes: &[u8]) -> io::Result<FileFingerprint> {
    let length = u64::try_from(bytes.len())
        .map_err(|_| io::Error::other("build artifact is too large to fingerprint"))?;
    Ok(FileFingerprint {
        length,
        sha256: Sha256::digest(bytes).into(),
    })
}

pub(super) fn read_optional(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub(super) fn concurrent_change_error(target: &Path) -> io::Error {
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

pub(super) fn exported_path(directory: &Path, kind: &str, index: usize) -> PathBuf {
    directory.join(kind).join(format!("{index:03}"))
}
