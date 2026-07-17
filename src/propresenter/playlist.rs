//! `ProPresenter` playlist document and package support.
//!
//! [`crate::propresenter::playlist::PlaylistSet`] owns the flattened presentation
//! order used for document construction and embedded-name allocation. The native
//! ZIP boundary then applies ProPresenter's evidenced global lexicographic member
//! order.

mod document;
mod domain;
mod naming;
mod package_validation;
mod package_write;

pub use document::{build_playlist, build_playlist_set};
pub use domain::{
    NamedPlaylist, PlaylistEntry, PlaylistEntryError, PlaylistError, PlaylistExportIntent,
    PlaylistItemContractField, PlaylistMediaAsset, PlaylistMetadata, PlaylistMetadataError,
    PlaylistSet, PlaylistSetError, SelectedArrangement, SelectedArrangementError,
};
pub use naming::{
    canonical_presentation_name, linked_presentation_filename, playlist_output_path,
    sanitize_filename, CanonicalPresentationNameError,
};
pub use package_write::{write_playlist_document_for_fidelity, write_playlist_set_file};

pub(crate) use domain::ReviewedPlaylistExportIntent;
pub(crate) use package_write::{
    write_playlist_document_file_with_intent, write_playlist_set_file_with_reviewed_media,
};

#[cfg(test)]
mod tests;
