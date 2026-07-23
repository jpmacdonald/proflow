use std::path::Path;

use super::document::build_playlist_set;
use super::domain::{
    PlaylistEntry, PlaylistError, PlaylistExportIntent, PlaylistMetadata, PlaylistSet,
    ReviewedPlaylistExportIntent, ReviewedPlaylistMediaAsset,
};
use super::package_plan::PlaylistPackagePlan;
use super::package_validation::{media_assets_for_portable_import, read_playlist_media_assets};
use crate::propresenter::generated::rv_data;

/// Build and write a checked playlist bundle with explicit export behavior.
pub fn write_playlist_set_file(
    playlist_set: &PlaylistSet,
    metadata: &PlaylistMetadata,
    path: impl AsRef<Path>,
    export: PlaylistExportIntent,
) -> Result<(), PlaylistError> {
    let document = build_playlist_set(playlist_set, metadata);
    match export {
        PlaylistExportIntent::LibraryLinks => write_playlist_document_with_reviewed_media(
            &document,
            &playlist_set.entries,
            path.as_ref(),
            false,
            &[],
        ),
        PlaylistExportIntent::PortableImport {
            additional_media_assets,
        } => {
            let media_assets =
                media_assets_for_portable_import(&playlist_set.entries, &additional_media_assets)?;
            let reviewed_media = read_playlist_media_assets(&media_assets)?;
            write_playlist_document_with_reviewed_media(
                &document,
                &playlist_set.entries,
                path.as_ref(),
                true,
                &reviewed_media,
            )
        }
    }
}

/// Write a raw document and its embedded presentations for format-fidelity tools.
///
/// Product code must use [`write_playlist_set_file`] so the protobuf document
/// and archive members are derived from the same checked [`PlaylistSet`]. This
/// diagnostic boundary deliberately does not discover media: callers are
/// reconstructing an already-observed document/member pair, not requesting a
/// new portable import.
#[cfg(any(test, feature = "dev-tools"))]
pub fn write_playlist_document_for_fidelity(
    playlist: &rv_data::PlaylistDocument,
    entries: &[PlaylistEntry],
    path: impl AsRef<Path>,
) -> Result<(), PlaylistError> {
    write_playlist_document_with_reviewed_media(playlist, entries, path.as_ref(), true, &[])
}

/// Write a raw document with explicit intent for format-boundary tests.
#[cfg(test)]
pub fn write_playlist_document_file_with_intent(
    playlist: &rv_data::PlaylistDocument,
    entries: &[PlaylistEntry],
    path: impl AsRef<Path>,
    export: PlaylistExportIntent,
) -> Result<(), PlaylistError> {
    match export {
        PlaylistExportIntent::LibraryLinks => write_playlist_document_with_reviewed_media(
            playlist,
            entries,
            path.as_ref(),
            false,
            &[],
        ),
        PlaylistExportIntent::PortableImport {
            additional_media_assets,
        } => {
            let media_assets = media_assets_for_portable_import(entries, &additional_media_assets)?;
            let reviewed_media = read_playlist_media_assets(&media_assets)?;
            write_playlist_document_with_reviewed_media(
                playlist,
                entries,
                path.as_ref(),
                true,
                &reviewed_media,
            )
        }
    }
}

/// Write a checked set using media bytes captured by the reviewed-build boundary.
///
/// The member set is assembled here; `native_zip` is the single owner of the
/// evidenced global lexicographic physical order. No media path is read here.
pub fn write_playlist_set_file_with_reviewed_media(
    playlist_set: &PlaylistSet,
    metadata: &PlaylistMetadata,
    path: impl AsRef<Path>,
    export: ReviewedPlaylistExportIntent<'_>,
) -> Result<(), PlaylistError> {
    let document = build_playlist_set(playlist_set, metadata);
    match export {
        ReviewedPlaylistExportIntent::LibraryLinks => write_playlist_document_with_reviewed_media(
            &document,
            &playlist_set.entries,
            path.as_ref(),
            false,
            &[],
        ),
        ReviewedPlaylistExportIntent::PortableImport(media_assets) => {
            write_playlist_document_with_reviewed_media(
                &document,
                &playlist_set.entries,
                path.as_ref(),
                true,
                media_assets,
            )
        }
    }
}

fn write_playlist_document_with_reviewed_media(
    playlist: &rv_data::PlaylistDocument,
    entries: &[PlaylistEntry],
    path: &Path,
    embed_presentations: bool,
    media_assets: &[ReviewedPlaylistMediaAsset<'_>],
) -> Result<(), PlaylistError> {
    PlaylistPackagePlan::new(playlist, entries, embed_presentations, media_assets)?
        .write_and_verify(path)
}
