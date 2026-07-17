//! Reviewed output identity and integrity checks.

use std::path::{Path, PathBuf};

use crate::paths::physical_path_identity;

use super::{digest_hex, hash_bytes};

/// Failure to capture or verify one reviewed output target.
#[derive(Debug, thiserror::Error)]
pub enum OutputReviewError {
    /// A reviewed path could not be resolved to one stable physical identity.
    #[error("failed to resolve reviewed path '{}': {source}", path.display())]
    Resolve {
        /// Path whose existing ancestor could not be resolved.
        path: PathBuf,
        /// Underlying filesystem failure.
        source: std::io::Error,
    },
    /// A reviewed output is a symlink whose referent differs from its write target.
    #[error("reviewed output target '{}' is a symbolic link", path.display())]
    SymlinkTarget {
        /// Lexical output path rejected before review.
        path: PathBuf,
    },
    /// Two reviewed operations would write the same physical target.
    #[error(
        "reviewed outputs '{first}' and '{second}' both write '{}'",
        path.display()
    )]
    DuplicateTarget {
        /// Canonical physical output path shared by both operations.
        path: PathBuf,
        /// First reviewed operation targeting the path.
        first: String,
        /// Second reviewed operation targeting the path.
        second: String,
    },
    /// One reviewed operation would overwrite another operation's input.
    #[error(
        "reviewed output '{output}' would overwrite input '{input}' at '{}'",
        path.display()
    )]
    SourceOutputOverlap {
        /// Canonical physical path used as both source and output.
        path: PathBuf,
        /// Reviewed operation reading the path.
        input: String,
        /// Reviewed operation writing the path.
        output: String,
    },
    /// An output target could not be read while capturing or verifying it.
    #[error("failed to read reviewed output '{}': {source}", path.display())]
    Read {
        /// Output path that could not be read.
        path: PathBuf,
        /// Underlying filesystem failure.
        source: std::io::Error,
    },
    /// A target reviewed as absent appeared before the build could stage it.
    #[error(
        "reviewed output '{}' appeared after preview with SHA-256 {actual}",
        path.display()
    )]
    Appeared {
        /// Output path that appeared.
        path: PathBuf,
        /// SHA-256 of the newly present bytes.
        actual: String,
    },
    /// A target reviewed as present disappeared before the build could stage it.
    #[error(
        "reviewed output '{}' disappeared after preview (expected SHA-256 {expected})",
        path.display()
    )]
    Disappeared {
        /// Output path that disappeared.
        path: PathBuf,
        /// SHA-256 reviewed during preview.
        expected: String,
    },
    /// A target reviewed as present changed before the build could stage it.
    #[error(
        "reviewed output '{}' changed after preview (expected SHA-256 {expected}, found {actual})",
        path.display()
    )]
    Changed {
        /// Output path whose bytes changed.
        path: PathBuf,
        /// SHA-256 reviewed during preview.
        expected: String,
        /// SHA-256 observed before execution.
        actual: String,
    },
    /// Execution attempted to stage a path absent from the reviewed output set.
    #[error("build attempted to stage unreviewed output '{}'", path.display())]
    UnreviewedTarget {
        /// Output path that was not part of the preview.
        path: PathBuf,
    },
    /// Execution attempted to stage one reviewed target more than once.
    #[error("build attempted to stage reviewed output more than once: '{}'", path.display())]
    AlreadyStagedTarget {
        /// Reviewed output path requested a second time.
        path: PathBuf,
    },
    /// A reviewed target was never materialized before transaction sealing.
    #[error("reviewed output was not staged before sealing: '{}'", path.display())]
    UnstagedTarget {
        /// Reviewed output path omitted by rendering.
        path: PathBuf,
    },
    /// A reviewed lexical path now resolves to a different physical target.
    #[error(
        "reviewed output '{}' changed physical identity (expected '{}', found '{}')",
        path.display(),
        expected.display(),
        actual.display()
    )]
    Retargeted {
        /// Lexical path included in the review.
        path: PathBuf,
        /// Physical identity captured during review.
        expected: PathBuf,
        /// Physical identity resolved during verification.
        actual: PathBuf,
    },
    /// A sibling staging file could not be prepared or sealed.
    #[error("failed to prepare reviewed output '{}': {source}", path.display())]
    Stage {
        /// Reviewed target whose staging operation failed.
        path: PathBuf,
        /// Underlying filesystem failure.
        source: std::io::Error,
    },
}

/// Canonical physical identity for an existing source or a potentially absent
/// output target.
///
/// `Path::canonicalize` cannot resolve an absent output. Resolve the nearest
/// existing ancestor instead, then append the remaining lexical components to
/// give reads, writes, and aliases one comparison representation.
#[derive(Debug, Clone)]
pub(in crate::workflow) struct PhysicalPath {
    requested: PathBuf,
    identity: PathBuf,
}

impl PartialEq for PhysicalPath {
    fn eq(&self, other: &Self) -> bool {
        self.identity == other.identity
    }
}

impl Eq for PhysicalPath {}

impl PartialOrd for PhysicalPath {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PhysicalPath {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.identity.cmp(&other.identity)
    }
}

impl PhysicalPath {
    pub(in crate::workflow) fn resolve(path: &Path) -> Result<Self, OutputReviewError> {
        let identity =
            physical_path_identity(path).map_err(|source| OutputReviewError::Resolve {
                path: path.to_path_buf(),
                source,
            })?;
        Ok(Self {
            requested: path.to_path_buf(),
            identity,
        })
    }

    pub(in crate::workflow) fn resolve_output(path: &Path) -> Result<Self, OutputReviewError> {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(OutputReviewError::SymlinkTarget {
                    path: path.to_path_buf(),
                });
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(OutputReviewError::Resolve {
                    path: path.to_path_buf(),
                    source,
                });
            }
        }
        Self::resolve(path)
    }

    pub(in crate::workflow) fn as_path(&self) -> &Path {
        &self.identity
    }

    pub(in crate::workflow) fn requested_path(&self) -> &Path {
        &self.requested
    }

    pub(in crate::workflow) fn verify_identity(&self) -> Result<(), OutputReviewError> {
        let current = Self::resolve_output(&self.requested)?;
        if current == *self {
            return Ok(());
        }
        Err(OutputReviewError::Retargeted {
            path: self.requested.clone(),
            expected: self.identity.clone(),
            actual: current.identity,
        })
    }
}

/// Exact present/absent state of every path one reviewed build may overwrite.
#[derive(Debug)]
pub(in crate::workflow) struct OutputManifest {
    outputs: Vec<ReviewedOutput>,
}

impl OutputManifest {
    pub(in crate::workflow) fn capture(
        paths: impl IntoIterator<Item = PhysicalPath>,
    ) -> Result<Self, OutputReviewError> {
        let mut outputs = Vec::new();
        let mut seen = std::collections::BTreeMap::new();
        for (index, path) in paths.into_iter().enumerate() {
            if let Some(first_index) = seen.insert(path.clone(), index) {
                return Err(OutputReviewError::DuplicateTarget {
                    path: path.as_path().to_path_buf(),
                    first: format!("reviewed output #{}", first_index + 1),
                    second: format!("reviewed output #{}", index + 1),
                });
            }
            outputs.push(ReviewedOutput::capture(path)?);
        }
        Ok(Self { outputs })
    }

    pub(in crate::workflow) fn into_outputs(self) -> Vec<ReviewedOutput> {
        self.outputs
    }
}

#[derive(Debug)]
pub(in crate::workflow) struct ReviewedOutput {
    path: PhysicalPath,
    state: ReviewedOutputState,
}

impl ReviewedOutput {
    fn capture(path: PhysicalPath) -> Result<Self, OutputReviewError> {
        let state = ReviewedOutputState::read(path.as_path())?;
        Ok(Self { path, state })
    }

    pub(in crate::workflow) fn verify(&self) -> Result<(), OutputReviewError> {
        self.path.verify_identity()?;
        let actual = ReviewedOutputState::read(self.path.as_path())?;
        match (&self.state, actual) {
            (ReviewedOutputState::Absent, ReviewedOutputState::Absent) => Ok(()),
            (ReviewedOutputState::Absent, ReviewedOutputState::Present { sha256, .. }) => {
                Err(OutputReviewError::Appeared {
                    path: self.path.requested_path().to_path_buf(),
                    actual: digest_hex(&sha256),
                })
            }
            (ReviewedOutputState::Present { sha256, .. }, ReviewedOutputState::Absent) => {
                Err(OutputReviewError::Disappeared {
                    path: self.path.requested_path().to_path_buf(),
                    expected: digest_hex(sha256),
                })
            }
            (
                ReviewedOutputState::Present {
                    bytes: expected_bytes,
                    sha256: expected,
                },
                ReviewedOutputState::Present {
                    bytes: actual_bytes,
                    sha256: actual,
                },
            ) if *expected == actual && *expected_bytes == actual_bytes => Ok(()),
            (
                ReviewedOutputState::Present {
                    sha256: expected, ..
                },
                ReviewedOutputState::Present { sha256: actual, .. },
            ) => Err(OutputReviewError::Changed {
                path: self.path.requested_path().to_path_buf(),
                expected: digest_hex(expected),
                actual: digest_hex(&actual),
            }),
        }
    }

    pub(in crate::workflow) const fn path(&self) -> &PhysicalPath {
        &self.path
    }

    pub(in crate::workflow) fn into_parts(self) -> (PhysicalPath, Option<Vec<u8>>) {
        let original = match self.state {
            ReviewedOutputState::Absent => None,
            ReviewedOutputState::Present { bytes, .. } => Some(bytes),
        };
        (self.path, original)
    }
}

#[derive(Debug)]
enum ReviewedOutputState {
    Absent,
    Present { bytes: Vec<u8>, sha256: [u8; 32] },
}

impl ReviewedOutputState {
    fn read(path: &Path) -> Result<Self, OutputReviewError> {
        match std::fs::read(path) {
            Ok(bytes) => {
                let sha256 = hash_bytes(&bytes);
                Ok(Self::Present { bytes, sha256 })
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::Absent),
            Err(source) => Err(OutputReviewError::Read {
                path: path.to_path_buf(),
                source,
            }),
        }
    }
}
