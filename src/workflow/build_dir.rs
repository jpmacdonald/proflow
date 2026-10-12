//! Hashed, write-once build directories.
//!
//! A build lives at `<builds root>/<plan id>/<input hash>/`. The hash names
//! everything the build is derived from, so the same inputs always reach the
//! same directory and any changed input reaches a different one. Each stage is
//! a subdirectory that either exists, complete and sealed, or does not exist.
//!
//! The stage a caller holds is part of the type: a stage can only be written
//! from the one before it:
//!
//! ```no_run
//! use proflow::workflow::build_dir::{BuildDir, Opened, Snapshotted};
//!
//! fn first(build: BuildDir<Opened>) {
//!     let _ = build.write::<Snapshotted>(|_| Ok(()));
//! }
//! ```
//!
//! Skipping a stage does not compile:
//!
//! ```compile_fail
//! use proflow::workflow::build_dir::{BuildDir, Opened, Rendered};
//!
//! fn skip(build: BuildDir<Opened>) {
//!     let _ = build.write::<Rendered>(|_| Ok(()));
//! }
//! ```

use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File};
use std::io;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const INPUT_HASH_DOMAIN: &[u8] = b"proflow.build-inputs.v1";
const SEAL_FILE: &str = "stage.json";
const SEAL_VERSION: u32 = 1;
const REVIEW_FILE: &str = "review.toml";

/// Planning Center plan identity, checked to be one safe directory name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildPlanId(String);

impl BuildPlanId {
    /// Accept 1 to 64 ASCII letters, digits, `-`, or `_`.
    pub fn new(value: impl Into<String>) -> Result<Self, BuildDirError> {
        let value = value.into();
        let safe = !value.is_empty()
            && value.len() <= 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
        if safe {
            Ok(Self(value))
        } else {
            Err(BuildDirError::InvalidPlanId { value })
        }
    }

    /// Return the plan identity text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Everything a build is derived from, as the exact bytes that were read.
#[derive(Debug, Clone, Copy)]
pub struct BuildInputs<'a> {
    /// Captured Planning Center plan snapshot.
    pub plan_snapshot: &'a [u8],
    /// Fingerprint of the project configuration and native display assets.
    pub render_assets: &'a [u8],
    /// Fingerprints of the Bible corpora the build reads.
    pub bible_corpora: &'a [u8],
    /// Every presentation in the library the review could match, by content.
    pub library: &'a [u8],
    /// The operator's decisions in canonical form.
    pub review_decisions: &'a [u8],
}

/// SHA-256 over every build input, each bound to its name and length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputHash([u8; 32]);

impl InputHash {
    /// Hash one set of build inputs.
    #[must_use]
    pub fn of(inputs: &BuildInputs<'_>) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(INPUT_HASH_DOMAIN);
        for (name, bytes) in [
            ("plan_snapshot", inputs.plan_snapshot),
            ("render_assets", inputs.render_assets),
            ("bible_corpora", inputs.bible_corpora),
            ("library", inputs.library),
            ("review_decisions", inputs.review_decisions),
        ] {
            // Name and length keep a byte from moving between two inputs
            // without changing the hash.
            hasher.update((name.len() as u64).to_be_bytes());
            hasher.update(name.as_bytes());
            hasher.update((bytes.len() as u64).to_be_bytes());
            hasher.update(bytes);
        }
        Self(hasher.finalize().into())
    }
}

impl fmt::Display for InputHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&hex(&self.0))
    }
}

mod sealed {
    pub trait Sealed {}
}

/// Position of a build in the pipeline.
pub trait Stage: sealed::Sealed {}

/// A stage that owns one sealed subdirectory.
pub trait WrittenStage: Stage {
    /// Directory name below the build directory.
    const DIRECTORY: &'static str;
    /// The stage that must already be written.
    type Previous: Stage;
}

macro_rules! stages {
    ($first:ident; $($stage:ident($directory:literal) after $previous:ident: $doc:literal),+ $(,)?) => {
        /// A build directory with no stage written yet.
        #[derive(Debug)]
        pub struct $first;
        impl sealed::Sealed for $first {}
        impl Stage for $first {}
        $(
            #[doc = $doc]
            #[derive(Debug)]
            pub struct $stage;
            impl sealed::Sealed for $stage {}
            impl Stage for $stage {}
            impl WrittenStage for $stage {
                const DIRECTORY: &'static str = $directory;
                type Previous = $previous;
            }
        )+
    };
}

stages! {
    Opened;
    Snapshotted("snapshot") after Opened: "The plan snapshot and the library catalog it was reviewed against.",
    Classified("classified") after Snapshotted: "The semantic plan for every item.",
    Rendered("rendered") after Classified: "The native artifacts, staged and not yet verified.",
    Verified("verified") after Rendered: "The record of every check the rendered artifacts passed.",
    Committed("committed") after Verified: "The receipt of the committed build.",
}

/// One build directory, held at stage `S`.
#[derive(Debug)]
pub struct BuildDir<S: Stage> {
    path: PathBuf,
    stage: PhantomData<S>,
}

/// Result of looking for the next stage of a build that may have run before.
#[derive(Debug)]
pub enum Resumed<S: Stage, N: Stage> {
    /// The next stage is on disk and its seal matches its files.
    Written(BuildDir<N>),
    /// The next stage has not been written.
    NotYet(BuildDir<S>),
}

impl BuildDir<Opened> {
    /// Create or reopen the directory named by `plan` and `inputs`.
    pub fn open(
        builds_root: &Path,
        plan: &BuildPlanId,
        inputs: &BuildInputs<'_>,
    ) -> Result<Self, BuildDirError> {
        let path = builds_root
            .join(plan.as_str())
            .join(InputHash::of(inputs).to_string());
        fs::create_dir_all(&path).map_err(|source| io_error(&path, source))?;
        Ok(Self {
            path,
            stage: PhantomData,
        })
    }
}

/// Where the review decisions for one plan are recorded.
///
/// The file sits beside the hashed build directories, not inside one: its
/// bytes are a build input, so editing an answer selects a different build.
#[must_use]
pub fn review_decisions_path(builds_root: &Path, plan: &BuildPlanId) -> PathBuf {
    builds_root.join(plan.as_str()).join(REVIEW_FILE)
}

impl<S: Stage> BuildDir<S> {
    /// Directory holding every stage of this build.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Write the next stage, exactly once.
    ///
    /// `write` fills an empty temporary directory. The stage appears under its
    /// real name only after every file is hashed into a seal and flushed, so a
    /// crash or an error leaves no stage behind.
    pub fn write<N>(
        self,
        write: impl FnOnce(&Path) -> io::Result<()>,
    ) -> Result<BuildDir<N>, BuildDirError>
    where
        N: WrittenStage<Previous = S>,
    {
        let stage = self.path.join(N::DIRECTORY);
        if stage.exists() {
            return Err(BuildDirError::AlreadyWritten {
                stage: N::DIRECTORY,
            });
        }
        let temporary = self.path.join(format!("{}.tmp", N::DIRECTORY));
        // Whatever a crashed run left is discarded. If it cannot be, creating
        // the directory fails and reports why.
        let _ = fs::remove_dir_all(&temporary);
        fs::create_dir(&temporary).map_err(|source| io_error(&temporary, source))?;
        let sealed = write(&temporary)
            .map_err(|source| io_error(&temporary, source))
            .and_then(|()| seal(&temporary, N::DIRECTORY))
            .and_then(|()| {
                fs::rename(&temporary, &stage).map_err(|source| io_error(&stage, source))
            })
            .and_then(|()| sync_directory(&self.path));
        if let Err(error) = sealed {
            let _ = fs::remove_dir_all(&temporary);
            return Err(error);
        }
        Ok(BuildDir {
            path: self.path,
            stage: PhantomData,
        })
    }

    /// Reopen the next stage if an earlier run wrote it, otherwise write it.
    ///
    /// Only for a stage whose content is a function of the build inputs, so
    /// that an earlier run's stage is the stage this run would write.
    pub fn resume_or_write<N>(
        self,
        write: impl FnOnce(&Path) -> io::Result<()>,
    ) -> Result<BuildDir<N>, BuildDirError>
    where
        N: WrittenStage<Previous = S>,
    {
        match self.resume()? {
            Resumed::Written(build) => Ok(build),
            Resumed::NotYet(build) => build.write(write),
        }
    }

    /// Reopen the next stage if an earlier run wrote it.
    ///
    /// A stage whose files no longer match its seal is an error, never a
    /// stage to continue from.
    pub fn resume<N>(self) -> Result<Resumed<S, N>, BuildDirError>
    where
        N: WrittenStage<Previous = S>,
    {
        let stage = self.path.join(N::DIRECTORY);
        if !stage.exists() {
            return Ok(Resumed::NotYet(self));
        }
        verify_seal(&stage, N::DIRECTORY)?;
        Ok(Resumed::Written(BuildDir {
            path: self.path,
            stage: PhantomData,
        }))
    }
}

impl<S: WrittenStage> BuildDir<S> {
    /// Directory of the stage this build is held at.
    #[must_use]
    pub fn stage_path(&self) -> PathBuf {
        self.path.join(S::DIRECTORY)
    }
}

/// Why a build directory operation was refused.
#[derive(Debug, thiserror::Error)]
pub enum BuildDirError {
    /// The plan identity cannot name a directory.
    #[error("plan id '{value}' must be 1-64 ASCII letters, digits, '-' or '_'")]
    InvalidPlanId {
        /// Rejected identity.
        value: String,
    },
    /// The stage was written by an earlier run; stages are never rewritten.
    #[error("build stage '{stage}' is already written")]
    AlreadyWritten {
        /// Stage directory name.
        stage: &'static str,
    },
    /// A stage holds something other than regular files and directories.
    #[error("build stage '{stage}' contains unsupported entry {path}")]
    UnsupportedEntry {
        /// Stage directory name.
        stage: &'static str,
        /// Offending entry.
        path: PathBuf,
    },
    /// A stage's files differ from what its seal recorded.
    #[error("build stage '{stage}' does not match its seal: {detail}")]
    SealMismatch {
        /// Stage directory name.
        stage: &'static str,
        /// What differs.
        detail: String,
    },
    /// Reading or writing the build directory failed.
    #[error("build directory I/O failed at {path}: {source}")]
    Io {
        /// Path the operation touched.
        path: PathBuf,
        /// Underlying failure.
        source: io::Error,
    },
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Seal {
    version: u32,
    stage: String,
    /// Stage-relative path, `/`-separated, to the SHA-256 of the file.
    files: BTreeMap<String, String>,
}

fn seal(directory: &Path, stage: &'static str) -> Result<(), BuildDirError> {
    let seal = Seal {
        version: SEAL_VERSION,
        stage: stage.to_string(),
        files: hash_files(directory, stage)?,
    };
    let path = directory.join(SEAL_FILE);
    let bytes = serde_json::to_vec_pretty(&seal)
        .map_err(|source| io_error(&path, io::Error::other(source)))?;
    fs::write(&path, bytes).map_err(|source| io_error(&path, source))?;
    sync_tree(directory)
}

fn verify_seal(directory: &Path, stage: &'static str) -> Result<(), BuildDirError> {
    let path = directory.join(SEAL_FILE);
    let bytes = fs::read(&path).map_err(|source| io_error(&path, source))?;
    let recorded: Seal = serde_json::from_slice(&bytes).map_err(|error| mismatch(stage, error))?;
    let actual = Seal {
        version: SEAL_VERSION,
        stage: stage.to_string(),
        files: hash_files(directory, stage)?,
    };
    if recorded == actual {
        return Ok(());
    }
    let changed = recorded
        .files
        .iter()
        .find(|(name, hash)| actual.files.get(*name) != Some(hash))
        .map(|(name, _)| name.as_str())
        .or_else(|| {
            actual
                .files
                .keys()
                .find(|name| !recorded.files.contains_key(*name))
                .map(String::as_str)
        });
    Err(mismatch(
        stage,
        changed.map_or_else(
            || "seal header differs".to_string(),
            |name| format!("file '{name}' was added, removed, or changed"),
        ),
    ))
}

/// Hash every file below `directory` except the seal itself.
fn hash_files(
    directory: &Path,
    stage: &'static str,
) -> Result<BTreeMap<String, String>, BuildDirError> {
    let mut files = BTreeMap::new();
    let mut pending = vec![(directory.to_path_buf(), String::new())];
    while let Some((current, prefix)) = pending.pop() {
        for entry in fs::read_dir(&current).map_err(|source| io_error(&current, source))? {
            let entry = entry.map_err(|source| io_error(&current, source))?;
            let path = entry.path();
            let unsupported = || BuildDirError::UnsupportedEntry {
                stage,
                path: path.clone(),
            };
            let name = entry.file_name().into_string().map_err(|_| unsupported())?;
            let relative = format!("{prefix}{name}");
            // `file_type` does not follow symlinks, so a link is rejected
            // instead of being hashed as whatever it points at.
            let kind = entry
                .file_type()
                .map_err(|source| io_error(&path, source))?;
            if kind.is_dir() {
                pending.push((path, format!("{relative}/")));
            } else if !kind.is_file() {
                return Err(unsupported());
            } else if relative != SEAL_FILE {
                let bytes = fs::read(&path).map_err(|source| io_error(&path, source))?;
                files.insert(relative, hex(&Sha256::digest(bytes)));
            }
        }
    }
    Ok(files)
}

/// Flush every file and directory below `directory`, then `directory` itself.
fn sync_tree(directory: &Path) -> Result<(), BuildDirError> {
    for entry in fs::read_dir(directory).map_err(|source| io_error(directory, source))? {
        let path = entry.map_err(|source| io_error(directory, source))?.path();
        if path.is_dir() {
            sync_tree(&path)?;
        } else {
            File::open(&path)
                .and_then(|file| file.sync_all())
                .map_err(|source| io_error(&path, source))?;
        }
    }
    sync_directory(directory)
}

fn sync_directory(directory: &Path) -> Result<(), BuildDirError> {
    File::open(directory)
        .and_then(|handle| handle.sync_all())
        .map_err(|source| io_error(directory, source))
}

fn io_error(path: &Path, source: io::Error) -> BuildDirError {
    BuildDirError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn mismatch(stage: &'static str, detail: impl fmt::Display) -> BuildDirError {
    BuildDirError::SealMismatch {
        stage,
        detail: detail.to_string(),
    }
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    use fmt::Write as _;

    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}

#[cfg(test)]
mod tests;
