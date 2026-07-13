//! `ProPresenter` playlist file support.
//!
//! Writes protobuf-encoded playlist files (.proplaylist) to disk.

use prost::Message;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use super::media::presentation_media_dependencies_from_bytes;
use super::native_zip::{self, Entry as NativeZipEntry};
use super::package::{presentation_items, PlaylistPackageMode};
use super::serialize::write_file_atomically;
use super::SlideType;
use crate::propresenter::generated::rv_data::{
    self, playlist, playlist_document, playlist_item, url,
};

/// Errors that can occur when writing playlist files
#[derive(Debug, thiserror::Error)]
pub enum PlaylistError {
    /// An I/O error occurred during file operations
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Failed to encode the protobuf playlist data
    #[error("Encoding error: {0}")]
    Encode(String),

    /// A zip archive error occurred
    #[error("Zip error: {0}")]
    Zip(#[from] zip::result::ZipError),

    /// A media asset path could not be represented inside the package.
    #[error("Invalid media asset path: {0:?}")]
    InvalidMediaAsset(PathBuf),

    /// An archive entry path was unsafe or not canonical.
    #[error("Invalid archive entry path: {0}")]
    InvalidArchivePath(String),

    /// Two package members resolved to the same archive path.
    #[error("Duplicate archive entry path: {0}")]
    DuplicateArchiveEntry(String),

    /// The encoded playlist document and supplied package entries disagree.
    #[error("Playlist document does not match package entries: {0}")]
    PackageMismatch(String),

    /// Embedded bytes were not a decodable `ProPresenter` presentation.
    #[error("Embedded presentation {index} ({name:?}) is invalid: {reason}")]
    InvalidEmbeddedPresentation {
        /// Zero-based playlist item index.
        index: usize,
        /// Playlist item display name.
        name: String,
        /// Protobuf decoding failure.
        #[source]
        reason: prost::DecodeError,
    },

    /// A discovered media reference could not be resolved to a local file.
    #[error(
        "Media dependency {reference:?} in presentation {name:?} is not an absolute local file"
    )]
    UnresolvedMediaDependency {
        /// Playlist item display name.
        name: String,
        /// URL or path stored in the presentation.
        reference: String,
    },

    /// A discovered media reference resolved to a file that does not exist.
    #[error("Media dependency {path:?} in presentation {name:?} does not exist")]
    MissingMediaDependency {
        /// Playlist item display name.
        name: String,
        /// Resolved local path.
        path: PathBuf,
    },

    /// One source presentation was supplied with different embedded bytes.
    #[error(
        "Presentation source {presentation_path:?} has conflicting embedded data at playlist items {first_index} and {conflicting_index}"
    )]
    ConflictingEmbeddedSource {
        /// Presentation path used as the shared source identity.
        presentation_path: String,
        /// Index of the first embedded copy.
        first_index: usize,
        /// Index of the conflicting embedded copy.
        conflicting_index: usize,
    },
}

/// Errors raised while capturing the immutable native playlist metadata used
/// by a build process.
#[derive(Debug, thiserror::Error)]
pub enum PlaylistMetadataError {
    /// The configured presentation library is not inside a `ProPresenter`
    /// installation root.
    #[error("Could not locate ProPresenter root from library path {0:?}")]
    InvalidLibraryPath(PathBuf),

    /// The live playlist library could not be read.
    #[error("Could not read playlist library {path:?}: {reason}")]
    Read {
        /// Native playlist library path.
        path: PathBuf,
        /// Filesystem failure.
        #[source]
        reason: std::io::Error,
    },

    /// The live playlist library was not a decodable playlist document.
    #[error("Could not decode playlist library {path:?}: {reason}")]
    Decode {
        /// Native playlist library path.
        path: PathBuf,
        /// Protobuf decoding failure.
        #[source]
        reason: prost::DecodeError,
    },

    /// A playlist document omitted the producer metadata required for a new
    /// native file.
    #[error("Playlist document has no application metadata")]
    MissingApplicationInfo,
}

/// Immutable producer metadata captured from the live `Playlists/Library`
/// document once at process startup.
///
/// Playlist node defaults are deliberately not carried as mutable runtime
/// state. Native exports consistently use `Unknown` node types, while
/// `expanded` reflects transient UI state; fresh documents choose collapsed
/// nodes explicitly.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaylistMetadata {
    application_info: rv_data::ApplicationInfo,
}

impl PlaylistMetadata {
    /// Producer metadata captured from the live playlist library.
    #[must_use]
    pub const fn application_info(&self) -> &rv_data::ApplicationInfo {
        &self.application_info
    }

    /// Capture producer metadata from an already decoded native document.
    pub fn from_document(
        document: &rv_data::PlaylistDocument,
    ) -> Result<Self, PlaylistMetadataError> {
        let application_info = document
            .application_info
            .clone()
            .ok_or(PlaylistMetadataError::MissingApplicationInfo)?;
        Ok(Self { application_info })
    }

    /// Read the live `Playlists/Library` document associated with a configured
    /// presentation library and capture one immutable producer snapshot.
    pub fn read_from_library_dir(
        library_dir: impl AsRef<Path>,
    ) -> Result<Self, PlaylistMetadataError> {
        let library_dir = library_dir.as_ref();
        let root = propresenter_root_for_library(library_dir)
            .ok_or_else(|| PlaylistMetadataError::InvalidLibraryPath(library_dir.to_path_buf()))?;
        let path = root.join("Playlists/Library");
        let bytes = std::fs::read(&path).map_err(|reason| PlaylistMetadataError::Read {
            path: path.clone(),
            reason,
        })?;
        let document = rv_data::PlaylistDocument::decode(bytes.as_slice()).map_err(|reason| {
            PlaylistMetadataError::Decode {
                path: path.clone(),
                reason,
            }
        })?;
        Self::from_document(&document)
    }

    /// Canonical metadata for hermetic unit tests with no installed
    /// `ProPresenter` runtime. Production entry points must use
    /// [`Self::read_from_library_dir`] or [`Self::from_document`].
    #[cfg(test)]
    pub(crate) fn offline_test() -> Self {
        Self {
            application_info: rv_data::ApplicationInfo {
                platform: rv_data::application_info::Platform::Macos as i32,
                platform_version: Some(rv_data::Version {
                    major_version: 26,
                    minor_version: 6,
                    patch_version: 0,
                    build: String::new(),
                }),
                application: rv_data::application_info::Application::Propresenter as i32,
                application_version: Some(rv_data::Version {
                    major_version: 21,
                    minor_version: 3,
                    patch_version: 0,
                    build: "352518178".to_string(),
                }),
            },
        }
    }
}

fn propresenter_root_for_library(library_dir: &Path) -> Option<&Path> {
    if library_dir.join("Playlists/Library").is_file() {
        return Some(library_dir);
    }
    library_dir
        .ancestors()
        .find(|ancestor| ancestor.file_name().is_some_and(|name| name == "Libraries"))
        .and_then(Path::parent)
}

/// Errors raised while constructing a selected playlist arrangement.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SelectedArrangementError {
    /// A selected arrangement must carry its exact native display name.
    #[error("selected arrangement name cannot be empty")]
    EmptyName,
}

/// The exact native arrangement selected for a playlist item.
///
/// UUID and name travel as one value so playlist metadata cannot accidentally
/// identify an arrangement while omitting or restating its native name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedArrangement {
    uuid: Uuid,
    name: String,
}

impl SelectedArrangement {
    /// Bind an arrangement UUID to its exact native name.
    pub fn new(uuid: Uuid, name: impl Into<String>) -> Result<Self, SelectedArrangementError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(SelectedArrangementError::EmptyName);
        }
        Ok(Self { uuid, name })
    }

    /// Native arrangement UUID.
    pub const fn uuid(&self) -> &Uuid {
        &self.uuid
    }

    /// Exact native arrangement name, including its original casing.
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// A playlist entry representing a matched file for a service item
#[derive(Debug, Clone)]
pub struct PlaylistEntry {
    /// Display name for the playlist item
    pub name: String,
    /// Slide type for type-aware filename sanitization
    pub slide_type: SlideType,
    /// When true, `name` is already a valid filename stem from an existing file
    /// on disk and should not be re-sanitized.
    pub from_matched_file: bool,
    /// Path to the .pro file (external reference)
    pub presentation_path: String,
    /// Optional exact native arrangement selected for this playlist item.
    pub selected_arrangement: Option<SelectedArrangement>,
    /// Optional source-supplied music key. Generated items must leave this
    /// absent rather than inventing a default key.
    pub user_music_key: Option<rv_data::MusicKeyScale>,
    /// Optional presentation bytes to embed. Entries with the same non-empty
    /// `presentation_path` share one archive member.
    pub embedded_data: Option<Vec<u8>>,
}

/// Errors raised while constructing a bundle with multiple named playlists.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlaylistSetError {
    /// A native playlist document must contain at least one child playlist.
    #[error("playlist set must contain at least one playlist")]
    Empty,

    /// A child playlist needs an operator-visible name.
    #[error("playlist name cannot be empty")]
    EmptyName,
}

/// One named child in a multi-playlist bundle.
///
/// Entries remain owned by their child until [`PlaylistSet::new`] establishes
/// the single flattened package order used by both the protobuf document and
/// its embedded presentation members.
#[derive(Debug, Clone)]
pub struct NamedPlaylist {
    name: String,
    entries: Vec<PlaylistEntry>,
}

impl NamedPlaylist {
    /// Bind a non-empty display name to its ordered presentation entries.
    pub fn new(
        name: impl Into<String>,
        entries: Vec<PlaylistEntry>,
    ) -> Result<Self, PlaylistSetError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(PlaylistSetError::EmptyName);
        }
        Ok(Self { name, entries })
    }

    /// Native child-playlist name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Presentation entries in native playlist order.
    #[must_use]
    pub fn entries(&self) -> &[PlaylistEntry] {
        &self.entries
    }
}

#[derive(Debug, Clone)]
struct PlaylistChild {
    name: String,
    entries: std::ops::Range<usize>,
}

/// A checked one-level native playlist bundle.
///
/// `ProPresenter` exports represent both a single playlist and a bundle as one
/// root node containing named child playlists. This type owns that boundary
/// and one canonical flattened entry order, preventing the document and ZIP
/// package from being assembled from differently ordered inputs.
#[derive(Debug, Clone)]
pub struct PlaylistSet {
    children: Vec<PlaylistChild>,
    entries: Vec<PlaylistEntry>,
}

impl PlaylistSet {
    /// Normalize one or more named playlists into canonical package order.
    pub fn new(playlists: Vec<NamedPlaylist>) -> Result<Self, PlaylistSetError> {
        if playlists.is_empty() {
            return Err(PlaylistSetError::Empty);
        }

        let mut children = Vec::with_capacity(playlists.len());
        let mut entries = Vec::new();
        for playlist in playlists {
            let start = entries.len();
            entries.extend(playlist.entries);
            children.push(PlaylistChild {
                name: playlist.name,
                entries: start..entries.len(),
            });
        }
        Ok(Self { children, entries })
    }

    /// Named child playlists and their entry slices in document order.
    pub fn children(&self) -> impl ExactSizeIterator<Item = (&str, &[PlaylistEntry])> {
        self.children
            .iter()
            .map(|child| (child.name.as_str(), &self.entries[child.entries.clone()]))
    }

    /// Total presentation references across every child playlist.
    #[must_use]
    pub const fn presentation_count(&self) -> usize {
        self.entries.len()
    }
}

/// A media asset to include in an exported portable playlist package.
#[derive(Debug, Clone)]
pub struct PlaylistMediaAsset {
    /// Source media file on disk.
    pub source_path: PathBuf,
    /// Optional confined archive entry path. When absent, the writer derives
    /// the canonical absolute source path used by native portable exports.
    pub archive_path: Option<String>,
}

impl PlaylistMediaAsset {
    /// Create a portable media asset using its canonical absolute source path
    /// as the native archive identity.
    pub fn new(source_path: impl Into<PathBuf>) -> Self {
        Self {
            source_path: source_path.into(),
            archive_path: None,
        }
    }

    /// Bind this reviewed archive identity to bytes captured at preview time.
    pub(crate) fn bind_reviewed(
        &self,
        data: &[u8],
    ) -> Result<ReviewedPlaylistMediaAsset, PlaylistError> {
        Ok(ReviewedPlaylistMediaAsset {
            archive_path: media_archive_path(self)?,
            data: data.to_vec(),
        })
    }
}

/// Portable media whose archive identity and bytes were both bound during
/// preview approval.
#[derive(Debug)]
pub(crate) struct ReviewedPlaylistMediaAsset {
    archive_path: String,
    data: Vec<u8>,
}

/// Options controlling how a playlist package is written.
#[derive(Debug, Clone, Default)]
pub struct PlaylistWriteOptions {
    /// Package mode. `LibraryLocal` matches normal in-library playlist writes.
    pub package_mode: PlaylistPackageMode,
    /// Media assets included when `package_mode` is `ExportPortable`.
    pub media_assets: Vec<PlaylistMediaAsset>,
    /// Discover and include existing media files referenced by embedded
    /// presentations when writing an `ExportPortable` package.
    ///
    /// The writer verifies every discovered reference and stores it under its
    /// canonical absolute source path. Native exports use that same member
    /// identity, so presentation URLs remain unchanged.
    pub include_discovered_media_assets: bool,
}

impl PlaylistEntry {
    /// Get the filesystem-safe archive identity for this entry.
    ///
    /// Display text and package identity are separate concepts. When a source
    /// path is known, its filename owns the archive identity; `name` remains
    /// only the operator-facing playlist label. Entries without a source path
    /// retain the legacy generated-name behavior.
    pub fn embedded_filename(&self) -> String {
        if let Some(filename) = presentation_filename(&self.presentation_path) {
            return filename;
        }
        if self.from_matched_file {
            format!("{}.pro", self.name)
        } else {
            get_embedded_filename(&self.name, self.slide_type)
        }
    }
}

fn presentation_filename(path: &str) -> Option<String> {
    let filename = path
        .trim()
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|filename| !filename.is_empty())?;
    let filename = percent_decode_file_component(filename)?;
    Path::new(&filename)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pro"))
        .then_some(filename)
}

fn percent_decode_file_component(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hi = bytes.get(index + 1).and_then(|byte| hex_value(*byte))?;
            let lo = bytes.get(index + 2).and_then(|byte| hex_value(*byte))?;
            decoded.push((hi << 4) | lo);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn deduplicated_embedded_filenames(entries: &[PlaylistEntry]) -> Vec<Option<String>> {
    let mut used_names = HashSet::new();
    let mut embedded_sources = HashSet::new();

    entries
        .iter()
        .map(|entry| {
            entry.embedded_data.as_ref().and_then(|_| {
                if embedded_source_identity(entry)
                    .is_some_and(|source| !embedded_sources.insert(source))
                {
                    return None;
                }
                let base = entry.embedded_filename();
                if used_names.insert(base.clone()) {
                    Some(base)
                } else {
                    let stem = base.trim_end_matches(".pro");
                    let mut n = 2u32;
                    loop {
                        let candidate = format!("{stem} ({n}).pro");
                        if used_names.insert(candidate.clone()) {
                            break Some(candidate);
                        }
                        n += 1;
                    }
                }
            })
        })
        .collect()
}

fn embedded_source_identity(entry: &PlaylistEntry) -> Option<&str> {
    (!entry.presentation_path.trim().is_empty()).then_some(entry.presentation_path.as_str())
}

/// Convert a file path to a `ProPresenter` `file:///` URL
fn path_to_file_url(path: &str) -> String {
    if path.starts_with("file://") {
        return path.to_string();
    }

    // URL-encode characters that have special meaning in URIs.
    // Note: file:/// has three slashes for absolute paths on macOS.
    let encoded = percent_encode_file_path(path);
    format!("file://{encoded}")
}

fn percent_encode_file_path(path: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(path.len());
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'/'
            | b'-'
            | b'_'
            | b'.'
            | b'~'
            | b','
            | b'('
            | b')'
            | b'\'' => encoded.push(char::from(byte)),
            b' ' => encoded.push_str("%20"),
            _ => {
                encoded.push('%');
                encoded.push(char::from(HEX[usize::from(byte >> 4)]));
                encoded.push(char::from(HEX[usize::from(byte & 0x0F)]));
            }
        }
    }
    encoded
}

/// Extract relative path from absolute (e.g., ".../Libraries/Default/foo.pro" -> "Libraries/Default/foo.pro")
fn extract_relative_path(path: &str) -> Option<url::RelativeFilePath> {
    // Look for "Libraries/" in the path
    let rel_path = if let Some(idx) = path.find("Libraries/") {
        path[idx..].to_string()
    } else {
        // Fallback: just the filename
        std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .map(String::from)?
    };

    // Use LocalRelativePath with root = Show (10) for library paths
    Some(url::RelativeFilePath::Local(url::LocalRelativePath {
        root: url::local_relative_path::Root::Show as i32,
        path: rel_path,
    }))
}

fn embedded_document_path(
    entry: &PlaylistEntry,
    embedded_filename: &str,
) -> (String, Option<url::RelativeFilePath>) {
    if entry.presentation_path.trim().is_empty() {
        fallback_embedded_document_path(embedded_filename)
    } else {
        document_path_for_presentation_path(&entry.presentation_path)
    }
}

fn fallback_embedded_document_path(
    embedded_filename: &str,
) -> (String, Option<url::RelativeFilePath>) {
    let encoded_name = percent_encode_file_path(embedded_filename);
    let abs_path = format!("file:///Libraries/Default/{encoded_name}");
    let rel = url::RelativeFilePath::Local(url::LocalRelativePath {
        root: url::local_relative_path::Root::Show as i32,
        path: format!("Libraries/Default/{embedded_filename}"),
    });
    (abs_path, Some(rel))
}

fn document_path_for_presentation_path(path: &str) -> (String, Option<url::RelativeFilePath>) {
    let relative_path = extract_relative_path(path);
    (path_to_file_url(path), relative_path)
}

/// Sanitize a name for use as a filename, applying type-specific rules.
///
/// **Song**: name passed in should already be the song DB title; kept as-is
/// (parenthetical content is part of the song name).
///
/// **Scripture**: strips common prefixes ("Scripture", "Reading") and speaker
/// names in parentheses, converts verse colons to `v`.
///
/// **Title / Text / Graphic**: strips parenthetical speaker names, converts
/// colons to ` - `.
///
/// All types strip unsafe filesystem characters and normalize whitespace.
pub fn sanitize_filename(name: &str, slide_type: SlideType) -> String {
    match slide_type {
        SlideType::Lyrics => sanitize_song(name),
        SlideType::Scripture => sanitize_scripture(name),
        _ => sanitize_general(name),
    }
}

/// Songs: keep the name mostly verbatim (parenthetical content is part of the
/// title). Only strip unsafe filesystem chars.
fn sanitize_song(name: &str) -> String {
    strip_unsafe_chars(name)
}

/// Scripture: strip prefix labels and speaker names, convert verse colons to `v`.
fn sanitize_scripture(name: &str) -> String {
    let mut s = name.to_string();

    // Strip parenthetical speaker names
    s = strip_parens(&s);

    // Strip common prefixes: "Scripture Reading", "Scripture", "Reading"
    for prefix in &["Scripture Reading", "Scripture", "Reading"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            // Strip the separator after prefix (" - ", ": ", " ")
            s = rest
                .strip_prefix(" - ")
                .or_else(|| rest.strip_prefix(": "))
                .or_else(|| rest.strip_prefix(" -"))
                .or_else(|| rest.strip_prefix(':'))
                .unwrap_or(rest)
                .trim()
                .to_string();
            break;
        }
    }

    // Convert verse colons: digit:digit → digit v digit
    let chars: Vec<char> = s.chars().collect();
    let mut result = String::with_capacity(s.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == ':'
            && i > 0
            && chars[i - 1].is_ascii_digit()
            && i + 1 < chars.len()
            && chars[i + 1].is_ascii_digit()
        {
            result.push('v');
        } else {
            result.push(c);
        }
    }

    strip_unsafe_chars(result.trim())
}

/// General items (Title, Text, Graphic): strip parenthetical speaker names,
/// convert colons to ` - `.
fn sanitize_general(name: &str) -> String {
    let s = strip_parens(name);

    let chars: Vec<char> = s.chars().collect();
    let mut result = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == ':' {
            // Trim trailing space before inserting to avoid double space
            if result.ends_with(' ') {
                result.pop();
            }
            result.push_str(" - ");
            // Skip a following space to avoid " -  foo"
            if i + 1 < chars.len() && chars[i + 1] == ' ' {
                i += 1;
            }
        } else {
            result.push(chars[i]);
        }
        i += 1;
    }

    strip_unsafe_chars(result.trim())
}

/// Strip parenthetical content (including nested parens), then trim.
///
/// Unmatched `)` at depth 0 is also discarded — stray closing parens
/// appear in real Planning Center data (e.g. double-paren typos).
fn strip_parens(name: &str) -> String {
    let mut result = String::with_capacity(name.len());
    let mut depth = 0u32;
    for c in name.chars() {
        match c {
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            ')' => {}
            _ if depth == 0 => result.push(c),
            _ => {}
        }
    }
    result.trim().to_string()
}

/// Strip characters that are unsafe in filenames and collapse whitespace.
///
/// Includes `:` which is forbidden on macOS (legacy HFS+ path separator).
fn strip_unsafe_chars(name: &str) -> String {
    name.chars()
        .filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Get the embedded filename for a presentation (sanitized for zip entry).
///
/// Falls back to "Untitled" if sanitization produces an empty name
/// (e.g. unfilled placeholder like "Scripture (Robert)").
pub fn get_embedded_filename(name: &str, slide_type: SlideType) -> String {
    let sanitized = sanitize_filename(name, slide_type);
    if sanitized.is_empty() {
        "Untitled.pro".to_string()
    } else {
        format!("{sanitized}.pro")
    }
}

/// Build a `PlaylistDocument` from a list of entries.
///
/// `ProPresenter` expects a two-level structure:
/// - Root Playlist (container) with `playlists` field containing child playlists
/// - Child Playlist with `items` field containing the actual `PlaylistItems`
pub fn build_playlist(
    name: &str,
    entries: &[PlaylistEntry],
    metadata: &PlaylistMetadata,
) -> rv_data::PlaylistDocument {
    let embedded_filenames = deduplicated_embedded_filenames(entries);
    build_playlist_document(
        vec![build_child_playlist(name, entries, &embedded_filenames)],
        metadata,
    )
}

/// Build one native document containing every named child in a checked set.
///
/// Embedded filenames are allocated across the complete set, so distinct
/// sources with colliding basenames remain unique across child boundaries.
#[must_use]
pub fn build_playlist_set(
    playlist_set: &PlaylistSet,
    metadata: &PlaylistMetadata,
) -> rv_data::PlaylistDocument {
    let embedded_filenames = deduplicated_embedded_filenames(&playlist_set.entries);
    let children = playlist_set
        .children
        .iter()
        .map(|child| {
            build_child_playlist(
                &child.name,
                &playlist_set.entries[child.entries.clone()],
                &embedded_filenames[child.entries.clone()],
            )
        })
        .collect();
    build_playlist_document(children, metadata)
}

fn build_child_playlist(
    name: &str,
    entries: &[PlaylistEntry],
    embedded_filenames: &[Option<String>],
) -> rv_data::Playlist {
    let items = entries
        .iter()
        .zip(embedded_filenames.iter())
        .map(|(entry, embedded_filename)| build_playlist_item(entry, embedded_filename.as_ref()))
        .collect();

    rv_data::Playlist {
        uuid: Some(rv_data::Uuid {
            string: Uuid::new_v4().to_string(),
        }),
        name: name.to_string(),
        r#type: playlist::Type::Unknown as i32,
        expanded: false,
        targeted_layer_uuid: None,
        smart_directory_path: None,
        hot_key: None,
        cues: Vec::new(),
        children: Vec::new(),
        timecode_enabled: false,
        timing: playlist::TimingType::None as i32,
        startup_info: None,
        children_type: Some(playlist::ChildrenType::Items(playlist::PlaylistItems {
            items,
        })),
        link_data: None,
    }
}

fn build_playlist_item(
    entry: &PlaylistEntry,
    embedded_filename: Option<&String>,
) -> rv_data::PlaylistItem {
    // Preserve the source document path when we know it. The zip filename is
    // only the package entry name; the link should keep the library identity.
    let (file_url, relative_path) = embedded_filename.map_or_else(
        || document_path_for_presentation_path(&entry.presentation_path),
        |filename| embedded_document_path(entry, filename),
    );

    rv_data::PlaylistItem {
        uuid: Some(rv_data::Uuid {
            string: Uuid::new_v4().to_string(),
        }),
        name: entry.name.clone(),
        tags: Vec::new(),
        is_hidden: false,
        item_type: Some(playlist_item::ItemType::Presentation(
            playlist_item::Presentation {
                document_path: Some(rv_data::Url {
                    platform: rv_data::url::Platform::Macos as i32,
                    storage: Some(rv_data::url::Storage::AbsoluteString(file_url)),
                    relative_file_path: relative_path,
                }),
                arrangement: entry
                    .selected_arrangement
                    .as_ref()
                    .map(|arrangement| rv_data::Uuid {
                        string: arrangement.uuid().to_string(),
                    }),
                content_destination: rv_data::action::ContentDestination::Global as i32,
                user_music_key: entry.user_music_key.clone(),
                arrangement_name: entry
                    .selected_arrangement
                    .as_ref()
                    .map_or_else(String::new, |arrangement| arrangement.name().to_string()),
            },
        )),
    }
}

fn build_playlist_document(
    children: Vec<rv_data::Playlist>,
    metadata: &PlaylistMetadata,
) -> rv_data::PlaylistDocument {
    let root_node = rv_data::Playlist {
        uuid: Some(rv_data::Uuid {
            string: Uuid::new_v4().to_string(),
        }),
        name: "PLAYLIST".to_string(),
        r#type: playlist::Type::Unknown as i32, // Root uses default/unknown
        expanded: false,
        targeted_layer_uuid: None,
        smart_directory_path: None,
        hot_key: None,
        cues: Vec::new(),
        children: Vec::new(),
        timecode_enabled: false,
        timing: playlist::TimingType::None as i32,
        startup_info: None,
        children_type: Some(playlist::ChildrenType::Playlists(playlist::PlaylistArray {
            playlists: children,
        })),
        link_data: None,
    };

    rv_data::PlaylistDocument {
        application_info: Some(metadata.application_info.clone()),
        r#type: playlist_document::Type::Presentation as i32,
        root_node: Some(root_node),
        tags: Vec::new(),
        live_video_playlist: None,
        downloads_playlist: None,
    }
}

/// Write a playlist document to a .proplaylist file
///
/// If entries have `embedded_data`, those .pro files are bundled into the zip.
pub fn write_playlist_file(
    playlist: &rv_data::PlaylistDocument,
    entries: &[PlaylistEntry],
    path: impl AsRef<Path>,
) -> Result<(), PlaylistError> {
    write_playlist_file_with_options(playlist, entries, path, &PlaylistWriteOptions::default())
}

/// Write a playlist document to a .proplaylist file using explicit package options.
///
/// `LibraryLocal` writes only the playlist document and embedded `.pro`
/// presentations. `ExportPortable` also bundles configured media assets. Media
/// discovery is strict and follows native absolute-path member identity; it
/// does not rewrite presentation URLs.
pub fn write_playlist_file_with_options(
    playlist: &rv_data::PlaylistDocument,
    entries: &[PlaylistEntry],
    path: impl AsRef<Path>,
    write_options: &PlaylistWriteOptions,
) -> Result<(), PlaylistError> {
    let media_assets = if matches!(
        write_options.package_mode,
        PlaylistPackageMode::ExportPortable
    ) {
        let media_assets = media_assets_for_package(entries, write_options)?;
        read_playlist_media_assets(&media_assets)?
    } else {
        Vec::new()
    };
    write_playlist_file_with_reviewed_media(
        playlist,
        entries,
        path,
        write_options.package_mode,
        &media_assets,
    )
}

/// Build and write a checked multi-playlist bundle.
///
/// The set owns the document order and package-entry order together; callers
/// do not need to flatten entries or keep a second ordering in sync.
pub fn write_playlist_set_file(
    playlist_set: &PlaylistSet,
    metadata: &PlaylistMetadata,
    path: impl AsRef<Path>,
) -> Result<(), PlaylistError> {
    write_playlist_set_file_with_options(
        playlist_set,
        metadata,
        path,
        &PlaylistWriteOptions::default(),
    )
}

/// Build and write a checked multi-playlist bundle with package options.
pub fn write_playlist_set_file_with_options(
    playlist_set: &PlaylistSet,
    metadata: &PlaylistMetadata,
    path: impl AsRef<Path>,
    write_options: &PlaylistWriteOptions,
) -> Result<(), PlaylistError> {
    let document = build_playlist_set(playlist_set, metadata);
    write_playlist_file_with_options(&document, &playlist_set.entries, path, write_options)
}

fn read_playlist_media_assets(
    media_assets: &[PlaylistMediaAsset],
) -> Result<Vec<ReviewedPlaylistMediaAsset>, PlaylistError> {
    let mut archive_paths = HashSet::from(["data".to_string()]);
    let mut reviewed = Vec::with_capacity(media_assets.len());
    for asset in media_assets {
        let bound = asset.bind_reviewed(&[])?;
        if bound
            .archive_path
            .rsplit('/')
            .next()
            .is_some_and(|name| name.to_ascii_lowercase().ends_with(".pro"))
        {
            return Err(PlaylistError::InvalidArchivePath(bound.archive_path));
        }
        reserve_archive_path(&mut archive_paths, &bound.archive_path)?;
        reviewed.push(bound);
    }
    for (bound, asset) in reviewed.iter_mut().zip(media_assets) {
        bound.data = std::fs::read(&asset.source_path)?;
    }
    Ok(reviewed)
}

/// Write a playlist using portable-media bytes captured by the reviewed-build
/// boundary. No media path is read by this function.
pub(crate) fn write_playlist_file_with_reviewed_media(
    playlist: &rv_data::PlaylistDocument,
    entries: &[PlaylistEntry],
    path: impl AsRef<Path>,
    package_mode: PlaylistPackageMode,
    media_assets: &[ReviewedPlaylistMediaAsset],
) -> Result<(), PlaylistError> {
    let embedded_filenames = deduplicated_embedded_filenames(entries);
    let mut archive_paths = HashSet::from(["data".to_string()]);
    let embedded_filenames = embedded_filenames
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
    validate_embedded_source_consistency(entries)?;
    validate_embedded_presentations(entries)?;

    let prepared_media_assets = if matches!(package_mode, PlaylistPackageMode::ExportPortable) {
        media_assets
    } else {
        &[]
    }
    .iter()
    .map(|asset| {
        // The private reviewed-media constructor has already validated an
        // explicit member path or derived the canonical absolute identity
        // used by native portable exports.
        let archive_path = asset.archive_path.clone();
        if archive_path
            .rsplit('/')
            .next()
            .is_some_and(|name| name.to_ascii_lowercase().ends_with(".pro"))
        {
            return Err(PlaylistError::InvalidArchivePath(archive_path));
        }
        reserve_archive_path(&mut archive_paths, &archive_path)?;
        Ok((asset, archive_path))
    })
    .collect::<Result<Vec<_>, PlaylistError>>()?;

    let mut buf = Vec::new();
    playlist
        .encode(&mut buf)
        .map_err(|e| PlaylistError::Encode(e.to_string()))?;

    let mut archive_members = entries
        .iter()
        .zip(embedded_filenames.iter())
        .filter_map(|(entry, filename)| {
            entry
                .embedded_data
                .as_deref()
                .zip(filename.as_ref())
                .map(|(data, filename)| NativeZipEntry::borrowed(filename.clone(), data))
        })
        .collect::<Vec<_>>();
    for (asset, archive_path) in &prepared_media_assets {
        archive_members.push(NativeZipEntry::borrowed(archive_path.clone(), &asset.data));
    }
    archive_members.push(NativeZipEntry::borrowed("data".to_string(), &buf));

    write_file_atomically::<PlaylistError, _>(path.as_ref(), |file| {
        Ok(native_zip::write(file, archive_members)?)
    })
}

fn validate_playlist_matches_entries(
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
        let (_absolute_string, relative_path) = embedded_filename.as_ref().map_or_else(
            || document_path_for_presentation_path(&entry.presentation_path),
            |filename| embedded_document_path(entry, filename),
        );
        let arrangement_uuid = entry
            .selected_arrangement
            .as_ref()
            .map(|arrangement| arrangement.uuid().to_string());
        let arrangement_name = entry
            .selected_arrangement
            .as_ref()
            .map_or("", SelectedArrangement::name);
        let user_music_key = entry
            .user_music_key
            .as_ref()
            .map(|key| (key.music_key, key.music_scale));
        let relative_matches = match relative_path {
            Some(url::RelativeFilePath::Local(local)) => {
                item.local_relative_path.as_deref() == Some(local.path.as_str())
                    && item.local_root == Some(local.root)
                    && item.external_relative_path.is_none()
            }
            Some(url::RelativeFilePath::External(external)) => {
                item.local_relative_path.is_none()
                    && item.local_root.is_none()
                    && item.external_relative_path.as_deref() == Some(external.path.as_str())
            }
            None => {
                item.local_relative_path.is_none()
                    && item.local_root.is_none()
                    && item.external_relative_path.is_none()
            }
        };
        if item.name != entry.name
            || item.storage_relative_path.is_some()
            || !relative_matches
            || item.arrangement_uuid != arrangement_uuid
            || item.arrangement_name != arrangement_name
            || item.user_music_key != user_music_key
        {
            return Err(PlaylistError::PackageMismatch(format!(
                "presentation item {index} ({:?}) disagrees with its package entry",
                entry.name
            )));
        }

        if let Some(embedded_filename) = embedded_filename {
            let linked_filename = linked_presentation_filename(item).ok_or_else(|| {
                PlaylistError::PackageMismatch(format!(
                    "presentation item {index} ({:?}) has no usable linked filename",
                    entry.name
                ))
            })?;
            if !linked_filename.eq_ignore_ascii_case(embedded_filename) {
                return Err(PlaylistError::PackageMismatch(format!(
                    "presentation item {index} ({:?}) links to {linked_filename:?} but embeds {embedded_filename:?}",
                    entry.name
                )));
            }
        }
    }

    Ok(())
}

fn validate_embedded_source_consistency(entries: &[PlaylistEntry]) -> Result<(), PlaylistError> {
    let mut embedded_sources: HashMap<&str, (usize, &[u8])> = HashMap::new();
    for (index, entry) in entries.iter().enumerate() {
        let (Some(source), Some(data)) = (
            embedded_source_identity(entry),
            entry.embedded_data.as_deref(),
        ) else {
            continue;
        };
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

fn validate_embedded_presentations(entries: &[PlaylistEntry]) -> Result<(), PlaylistError> {
    for (index, entry) in entries.iter().enumerate() {
        let Some(data) = entry.embedded_data.as_deref() else {
            continue;
        };
        rv_data::Presentation::decode(data).map_err(|reason| {
            PlaylistError::InvalidEmbeddedPresentation {
                index,
                name: entry.name.clone(),
                reason,
            }
        })?;
    }
    Ok(())
}

/// Return the archive filename linked by a decoded playlist presentation item.
pub fn linked_presentation_filename(item: &super::package::PlaylistItemSummary) -> Option<String> {
    item.local_relative_path
        .as_deref()
        .or(item.storage_relative_path.as_deref())
        .or(item.external_relative_path.as_deref())
        .and_then(presentation_filename)
        .or_else(|| {
            item.absolute_string
                .as_deref()
                .and_then(presentation_filename)
        })
}

fn media_assets_for_package(
    entries: &[PlaylistEntry],
    write_options: &PlaylistWriteOptions,
) -> Result<Vec<PlaylistMediaAsset>, PlaylistError> {
    let mut media_assets = write_options.media_assets.clone();
    if write_options.include_discovered_media_assets {
        append_discovered_media_assets(entries, &mut media_assets)?;
    }
    Ok(media_assets)
}

fn append_discovered_media_assets(
    entries: &[PlaylistEntry],
    media_assets: &mut Vec<PlaylistMediaAsset>,
) -> Result<(), PlaylistError> {
    for (index, entry) in entries.iter().enumerate() {
        let Some(data) = &entry.embedded_data else {
            continue;
        };
        let dependencies = presentation_media_dependencies_from_bytes(data).map_err(|reason| {
            PlaylistError::InvalidEmbeddedPresentation {
                index,
                name: entry.name.clone(),
                reason,
            }
        })?;
        for dependency in dependencies {
            let path = dependency
                .path
                .ok_or_else(|| PlaylistError::UnresolvedMediaDependency {
                    name: entry.name.clone(),
                    reference: dependency.source.clone(),
                })?;
            if !path.is_file() {
                return Err(PlaylistError::MissingMediaDependency {
                    name: entry.name.clone(),
                    path,
                });
            }
            if !media_assets.iter().any(|asset| asset.source_path == path) {
                media_assets.push(PlaylistMediaAsset::new(path));
            }
        }
    }
    Ok(())
}

fn media_archive_path(asset: &PlaylistMediaAsset) -> Result<String, PlaylistError> {
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

fn validate_archive_path(path: &str, allow_directories: bool) -> Result<String, PlaylistError> {
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

fn reserve_archive_path(
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

/// Sanitize a name into a canonical `ProPresenter` presentation filename.
pub fn canonical_presentation_name(name: &str, slide_type: SlideType) -> String {
    let normalized = sanitize_filename(name, slide_type);
    if normalized.is_empty() {
        "Untitled".to_string()
    } else {
        normalized
    }
}

/// Compute the canonical output path for a `.proplaylist` file.
///
/// Service rebuilds intentionally overwrite the same playlist file so the
/// operator does not end up with duplicate `(2)` playlists.
pub fn playlist_output_path(library_path: Option<&Path>, name: &str) -> PathBuf {
    let base_path = library_path.map_or_else(|| PathBuf::from("."), Path::to_path_buf);

    let safe_name: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | ',' | '(' | ')') {
                c
            } else {
                '_'
            }
        })
        .collect();

    base_path.join(format!("{safe_name}.proplaylist"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;
    use std::path::PathBuf;

    fn get_test_output_path(filename: &str) -> PathBuf {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("out");
        path.push("test");
        // Ensure directory exists
        std::fs::create_dir_all(&path).ok();
        path.push(filename);
        path
    }

    fn presentation_bytes(name: &str) -> Vec<u8> {
        let presentation = rv_data::Presentation {
            name: name.to_string(),
            ..rv_data::Presentation::default()
        };
        presentation.encode_to_vec()
    }

    fn test_metadata() -> PlaylistMetadata {
        PlaylistMetadata::offline_test()
    }

    #[test]
    fn test_build_empty_playlist() {
        let playlist = build_playlist("Test Playlist", &[], &test_metadata());

        assert!(playlist.root_node.is_some());
        let root = playlist.root_node.unwrap();
        // Root node is the container named "PLAYLIST"
        assert_eq!(root.name, "PLAYLIST");
        // The inner playlist holds the actual name
        match root.children_type {
            Some(playlist::ChildrenType::Playlists(arr)) => {
                assert_eq!(arr.playlists.len(), 1);
                assert_eq!(arr.playlists[0].name, "Test Playlist");
            }
            _ => panic!("Expected Playlists in root"),
        }
    }

    #[test]
    fn playlist_set_rejects_missing_structure() {
        assert_eq!(
            NamedPlaylist::new("  ", Vec::new()).expect_err("empty name"),
            PlaylistSetError::EmptyName
        );
        assert_eq!(
            PlaylistSet::new(Vec::new()).expect_err("empty set"),
            PlaylistSetError::Empty
        );
    }

    #[test]
    fn playlist_set_writes_named_children_and_deduplicates_shared_presentations() {
        let shared_path = "/Libraries/Default/Shared Song.pro";
        let shared_bytes = presentation_bytes("Shared Song");
        let shared_entry = |name: &str| PlaylistEntry {
            name: name.to_string(),
            slide_type: SlideType::Lyrics,
            from_matched_file: true,
            presentation_path: shared_path.to_string(),
            selected_arrangement: None,
            user_music_key: None,
            embedded_data: Some(shared_bytes.clone()),
        };
        let set = PlaylistSet::new(vec![
            NamedPlaylist::new("Sunday Morning", vec![shared_entry("Shared Song")])
                .expect("named playlist"),
            NamedPlaylist::new(
                "Sunday Evening",
                vec![shared_entry("Shared Song (Reprise)")],
            )
            .expect("named playlist"),
        ])
        .expect("playlist set");

        assert_eq!(set.presentation_count(), 2);
        assert_eq!(
            set.children()
                .map(|(name, entries)| (name.to_string(), entries.len()))
                .collect::<Vec<_>>(),
            vec![
                ("Sunday Morning".to_string(), 1),
                ("Sunday Evening".to_string(), 1),
            ]
        );

        let document = build_playlist_set(&set, &test_metadata());
        let root = document.root_node.as_ref().expect("root");
        let Some(playlist::ChildrenType::Playlists(children)) = &root.children_type else {
            panic!("root playlists");
        };
        assert_eq!(
            children
                .playlists
                .iter()
                .map(|child| child.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Sunday Morning", "Sunday Evening"]
        );

        let directory = tempfile::tempdir().expect("tempdir");
        let output = directory.path().join("Playlists.proplaylist");
        write_playlist_set_file(&set, &test_metadata(), &output).expect("write playlist set");
        let package = crate::propresenter::package::read_playlist_package(&output)
            .expect("read playlist set");
        assert_eq!(presentation_items(&package.document).len(), 2);
        assert_eq!(package.embedded_files, vec!["Shared Song.pro"]);
    }

    #[test]
    fn builder_uses_native_fixture_metadata_and_current_node_defaults() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/test.proplaylist");
        let native = crate::propresenter::package::read_playlist_package(fixture)
            .expect("read native fixture");
        let metadata = PlaylistMetadata::from_document(&native.document).expect("native metadata");

        let built = build_playlist("Native Defaults", &[], &metadata);
        assert_eq!(built.application_info, native.document.application_info);
        let root = built.root_node.expect("root playlist");
        assert_eq!(root.r#type, playlist::Type::Unknown as i32);
        assert!(!root.expanded);
        let Some(playlist::ChildrenType::Playlists(children)) = root.children_type else {
            panic!("playlist children");
        };
        assert_eq!(children.playlists.len(), 1);
        assert_eq!(children.playlists[0].r#type, playlist::Type::Unknown as i32);
        assert!(!children.playlists[0].expanded);
    }

    #[test]
    fn live_metadata_snapshot_survives_source_removal() {
        let directory = tempfile::tempdir().expect("tempdir");
        let root = directory.path();
        let library = root.join("Libraries/Default");
        std::fs::create_dir_all(&library).expect("create library");
        std::fs::create_dir_all(root.join("Playlists")).expect("create playlists");
        let document = build_playlist("Snapshot", &[], &test_metadata());
        let source = root.join("Playlists/Library");
        std::fs::write(&source, document.encode_to_vec()).expect("write live library");

        let metadata = PlaylistMetadata::read_from_library_dir(&library)
            .expect("capture metadata exactly once");
        std::fs::remove_file(source).expect("remove live library");

        assert_eq!(
            metadata.application_info(),
            test_metadata().application_info()
        );
    }

    #[test]
    fn test_build_playlist_with_entries() {
        let entries = vec![
            PlaylistEntry {
                name: "Amazing Grace".to_string(),
                slide_type: SlideType::Lyrics,
                from_matched_file: true,
                presentation_path: "/path/to/amazing_grace.pro".to_string(),
                selected_arrangement: None,
                user_music_key: None,
                embedded_data: None,
            },
            PlaylistEntry {
                name: "How Great Thou Art".to_string(),
                slide_type: SlideType::Lyrics,
                from_matched_file: true,
                presentation_path: "/path/to/how_great.pro".to_string(),
                selected_arrangement: Some(
                    SelectedArrangement::new(Uuid::new_v4(), "Default").expect("valid arrangement"),
                ),
                user_music_key: None,
                embedded_data: None,
            },
        ];

        let playlist = build_playlist("Sunday Service", &entries, &test_metadata());

        let root = playlist.root_node.unwrap();
        match root.children_type {
            Some(playlist::ChildrenType::Playlists(arr)) => {
                assert_eq!(arr.playlists.len(), 1);
                let inner = &arr.playlists[0];
                match &inner.children_type {
                    Some(playlist::ChildrenType::Items(items)) => {
                        assert_eq!(items.items.len(), 2);
                        assert_eq!(items.items[0].name, "Amazing Grace");
                        assert_eq!(items.items[1].name, "How Great Thou Art");
                    }
                    _ => panic!("Expected Items in inner playlist"),
                }
            }
            _ => panic!("Expected Playlists in root"),
        }
    }

    // -- Scripture sanitization --

    #[test]
    fn test_scripture_strips_prefix_and_converts_colons() {
        assert_eq!(
            sanitize_filename(
                "Scripture - 1 Kings 18:18-21 (Connie)",
                SlideType::Scripture
            ),
            "1 Kings 18v18-21"
        );
        assert_eq!(
            sanitize_filename("Scripture: 1 Kings 18:18-21", SlideType::Scripture),
            "1 Kings 18v18-21"
        );
        assert_eq!(
            sanitize_filename("Reading - John 3:16", SlideType::Scripture),
            "John 3v16"
        );
    }

    #[test]
    fn test_scripture_bare_reference() {
        assert_eq!(
            sanitize_filename("Matthew 6:1-2", SlideType::Scripture),
            "Matthew 6v1-2"
        );
        assert_eq!(
            sanitize_filename("Psalm 119:105-106", SlideType::Scripture),
            "Psalm 119v105-106"
        );
    }

    #[test]
    fn test_scripture_strips_speaker_parens() {
        // "Scripture (Robert)" is an unfilled placeholder — stripping the speaker
        // and the "Scripture" prefix leaves nothing, which is expected.
        assert_eq!(
            sanitize_filename("Scripture (Robert)", SlideType::Scripture),
            ""
        );
        // A filled-in scripture title with speaker should produce just the reference.
        assert_eq!(
            sanitize_filename("Scripture - John 3:16 (Robert)", SlideType::Scripture),
            "John 3v16"
        );
    }

    // -- Song sanitization --

    #[test]
    fn test_song_keeps_parens() {
        assert_eq!(
            sanitize_filename("Firm Foundation (He Won't)", SlideType::Lyrics),
            "Firm Foundation (He Won't)"
        );
        assert_eq!(
            sanitize_filename("Morning By Morning (I Will Trust)", SlideType::Lyrics),
            "Morning By Morning (I Will Trust)"
        );
        assert_eq!(
            sanitize_filename("Oceans (Where Feet May Fail)", SlideType::Lyrics),
            "Oceans (Where Feet May Fail)"
        );
    }

    #[test]
    fn test_song_strips_unsafe_chars() {
        assert_eq!(sanitize_filename("What?", SlideType::Lyrics), "What");
    }

    // -- General (Text/Title/Graphic) sanitization --

    #[test]
    fn test_general_strips_speaker_parens() {
        assert_eq!(
            sanitize_filename("Welcome (Robert)", SlideType::Graphic),
            "Welcome"
        );
        assert_eq!(
            sanitize_filename("Children's Message (Connie)", SlideType::Title),
            "Children's Message"
        );
        assert_eq!(
            sanitize_filename("Benediction (Robert)", SlideType::Text),
            "Benediction"
        );
    }

    #[test]
    fn test_general_colon_to_dash() {
        assert_eq!(
            sanitize_filename("Prelude: Truro Procession", SlideType::Text),
            "Prelude - Truro Procession"
        );
        assert_eq!(
            sanitize_filename("Sermon: Showdown (Robert)", SlideType::Title),
            "Sermon - Showdown"
        );
    }

    #[test]
    fn test_general_unsafe_chars_stripped() {
        assert_eq!(
            sanitize_filename("He said \"hello\"", SlideType::Text),
            "He said hello"
        );
    }

    #[test]
    fn test_general_passthrough() {
        assert_eq!(
            sanitize_filename("Amazing Grace", SlideType::Text),
            "Amazing Grace"
        );
    }

    // -- get_embedded_filename --

    #[test]
    fn test_get_embedded_filename() {
        assert_eq!(
            get_embedded_filename("Scripture - Matthew 6:1-2 (Connie)", SlideType::Scripture),
            "Matthew 6v1-2.pro"
        );
        assert_eq!(
            get_embedded_filename("Prelude: lalala", SlideType::Text),
            "Prelude - lalala.pro"
        );
        assert_eq!(
            get_embedded_filename("Firm Foundation (He Won't)", SlideType::Lyrics),
            "Firm Foundation (He Won't).pro"
        );
    }

    #[test]
    fn test_matched_file_skips_sanitization() {
        let entry = PlaylistEntry {
            name: "Morning By Morning (I Will Trust)".to_string(),
            slide_type: SlideType::Text, // Wrong type, but from_matched_file should bypass
            from_matched_file: true,
            presentation_path: String::new(),
            selected_arrangement: None,
            user_music_key: None,
            embedded_data: None,
        };
        // Parens preserved because matched files skip sanitization
        assert_eq!(
            entry.embedded_filename(),
            "Morning By Morning (I Will Trust).pro"
        );
    }

    #[test]
    fn test_deduplication_in_embedded_filenames() {
        let entries = vec![
            PlaylistEntry {
                name: "Scripture (Robert)".to_string(),
                slide_type: SlideType::Scripture,
                from_matched_file: false,
                presentation_path: String::new(),
                selected_arrangement: None,
                user_music_key: None,
                embedded_data: Some(presentation_bytes("Scripture Robert")),
            },
            PlaylistEntry {
                name: "Scripture (Hope)".to_string(),
                slide_type: SlideType::Scripture,
                from_matched_file: false,
                presentation_path: String::new(),
                selected_arrangement: None,
                user_music_key: None,
                embedded_data: Some(presentation_bytes("Scripture Hope")),
            },
        ];

        let playlist = build_playlist("Test", &entries, &test_metadata());
        let root = playlist.root_node.as_ref().expect("root");
        let inner = match &root.children_type {
            Some(playlist::ChildrenType::Playlists(arr)) => &arr.playlists[0],
            _ => panic!("Expected playlist array"),
        };
        let items = match &inner.children_type {
            Some(playlist::ChildrenType::Items(items)) => &items.items,
            _ => panic!("Expected playlist items"),
        };
        let first_path = match &items[0].item_type {
            Some(playlist_item::ItemType::Presentation(presentation)) => presentation
                .document_path
                .as_ref()
                .and_then(|url| url.relative_file_path.as_ref())
                .expect("relative file path"),
            _ => panic!("Expected presentation item"),
        };
        let second_path = match &items[1].item_type {
            Some(playlist_item::ItemType::Presentation(presentation)) => presentation
                .document_path
                .as_ref()
                .and_then(|url| url.relative_file_path.as_ref())
                .expect("relative file path"),
            _ => panic!("Expected presentation item"),
        };
        match first_path {
            url::RelativeFilePath::Local(local) => {
                assert_eq!(local.path, "Libraries/Default/Untitled.pro");
            }
            url::RelativeFilePath::External(_) => panic!("Expected local relative file path"),
        }
        match second_path {
            url::RelativeFilePath::Local(local) => {
                assert_eq!(local.path, "Libraries/Default/Untitled (2).pro");
            }
            url::RelativeFilePath::External(_) => panic!("Expected local relative file path"),
        }

        let output_path = get_test_output_path("test_dedup.proplaylist");
        // Should not panic from duplicate zip entries
        write_playlist_file(&playlist, &entries, &output_path).expect("Failed to write playlist");

        // Verify both entries are in the zip
        let file = std::fs::File::open(&output_path).expect("open");
        let mut archive = zip::ZipArchive::new(file).expect("zip");
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(names.contains(&"Untitled.pro".to_string()));
        assert!(names.contains(&"Untitled (2).pro".to_string()));
    }

    #[test]
    fn embedded_entry_keeps_known_source_document_path() {
        let entries = vec![PlaylistEntry {
            name: "Call to Worship".to_string(),
            slide_type: SlideType::Text,
            from_matched_file: true,
            presentation_path:
                "/Users/jimmy/Documents/ProPresenter/Libraries/Default/Call to Worship.pro"
                    .to_string(),
            selected_arrangement: None,
            user_music_key: None,
            embedded_data: Some(vec![1]),
        }];

        let playlist = build_playlist("Service", &entries, &test_metadata());
        let items = crate::propresenter::package::presentation_items(&playlist);

        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].absolute_string.as_deref(),
            Some(
                "file:///Users/jimmy/Documents/ProPresenter/Libraries/Default/Call%20to%20Worship.pro"
            )
        );
        assert_eq!(
            items[0].local_relative_path.as_deref(),
            Some("Libraries/Default/Call to Worship.pro")
        );
    }

    #[test]
    fn embedded_archive_identity_comes_from_source_not_display_name() {
        let directory = tempfile::tempdir().expect("tempdir");
        let output = directory.path().join("alias.proplaylist");
        let entries = vec![PlaylistEntry {
            name: "Display Alias".to_string(),
            slide_type: SlideType::Text,
            from_matched_file: true,
            presentation_path: "/Libraries/Default/Actual File.pro".to_string(),
            selected_arrangement: None,
            user_music_key: None,
            embedded_data: Some(presentation_bytes("Actual File")),
        }];
        let playlist = build_playlist("Service", &entries, &test_metadata());

        write_playlist_file(&playlist, &entries, &output).expect("write playlist");
        let package =
            crate::propresenter::package::read_playlist_package(&output).expect("read playlist");
        let items = crate::propresenter::package::presentation_items(&package.document);

        assert_eq!(items[0].name, "Display Alias");
        assert_eq!(
            items[0].local_relative_path.as_deref(),
            Some("Libraries/Default/Actual File.pro")
        );
        assert_eq!(package.embedded_files, vec!["Actual File.pro"]);
    }

    #[test]
    fn repeated_source_entries_share_one_embedded_presentation() {
        let directory = tempfile::tempdir().expect("tempdir");
        let output = directory.path().join("repeated.proplaylist");
        let source_path = "/Users/jimmy/Documents/ProPresenter/Libraries/Default/Shared Source.pro";
        let embedded_data = presentation_bytes("Shared Source");
        let entries = vec![
            PlaylistEntry {
                name: "Opening".to_string(),
                slide_type: SlideType::Text,
                from_matched_file: true,
                presentation_path: source_path.to_string(),
                selected_arrangement: None,
                user_music_key: None,
                embedded_data: Some(embedded_data.clone()),
            },
            PlaylistEntry {
                name: "Closing".to_string(),
                slide_type: SlideType::Text,
                from_matched_file: true,
                presentation_path: source_path.to_string(),
                selected_arrangement: None,
                user_music_key: None,
                embedded_data: Some(embedded_data),
            },
        ];
        let playlist = build_playlist("Service", &entries, &test_metadata());

        write_playlist_file(&playlist, &entries, &output).expect("write playlist");
        let package =
            crate::propresenter::package::read_playlist_package(&output).expect("read playlist");
        let items = crate::propresenter::package::presentation_items(&package.document);

        assert_eq!(package.embedded_files, vec!["Shared Source.pro"]);
        assert_eq!(items.len(), 2);
        assert_eq!(
            items[0].local_relative_path.as_deref(),
            Some("Libraries/Default/Shared Source.pro")
        );
        assert_eq!(items[1].local_relative_path, items[0].local_relative_path);
    }

    #[test]
    fn repeated_source_entries_reject_conflicting_embedded_bytes() {
        let source_path = "/Libraries/Default/Shared Source.pro";
        let entries = vec![
            PlaylistEntry {
                name: "First".to_string(),
                slide_type: SlideType::Text,
                from_matched_file: true,
                presentation_path: source_path.to_string(),
                selected_arrangement: None,
                user_music_key: None,
                embedded_data: Some(presentation_bytes("First")),
            },
            PlaylistEntry {
                name: "Second".to_string(),
                slide_type: SlideType::Text,
                from_matched_file: true,
                presentation_path: source_path.to_string(),
                selected_arrangement: None,
                user_music_key: None,
                embedded_data: Some(presentation_bytes("Second")),
            },
        ];
        let playlist = build_playlist("Service", &entries, &test_metadata());
        let directory = tempfile::tempdir().expect("tempdir");

        let error = write_playlist_file(
            &playlist,
            &entries,
            directory.path().join("conflict.proplaylist"),
        )
        .expect_err("same source cannot carry different bytes");

        assert!(matches!(
            error,
            PlaylistError::ConflictingEmbeddedSource {
                first_index: 0,
                conflicting_index: 1,
                ..
            }
        ));
    }

    #[test]
    fn selected_arrangement_round_trips_uuid_and_exact_name() {
        let directory = tempfile::tempdir().expect("tempdir");
        let output = directory.path().join("arrangement.proplaylist");
        let arrangement_uuid = Uuid::new_v4();
        let arrangement_uuid_text = arrangement_uuid.to_string();
        let entries = vec![PlaylistEntry {
            name: "Song".to_string(),
            slide_type: SlideType::Lyrics,
            from_matched_file: true,
            presentation_path: "/Libraries/Default/Song.pro".to_string(),
            selected_arrangement: Some(
                SelectedArrangement::new(arrangement_uuid, "Default").expect("valid arrangement"),
            ),
            user_music_key: None,
            embedded_data: Some(presentation_bytes("Song")),
        }];
        let playlist = build_playlist("Service", &entries, &test_metadata());

        write_playlist_file(&playlist, &entries, &output).expect("write playlist");
        let package =
            crate::propresenter::package::read_playlist_package(&output).expect("read playlist");
        let items = crate::propresenter::package::presentation_items(&package.document);

        assert_eq!(
            items[0].arrangement_uuid.as_deref(),
            Some(arrangement_uuid_text.as_str())
        );
        assert_eq!(items[0].arrangement_name, "Default");
    }

    #[test]
    fn selected_arrangement_rejects_an_empty_name() {
        assert_eq!(
            SelectedArrangement::new(Uuid::new_v4(), "  "),
            Err(SelectedArrangementError::EmptyName)
        );
    }

    #[test]
    fn file_urls_encode_reserved_filename_characters() {
        assert_eq!(
            path_to_file_url("/Libraries/Default/[Hymn] A&B #1.pro"),
            "file:///Libraries/Default/%5BHymn%5D%20A%26B%20%231.pro"
        );
        assert_eq!(
            path_to_file_url("file:///Libraries/Default/Already%20Encoded.pro"),
            "file:///Libraries/Default/Already%20Encoded.pro"
        );
    }

    #[test]
    fn test_write_playlist_file() {
        let entries = vec![PlaylistEntry {
            name: "Test Song".to_string(),
            slide_type: SlideType::Lyrics,
            from_matched_file: true,
            presentation_path: "/Users/Shared/ProPresenter/Libraries/Default/Test.pro".to_string(),
            selected_arrangement: None,
            user_music_key: None,
            embedded_data: None,
        }];

        let playlist = build_playlist("Test Playlist", &entries, &test_metadata());
        let output_path = get_test_output_path("test_playlist.proplaylist");

        write_playlist_file(&playlist, &entries, &output_path).expect("Failed to write playlist");

        assert!(output_path.exists());

        let contents = std::fs::read(&output_path).expect("Failed to read playlist");
        assert!(!contents.is_empty());
    }

    #[test]
    fn portable_write_embeds_media_assets() {
        let dir = tempfile::tempdir().expect("tempdir");
        let media_path = dir.path().join("default.jpg");
        std::fs::write(&media_path, [1, 2, 3]).expect("write media asset");
        let native_archive_path = media_path
            .canonicalize()
            .expect("canonical media path")
            .display()
            .to_string();
        let output_path = dir.path().join("portable.proplaylist");
        let entries = vec![PlaylistEntry {
            name: "Test Song".to_string(),
            slide_type: SlideType::Lyrics,
            from_matched_file: true,
            presentation_path: "/Users/Shared/ProPresenter/Libraries/Default/Test.pro".to_string(),
            selected_arrangement: None,
            user_music_key: None,
            embedded_data: None,
        }];
        let playlist = build_playlist("Test Playlist", &entries, &test_metadata());
        let options = PlaylistWriteOptions {
            package_mode: PlaylistPackageMode::ExportPortable,
            media_assets: vec![PlaylistMediaAsset::new(media_path)],
            include_discovered_media_assets: false,
        };

        write_playlist_file_with_options(&playlist, &entries, &output_path, &options)
            .expect("write portable playlist");

        let package = crate::propresenter::package::read_playlist_package(&output_path)
            .expect("read package");
        assert_eq!(
            crate::propresenter::package::infer_package_mode(&package),
            PlaylistPackageMode::ExportPortable
        );
        assert_eq!(package.embedded_files, vec![native_archive_path]);
        assert_eq!(package.embedded_file_details[0].basename, "default.jpg");
        assert!(!package.embedded_file_details[0].is_presentation);
    }

    #[test]
    fn reviewed_portable_write_uses_captured_media_bytes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let media_path = dir.path().join("reviewed.jpg");
        let reviewed_bytes = [1, 2, 3];
        std::fs::write(&media_path, reviewed_bytes).expect("write reviewed media");
        let asset = PlaylistMediaAsset::new(
            media_path
                .canonicalize()
                .expect("canonical reviewed media path"),
        );
        let archive_path = media_archive_path(&asset).expect("native archive path");
        let reviewed_asset = asset
            .bind_reviewed(&reviewed_bytes)
            .expect("bind reviewed bytes");
        std::fs::write(&media_path, [9, 9, 9]).expect("change live media bytes");
        let output_path = dir.path().join("reviewed.proplaylist");
        let playlist = build_playlist("Reviewed", &[], &test_metadata());

        write_playlist_file_with_reviewed_media(
            &playlist,
            &[],
            &output_path,
            PlaylistPackageMode::ExportPortable,
            &[reviewed_asset],
        )
        .expect("write reviewed portable playlist");

        let package = crate::propresenter::package::read_playlist_package(&output_path)
            .expect("read reviewed package");
        assert_eq!(
            package
                .embedded_file_data
                .get(&archive_path)
                .expect("reviewed archive member"),
            &reviewed_bytes
        );
    }

    #[test]
    fn portable_write_embeds_discovered_media_assets() {
        let dir = tempfile::tempdir().expect("tempdir");
        let media_path = dir.path().join("default.jpg");
        std::fs::write(&media_path, [1, 2, 3]).expect("write media asset");
        let native_archive_path = media_path
            .canonicalize()
            .expect("canonical media path")
            .display()
            .to_string();

        let presentation = rv_data::Presentation {
            cues: vec![rv_data::Cue {
                actions: vec![
                    crate::propresenter::background::make_background_media_action(&media_path),
                ],
                ..rv_data::Cue::default()
            }],
            ..rv_data::Presentation::default()
        };
        let mut presentation_data = Vec::new();
        presentation
            .encode(&mut presentation_data)
            .expect("encode presentation");

        let output_path = dir.path().join("portable-discovered.proplaylist");
        let entries = vec![PlaylistEntry {
            name: "With Media".to_string(),
            slide_type: SlideType::Graphic,
            from_matched_file: true,
            presentation_path: "/Users/Shared/ProPresenter/Libraries/Default/With Media.pro"
                .to_string(),
            selected_arrangement: None,
            user_music_key: None,
            embedded_data: Some(presentation_data),
        }];
        let playlist = build_playlist("Test Playlist", &entries, &test_metadata());
        let options = PlaylistWriteOptions {
            package_mode: PlaylistPackageMode::ExportPortable,
            media_assets: Vec::new(),
            include_discovered_media_assets: true,
        };

        write_playlist_file_with_options(&playlist, &entries, &output_path, &options)
            .expect("write portable playlist");

        let package = crate::propresenter::package::read_playlist_package(&output_path)
            .expect("read package");
        assert!(package
            .embedded_files
            .iter()
            .any(|file| file == &native_archive_path));
    }

    #[test]
    fn discovered_missing_media_is_a_typed_error() {
        let directory = tempfile::tempdir().expect("tempdir");
        let missing = directory.path().join("missing.jpg");
        let presentation = rv_data::Presentation {
            cues: vec![rv_data::Cue {
                actions: vec![
                    crate::propresenter::background::make_background_media_action(&missing),
                ],
                ..rv_data::Cue::default()
            }],
            ..rv_data::Presentation::default()
        };
        let entries = vec![PlaylistEntry {
            name: "Missing Media".to_string(),
            slide_type: SlideType::Graphic,
            from_matched_file: true,
            presentation_path: "/Libraries/Default/Missing Media.pro".to_string(),
            selected_arrangement: None,
            user_music_key: None,
            embedded_data: Some(presentation.encode_to_vec()),
        }];
        let playlist = build_playlist("Service", &entries, &test_metadata());
        let options = PlaylistWriteOptions {
            package_mode: PlaylistPackageMode::ExportPortable,
            media_assets: Vec::new(),
            include_discovered_media_assets: true,
        };

        let result = write_playlist_file_with_options(
            &playlist,
            &entries,
            directory.path().join("missing.proplaylist"),
            &options,
        );

        assert!(matches!(
            result,
            Err(PlaylistError::MissingMediaDependency { path, .. }) if path == missing
        ));
    }

    #[test]
    fn malformed_embedded_presentation_is_a_typed_error() {
        let directory = tempfile::tempdir().expect("tempdir");
        let entries = vec![PlaylistEntry {
            name: "Broken".to_string(),
            slide_type: SlideType::Text,
            from_matched_file: true,
            presentation_path: "/Libraries/Default/Broken.pro".to_string(),
            selected_arrangement: None,
            user_music_key: None,
            embedded_data: Some(vec![1, 2, 3]),
        }];
        let playlist = build_playlist("Service", &entries, &test_metadata());

        let result = write_playlist_file(
            &playlist,
            &entries,
            directory.path().join("broken.proplaylist"),
        );

        assert!(matches!(
            result,
            Err(PlaylistError::InvalidEmbeddedPresentation { index: 0, .. })
        ));
    }

    #[test]
    fn rejects_unsafe_and_reserved_archive_paths() {
        let directory = tempfile::tempdir().expect("tempdir");
        let media_path = directory.path().join("asset.jpg");
        std::fs::write(&media_path, b"asset").expect("write media");
        let playlist = build_playlist("Test", &[], &test_metadata());

        let traversal_options = PlaylistWriteOptions {
            package_mode: PlaylistPackageMode::ExportPortable,
            media_assets: vec![PlaylistMediaAsset {
                source_path: media_path.clone(),
                archive_path: Some("../asset.jpg".to_string()),
            }],
            include_discovered_media_assets: false,
        };
        let traversal_result = write_playlist_file_with_options(
            &playlist,
            &[],
            directory.path().join("traversal.proplaylist"),
            &traversal_options,
        );
        assert!(matches!(
            traversal_result,
            Err(PlaylistError::InvalidArchivePath(_))
        ));

        let absolute_options = PlaylistWriteOptions {
            package_mode: PlaylistPackageMode::ExportPortable,
            media_assets: vec![PlaylistMediaAsset {
                source_path: media_path.clone(),
                archive_path: Some("/untrusted/asset.jpg".to_string()),
            }],
            include_discovered_media_assets: false,
        };
        let absolute_result = write_playlist_file_with_options(
            &playlist,
            &[],
            directory.path().join("absolute.proplaylist"),
            &absolute_options,
        );
        assert!(matches!(
            absolute_result,
            Err(PlaylistError::InvalidArchivePath(_))
        ));

        let reserved_options = PlaylistWriteOptions {
            package_mode: PlaylistPackageMode::ExportPortable,
            media_assets: vec![PlaylistMediaAsset {
                source_path: media_path,
                archive_path: Some("data".to_string()),
            }],
            include_discovered_media_assets: false,
        };
        let reserved_result = write_playlist_file_with_options(
            &playlist,
            &[],
            directory.path().join("reserved.proplaylist"),
            &reserved_options,
        );
        assert!(matches!(
            reserved_result,
            Err(PlaylistError::DuplicateArchiveEntry(path)) if path == "data"
        ));

        let duplicate_options = PlaylistWriteOptions {
            package_mode: PlaylistPackageMode::ExportPortable,
            media_assets: vec![
                PlaylistMediaAsset {
                    source_path: directory.path().join("first.jpg"),
                    archive_path: Some("media/shared.jpg".to_string()),
                },
                PlaylistMediaAsset {
                    source_path: directory.path().join("second.jpg"),
                    archive_path: Some("media/shared.jpg".to_string()),
                },
            ],
            include_discovered_media_assets: false,
        };
        let duplicate_result = write_playlist_file_with_options(
            &playlist,
            &[],
            directory.path().join("duplicate.proplaylist"),
            &duplicate_options,
        );
        assert!(matches!(
            duplicate_result,
            Err(PlaylistError::DuplicateArchiveEntry(path)) if path == "media/shared.jpg"
        ));
    }

    #[test]
    fn rejects_unsafe_matched_presentation_name() {
        let directory = tempfile::tempdir().expect("tempdir");
        let entries = vec![PlaylistEntry {
            name: "../escape".to_string(),
            slide_type: SlideType::Text,
            from_matched_file: true,
            presentation_path: String::new(),
            selected_arrangement: None,
            user_music_key: None,
            embedded_data: Some(presentation_bytes("Unsafe")),
        }];
        let playlist = build_playlist("Test", &entries, &test_metadata());

        let result = write_playlist_file(
            &playlist,
            &entries,
            directory.path().join("unsafe.proplaylist"),
        );

        assert!(matches!(result, Err(PlaylistError::InvalidArchivePath(_))));
    }

    #[test]
    fn rejects_document_and_archive_entry_mismatch() {
        let directory = tempfile::tempdir().expect("tempdir");
        let original_entries = vec![PlaylistEntry {
            name: "Original".to_string(),
            slide_type: SlideType::Text,
            from_matched_file: true,
            presentation_path: "/Libraries/Default/Original.pro".to_string(),
            selected_arrangement: None,
            user_music_key: None,
            embedded_data: Some(presentation_bytes("Original")),
        }];
        let playlist = build_playlist("Test", &original_entries, &test_metadata());
        let mut different_entries = original_entries;
        different_entries[0].name = "Different".to_string();

        let result = write_playlist_file(
            &playlist,
            &different_entries,
            directory.path().join("mismatch.proplaylist"),
        );

        assert!(matches!(result, Err(PlaylistError::PackageMismatch(_))));
    }

    #[test]
    fn atomic_playlist_write_preserves_existing_file_on_media_error() {
        let directory = tempfile::tempdir().expect("tempdir");
        let output_path = directory.path().join("existing.proplaylist");
        std::fs::write(&output_path, b"known-good").expect("write existing output");
        let playlist = build_playlist("Test", &[], &test_metadata());
        let options = PlaylistWriteOptions {
            package_mode: PlaylistPackageMode::ExportPortable,
            media_assets: vec![PlaylistMediaAsset::new(
                directory.path().join("missing.jpg"),
            )],
            include_discovered_media_assets: false,
        };

        let result = write_playlist_file_with_options(&playlist, &[], &output_path, &options);

        assert!(result.is_err());
        assert_eq!(
            std::fs::read(&output_path).expect("read preserved output"),
            b"known-good"
        );
    }

    #[test]
    fn canonical_name_replaces_colon_with_v() {
        let canonical = canonical_presentation_name("Matthew 3:16-17", SlideType::Scripture);
        assert_eq!(canonical, "Matthew 3v16-17");
    }

    #[test]
    fn canonical_name_falls_back_when_empty() {
        let canonical = canonical_presentation_name("", SlideType::Lyrics);
        assert_eq!(canonical, "Untitled");
    }
}
