//! Reviewed workflow inputs plus source and output integrity checks.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::plan::{ContentSource, ResolvedItemPlan, ScriptureRequest};
use crate::bible::BibleVersion;

/// A resolved plan whose file-backed inputs have been captured for later
/// verification.
///
/// The fields are private so callers cannot manufacture a reviewed plan by
/// attaching paths without also hashing their bytes.
#[derive(Debug)]
pub(super) struct ReviewedServicePlan {
    plans: Vec<ResolvedItemPlan>,
    sources: SourceManifest,
}

impl ReviewedServicePlan {
    pub(super) fn capture_with_additional_sources(
        plans: Vec<ResolvedItemPlan>,
        project_data_root: &Path,
        additional_sources: impl IntoIterator<Item = PathBuf>,
    ) -> Result<Self, SourceReviewError> {
        let mut paths = plan_source_paths(&plans, project_data_root)?;
        paths.extend(additional_sources);
        Ok(Self {
            plans,
            sources: SourceManifest::capture(paths)?,
        })
    }

    /// Return the immutable decisions used to render the operator preview.
    pub(super) fn plans(&self) -> &[ResolvedItemPlan] {
        &self.plans
    }

    pub(super) fn source_bytes(&self, path: &Path) -> Option<&[u8]> {
        self.sources.bytes(path)
    }

    pub(super) fn extend_sources(
        &mut self,
        paths: impl IntoIterator<Item = PathBuf>,
    ) -> Result<(), SourceReviewError> {
        self.sources.extend_capture(paths)
    }

    pub(super) fn into_verified_parts(
        self,
    ) -> Result<(Vec<ResolvedItemPlan>, SourceManifest), SourceReviewError> {
        self.sources.verify()?;
        Ok((self.plans, self.sources))
    }
}

/// Failure to capture or verify a reviewed source file.
#[derive(Debug, thiserror::Error)]
pub enum SourceReviewError {
    /// A reviewed source could not be read.
    #[error("failed to read reviewed source '{}': {source}", path.display())]
    Read {
        /// Source path that could not be read.
        path: PathBuf,
        /// Underlying filesystem failure.
        source: std::io::Error,
    },
    /// A configured background could not be resolved inside the data bundle.
    #[error("failed to resolve reviewed background '{}': {message}", path.display())]
    Background {
        /// Configured project-relative background path.
        path: PathBuf,
        /// Background resolution failure.
        message: String,
    },
    /// A scripture plan named an unsupported Bible version.
    #[error("reviewed scripture source uses unsupported Bible version '{0}'")]
    UnsupportedBibleVersion(String),
    /// Source bytes no longer match the reviewed preview.
    #[error(
        "reviewed source '{}' changed after preview (expected SHA-256 {expected}, found {actual})",
        path.display()
    )]
    Changed {
        /// Source path whose bytes changed.
        path: PathBuf,
        /// SHA-256 captured with the preview.
        expected: String,
        /// SHA-256 observed immediately before execution.
        actual: String,
    },
}

/// Failure to capture or verify one reviewed output target.
#[derive(Debug, thiserror::Error)]
pub enum OutputReviewError {
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
}

/// Exact present/absent state of every path one reviewed build may overwrite.
#[derive(Debug)]
pub(super) struct OutputManifest {
    outputs: Vec<ReviewedOutput>,
}

impl OutputManifest {
    pub(super) fn capture(
        paths: impl IntoIterator<Item = PathBuf>,
    ) -> Result<Self, OutputReviewError> {
        let paths = paths.into_iter().collect::<BTreeSet<_>>();
        let mut outputs = Vec::with_capacity(paths.len());
        for path in paths {
            outputs.push(ReviewedOutput::capture(path)?);
        }
        Ok(Self { outputs })
    }

    pub(super) fn verify(&self) -> Result<(), OutputReviewError> {
        for output in &self.outputs {
            output.verify()?;
        }
        Ok(())
    }

    pub(super) fn verify_target(&self, path: &Path) -> Result<(), OutputReviewError> {
        let output = self
            .outputs
            .iter()
            .find(|output| output.path == path)
            .ok_or_else(|| OutputReviewError::UnreviewedTarget {
                path: path.to_path_buf(),
            })?;
        output.verify()
    }
}

#[derive(Debug)]
struct ReviewedOutput {
    path: PathBuf,
    state: ReviewedOutputState,
}

impl ReviewedOutput {
    fn capture(path: PathBuf) -> Result<Self, OutputReviewError> {
        let state = ReviewedOutputState::read(&path)?;
        Ok(Self { path, state })
    }

    fn verify(&self) -> Result<(), OutputReviewError> {
        let actual = ReviewedOutputState::read(&self.path)?;
        match (&self.state, actual) {
            (ReviewedOutputState::Absent, ReviewedOutputState::Absent) => Ok(()),
            (ReviewedOutputState::Absent, ReviewedOutputState::Present { sha256, .. }) => {
                Err(OutputReviewError::Appeared {
                    path: self.path.clone(),
                    actual: digest_hex(&sha256),
                })
            }
            (ReviewedOutputState::Present { sha256, .. }, ReviewedOutputState::Absent) => {
                Err(OutputReviewError::Disappeared {
                    path: self.path.clone(),
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
                path: self.path.clone(),
                expected: digest_hex(expected),
                actual: digest_hex(&actual),
            }),
        }
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

#[derive(Debug)]
pub(super) struct SourceManifest {
    sources: Vec<SourceFingerprint>,
}

impl SourceManifest {
    pub(super) fn capture(
        paths: impl IntoIterator<Item = PathBuf>,
    ) -> Result<Self, SourceReviewError> {
        let paths = paths.into_iter().collect::<BTreeSet<_>>();
        let mut sources = Vec::with_capacity(paths.len());
        for path in paths {
            sources.push(SourceFingerprint::capture(path)?);
        }
        Ok(Self { sources })
    }

    pub(super) fn verify(&self) -> Result<(), SourceReviewError> {
        for source in &self.sources {
            source.verify()?;
        }
        Ok(())
    }

    pub(super) fn bytes(&self, path: &Path) -> Option<&[u8]> {
        self.sources
            .iter()
            .find(|source| source.path == path)
            .map(|source| source.bytes.as_slice())
    }

    pub(super) fn extend_capture(
        &mut self,
        paths: impl IntoIterator<Item = PathBuf>,
    ) -> Result<(), SourceReviewError> {
        let existing = self
            .sources
            .iter()
            .map(|source| source.path.clone())
            .collect::<BTreeSet<_>>();
        for path in paths
            .into_iter()
            .collect::<BTreeSet<_>>()
            .difference(&existing)
        {
            self.sources.push(SourceFingerprint::capture(path.clone())?);
        }
        self.sources
            .sort_by(|left, right| left.path.cmp(&right.path));
        Ok(())
    }
}

#[derive(Debug)]
struct SourceFingerprint {
    path: PathBuf,
    sha256: [u8; 32],
    bytes: Vec<u8>,
}

impl SourceFingerprint {
    fn capture(path: PathBuf) -> Result<Self, SourceReviewError> {
        let bytes = read_file(&path)?;
        let sha256 = hash_bytes(&bytes);
        Ok(Self {
            path,
            sha256,
            bytes,
        })
    }

    fn verify(&self) -> Result<(), SourceReviewError> {
        let actual = hash_file(&self.path)?;
        if actual == self.sha256 {
            return Ok(());
        }
        Err(SourceReviewError::Changed {
            path: self.path.clone(),
            expected: digest_hex(&self.sha256),
            actual: digest_hex(&actual),
        })
    }
}

pub(super) fn plan_source_paths(
    plans: &[ResolvedItemPlan],
    project_data_root: &Path,
) -> Result<Vec<PathBuf>, SourceReviewError> {
    let mut paths = BTreeSet::new();
    for plan in plans {
        if let Some(path) = plan.file_path.as_deref().filter(|path| !path.is_empty()) {
            paths.insert(PathBuf::from(path));
        }
        if let Some(background) = &plan.style.background {
            let configured = background.file().as_path();
            let resolved = crate::propresenter::background::resolve_background_image(
                project_data_root,
                configured,
            )
            .map_err(|error| SourceReviewError::Background {
                path: configured.to_path_buf(),
                message: error.to_string(),
            })?;
            paths.insert(resolved);
        }
        add_bible_source_paths(plan, project_data_root, &mut paths)?;
    }
    Ok(paths.into_iter().collect())
}

fn add_bible_source_paths(
    plan: &ResolvedItemPlan,
    project_data_root: &Path,
    paths: &mut BTreeSet<PathBuf>,
) -> Result<(), SourceReviewError> {
    let ContentSource::Scripture { scripture } = &plan.content_source else {
        return Ok(());
    };

    match scripture.request() {
        ScriptureRequest::Single { bible_version, .. } => {
            let version = parse_bible_version(bible_version)?;
            paths.insert(project_data_root.join("bibles").join(version.file_name()));
        }
        ScriptureRequest::Combined(references) => {
            for reference in references {
                let version = parse_bible_version(&reference.version)?;
                paths.insert(project_data_root.join("bibles").join(version.file_name()));
            }
        }
    }
    Ok(())
}

fn parse_bible_version(name: &str) -> Result<BibleVersion, SourceReviewError> {
    BibleVersion::from_name(name)
        .ok_or_else(|| SourceReviewError::UnsupportedBibleVersion(name.to_string()))
}

fn hash_file(path: &Path) -> Result<[u8; 32], SourceReviewError> {
    read_file(path).map(|bytes| hash_bytes(&bytes))
}

fn read_file(path: &Path) -> Result<Vec<u8>, SourceReviewError> {
    std::fs::read(path).map_err(|source| SourceReviewError::Read {
        path: path.to_path_buf(),
        source,
    })
}

fn hash_bytes(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
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

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::workflow::plan::{PlanAction, ResolvedItemPlan};

    #[test]
    fn reviewed_plan_rejects_changed_source_bytes() {
        let root = tempfile::tempdir().expect("temporary root");
        let source = root.path().join("source.pro");
        std::fs::write(&source, b"reviewed bytes").expect("write reviewed source");
        let reviewed = ReviewedServicePlan::capture_with_additional_sources(
            vec![ResolvedItemPlan {
                output_key: "pco:item-1:main".to_string(),
                file_path: Some(source.display().to_string()),
                action: PlanAction::UseExisting,
                ..ResolvedItemPlan::default()
            }],
            root.path(),
            std::iter::empty(),
        )
        .expect("capture source bytes");

        std::fs::write(&source, b"changed bytes").expect("change reviewed source");
        let error = reviewed
            .into_verified_parts()
            .expect_err("changed source must invalidate approval");

        assert!(matches!(error, SourceReviewError::Changed { path, .. } if path == source));
    }

    #[test]
    fn reviewed_plan_deduplicates_shared_sources() {
        let root = tempfile::tempdir().expect("temporary root");
        let source = root.path().join("source.pro");
        std::fs::write(&source, b"same bytes").expect("write source");
        let plans = ["one", "two"]
            .into_iter()
            .map(|key| ResolvedItemPlan {
                output_key: key.to_string(),
                file_path: Some(source.display().to_string()),
                action: PlanAction::UseExisting,
                ..ResolvedItemPlan::default()
            })
            .collect();

        let reviewed = ReviewedServicePlan::capture_with_additional_sources(
            plans,
            root.path(),
            std::iter::empty(),
        )
        .expect("capture shared source");
        assert_eq!(reviewed.sources.sources.len(), 1);
    }

    #[test]
    fn manifest_retains_reviewed_bytes_instead_of_rereading_the_path() {
        let root = tempfile::tempdir().expect("temporary root");
        let source = root.path().join("source.pro");
        std::fs::write(&source, b"reviewed").expect("write reviewed source");
        let manifest = SourceManifest::capture([source.clone()]).expect("capture source");

        std::fs::write(&source, b"changed").expect("change source path");

        assert_eq!(manifest.bytes(&source), Some(b"reviewed".as_slice()));
        assert!(matches!(
            manifest.verify(),
            Err(SourceReviewError::Changed { path, .. }) if path == source
        ));
    }
}
