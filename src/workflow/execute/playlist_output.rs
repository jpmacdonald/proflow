//! Reviewed playlist packaging and media export.

use std::path::{Path, PathBuf};

use crate::propresenter::playlist::{
    playlist_output_path, resolve_playlist_export, write_playlist_set_file_with_reviewed_media,
    PlaylistEntry, PlaylistError, PlaylistExportEvidence, PlaylistMetadata, PlaylistSet,
    ResolvedPlaylistExport, ReviewedPlaylistExportIntent,
};
use crate::workflow::approval::CapturedSources;
use crate::workflow::transaction::BuildFileTransaction;

use super::request::BoundBuildRequest;
use super::{captured_source_bytes, BuildServiceError, ServiceBuildExecutor};

pub(super) struct PlaylistExport {
    pub(super) path: PathBuf,
    pub(super) evidence: PlaylistExportEvidence,
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
        let resolved_export =
            resolve_playlist_export(&request.playlist_export, entries, propresenter_root)?;
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
