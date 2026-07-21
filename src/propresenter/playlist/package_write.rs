use std::collections::HashSet;
use std::path::Path;

use prost::Message;

use super::document::build_playlist_set;
use super::domain::{
    PlaylistEntry, PlaylistError, PlaylistExportIntent, PlaylistMetadata, PlaylistSet,
    ReviewedPlaylistExportIntent, ReviewedPlaylistMediaAsset,
};
use super::naming::embedded_filenames;
use super::package_validation::{
    media_assets_for_portable_import, read_playlist_media_assets, reject_presentation_media_path,
    reserve_archive_path, validate_archive_path, validate_embedded_source_consistency,
    validate_playlist_matches_entries,
};
use crate::propresenter::generated::rv_data;
use crate::propresenter::native_zip::{self, Entry as NativeZipEntry};
use crate::propresenter::serialize::write_file_atomically;

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
    let mut archive_paths = HashSet::from(["data".to_string()]);
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
                reserve_archive_path(&mut archive_paths, &filename)?;
                Ok(filename)
            })
            .transpose()
    })
    .collect::<Result<Vec<_>, PlaylistError>>()?;

    validate_playlist_matches_entries(playlist, entries, &embedded_filenames)?;
    if embed_presentations {
        validate_embedded_source_consistency(entries)?;
    }

    let prepared_media_assets = media_assets
        .iter()
        .map(|asset| {
            reject_presentation_media_path(&asset.archive_path)?;
            reserve_archive_path(&mut archive_paths, &asset.archive_path)?;
            Ok((asset, asset.archive_path.clone()))
        })
        .collect::<Result<Vec<_>, PlaylistError>>()?;

    let mut document_bytes = Vec::new();
    playlist
        .encode(&mut document_bytes)
        .map_err(|error| PlaylistError::Encode(error.to_string()))?;

    let mut archive_members = entries
        .iter()
        .zip(&embedded_filenames)
        .filter_map(|(entry, filename)| {
            entry
                .embedded_data()
                .zip(filename.as_ref())
                .map(|(data, filename)| NativeZipEntry::borrowed(filename.clone(), data))
        })
        .collect::<Vec<_>>();
    for (asset, archive_path) in &prepared_media_assets {
        archive_members.push(NativeZipEntry::borrowed(
            archive_path.clone(),
            asset.data.as_ref(),
        ));
    }
    archive_members.push(NativeZipEntry::borrowed(
        "data".to_string(),
        &document_bytes,
    ));

    write_file_atomically::<PlaylistError, _>(path, |file| {
        Ok(native_zip::write(file, archive_members)?)
    })
}
