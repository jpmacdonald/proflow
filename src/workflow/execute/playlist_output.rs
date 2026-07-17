//! Reviewed playlist packaging and media export.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::propresenter::media::presentation_media_dependencies_from_bytes;
use crate::propresenter::package::PlaylistPackageMode;
use crate::propresenter::playlist::{
    playlist_output_path, write_playlist_set_file_with_reviewed_media, NamedPlaylist,
    PlaylistEntry, PlaylistError, PlaylistMetadata, PlaylistSet, ReviewedPlaylistExportIntent,
};
use crate::workflow::approval::CapturedSources;
use crate::workflow::transaction::BuildFileTransaction;

use super::request::{portable_media_source, BoundBuildRequest};
use super::{captured_source_bytes, BuildServiceError, ServiceBuildExecutor};

pub(super) struct PlaylistExport {
    pub(super) path: PathBuf,
    pub(super) media_asset_count: usize,
    pub(super) warnings: Vec<String>,
}

struct PortableMedia {
    assets: Vec<crate::propresenter::playlist::PlaylistMediaAsset>,
    warnings: Vec<String>,
}

struct DiscoveredMedia {
    paths: Vec<PathBuf>,
    warnings: Vec<String>,
}

impl ServiceBuildExecutor<'_> {
    pub(super) fn stage_playlist(
        &self,
        request: &mut BoundBuildRequest,
        sources: &CapturedSources,
        entries: &[PlaylistEntry],
        transaction: &mut BuildFileTransaction,
    ) -> Result<PlaylistExport, BuildServiceError> {
        let propresenter_root = self.render_assets.locations().propresenter_root();
        let portable_media = exact_portable_media_assets(request, entries, propresenter_root)?;
        request.media_assets = portable_media.assets;
        let named_playlist = NamedPlaylist::new(request.playlist_name.clone(), entries.to_vec())
            .map_err(PlaylistError::from)?;
        let playlist_set = PlaylistSet::new(vec![named_playlist]).map_err(PlaylistError::from)?;
        let output_path = playlist_output_path(
            self.render_assets.locations().playlist_output(),
            &request.playlist_name,
        );
        let staged_path = transaction.stage_reviewed(&output_path)?;
        write_reviewed_playlist(
            &playlist_set,
            self.playlist_metadata,
            request,
            &staged_path,
            sources,
        )?;
        Ok(PlaylistExport {
            path: output_path,
            media_asset_count: request.media_assets.len(),
            warnings: portable_media.warnings,
        })
    }
}

/// Resolve the package media set from the final presentation bytes.
///
/// Review captures every source that rendering might retain. Packaging is
/// narrower: native exports contain only media used by the presentations they
/// embed. In particular, a restyle must not carry the background it replaced.
fn exact_portable_media_assets(
    request: &BoundBuildRequest,
    entries: &[PlaylistEntry],
    propresenter_root: &Path,
) -> Result<PortableMedia, BuildServiceError> {
    if !matches!(
        request.playlist_package_mode,
        PlaylistPackageMode::ExportPortable
    ) {
        return Ok(PortableMedia {
            assets: Vec::new(),
            warnings: Vec::new(),
        });
    }

    let discovered = discovered_media_paths(entries, propresenter_root)?;
    let referenced = discovered.paths.iter().collect::<BTreeSet<_>>();
    for asset in &request.media_assets {
        if !referenced.contains(&asset.source_path) {
            return Err(
                crate::propresenter::playlist::PlaylistError::UnreferencedPortableMediaAsset {
                    path: asset.source_path.clone(),
                }
                .into(),
            );
        }
        if let Some(archive_path) = asset
            .archive_path
            .as_deref()
            .filter(|path| !path.trim().is_empty())
        {
            return Err(
                crate::propresenter::playlist::PlaylistError::MediaDependencyArchiveOverride {
                    name: "portable export".to_string(),
                    path: asset.source_path.clone(),
                    archive_path: archive_path.to_string(),
                }
                .into(),
            );
        }
    }

    Ok(PortableMedia {
        assets: discovered
            .paths
            .into_iter()
            .map(crate::propresenter::playlist::PlaylistMediaAsset::new)
            .collect(),
        warnings: discovered.warnings,
    })
}

fn write_reviewed_playlist(
    playlist_set: &PlaylistSet,
    metadata: &PlaylistMetadata,
    request: &BoundBuildRequest,
    staged_path: &Path,
    sources: &CapturedSources,
) -> Result<(), BuildServiceError> {
    let reviewed_media;
    let export = match request.playlist_package_mode {
        PlaylistPackageMode::LibraryLocal => ReviewedPlaylistExportIntent::LibraryLinks,
        PlaylistPackageMode::ExportPortable => {
            sources.verify()?;
            reviewed_media = request
                .media_assets
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
    let mut missing = BTreeSet::new();
    for entry in entries {
        let Some(data) = entry.embedded_data() else {
            continue;
        };
        let dependencies = presentation_media_dependencies_from_bytes(data).map_err(|error| {
            BuildServiceError::message(format!(
                "failed to inspect media dependencies for '{}': {error}",
                entry.name()
            ))
        })?;
        for dependency in dependencies {
            let path = dependency.path.ok_or_else(|| {
                BuildServiceError::message(format!(
                    "rendered media dependency is not an absolute local file: {}",
                    dependency.source
                ))
            })?;
            match portable_media_source(&path, propresenter_root) {
                Ok(path) => {
                    paths.insert(path);
                }
                Err(BuildServiceError::MediaSource { .. }) => {
                    missing.insert(path);
                }
                Err(error) => return Err(error),
            }
        }
    }
    Ok(DiscoveredMedia {
        paths: paths.into_iter().collect(),
        warnings: missing
            .into_iter()
            .map(|path| {
                format!(
                    "Media was not embedded and retains its original external reference: {}",
                    path.display()
                )
            })
            .collect(),
    })
}
