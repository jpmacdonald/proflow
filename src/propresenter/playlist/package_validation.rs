use std::collections::{HashMap, HashSet};

use super::domain::{
    PlaylistEntry, PlaylistError, PlaylistItemContractField, PlaylistMediaAsset,
    ReviewedPlaylistMediaAsset, SelectedArrangement,
};
use super::naming::{document_path_for_presentation_path, linked_presentation_filename};
use crate::propresenter::generated::rv_data::{self, url};
use crate::propresenter::media::presentation_media_dependencies_from_bytes;
use crate::propresenter::package::presentation_items;

pub(super) fn read_playlist_media_assets(
    media_assets: &[PlaylistMediaAsset],
) -> Result<Vec<ReviewedPlaylistMediaAsset>, PlaylistError> {
    let mut archive_paths = HashSet::from(["data".to_string()]);
    let mut reviewed = Vec::with_capacity(media_assets.len());
    for asset in media_assets {
        let bound = asset.bind_reviewed(&[])?;
        reject_presentation_media_path(&bound.archive_path)?;
        reserve_archive_path(&mut archive_paths, &bound.archive_path)?;
        reviewed.push(bound);
    }

    // Read only after every archive identity has passed validation. This keeps
    // malformed package requests from partially observing source files.
    for (bound, asset) in reviewed.iter_mut().zip(media_assets) {
        bound.data = std::fs::read(&asset.source_path)?;
    }
    Ok(reviewed)
}

pub(super) fn validate_playlist_matches_entries(
    playlist: &rv_data::PlaylistDocument,
    entries: &[PlaylistEntry],
    embedded_filenames: &[Option<String>],
) -> Result<(), PlaylistError> {
    let items = presentation_items(playlist);
    if items.len() != entries.len() {
        return Err(PlaylistError::PackageMismatch(format!(
            "document contains {} presentation items but {} package entries were supplied",
            items.len(),
            entries.len()
        )));
    }

    for (index, ((item, entry), embedded_filename)) in items
        .iter()
        .zip(entries)
        .zip(embedded_filenames)
        .enumerate()
    {
        let (absolute_file_url, relative_path) =
            document_path_for_presentation_path(entry.presentation_path());
        let arrangement_uuid = entry
            .selected_arrangement()
            .map(|arrangement| arrangement.uuid().to_string());
        let arrangement_name = entry
            .selected_arrangement()
            .map_or("", SelectedArrangement::name);
        let user_music_key = entry
            .user_music_key()
            .map(|key| (key.music_key, key.music_scale));
        let (local_relative_path, local_root, external_relative_path) = match &relative_path {
            Some(url::RelativeFilePath::Local(local)) => {
                (Some(local.path.as_str()), Some(local.root), None)
            }
            Some(url::RelativeFilePath::External(external)) => {
                (None, None, Some(external.path.as_str()))
            }
            None => (None, None, None),
        };
        let mismatch = |field| PlaylistError::PackageItemMismatch {
            index,
            name: entry.name().to_string(),
            field,
        };

        if item.name != entry.name() {
            return Err(mismatch(PlaylistItemContractField::Name));
        }
        if item.document_platform != Some(url::Platform::Macos as i32) {
            return Err(mismatch(PlaylistItemContractField::DocumentPlatform));
        }
        if item.absolute_string.as_deref() != Some(absolute_file_url.as_str()) {
            return Err(mismatch(PlaylistItemContractField::AbsoluteFileUrl));
        }
        if item.storage_relative_path.is_some() {
            return Err(mismatch(PlaylistItemContractField::StorageRelativePath));
        }
        if item.local_relative_path.as_deref() != local_relative_path {
            return Err(mismatch(PlaylistItemContractField::LocalRelativePath));
        }
        if item.local_root != local_root {
            return Err(mismatch(PlaylistItemContractField::LocalRelativeRoot));
        }
        if item.external_relative_path.as_deref() != external_relative_path {
            return Err(mismatch(PlaylistItemContractField::ExternalRelativePath));
        }
        if item.arrangement_uuid != arrangement_uuid {
            return Err(mismatch(PlaylistItemContractField::ArrangementUuid));
        }
        if item.arrangement_name != arrangement_name {
            return Err(mismatch(PlaylistItemContractField::ArrangementName));
        }
        if item.user_music_key != user_music_key {
            return Err(mismatch(PlaylistItemContractField::UserMusicKey));
        }
        if item.content_destination != rv_data::action::ContentDestination::Global as i32 {
            return Err(mismatch(PlaylistItemContractField::ContentDestination));
        }

        if let Some(embedded_filename) = embedded_filename {
            let linked_filename = linked_presentation_filename(item).ok_or_else(|| {
                PlaylistError::PackageMismatch(format!(
                    "presentation item {index} ({:?}) has no usable linked filename",
                    entry.name()
                ))
            })?;
            if !linked_filename.eq_ignore_ascii_case(embedded_filename) {
                return Err(PlaylistError::PackageMismatch(format!(
                    "presentation item {index} ({:?}) links to {linked_filename:?} but embeds {embedded_filename:?}",
                    entry.name()
                )));
            }
        }
    }
    Ok(())
}

pub(super) fn validate_embedded_source_consistency(
    entries: &[PlaylistEntry],
) -> Result<(), PlaylistError> {
    let mut embedded_sources: HashMap<&str, (usize, &[u8])> = HashMap::new();
    for (index, entry) in entries.iter().enumerate() {
        let Some(data) = entry.embedded_data() else {
            continue;
        };
        let source = entry.presentation_path();
        if let Some((first_index, first_data)) = embedded_sources.get(source) {
            if *first_data != data {
                return Err(PlaylistError::ConflictingEmbeddedSource {
                    presentation_path: source.to_string(),
                    first_index: *first_index,
                    conflicting_index: index,
                });
            }
        } else {
            embedded_sources.insert(source, (index, data));
        }
    }
    Ok(())
}

pub(super) fn media_assets_for_portable_import(
    entries: &[PlaylistEntry],
    additional_media_assets: &[PlaylistMediaAsset],
) -> Result<Vec<PlaylistMediaAsset>, PlaylistError> {
    let mut media_assets = additional_media_assets.to_vec();
    append_discovered_media_assets(entries, &mut media_assets)?;
    Ok(media_assets)
}

fn append_discovered_media_assets(
    entries: &[PlaylistEntry],
    media_assets: &mut Vec<PlaylistMediaAsset>,
) -> Result<(), PlaylistError> {
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
            let path = dependency
                .path
                .ok_or_else(|| PlaylistError::UnresolvedMediaDependency {
                    name: entry.name().to_string(),
                    reference: dependency.source.clone(),
                })?;
            if !path.is_file() {
                return Err(PlaylistError::MissingMediaDependency {
                    name: entry.name().to_string(),
                    path,
                });
            }
            let path = path.canonicalize().map_err(PlaylistError::Io)?;
            let mut already_included = false;
            for asset in media_assets.iter() {
                if asset
                    .source_path
                    .canonicalize()
                    .map_err(PlaylistError::Io)?
                    != path
                {
                    continue;
                }
                if let Some(archive_path) = asset
                    .archive_path
                    .as_deref()
                    .filter(|archive_path| !archive_path.trim().is_empty())
                {
                    return Err(PlaylistError::MediaDependencyArchiveOverride {
                        name: entry.name().to_string(),
                        path,
                        archive_path: archive_path.to_string(),
                    });
                }
                already_included = true;
            }
            if !already_included {
                media_assets.push(PlaylistMediaAsset::new(path));
            }
        }
    }
    Ok(())
}

pub(super) fn media_archive_path(asset: &PlaylistMediaAsset) -> Result<String, PlaylistError> {
    if let Some(path) = asset
        .archive_path
        .as_deref()
        .filter(|path| !path.trim().is_empty())
    {
        return validate_archive_path(path, true);
    }

    let canonical = asset
        .source_path
        .canonicalize()
        .map_err(PlaylistError::Io)?;
    let absolute = canonical
        .to_str()
        .ok_or_else(|| PlaylistError::InvalidMediaAsset(asset.source_path.clone()))?;
    if absolute.chars().any(char::is_control) {
        return Err(PlaylistError::InvalidMediaAsset(asset.source_path.clone()));
    }
    Ok(absolute.replace('\\', "/"))
}

pub(super) fn validate_archive_path(
    path: &str,
    allow_directories: bool,
) -> Result<String, PlaylistError> {
    let normalized = path.replace('\\', "/");
    let component_count = normalized.split('/').count();
    let has_invalid_component = normalized
        .split('/')
        .any(|component| component.is_empty() || matches!(component, "." | ".."));
    if normalized.is_empty()
        || normalized.starts_with('/')
        || normalized.ends_with('/')
        || has_windows_drive_prefix(&normalized)
        || has_invalid_component
        || normalized.chars().any(char::is_control)
        || (!allow_directories && component_count != 1)
    {
        return Err(PlaylistError::InvalidArchivePath(path.to_string()));
    }
    Ok(normalized)
}

const fn has_windows_drive_prefix(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

pub(super) fn reserve_archive_path(
    archive_paths: &mut HashSet<String>,
    archive_path: &str,
) -> Result<(), PlaylistError> {
    if archive_paths.insert(archive_path.to_lowercase()) {
        Ok(())
    } else {
        Err(PlaylistError::DuplicateArchiveEntry(
            archive_path.to_string(),
        ))
    }
}

pub(super) fn reject_presentation_media_path(path: &str) -> Result<(), PlaylistError> {
    if path
        .rsplit('/')
        .next()
        .is_some_and(|name| name.to_ascii_lowercase().ends_with(".pro"))
    {
        Err(PlaylistError::InvalidArchivePath(path.to_string()))
    } else {
        Ok(())
    }
}
