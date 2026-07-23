//! One checked owner for playlist package membership and read-back evidence.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use prost::Message;
use serde::Serialize;

use super::domain::{
    PackageReadbackField, PlaylistEntry, PlaylistError, PlaylistExportIntent, PlaylistMediaAsset,
    ReviewedPlaylistMediaAsset,
};
use super::naming::embedded_filenames;
use super::package_validation::{
    reject_presentation_media_path, reserve_archive_path, validate_archive_path,
    validate_embedded_source_consistency, validate_playlist_matches_entries,
};
use crate::propresenter::generated::rv_data;
use crate::propresenter::media::{
    presentation_media_dependencies_from_bytes, MediaDependencyResolution,
};
use crate::propresenter::native_zip::{self, Entry as NativeZipEntry};
use crate::propresenter::package::{presentation_items, read_playlist_package, PlaylistPackage};
use crate::propresenter::serialize::write_file_atomically;

/// Deterministic package-dependency decisions sealed into the build receipt.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct PlaylistExportEvidence {
    warnings: Vec<String>,
    media_manifest: PlaylistMediaManifest,
}

impl PlaylistExportEvidence {
    pub(crate) fn warnings(&self) -> &[String] {
        &self.warnings
    }

    pub(crate) const fn media_asset_count(&self) -> usize {
        self.media_manifest.members.len()
    }

    #[cfg(test)]
    pub(crate) fn diagnostic_missing(candidate_path: &str) -> Self {
        let unresolved = UnresolvedMediaReference {
            presentation: "Diagnostic".to_string(),
            native_locator: format!("file://{candidate_path}"),
            reason: UnresolvedMediaReason::MissingLocalFile {
                candidate_path: candidate_path.to_string(),
            },
        };
        Self {
            warnings: vec![unresolved.warning()],
            media_manifest: PlaylistMediaManifest {
                references: Vec::new(),
                members: Vec::new(),
                unresolved: vec![unresolved],
            },
        }
    }

    #[cfg(test)]
    pub(crate) fn diagnostic_member(source_path: &str) -> Self {
        Self {
            warnings: Vec::new(),
            media_manifest: PlaylistMediaManifest {
                references: Vec::new(),
                members: vec![EmbeddedMediaMember {
                    source_path: source_path.to_string(),
                    archive_member: source_path.to_string(),
                    origin: MediaMemberOrigin::PresentationReference,
                }],
                unresolved: Vec::new(),
            },
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
struct PlaylistMediaManifest {
    references: Vec<EmbeddedMediaReference>,
    members: Vec<EmbeddedMediaMember>,
    unresolved: Vec<UnresolvedMediaReference>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct EmbeddedMediaReference {
    presentation: String,
    native_locator: String,
    source_path: String,
    archive_member: String,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct EmbeddedMediaMember {
    source_path: String,
    archive_member: String,
    origin: MediaMemberOrigin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum MediaMemberOrigin {
    PresentationReference,
    AdditionalRequest,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct UnresolvedMediaReference {
    presentation: String,
    native_locator: String,
    #[serde(flatten)]
    reason: UnresolvedMediaReason,
}

impl UnresolvedMediaReference {
    fn warning(&self) -> String {
        match &self.reason {
            UnresolvedMediaReason::MissingLocalFile { candidate_path } => format!(
                "Media was not embedded and retains its original external reference: {candidate_path}"
            ),
            UnresolvedMediaReason::NonLocalLocator => format!(
                "Media was not embedded because its native locator is not a local file: {}",
                self.native_locator
            ),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
enum UnresolvedMediaReason {
    MissingLocalFile { candidate_path: String },
    NonLocalLocator,
}

/// Exact media membership and receipt evidence for one reviewed export.
pub enum ResolvedPlaylistExport {
    LibraryLinks,
    PortableImport {
        media_assets: Vec<PlaylistMediaAsset>,
        evidence: PlaylistExportEvidence,
    },
}

impl ResolvedPlaylistExport {
    pub(crate) fn into_evidence(self) -> PlaylistExportEvidence {
        match self {
            Self::LibraryLinks => PlaylistExportEvidence::default(),
            Self::PortableImport { evidence, .. } => evidence,
        }
    }
}

struct DiscoveredMedia {
    paths: Vec<PathBuf>,
    references: Vec<AvailableMediaReference>,
    unresolved: Vec<UnresolvedMediaReference>,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct AvailableMediaReference {
    presentation: String,
    native_locator: String,
    source_path: PathBuf,
}

/// Resolve portable dependencies once from final presentation bytes.
pub fn resolve_playlist_export(
    intent: &PlaylistExportIntent,
    entries: &[PlaylistEntry],
    propresenter_root: &Path,
) -> Result<ResolvedPlaylistExport, PlaylistError> {
    let PlaylistExportIntent::PortableImport {
        additional_media_assets,
    } = intent
    else {
        return Ok(ResolvedPlaylistExport::LibraryLinks);
    };

    let DiscoveredMedia {
        paths,
        references,
        unresolved,
    } = discover_media(entries, propresenter_root)?;
    let referenced = paths.iter().cloned().collect::<BTreeSet<_>>();
    let mut media_assets = paths
        .into_iter()
        .map(PlaylistMediaAsset::new)
        .collect::<Vec<_>>();
    for asset in additional_media_assets {
        if referenced.contains(&asset.source_path) {
            if let Some(archive_path) = asset
                .archive_path
                .as_deref()
                .filter(|path| !path.trim().is_empty())
            {
                return Err(PlaylistError::MediaDependencyArchiveOverride {
                    name: "portable export".to_string(),
                    path: asset.source_path.clone(),
                    archive_path: archive_path.to_string(),
                });
            }
            continue;
        }
        media_assets.push(asset.clone());
    }

    let evidence = export_evidence(&media_assets, &referenced, references, unresolved)?;
    Ok(ResolvedPlaylistExport::PortableImport {
        media_assets,
        evidence,
    })
}

fn discover_media(
    entries: &[PlaylistEntry],
    propresenter_root: &Path,
) -> Result<DiscoveredMedia, PlaylistError> {
    let mut paths = BTreeSet::new();
    let mut references = BTreeSet::new();
    let mut unresolved = BTreeSet::new();
    for (index, entry) in entries.iter().enumerate() {
        let Some(data) = entry.embedded_data() else {
            continue;
        };
        let dependencies = presentation_media_dependencies_from_bytes(data).map_err(|reason| {
            PlaylistError::InvalidEmbeddedPresentation {
                index,
                name: entry.name().to_string(),
                reason,
            }
        })?;
        for dependency in dependencies {
            match dependency.resolve(Some(propresenter_root)) {
                MediaDependencyResolution::Available(path) => {
                    let source_path = canonical_media_file(&path)?;
                    paths.insert(source_path.clone());
                    references.insert(AvailableMediaReference {
                        presentation: entry.name().to_string(),
                        native_locator: dependency.source().to_string(),
                        source_path,
                    });
                }
                MediaDependencyResolution::Missing(path) => {
                    unresolved.insert(UnresolvedMediaReference {
                        presentation: entry.name().to_string(),
                        native_locator: dependency.source().to_string(),
                        reason: UnresolvedMediaReason::MissingLocalFile {
                            candidate_path: exact_path(&path)?,
                        },
                    });
                }
                MediaDependencyResolution::Unresolved => {
                    unresolved.insert(UnresolvedMediaReference {
                        presentation: entry.name().to_string(),
                        native_locator: dependency.source().to_string(),
                        reason: UnresolvedMediaReason::NonLocalLocator,
                    });
                }
            }
        }
    }
    Ok(DiscoveredMedia {
        paths: paths.into_iter().collect(),
        references: references.into_iter().collect(),
        unresolved: unresolved.into_iter().collect(),
    })
}

fn export_evidence(
    media_assets: &[PlaylistMediaAsset],
    referenced_paths: &BTreeSet<PathBuf>,
    references: Vec<AvailableMediaReference>,
    unresolved: Vec<UnresolvedMediaReference>,
) -> Result<PlaylistExportEvidence, PlaylistError> {
    let mut reference_evidence = references
        .into_iter()
        .map(|reference| {
            Ok(EmbeddedMediaReference {
                presentation: reference.presentation,
                native_locator: reference.native_locator,
                source_path: exact_path(&reference.source_path)?,
                archive_member: PlaylistMediaAsset::new(&reference.source_path)
                    .resolved_archive_path()?,
            })
        })
        .collect::<Result<Vec<_>, PlaylistError>>()?;
    reference_evidence.sort();
    let mut members = media_assets
        .iter()
        .map(|asset| {
            Ok(EmbeddedMediaMember {
                source_path: exact_path(&asset.source_path)?,
                archive_member: asset.resolved_archive_path()?,
                origin: if referenced_paths.contains(&asset.source_path) {
                    MediaMemberOrigin::PresentationReference
                } else {
                    MediaMemberOrigin::AdditionalRequest
                },
            })
        })
        .collect::<Result<Vec<_>, PlaylistError>>()?;
    members.sort();
    let mut unresolved = unresolved;
    unresolved.sort();
    let warnings = unresolved
        .iter()
        .map(UnresolvedMediaReference::warning)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    Ok(PlaylistExportEvidence {
        warnings,
        media_manifest: PlaylistMediaManifest {
            references: reference_evidence,
            members,
            unresolved,
        },
    })
}

fn canonical_media_file(path: &Path) -> Result<PathBuf, PlaylistError> {
    let canonical = path.canonicalize()?;
    if canonical.is_file() {
        Ok(canonical)
    } else {
        Err(PlaylistError::InvalidMediaAsset(canonical))
    }
}

fn exact_path(path: &Path) -> Result<String, PlaylistError> {
    path.to_str()
        .map(str::to_string)
        .ok_or_else(|| PlaylistError::InvalidMediaAsset(path.to_path_buf()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PackageMemberKind {
    Presentation,
    Media,
}

struct PackageMember<'a> {
    path: String,
    kind: PackageMemberKind,
    data: &'a [u8],
}

/// Fully checked immutable plan for one native playlist archive.
///
/// Construction is the only place that assigns archive identities. Writing is
/// therefore a mechanical serialization followed by an independent decode and
/// comparison against this same reviewed plan.
pub(super) struct PlaylistPackagePlan<'a> {
    document: &'a rv_data::PlaylistDocument,
    document_bytes: Vec<u8>,
    members: Vec<PackageMember<'a>>,
}

impl<'a> PlaylistPackagePlan<'a> {
    pub(super) fn new(
        document: &'a rv_data::PlaylistDocument,
        entries: &'a [PlaylistEntry],
        embed_presentations: bool,
        media_assets: &'a [ReviewedPlaylistMediaAsset<'a>],
    ) -> Result<Self, PlaylistError> {
        let mut reserved_paths = HashSet::from(["data".to_string()]);
        let embedded_filenames = if embed_presentations {
            embedded_filenames(entries)?
        } else {
            vec![None; entries.len()]
        }
        .into_iter()
        .map(|filename| {
            filename
                .map(|filename| {
                    let filename = validate_archive_path(&filename, false)?;
                    reserve_archive_path(&mut reserved_paths, &filename)?;
                    Ok(filename)
                })
                .transpose()
        })
        .collect::<Result<Vec<_>, PlaylistError>>()?;

        validate_playlist_matches_entries(document, entries, &embedded_filenames)?;
        if embed_presentations {
            validate_embedded_source_consistency(entries)?;
        }

        let mut members = entries
            .iter()
            .zip(&embedded_filenames)
            .filter_map(|(entry, filename)| {
                entry
                    .embedded_data()
                    .zip(filename.as_ref())
                    .map(|(data, path)| PackageMember {
                        path: path.clone(),
                        kind: PackageMemberKind::Presentation,
                        data,
                    })
            })
            .collect::<Vec<_>>();

        for asset in media_assets {
            reject_presentation_media_path(&asset.archive_path)?;
            reserve_archive_path(&mut reserved_paths, &asset.archive_path)?;
            members.push(PackageMember {
                path: asset.archive_path.clone(),
                kind: PackageMemberKind::Media,
                data: asset.data.as_ref(),
            });
        }

        let document_bytes = document.encode_to_vec();
        Ok(Self {
            document,
            document_bytes,
            members,
        })
    }

    pub(super) fn write_and_verify(self, path: &Path) -> Result<(), PlaylistError> {
        let mut archive_members = self
            .members
            .iter()
            .map(|member| NativeZipEntry::borrowed(member.path.clone(), member.data))
            .collect::<Vec<_>>();
        archive_members.push(NativeZipEntry::borrowed(
            "data".to_string(),
            &self.document_bytes,
        ));
        write_file_atomically::<PlaylistError, _>(path, |file| {
            Ok(native_zip::write(file, archive_members)?)
        })?;

        let package = read_playlist_package(path)?;
        self.verify_readback(&package)
    }

    fn verify_readback(&self, package: &PlaylistPackage) -> Result<(), PlaylistError> {
        if package.document_data() != self.document_bytes {
            return Err(mismatch(
                PackageReadbackField::Document,
                "the data member changed during archive serialization",
            ));
        }
        if presentation_items(package.document()) != presentation_items(self.document) {
            return Err(mismatch(
                PackageReadbackField::Items,
                "presentation item identity, URL, arrangement, or key changed",
            ));
        }

        let mut expected_order = self
            .members
            .iter()
            .map(|member| member.path.clone())
            .chain(std::iter::once("data".to_string()))
            .collect::<Vec<_>>();
        expected_order.sort();
        let actual_order = package
            .archive_entries()
            .iter()
            .map(|member| member.name.clone())
            .collect::<Vec<_>>();
        if actual_order != expected_order {
            return Err(mismatch(
                PackageReadbackField::ArchiveOrder,
                format!("expected {expected_order:?}, found {actual_order:?}"),
            ));
        }

        for member in &self.members {
            if package.embedded_file(&member.path) != Some(member.data) {
                let kind = match member.kind {
                    PackageMemberKind::Presentation => "presentation",
                    PackageMemberKind::Media => "media",
                };
                return Err(mismatch(
                    PackageReadbackField::MemberBytes,
                    format!("{kind} member {:?} changed or is missing", member.path),
                ));
            }
        }
        if !package.archive_comment().is_empty() {
            return Err(mismatch(
                PackageReadbackField::ArchiveComment,
                "native packages must not carry an archive comment",
            ));
        }
        Ok(())
    }
}

fn mismatch(field: PackageReadbackField, details: impl Into<String>) -> PlaylistError {
    PlaylistError::PackageReadbackMismatch {
        field,
        details: details.into(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use uuid::Uuid;

    use super::*;
    use crate::propresenter::playlist::{build_playlist, PlaylistMetadata};

    fn presentation_bytes(name: &str) -> Vec<u8> {
        rv_data::Presentation {
            name: name.to_string(),
            uuid: Some(rv_data::Uuid {
                string: Uuid::new_v4().to_string(),
            }),
            ..rv_data::Presentation::default()
        }
        .encode_to_vec()
    }

    #[test]
    fn independent_readback_rejects_a_different_reviewed_document() {
        let entries = vec![PlaylistEntry::embedded(
            "Song",
            "/Libraries/Default/Song.pro",
            presentation_bytes("Song"),
        )
        .expect("valid entry")];
        let metadata = PlaylistMetadata::offline_test();
        let expected_document = build_playlist("Expected", &entries, &metadata);
        let actual_document = build_playlist("Actual", &entries, &metadata);
        let directory = tempfile::tempdir().expect("temporary directory");
        let output = directory.path().join("actual.proplaylist");
        PlaylistPackagePlan::new(&actual_document, &entries, true, &[])
            .expect("actual plan")
            .write_and_verify(&output)
            .expect("write actual package");
        let actual_package = read_playlist_package(output).expect("read actual package");

        let error = PlaylistPackagePlan::new(&expected_document, &entries, true, &[])
            .expect("expected plan")
            .verify_readback(&actual_package)
            .expect_err("different document must fail");
        assert!(matches!(
            error,
            PlaylistError::PackageReadbackMismatch {
                field: PackageReadbackField::Document,
                ..
            }
        ));
    }

    #[test]
    fn independent_readback_rejects_changed_presentation_bytes() {
        let expected_entries = vec![PlaylistEntry::embedded(
            "Song",
            "/Libraries/Default/Song.pro",
            presentation_bytes("Expected Song"),
        )
        .expect("valid expected entry")];
        let actual_entries = vec![PlaylistEntry::embedded(
            "Song",
            "/Libraries/Default/Song.pro",
            presentation_bytes("Changed Song"),
        )
        .expect("valid actual entry")];
        let metadata = PlaylistMetadata::offline_test();
        let expected_document = build_playlist("Service", &expected_entries, &metadata);
        let actual_document = expected_document.clone();
        let directory = tempfile::tempdir().expect("temporary directory");
        let output = directory.path().join("actual.proplaylist");
        PlaylistPackagePlan::new(&actual_document, &actual_entries, true, &[])
            .expect("actual plan")
            .write_and_verify(&output)
            .expect("write actual package");
        let actual_package = read_playlist_package(output).expect("read actual package");

        let error = PlaylistPackagePlan::new(&expected_document, &expected_entries, true, &[])
            .expect("expected plan")
            .verify_readback(&actual_package)
            .expect_err("changed presentation must fail");
        assert!(matches!(
            error,
            PlaylistError::PackageReadbackMismatch {
                field: PackageReadbackField::MemberBytes,
                ..
            }
        ));
    }
}
