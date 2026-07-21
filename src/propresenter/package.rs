//! Inspection helpers for `ProPresenter` playlist packages.
//!
//! A `.proplaylist` file is a zip archive containing a protobuf `data` entry
//! and, for exported playlists, embedded `.pro` presentation files. The
//! submodules keep archive decoding, playlist-item inspection, presentation
//! structure, and fidelity comparison as separate boundaries.

mod comparison;
mod items;
mod model;
mod presentation;
mod read;

pub use comparison::{compare_playlist_packages, infer_archive_shape};
pub use items::{compare_playlist_items_aligned, presentation_items};
pub use model::{
    EmbeddedPresentationStructure, EmbeddedPresentationSummary, PackageError, PackageFileSummary,
    PlaylistArchiveShape, PlaylistItemAlignedDiff, PlaylistItemSummary, PlaylistPackage,
    PlaylistPackageComparison, PlaylistPackageIssue,
};
pub use presentation::{embedded_presentation_structures, embedded_presentation_summaries};
pub use read::read_playlist_package;

#[cfg(test)]
use comparison::compare_presentation_structure_summary;
#[cfg(test)]
use items::normalize_absolute_path_value;

#[cfg(test)]
mod tests;
