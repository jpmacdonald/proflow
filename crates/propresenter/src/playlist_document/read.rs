//! Build a [`PlaylistDocument`] from a playlist file and the presentations it
//! links to.

use std::path::{Path, PathBuf};

use prost::Message;

use super::model::{
    ArrangementChoice, ArrangementSummary, CueGroupSummary, DocumentSource, ItemDetail,
    ItemProblem, NativeId, NumberRange, Playlist, PlaylistDocument, PlaylistItem,
    PresentationContent, PresentationItem, PresentationLocator, PresentationSummary, SchemaVersion,
    ScriptureReference, UnavailableReason,
};
use crate::deserialize::decode_presentation_bytes;
use crate::generated::rv_data::{self, playlist, playlist_item, url};
use crate::inspection::{summarize_presentation_structure, PresentationStructureSummary};
use crate::native_url::{self, NativeFileLocator, NativeFileResolution};
use crate::package::{
    read_playlist_package, summarize_presentation_item, PackageError, PlaylistItemSummary,
    PlaylistPackage,
};
use crate::playlist::linked_presentation_filename;

/// Why a playlist document could not be produced at all.
///
/// A presentation that cannot be read is not an error here: it is reported
/// inside the document as [`UnavailableReason`].
#[derive(Debug, thiserror::Error)]
pub enum PlaylistDocumentError {
    /// The `.proplaylist` package could not be read or decoded.
    #[error("could not read playlist package {}: {source}", path.display())]
    Package {
        /// Package path.
        path: PathBuf,
        /// Package failure.
        #[source]
        source: PackageError,
    },
    /// The live playlist library file could not be read.
    #[error("could not read playlist library {}: {source}", path.display())]
    LibraryRead {
        /// Library file path.
        path: PathBuf,
        /// Filesystem failure.
        #[source]
        source: std::io::Error,
    },
    /// The live playlist library file is not a decodable playlist document.
    #[error("could not decode playlist library {}: {source}", path.display())]
    LibraryDecode {
        /// Library file path.
        path: PathBuf,
        /// Protobuf failure.
        #[source]
        source: prost::DecodeError,
    },
    /// The document has no root node, so it holds no playlists.
    #[error("playlist document has no root node")]
    NoRootNode,
    /// No playlist has the requested name.
    #[error("no playlist named {requested:?}; available: {available:?}")]
    PlaylistNotFound {
        /// The requested name.
        requested: String,
        /// Names of every playlist and folder in the library.
        available: Vec<String>,
    },
    /// Several playlists have the requested name.
    #[error("{count} playlists are named {requested:?}")]
    PlaylistNameAmbiguous {
        /// The requested name.
        requested: String,
        /// Number of playlists with that name.
        count: usize,
    },
}

/// Where linked presentations are read from.
#[derive(Clone, Copy)]
enum PresentationSource<'a> {
    /// Files embedded in a `.proplaylist` package.
    Package(&'a PlaylistPackage),
    /// Library files below a `ProPresenter` data root.
    Library { root: &'a Path },
}

/// Read a `.proplaylist` package into a document.
pub fn read_package_playlist_document(
    path: &Path,
) -> Result<PlaylistDocument, PlaylistDocumentError> {
    let package = read_playlist_package(path).map_err(|source| PlaylistDocumentError::Package {
        path: path.to_path_buf(),
        source,
    })?;
    let file_name = path
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    playlist_document_from_package(&file_name, &package)
}

/// Describe an already-read package; `file_name` labels the document source.
pub fn playlist_document_from_package(
    file_name: &str,
    package: &PlaylistPackage,
) -> Result<PlaylistDocument, PlaylistDocumentError> {
    let root = package
        .document()
        .root_node
        .as_ref()
        .ok_or(PlaylistDocumentError::NoRootNode)?;
    let mut found = Vec::new();
    collect_item_playlists(root, &[], true, &mut found);
    Ok(PlaylistDocument {
        schema_version: SchemaVersion::current(),
        source: DocumentSource::Package {
            file_name: file_name.to_string(),
        },
        playlists: describe(&found, PresentationSource::Package(package)),
    })
}

/// Read one named playlist (or folder of playlists) from the live
/// `Playlists/Library` document of a `ProPresenter` data root.
///
/// Linked presentations are read only from files that resolve inside `root`.
pub fn read_library_playlist_document(
    root: &Path,
    playlist_name: &str,
) -> Result<PlaylistDocument, PlaylistDocumentError> {
    let library_path = root.join("Playlists").join("Library");
    let bytes =
        std::fs::read(&library_path).map_err(|source| PlaylistDocumentError::LibraryRead {
            path: library_path.clone(),
            source,
        })?;
    let document = rv_data::PlaylistDocument::decode(bytes.as_slice()).map_err(|source| {
        PlaylistDocumentError::LibraryDecode {
            path: library_path,
            source,
        }
    })?;
    let library_root = document
        .root_node
        .as_ref()
        .ok_or(PlaylistDocumentError::NoRootNode)?;

    let mut nodes = Vec::new();
    collect_named_nodes(library_root, &[], &mut nodes);
    let exact: Vec<_> = nodes
        .iter()
        .filter(|(_, node)| node.name == playlist_name)
        .collect();
    let trimmed = playlist_name.trim();
    let matches = if exact.is_empty() {
        nodes
            .iter()
            .filter(|(_, node)| node.name.trim().eq_ignore_ascii_case(trimmed))
            .collect()
    } else {
        exact
    };
    let (folders, selected) = match matches.as_slice() {
        [] => {
            return Err(PlaylistDocumentError::PlaylistNotFound {
                requested: playlist_name.to_string(),
                available: nodes.iter().map(|(_, node)| node.name.clone()).collect(),
            })
        }
        [single] => (&single.0, single.1),
        many => {
            return Err(PlaylistDocumentError::PlaylistNameAmbiguous {
                requested: playlist_name.to_string(),
                count: many.len(),
            })
        }
    };

    let mut found = Vec::new();
    collect_item_playlists(selected, folders, false, &mut found);
    Ok(PlaylistDocument {
        schema_version: SchemaVersion::current(),
        source: DocumentSource::LiveLibrary {
            selected: selected.name.clone(),
        },
        playlists: describe(&found, PresentationSource::Library { root }),
    })
}

type Found<'a> = Vec<(Vec<String>, &'a rv_data::Playlist)>;

fn child_nodes(node: &rv_data::Playlist) -> impl Iterator<Item = &rv_data::Playlist> {
    let nested: &[rv_data::Playlist] = match &node.children_type {
        Some(playlist::ChildrenType::Playlists(array)) => &array.playlists,
        Some(playlist::ChildrenType::Items(_)) | None => &[],
    };
    node.children.iter().chain(nested)
}

/// Every named node below the unnamed root, with its enclosing folder names.
fn collect_named_nodes<'a>(node: &'a rv_data::Playlist, folders: &[String], out: &mut Found<'a>) {
    for child in child_nodes(node) {
        out.push((folders.to_vec(), child));
        let mut inner = folders.to_vec();
        inner.push(child.name.clone());
        collect_named_nodes(child, &inner, out);
    }
}

/// Every node at or below `node` that holds items, in file order. The root's
/// own name is not a folder name.
fn collect_item_playlists<'a>(
    node: &'a rv_data::Playlist,
    folders: &[String],
    is_root: bool,
    out: &mut Found<'a>,
) {
    if matches!(&node.children_type, Some(playlist::ChildrenType::Items(_))) {
        out.push((folders.to_vec(), node));
    }
    let mut inner = folders.to_vec();
    if !is_root {
        inner.push(node.name.clone());
    }
    for child in child_nodes(node) {
        collect_item_playlists(child, &inner, false, out);
    }
}

fn describe(found: &Found<'_>, source: PresentationSource<'_>) -> Vec<Playlist> {
    found
        .iter()
        .map(|(folders, node)| {
            let items = match &node.children_type {
                Some(playlist::ChildrenType::Items(items)) => items.items.as_slice(),
                Some(playlist::ChildrenType::Playlists(_)) | None => &[],
            };
            Playlist {
                id: native_id(node.uuid.as_ref()),
                name: node.name.clone(),
                folders: folders.clone(),
                items: items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| describe_item(index, item, source))
                    .collect(),
            }
        })
        .collect()
}

fn native_id(uuid: Option<&rv_data::Uuid>) -> Option<NativeId> {
    uuid.filter(|uuid| !uuid.string.trim().is_empty())
        .map(|uuid| NativeId(uuid.string.clone()))
}

fn describe_item(
    index: usize,
    item: &rv_data::PlaylistItem,
    source: PresentationSource<'_>,
) -> PlaylistItem {
    let detail = match &item.item_type {
        Some(playlist_item::ItemType::Presentation(presentation)) => {
            ItemDetail::Presentation(describe_presentation(item, presentation, source))
        }
        Some(playlist_item::ItemType::Header(_)) => ItemDetail::Header,
        Some(playlist_item::ItemType::Cue(_)) => ItemDetail::Cue,
        Some(playlist_item::ItemType::PlanningCenter(_)) => ItemDetail::PlanningCenter,
        Some(playlist_item::ItemType::Placeholder(_)) => ItemDetail::Placeholder,
        None => ItemDetail::Untyped,
    };
    PlaylistItem {
        index,
        id: native_id(item.uuid.as_ref()),
        name: item.name.clone(),
        hidden: item.is_hidden,
        detail,
    }
}

fn describe_presentation(
    item: &rv_data::PlaylistItem,
    presentation: &playlist_item::Presentation,
    source: PresentationSource<'_>,
) -> PresentationItem {
    let item_summary = summarize_presentation_item(item);
    let arrangement = native_id(presentation.arrangement.as_ref()).map_or(
        ArrangementChoice::PresentationDefault,
        |id| ArrangementChoice::Selected {
            id,
            name: presentation.arrangement_name.clone(),
        },
    );
    let loaded = item_summary
        .as_ref()
        .map_or(Err(UnavailableReason::NoLocator), |summary| {
            load_presentation(source, summary, presentation.document_path.as_ref())
        });
    let mut problems = Vec::new();
    let content = match loaded {
        Ok(structure) => {
            if let ArrangementChoice::Selected { id, .. } = &arrangement {
                let known = structure.arrangements.iter().any(|candidate| {
                    candidate
                        .uuid
                        .as_deref()
                        .is_some_and(|uuid| uuid.eq_ignore_ascii_case(id.as_str()))
                });
                if !known {
                    problems.push(ItemProblem::ArrangementNotInPresentation { id: id.clone() });
                }
            }
            PresentationContent::Available(summarize(&structure))
        }
        Err(reason) => PresentationContent::Unavailable { reason },
    };
    PresentationItem {
        source: item_summary
            .as_ref()
            .map_or(PresentationLocator::Unspecified, locator),
        arrangement,
        content,
        problems,
    }
}

fn locator(summary: &PlaylistItemSummary) -> PresentationLocator {
    let show_root = Some(url::local_relative_path::Root::Show as i32);
    if let Some(path) = &summary.local_relative_path {
        let path = native_url::percent_decode_lossy(path);
        return if summary.local_root == show_root {
            PresentationLocator::ShowRelative { path }
        } else {
            PresentationLocator::OtherRelative { path }
        };
    }
    if let Some(path) = &summary.storage_relative_path {
        return PresentationLocator::OtherRelative {
            path: native_url::percent_decode_lossy(path),
        };
    }
    if let Some(path) = &summary.external_relative_path {
        return PresentationLocator::External {
            path: native_url::percent_decode_lossy(path),
        };
    }
    summary
        .absolute_string
        .as_deref()
        .map_or(PresentationLocator::Unspecified, |value| {
            PresentationLocator::Absolute {
                path: native_url::decode_file_url_or_path_lossy(value),
            }
        })
}

fn load_presentation(
    source: PresentationSource<'_>,
    summary: &PlaylistItemSummary,
    document_path: Option<&rv_data::Url>,
) -> Result<PresentationStructureSummary, UnavailableReason> {
    let (bytes, label) = match source {
        PresentationSource::Package(package) => {
            let file_name =
                linked_presentation_filename(summary).ok_or(UnavailableReason::NoLocator)?;
            (embedded_bytes(package, &file_name)?.to_vec(), file_name)
        }
        PresentationSource::Library { root } => {
            let path = library_file(root, document_path)?;
            let bytes = std::fs::read(&path).map_err(|error| UnavailableReason::Unreadable {
                detail: error.to_string(),
            })?;
            (bytes, path.display().to_string())
        }
    };
    let presentation = decode_presentation_bytes(&bytes, &label).map_err(|error| {
        UnavailableReason::NotAPresentation {
            detail: error.to_string(),
        }
    })?;
    Ok(summarize_presentation_structure(&presentation))
}

fn embedded_bytes<'a>(
    package: &'a PlaylistPackage,
    file_name: &str,
) -> Result<&'a [u8], UnavailableReason> {
    if let Some(bytes) = package.embedded_file(file_name) {
        return Ok(bytes);
    }
    let candidates: Vec<&str> = package
        .embedded_files()
        .filter(|name| name.eq_ignore_ascii_case(file_name))
        .collect();
    match candidates.as_slice() {
        [] => Err(UnavailableReason::EmbeddedFileMissing {
            file_name: file_name.to_string(),
        }),
        [only] => {
            package
                .embedded_file(only)
                .ok_or_else(|| UnavailableReason::EmbeddedFileMissing {
                    file_name: file_name.to_string(),
                })
        }
        many => Err(UnavailableReason::EmbeddedFileAmbiguous {
            candidates: many.iter().map(|name| (*name).to_string()).collect(),
        }),
    }
}

/// Resolve a linked library file, refusing anything outside the data root.
fn library_file(
    root: &Path,
    document_path: Option<&rv_data::Url>,
) -> Result<PathBuf, UnavailableReason> {
    let locator = document_path
        .and_then(NativeFileLocator::from_url)
        .ok_or(UnavailableReason::NoLocator)?;
    let unreadable = |error: std::io::Error| UnavailableReason::Unreadable {
        detail: error.to_string(),
    };
    match locator.resolve(Some(root)) {
        NativeFileResolution::Unresolved => Err(UnavailableReason::NoLocator),
        NativeFileResolution::Missing(path) => Err(UnavailableReason::FileMissing {
            path: path
                .strip_prefix(root)
                .unwrap_or(&path)
                .display()
                .to_string(),
        }),
        NativeFileResolution::Available(path) => {
            let canonical = path.canonicalize().map_err(unreadable)?;
            let canonical_root = root.canonicalize().map_err(unreadable)?;
            if canonical.starts_with(&canonical_root) {
                Ok(canonical)
            } else {
                Err(UnavailableReason::FileOutsideRoot {
                    path: canonical.display().to_string(),
                })
            }
        }
    }
}

fn summarize(structure: &PresentationStructureSummary) -> PresentationSummary {
    let mut media: Vec<String> = Vec::new();
    for name in structure.cues.iter().flat_map(|cue| &cue.background_media) {
        if !media.contains(name) {
            media.push(name.clone());
        }
    }
    PresentationSummary {
        name: structure.name.clone(),
        id: structure.uuid.clone().map(NativeId),
        slide_count: structure.cues.len(),
        scripture: structure
            .bible_reference
            .as_ref()
            .map(|reference| ScriptureReference {
                book: reference.book_name.clone(),
                chapters: reference.chapter_range.map(|range| NumberRange {
                    start: range.start,
                    end: range.end,
                }),
                verses: reference.verse_range.map(|range| NumberRange {
                    start: range.start,
                    end: range.end,
                }),
                translation: reference.translation_display_abbreviation.clone(),
            }),
        cue_groups: structure
            .cue_groups
            .iter()
            .map(|group| {
                let mut labels: Vec<String> = Vec::new();
                for cue in group
                    .cue_indexes
                    .iter()
                    .filter_map(|&i| structure.cues.get(i))
                {
                    for label in &cue.slide_labels {
                        if !labels.contains(&label.text) {
                            labels.push(label.text.clone());
                        }
                    }
                }
                CueGroupSummary {
                    name: group.name.clone(),
                    slide_count: group.cue_indexes.len(),
                    labels,
                }
            })
            .collect(),
        arrangements: structure
            .arrangements
            .iter()
            .map(|arrangement| ArrangementSummary {
                id: arrangement.uuid.clone().map(NativeId),
                name: arrangement.name.clone(),
                groups: arrangement.group_names.clone(),
            })
            .collect(),
        media,
        structure_problems: structure.reference_diagnostics.clone(),
    }
}
