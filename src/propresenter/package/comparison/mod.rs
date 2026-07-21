mod archive;
mod document;
mod embedded;
mod items;

use std::path::Path;

use self::archive::{
    compare_archive_shape, compare_inferred_archive_shapes, compare_playlist_schema_coverage,
};
use self::document::compare_playlist_documents;
use self::embedded::{
    compare_embedded_presentation_structures, compare_embedded_presentations, compare_media_assets,
};
use self::items::compare_items;
use super::items::presentation_items;
use super::model::{
    PackageError, PlaylistArchiveShape, PlaylistPackage, PlaylistPackageComparison,
};
use super::read::read_playlist_package;

/// Infer whether a package depends only on library presentations or also
/// carries portable media/archive paths.
#[must_use]
pub fn infer_archive_shape(package: &PlaylistPackage) -> PlaylistArchiveShape {
    if package.embedded_file_details().any(|file| {
        !file.is_presentation
            || Path::new(&file.name)
                .parent()
                .is_some_and(|p| p != Path::new(""))
    }) {
        PlaylistArchiveShape::ContainsMedia
    } else {
        PlaylistArchiveShape::PresentationsOnly
    }
}

/// Compare two `.proplaylist` packages after normalizing volatile path roots.
pub fn compare_playlist_packages(
    expected_path: impl AsRef<Path>,
    actual_path: impl AsRef<Path>,
) -> Result<PlaylistPackageComparison, PackageError> {
    let expected_path = expected_path.as_ref();
    let actual_path = actual_path.as_ref();
    let expected = read_playlist_package(expected_path)?;
    let actual = read_playlist_package(actual_path)?;
    let expected_items = presentation_items(expected.document());
    let actual_items = presentation_items(actual.document());

    let mut issues = Vec::new();
    compare_inferred_archive_shapes(&expected, &actual, &mut issues);
    compare_archive_shape(&expected, &actual, &mut issues);
    compare_playlist_schema_coverage(&expected, &actual, &mut issues);
    compare_playlist_documents(expected.document(), actual.document(), &mut issues);
    compare_items(&expected_items, &actual_items, &mut issues);
    compare_embedded_presentations(&expected, &actual, &mut issues);
    compare_embedded_presentation_structures(&expected, &actual, &mut issues);
    compare_media_assets(&expected, &actual, &mut issues);

    Ok(PlaylistPackageComparison {
        expected_path: expected_path.display().to_string(),
        actual_path: actual_path.display().to_string(),
        expected_shape: infer_archive_shape(&expected),
        actual_shape: infer_archive_shape(&actual),
        compatible: issues.is_empty(),
        issues,
        expected_item_count: expected_items.len(),
        actual_item_count: actual_items.len(),
        expected_embedded_file_count: expected.embedded_file_count(),
        actual_embedded_file_count: actual.embedded_file_count(),
    })
}

#[cfg(test)]
pub use embedded::compare_presentation_structure_summary;
