//! Read-only playlist document tools: translate wire arguments into the
//! format crate's playlist readers, confined to the configured roots.

use std::path::PathBuf;

use propresenter::playlist_document::{
    read_library_playlist_document, read_package_playlist_document, PlaylistDocument,
    PlaylistDocumentError,
};

use super::schema::ReadPlaylistArgs;
use crate::paths::BuildLocations;

/// Which playlist the caller asked for. Exactly one source, never both.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum PlaylistRequest {
    /// A playlist or folder in the live playlist library.
    Named(String),
    /// A `.proplaylist` package file.
    Package(String),
}

#[derive(Debug, thiserror::Error)]
pub(super) enum ReadPlaylistError {
    #[error("pass exactly one non-empty value: name or path")]
    SourceNotExclusive,
    #[error("path is not a .proplaylist file inside the configured playlist output directory or ProPresenter data root")]
    PathNotAllowed,
    #[error(transparent)]
    Document(#[from] PlaylistDocumentError),
}

impl TryFrom<ReadPlaylistArgs> for PlaylistRequest {
    type Error = ReadPlaylistError;

    fn try_from(args: ReadPlaylistArgs) -> Result<Self, Self::Error> {
        match (args.name, args.path) {
            (Some(name), None) if !name.trim().is_empty() => Ok(Self::Named(name)),
            (None, Some(path)) if !path.trim().is_empty() => Ok(Self::Package(path)),
            _ => Err(ReadPlaylistError::SourceNotExclusive),
        }
    }
}

/// Request errors are the caller's problem; read and decode failures are not.
pub(super) fn read_playlist_err(error: &ReadPlaylistError) -> rmcp::ErrorData {
    match error {
        ReadPlaylistError::SourceNotExclusive
        | ReadPlaylistError::PathNotAllowed
        | ReadPlaylistError::Document(
            PlaylistDocumentError::PlaylistNotFound { .. }
            | PlaylistDocumentError::PlaylistNameAmbiguous { .. },
        ) => rmcp::ErrorData::invalid_params(error.to_string(), None),
        ReadPlaylistError::Document(_) => rmcp::ErrorData::internal_error(error.to_string(), None),
    }
}

pub(super) fn read_playlist_document(
    locations: &BuildLocations,
    request: &PlaylistRequest,
) -> Result<PlaylistDocument, ReadPlaylistError> {
    match request {
        PlaylistRequest::Named(name) => Ok(read_library_playlist_document(
            locations.propresenter_root(),
            name,
        )?),
        PlaylistRequest::Package(path) => {
            let package = confined_package_path(locations, path)?;
            Ok(read_package_playlist_document(&package)?)
        }
    }
}

/// Resolve `requested` to an existing `.proplaylist` file whose real path is
/// below the playlist output directory or the `ProPresenter` data root.
///
/// A missing file and an escaping file give the same error, so the tool does
/// not reveal what exists outside its roots.
fn confined_package_path(
    locations: &BuildLocations,
    requested: &str,
) -> Result<PathBuf, ReadPlaylistError> {
    let candidate = locations.playlist_output().join(requested);
    let real = candidate
        .canonicalize()
        .map_err(|_| ReadPlaylistError::PathNotAllowed)?;
    let inside_a_root = [locations.playlist_output(), locations.propresenter_root()]
        .iter()
        .filter_map(|root| root.canonicalize().ok())
        .any(|root| real.starts_with(root));
    let is_package = real.is_file()
        && real
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("proplaylist"));
    if inside_a_root && is_package {
        Ok(real)
    } else {
        Err(ReadPlaylistError::PathNotAllowed)
    }
}

#[cfg(test)]
mod tests;
