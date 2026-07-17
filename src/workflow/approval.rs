//! Reviewed workflow inputs plus source and output integrity checks.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::plan::{ResolvedItemPlan, ScriptureRequest};
use crate::bible::BibleVersion;

mod output;

pub use output::OutputReviewError;
pub(super) use output::{OutputManifest, PhysicalPath, ReviewedOutput};

/// A resolved plan whose file-backed inputs have been captured for later
/// verification.
///
/// The fields are private so callers cannot manufacture a reviewed plan by
/// attaching paths without also hashing their bytes.
#[derive(Debug)]
pub(super) struct ReviewedServicePlan {
    plans: Vec<ResolvedItemPlan>,
    sources: CapturedSources,
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
            sources: CapturedSources::capture(paths)?,
        })
    }

    /// Return the immutable decisions used to render the operator preview.
    pub(super) fn plans(&self) -> &[ResolvedItemPlan] {
        &self.plans
    }

    pub(super) fn source_bytes(&self, path: &Path) -> Option<&[u8]> {
        self.sources.bytes(path)
    }

    pub(super) const fn sources(&self) -> &CapturedSources {
        &self.sources
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
        Ok((self.plans, self.sources.into_verified_manifest()?))
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

/// Reviewed source payloads available only while native artifacts are prepared.
#[derive(Debug)]
pub(super) struct CapturedSources {
    sources: Vec<CapturedSource>,
}

impl CapturedSources {
    fn capture(paths: impl IntoIterator<Item = PathBuf>) -> Result<Self, SourceReviewError> {
        let paths = paths.into_iter().collect::<BTreeSet<_>>();
        let mut sources = Vec::with_capacity(paths.len());
        for path in paths {
            sources.push(CapturedSource::capture(path)?);
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
            .find(|source| source.fingerprint.path == path)
            .map(|source| source.bytes.as_slice())
    }

    pub(super) fn extend_capture(
        &mut self,
        paths: impl IntoIterator<Item = PathBuf>,
    ) -> Result<(), SourceReviewError> {
        let existing = self
            .sources
            .iter()
            .map(|source| source.fingerprint.path.clone())
            .collect::<BTreeSet<_>>();
        for path in paths
            .into_iter()
            .collect::<BTreeSet<_>>()
            .difference(&existing)
        {
            self.sources.push(CapturedSource::capture(path.clone())?);
        }
        self.sources
            .sort_by(|left, right| left.fingerprint.path.cmp(&right.fingerprint.path));
        Ok(())
    }

    fn into_verified_manifest(self) -> Result<SourceManifest, SourceReviewError> {
        self.verify()?;
        Ok(SourceManifest {
            sources: self
                .sources
                .into_iter()
                .map(|source| source.fingerprint)
                .collect(),
        })
    }
}

#[derive(Debug)]
struct CapturedSource {
    fingerprint: SourceFingerprint,
    bytes: Vec<u8>,
}

impl CapturedSource {
    fn capture(path: PathBuf) -> Result<Self, SourceReviewError> {
        let bytes = read_file(&path)?;
        Ok(Self {
            fingerprint: SourceFingerprint {
                path,
                sha256: hash_bytes(&bytes),
            },
            bytes,
        })
    }

    fn verify(&self) -> Result<(), SourceReviewError> {
        self.fingerprint.verify()
    }
}

/// Paths and hashes retained after reviewed source payloads have been consumed.
///
/// This is the only source state allowed to cross the prepared-build boundary.
#[derive(Debug)]
pub(super) struct SourceManifest {
    sources: Vec<SourceFingerprint>,
}

impl SourceManifest {
    pub(super) fn verify(&self) -> Result<(), SourceReviewError> {
        for source in &self.sources {
            source.verify()?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn contains(&self, path: &Path) -> bool {
        self.sources.iter().any(|source| source.path == path)
    }
}

#[derive(Debug)]
struct SourceFingerprint {
    path: PathBuf,
    sha256: [u8; 32],
}

impl SourceFingerprint {
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
        if let Some(path) = plan.file_path() {
            paths.insert(path.to_path_buf());
        }
        if let Some(background) = plan.background() {
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
    let Some(scripture) = plan.scripture_content() else {
        return Ok(());
    };

    match scripture.request() {
        ScriptureRequest::Single { bible_version, .. }
        | ScriptureRequest::PrefixExcerpt { bible_version, .. } => {
            let version = parse_bible_version(bible_version)?;
            paths.insert(project_data_root.join("bibles").join(version.file_name()));
        }
        ScriptureRequest::Combined(references) => {
            for reference in references {
                let version = parse_bible_version(reference.version())?;
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
    use crate::workflow::plan::{
        ItemKind, OutputKey, PlanDisposition, ReadyAction, ResolvedItemPlan,
    };

    fn existing_plan(output_key: &str, source: &Path) -> ResolvedItemPlan {
        ResolvedItemPlan {
            output_key: OutputKey::new(output_key.to_string()).expect("valid test output key"),
            position: 0,
            pco_title: "Existing presentation".to_string(),
            playlist_name: "Existing presentation".to_string(),
            reason: "Test fixture".to_string(),
            item_kind: ItemKind::Other,
            item_type: None,
            disposition: PlanDisposition::Ready(ReadyAction::UseExisting {
                file_path: source.to_path_buf(),
                arrangement: None,
            }),
        }
    }

    #[test]
    fn reviewed_plan_rejects_changed_source_bytes() {
        let root = tempfile::tempdir().expect("temporary root");
        let source = root.path().join("source.pro");
        std::fs::write(&source, b"reviewed bytes").expect("write reviewed source");
        let reviewed = ReviewedServicePlan::capture_with_additional_sources(
            vec![existing_plan("pco:item-1:main", &source)],
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
            .map(|key| existing_plan(key, &source))
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
    fn capture_owns_bytes_only_until_the_verified_manifest_transition() {
        let root = tempfile::tempdir().expect("temporary root");
        let source = root.path().join("source.pro");
        std::fs::write(&source, b"reviewed").expect("write reviewed source");
        let captured = CapturedSources::capture([source.clone()]).expect("capture source");

        assert_eq!(captured.bytes(&source), Some(b"reviewed".as_slice()));
        let manifest = captured
            .into_verified_manifest()
            .expect("seal source fingerprints");
        assert!(manifest.contains(&source));

        std::fs::write(&source, b"changed").expect("change source path");

        assert!(matches!(
            manifest.verify(),
            Err(SourceReviewError::Changed { path, .. }) if path == source
        ));
    }
}
