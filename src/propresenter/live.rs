//! Helpers for reading live ProPresenter playlist libraries.

use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use prost::Message;

use super::generated::rv_data::{self, playlist, playlist_item, url};
use super::native_zip::{self, Entry as NativeZipEntry};
use super::package::{presentation_items, PlaylistItemSummary};
use super::playlist::linked_presentation_filename;
use super::serialize::write_file_atomically;

/// Result of materializing one live playlist into a comparable package.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LivePlaylistMaterializeReport {
    /// `ProPresenter` root directory used for extraction.
    pub root: String,
    /// Playlist name extracted from `Playlists/Library`.
    pub playlist_name: String,
    /// Output `.proplaylist` path.
    pub output_path: String,
    /// Total item count in the live playlist.
    pub total_item_count: usize,
    /// Number of presentation items embedded in the output package.
    pub presentation_item_count: usize,
    /// Non-presentation items retained in playlist data without a `.pro` member.
    pub non_presentation_items: Vec<LivePlaylistItemSummary>,
}

/// Summary of a live playlist item that cannot be embedded as a presentation.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LivePlaylistItemSummary {
    /// Item index in traversal order.
    pub index: usize,
    /// Playlist item name.
    pub name: String,
    /// Protobuf item type name.
    pub item_type: String,
}

/// Materialize a named playlist from a live `ProPresenter` root.
pub fn materialize_live_playlist(
    root: &Path,
    playlist_name: &str,
    output_path: &Path,
) -> Result<LivePlaylistMaterializeReport> {
    let library_path = root.join("Playlists").join("Library");
    let bytes = std::fs::read(&library_path)
        .with_context(|| format!("read playlist library {}", library_path.display()))?;
    let document = rv_data::PlaylistDocument::decode(bytes.as_slice())
        .with_context(|| format!("decode playlist library {}", library_path.display()))?;
    let playlist = find_playlist(&document, playlist_name).with_context(|| {
        let names = playlist_names(&document).join("\n  ");
        format!("playlist {playlist_name:?} not found. Available playlists:\n  {names}")
    })?;

    let all_items = collect_playlist_items(playlist);
    let non_presentation_items = all_items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            !matches!(
                &item.item_type,
                Some(playlist_item::ItemType::Presentation(_))
            )
        })
        .map(|(index, item)| LivePlaylistItemSummary {
            index,
            name: item.name.clone(),
            item_type: item_type_name(item),
        })
        .collect::<Vec<_>>();
    let presentation_items = all_items
        .into_iter()
        .filter(|item| {
            matches!(
                &item.item_type,
                Some(playlist_item::ItemType::Presentation(_))
            )
        })
        .collect::<Vec<_>>();
    let embedded_presentations = presentation_items
        .iter()
        .map(|item| preserved_presentation_for_item(item, root))
        .collect::<Result<Vec<_>>>()?;

    if let Some(parent) = output_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }

    // Preserve the live protobuf source independently from the generated-file
    // builder. Native libraries can contain stale metadata (for example an
    // arrangement UUID with no name) that generated state deliberately cannot
    // represent, but a raw materialization must retain byte-level semantics.
    let package = extracted_playlist_document(&document, playlist)
        .context("extract live playlist document")?;
    write_preserved_playlist_file(&package, &embedded_presentations, output_path)
        .with_context(|| format!("write {}", output_path.display()))?;

    Ok(LivePlaylistMaterializeReport {
        root: root.display().to_string(),
        playlist_name: playlist_name.to_string(),
        output_path: output_path.display().to_string(),
        total_item_count: presentation_items.len() + non_presentation_items.len(),
        presentation_item_count: presentation_items.len(),
        non_presentation_items,
    })
}

/// Return playlist names from a live `ProPresenter` root.
pub fn live_playlist_names(root: &Path) -> Result<Vec<String>> {
    let library_path = root.join("Playlists").join("Library");
    let bytes = std::fs::read(&library_path)
        .with_context(|| format!("read playlist library {}", library_path.display()))?;
    let document = rv_data::PlaylistDocument::decode(bytes.as_slice())
        .with_context(|| format!("decode playlist library {}", library_path.display()))?;
    Ok(playlist_names(&document))
}

fn find_playlist<'a>(
    document: &'a rv_data::PlaylistDocument,
    playlist_name: &str,
) -> Option<&'a rv_data::Playlist> {
    let mut playlists = Vec::new();
    if let Some(root) = &document.root_node {
        collect_playlists(root, &mut playlists);
    }
    playlists
        .iter()
        .copied()
        .find(|playlist| playlist.name == playlist_name)
        .or_else(|| {
            playlists
                .iter()
                .copied()
                .find(|playlist| playlist.name.eq_ignore_ascii_case(playlist_name.trim()))
        })
}

fn playlist_names(document: &rv_data::PlaylistDocument) -> Vec<String> {
    let mut playlists = Vec::new();
    if let Some(root) = &document.root_node {
        collect_playlists(root, &mut playlists);
    }
    playlists
        .into_iter()
        .filter(|playlist| !playlist.name.trim().is_empty())
        .map(|playlist| playlist.name.clone())
        .collect()
}

fn collect_playlists<'a>(playlist: &'a rv_data::Playlist, out: &mut Vec<&'a rv_data::Playlist>) {
    out.push(playlist);
    for child in &playlist.children {
        collect_playlists(child, out);
    }
    if let Some(playlist::ChildrenType::Playlists(children)) = &playlist.children_type {
        for child in &children.playlists {
            collect_playlists(child, out);
        }
    }
}

fn collect_playlist_items(playlist: &rv_data::Playlist) -> Vec<&rv_data::PlaylistItem> {
    let mut items = Vec::new();
    collect_playlist_items_inner(playlist, &mut items);
    items
}

fn collect_playlist_items_inner<'a>(
    playlist: &'a rv_data::Playlist,
    out: &mut Vec<&'a rv_data::PlaylistItem>,
) {
    for child in &playlist.children {
        collect_playlist_items_inner(child, out);
    }
    match &playlist.children_type {
        Some(playlist::ChildrenType::Playlists(children)) => {
            for child in &children.playlists {
                collect_playlist_items_inner(child, out);
            }
        }
        Some(playlist::ChildrenType::Items(items)) => out.extend(items.items.iter()),
        None => {}
    }
}

fn extracted_playlist_document(
    source: &rv_data::PlaylistDocument,
    selected: &rv_data::Playlist,
) -> Result<rv_data::PlaylistDocument> {
    let source_root = source
        .root_node
        .as_ref()
        .context("playlist library has no root node")?;
    let root_node = if std::ptr::eq(source_root, selected) {
        source_root.clone()
    } else {
        let mut root = source_root.clone();
        root.children.clear();
        root.children_type = Some(playlist::ChildrenType::Playlists(playlist::PlaylistArray {
            playlists: vec![selected.clone()],
        }));
        root
    };

    Ok(rv_data::PlaylistDocument {
        application_info: source.application_info.clone(),
        r#type: source.r#type,
        root_node: Some(root_node),
        tags: source.tags.clone(),
        live_video_playlist: None,
        downloads_playlist: None,
    })
}

struct PreservedPresentation {
    archive_name: String,
    data: Vec<u8>,
}

fn preserved_presentation_for_item(
    item: &rv_data::PlaylistItem,
    root: &Path,
) -> Result<PreservedPresentation> {
    let Some(playlist_item::ItemType::Presentation(presentation)) = &item.item_type else {
        anyhow::bail!("playlist item {:?} is not a presentation", item.name);
    };
    let document_path = presentation
        .document_path
        .as_ref()
        .context("presentation item missing document path")?;
    let presentation_path = resolve_document_path(document_path, root)
        .with_context(|| format!("resolve linked presentation for {:?}", item.name))?;
    let embedded_data = std::fs::read(&presentation_path)
        .with_context(|| format!("read linked presentation {}", presentation_path.display()))?;
    rv_data::Presentation::decode(embedded_data.as_slice()).with_context(|| {
        format!(
            "decode linked presentation {} for playlist item {:?}",
            presentation_path.display(),
            item.name
        )
    })?;
    let archive_name = presentation_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| {
            Path::new(name)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("pro"))
        })
        .with_context(|| {
            format!(
                "linked presentation {} has no UTF-8 .pro filename",
                presentation_path.display()
            )
        })?
        .to_string();

    Ok(PreservedPresentation {
        archive_name,
        data: embedded_data,
    })
}

fn write_preserved_playlist_file(
    document: &rv_data::PlaylistDocument,
    presentations: &[PreservedPresentation],
    output_path: &Path,
) -> Result<()> {
    let items = presentation_items(document);
    if items.len() != presentations.len() {
        bail!(
            "playlist document contains {} presentation items but {} presentations were read",
            items.len(),
            presentations.len()
        );
    }

    let mut unique_presentations = BTreeMap::<String, &[u8]>::new();
    for (index, (item, presentation)) in items.iter().zip(presentations).enumerate() {
        validate_preserved_link(index, item, &presentation.archive_name)?;
        match unique_presentations.entry(presentation.archive_name.clone()) {
            Entry::Vacant(entry) => {
                entry.insert(&presentation.data);
            }
            Entry::Occupied(entry) if *entry.get() == presentation.data => {}
            Entry::Occupied(_) => bail!(
                "playlist references conflicting presentations named {:?}",
                presentation.archive_name
            ),
        }
    }

    let data = document.encode_to_vec();
    let mut archive_members = unique_presentations
        .into_iter()
        .map(|(name, bytes)| NativeZipEntry::borrowed(name, bytes))
        .collect::<Vec<_>>();
    archive_members.push(NativeZipEntry::borrowed("data".to_string(), &data));
    write_file_atomically::<anyhow::Error, _>(output_path, |file| {
        Ok(native_zip::write(file, archive_members)?)
    })
}

fn validate_preserved_link(
    index: usize,
    item: &PlaylistItemSummary,
    archive_name: &str,
) -> Result<()> {
    let linked_name = linked_presentation_filename(item).with_context(|| {
        format!(
            "presentation item {index} ({:?}) has no usable linked filename",
            item.name
        )
    })?;
    if !linked_name.eq_ignore_ascii_case(archive_name) {
        bail!(
            "presentation item {index} ({:?}) links to {linked_name:?} but resolves to {archive_name:?}",
            item.name
        );
    }
    Ok(())
}

fn item_type_name(item: &rv_data::PlaylistItem) -> String {
    match &item.item_type {
        Some(playlist_item::ItemType::Presentation(_)) => "presentation",
        Some(playlist_item::ItemType::Header(_)) => "header",
        Some(playlist_item::ItemType::Cue(_)) => "cue",
        Some(playlist_item::ItemType::PlanningCenter(_)) => "planning_center",
        Some(playlist_item::ItemType::Placeholder(_)) => "placeholder",
        None => "none",
    }
    .to_string()
}

fn resolve_document_path(document_path: &rv_data::Url, root: &Path) -> Result<PathBuf> {
    if let Some(url::RelativeFilePath::Local(local)) = &document_path.relative_file_path {
        if local.root == url::local_relative_path::Root::Show as i32 {
            return confined_existing_path(root, &root.join(&local.path));
        }
    }

    let source = match document_path
        .storage
        .as_ref()
        .context("document path has neither a show-relative path nor a storage path")?
    {
        url::Storage::AbsoluteString(value) | url::Storage::RelativePath(value) => value,
    };
    if let Some(index) = source.find("Libraries/") {
        return confined_existing_path(root, &root.join(&source[index..]));
    }
    if let Some(path) = source.strip_prefix("file://") {
        let path = path.strip_prefix("localhost").unwrap_or(path);
        return confined_existing_path(root, &PathBuf::from(percent_decode(path)?));
    }
    let path = PathBuf::from(source);
    let candidate = if path.is_absolute() {
        path
    } else {
        root.join(path)
    };
    confined_existing_path(root, &candidate)
}

fn confined_existing_path(root: &Path, candidate: &Path) -> Result<PathBuf> {
    let canonical_root = root
        .canonicalize()
        .with_context(|| format!("canonicalize ProPresenter root {}", root.display()))?;
    let canonical_candidate = candidate
        .canonicalize()
        .with_context(|| format!("canonicalize linked presentation {}", candidate.display()))?;
    if !canonical_candidate.starts_with(&canonical_root) {
        bail!(
            "linked presentation {} escapes ProPresenter root {}",
            canonical_candidate.display(),
            canonical_root.display()
        );
    }
    Ok(canonical_candidate)
}

fn percent_decode(value: &str) -> Result<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
            {
                decoded.push((hi << 4) | lo);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(decoded).context("document path is not valid UTF-8")
}

const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn show_relative_url(path: &str) -> rv_data::Url {
        rv_data::Url {
            relative_file_path: Some(url::RelativeFilePath::Local(url::LocalRelativePath {
                root: url::local_relative_path::Root::Show as i32,
                path: path.to_string(),
            })),
            ..rv_data::Url::default()
        }
    }

    #[test]
    fn resolves_show_relative_presentation_beneath_root() {
        let directory = tempfile::tempdir().expect("tempdir");
        let library = directory.path().join("Libraries/Default");
        std::fs::create_dir_all(&library).expect("create library");
        let presentation = library.join("safe.pro");
        std::fs::write(&presentation, b"presentation").expect("write presentation");

        let resolved = resolve_document_path(
            &show_relative_url("Libraries/Default/safe.pro"),
            directory.path(),
        )
        .expect("resolve presentation");

        assert_eq!(
            resolved,
            presentation.canonicalize().expect("canonical path")
        );
    }

    #[test]
    fn rejects_parent_traversal_outside_root() {
        let parent = tempfile::tempdir().expect("tempdir");
        let root = parent.path().join("ProPresenter");
        std::fs::create_dir(&root).expect("create root");
        std::fs::write(parent.path().join("secret.pro"), b"secret").expect("write secret");

        let result = resolve_document_path(&show_relative_url("../secret.pro"), &root);

        assert!(result.is_err());
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "the one-off native document fixture stays beside the assertions so its preserved fields are auditable"
    )]
    fn materialization_preserves_live_document_and_display_alias() {
        let directory = tempfile::tempdir().expect("tempdir");
        let root = directory.path();
        let library = root.join("Libraries/Default");
        std::fs::create_dir_all(&library).expect("create library");
        std::fs::create_dir_all(root.join("Playlists")).expect("create playlists");
        let presentation_path = library.join("Actual File.pro");
        let presentation = rv_data::Presentation {
            name: "Actual File".to_string(),
            ..rv_data::Presentation::default()
        };
        std::fs::write(&presentation_path, presentation.encode_to_vec())
            .expect("write presentation");

        let music_key = rv_data::MusicKeyScale {
            music_key: rv_data::music_key_scale::MusicKey::C as i32,
            music_scale: rv_data::music_key_scale::MusicScale::Major as i32,
        };
        let selected = rv_data::Playlist {
            uuid: Some(rv_data::Uuid {
                string: "selected-playlist".to_string(),
            }),
            name: "Native Service".to_string(),
            expanded: true,
            children_type: Some(playlist::ChildrenType::Items(playlist::PlaylistItems {
                items: vec![
                    rv_data::PlaylistItem {
                        name: "Operator Display Alias".to_string(),
                        item_type: Some(playlist_item::ItemType::Presentation(
                            playlist_item::Presentation {
                                document_path: Some(rv_data::Url {
                                    storage: Some(url::Storage::AbsoluteString(
                                        "C:\\stale\\Actual File.pro".to_string(),
                                    )),
                                    relative_file_path: Some(url::RelativeFilePath::Local(
                                        url::LocalRelativePath {
                                            root: url::local_relative_path::Root::Show as i32,
                                            path: "Libraries/Default/Actual File.pro".to_string(),
                                        },
                                    )),
                                    ..rv_data::Url::default()
                                }),
                                user_music_key: Some(music_key),
                                arrangement: Some(rv_data::Uuid {
                                    string: "stale-arrangement-id".to_string(),
                                }),
                                arrangement_name: String::new(),
                                ..playlist_item::Presentation::default()
                            },
                        )),
                        ..rv_data::PlaylistItem::default()
                    },
                    rv_data::PlaylistItem {
                        name: "Operator Note".to_string(),
                        item_type: Some(playlist_item::ItemType::Header(
                            playlist_item::Header::default(),
                        )),
                        ..rv_data::PlaylistItem::default()
                    },
                ],
            })),
            ..rv_data::Playlist::default()
        };
        let source = rv_data::PlaylistDocument {
            application_info: Some(rv_data::ApplicationInfo {
                application_version: Some(rv_data::Version {
                    major_version: 21,
                    minor_version: 3,
                    patch_version: 0,
                    build: "352518178".to_string(),
                }),
                ..rv_data::ApplicationInfo::default()
            }),
            r#type: 1,
            root_node: Some(rv_data::Playlist {
                uuid: Some(rv_data::Uuid {
                    string: "source-root".to_string(),
                }),
                name: "PLAYLIST".to_string(),
                expanded: true,
                children_type: Some(playlist::ChildrenType::Playlists(playlist::PlaylistArray {
                    playlists: vec![selected.clone()],
                })),
                ..rv_data::Playlist::default()
            }),
            tags: vec![playlist::Tag {
                name: "source-tag".to_string(),
                uuid: Some(rv_data::Uuid {
                    string: "source-tag-id".to_string(),
                }),
                ..playlist::Tag::default()
            }],
            ..rv_data::PlaylistDocument::default()
        };
        std::fs::write(root.join("Playlists/Library"), source.encode_to_vec())
            .expect("write playlist library");
        let expected = extracted_playlist_document(&source, &selected).expect("extract expected");
        let output = root.join("native.proplaylist");

        let report = materialize_live_playlist(root, "Native Service", &output)
            .expect("materialize live playlist");
        let package = crate::propresenter::package::read_playlist_package(output)
            .expect("read materialized package");

        assert_eq!(package.document, expected);
        assert_eq!(report.presentation_item_count, 1);
        assert_eq!(report.non_presentation_items.len(), 1);
        assert_eq!(package.embedded_files, vec!["Actual File.pro"]);
        let items = crate::propresenter::package::presentation_items(&package.document);
        assert_eq!(items[0].name, "Operator Display Alias");
        assert_eq!(
            items[0].arrangement_uuid.as_deref(),
            Some("stale-arrangement-id")
        );
        assert_eq!(items[0].arrangement_name, "");
        assert_eq!(
            items[0].user_music_key,
            Some((
                rv_data::music_key_scale::MusicKey::C as i32,
                rv_data::music_key_scale::MusicScale::Major as i32,
            ))
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape_outside_root() {
        use std::os::unix::fs::symlink;

        let parent = tempfile::tempdir().expect("tempdir");
        let root = parent.path().join("ProPresenter");
        let library = root.join("Libraries");
        std::fs::create_dir_all(&library).expect("create root");
        let secret = parent.path().join("secret.pro");
        std::fs::write(&secret, b"secret").expect("write secret");
        symlink(&secret, library.join("linked.pro")).expect("create symlink");

        let result = resolve_document_path(&show_relative_url("Libraries/linked.pro"), &root);

        assert!(result.is_err());
    }
}
