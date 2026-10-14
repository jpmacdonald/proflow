//! Typed failures at the build directory boundary.

use std::io;
use std::path::PathBuf;

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
    /// The inputs recorded in the snapshot stage do not hash to the name of
    /// the directory that holds them.
    #[error("build directory {path} is not named by the inputs recorded in it")]
    InputsMismatch {
        /// Build directory whose name and recorded inputs disagree.
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
