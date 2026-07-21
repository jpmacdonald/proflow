//! Reviewed playlist packaging and media export.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::propresenter::media::{
    presentation_media_dependencies_from_bytes, MediaDependencyResolution,
};
use crate::propresenter::playlist::{
    playlist_output_path, write_playlist_set_file_with_reviewed_media, PlaylistEntry,
    PlaylistError, PlaylistExportIntent, PlaylistMediaAsset, PlaylistMetadata, PlaylistSet,
    ReviewedPlaylistExportIntent,
};
use crate::workflow::approval::CapturedSources;
use crate::workflow::transaction::BuildFileTransaction;

use super::request::{canonical_media_source, BoundBuildRequest};
use super::{captured_source_bytes, BuildServiceError, ServiceBuildExecutor};

pub(super) struct PlaylistExport {
    pub(super) path: PathBuf,
    pub(super) evidence: PlaylistExportEvidence,
}

/// Deterministic package-dependency decisions sealed into the build receipt.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub(super) struct PlaylistExportEvidence {
    warnings: Vec<String>,
    media_manifest: PlaylistMediaManifest,
}

impl PlaylistExportEvidence {
    pub(super) fn warnings(&self) -> &[String] {
        &self.warnings
    }

    pub(super) const fn media_asset_count(&self) -> usize {
        self.media_manifest.members.len()
    }

    #[cfg(test)]
    pub(super) fn diagnostic_missing(candidate_path: &str) -> Self {
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
    pub(super) fn diagnostic_member(source_path: &str) -> Self {
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

/// Every native reference, embedded member, and unresolved dependency in one
/// portable export. Vectors are sorted so receipt identity does not depend on
/// hash iteration or additional-media request order.
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

/// Exact package membership resolved from reviewed intent and rendered output.
///
/// This is deliberately distinct from `PlaylistExportIntent`: the request
/// retains only what the operator supplied, while this checked phase value owns
/// the final inferred dependency set written to the package.
enum ResolvedPlaylistExport {
    LibraryLinks,
    PortableImport {
        media_assets: Vec<PlaylistMediaAsset>,
        evidence: PlaylistExportEvidence,
    },
}

impl ResolvedPlaylistExport {
    fn into_evidence(self) -> PlaylistExportEvidence {
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

impl ServiceBuildExecutor<'_> {
    pub(super) fn stage_playlist(
        &self,
        request: &BoundBuildRequest,
        sources: &CapturedSources,
        entries: &[PlaylistEntry],
        transaction: &mut BuildFileTransaction,
    ) -> Result<PlaylistExport, BuildServiceError> {
        let propresenter_root = self.render_assets.locations().propresenter_root();
        let resolved_export = resolve_playlist_export(request, entries, propresenter_root)?;
        let playlist_set = PlaylistSet::single(request.playlist_name.clone(), entries.to_vec())
            .map_err(PlaylistError::from)?;
        let output_path = playlist_output_path(
            self.render_assets.locations().playlist_output(),
            &request.playlist_name,
        );
        let staged_path = transaction.stage_reviewed(&output_path)?;
        write_reviewed_playlist(
            &playlist_set,
            self.playlist_metadata,
            &resolved_export,
            &staged_path,
            sources,
        )?;
        let evidence = resolved_export.into_evidence();
        Ok(PlaylistExport {
            path: output_path,
            evidence,
        })
    }
}

/// Resolve the package media set from the final presentation bytes.
///
/// Review captures every source that rendering might retain. Packaging is
/// narrower: native exports contain only media used by the presentations they
/// embed. In particular, a restyle must not carry the background it replaced.
fn resolve_playlist_export(
    request: &BoundBuildRequest,
    entries: &[PlaylistEntry],
    propresenter_root: &Path,
) -> Result<ResolvedPlaylistExport, BuildServiceError> {
    let PlaylistExportIntent::PortableImport {
        additional_media_assets,
    } = &request.playlist_export
    else {
        return Ok(ResolvedPlaylistExport::LibraryLinks);
    };

    let DiscoveredMedia {
        paths,
        references,
        unresolved,
    } = discovered_media_paths(entries, propresenter_root)?;
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
                }
                .into());
            }
            continue;
        }
        media_assets.push(asset.clone());
    }

    let evidence = playlist_export_evidence(&media_assets, &referenced, references, unresolved)?;

    Ok(ResolvedPlaylistExport::PortableImport {
        media_assets,
        evidence,
    })
}

fn playlist_export_evidence(
    media_assets: &[PlaylistMediaAsset],
    referenced_paths: &BTreeSet<PathBuf>,
    references: Vec<AvailableMediaReference>,
    unresolved: Vec<UnresolvedMediaReference>,
) -> Result<PlaylistExportEvidence, BuildServiceError> {
    let mut reference_evidence = references
        .into_iter()
        .map(|reference| {
            let source_path = exact_media_path(&reference.source_path)?;
            let archive_member =
                PlaylistMediaAsset::new(&reference.source_path).resolved_archive_path()?;
            Ok(EmbeddedMediaReference {
                presentation: reference.presentation,
                native_locator: reference.native_locator,
                source_path,
                archive_member,
            })
        })
        .collect::<Result<Vec<_>, BuildServiceError>>()?;
    reference_evidence.sort();

    let mut members = media_assets
        .iter()
        .map(|asset| {
            Ok(EmbeddedMediaMember {
                source_path: exact_media_path(&asset.source_path)?,
                archive_member: asset.resolved_archive_path()?,
                origin: if referenced_paths.contains(&asset.source_path) {
                    MediaMemberOrigin::PresentationReference
                } else {
                    MediaMemberOrigin::AdditionalRequest
                },
            })
        })
        .collect::<Result<Vec<_>, BuildServiceError>>()?;
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

fn exact_media_path(path: &Path) -> Result<String, BuildServiceError> {
    path.to_str()
        .map(str::to_string)
        .ok_or_else(|| PlaylistError::InvalidMediaAsset(path.to_path_buf()).into())
}

fn write_reviewed_playlist(
    playlist_set: &PlaylistSet,
    metadata: &PlaylistMetadata,
    export: &ResolvedPlaylistExport,
    staged_path: &Path,
    sources: &CapturedSources,
) -> Result<(), BuildServiceError> {
    let reviewed_media;
    let export = match export {
        ResolvedPlaylistExport::LibraryLinks => ReviewedPlaylistExportIntent::LibraryLinks,
        ResolvedPlaylistExport::PortableImport { media_assets, .. } => {
            sources.verify()?;
            reviewed_media = media_assets
                .iter()
                .map(|asset| {
                    let bytes = captured_source_bytes(sources, &asset.source_path)?;
                    asset.bind_reviewed(bytes).map_err(BuildServiceError::from)
                })
                .collect::<Result<Vec<_>, BuildServiceError>>()?;
            ReviewedPlaylistExportIntent::PortableImport(&reviewed_media)
        }
    };
    write_playlist_set_file_with_reviewed_media(playlist_set, metadata, staged_path, export)?;
    Ok(())
}

fn discovered_media_paths(
    entries: &[PlaylistEntry],
    propresenter_root: &Path,
) -> Result<DiscoveredMedia, BuildServiceError> {
    let mut paths = BTreeSet::new();
    let mut references = BTreeSet::new();
    let mut unresolved = BTreeSet::new();
    for entry in entries {
        let Some(data) = entry.embedded_data() else {
            continue;
        };
        let dependencies = presentation_media_dependencies_from_bytes(data).map_err(|error| {
            BuildServiceError::RenderedMediaInspection {
                entry: entry.name().to_string(),
                source: error,
            }
        })?;
        for dependency in dependencies {
            match dependency.resolve(Some(propresenter_root)) {
                MediaDependencyResolution::Available(path) => {
                    let path = canonical_media_source(&path)?;
                    paths.insert(path.clone());
                    references.insert(AvailableMediaReference {
                        presentation: entry.name().to_string(),
                        native_locator: dependency.source().to_string(),
                        source_path: path,
                    });
                }
                MediaDependencyResolution::Missing(path) => {
                    unresolved.insert(UnresolvedMediaReference {
                        presentation: entry.name().to_string(),
                        native_locator: dependency.source().to_string(),
                        reason: UnresolvedMediaReason::MissingLocalFile {
                            candidate_path: exact_media_path(&path)?,
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
