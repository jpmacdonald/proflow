use std::borrow::Cow;
use std::fmt;
use std::ops::Range;
use std::path::{Path, PathBuf};

use prost::Message;
use uuid::Uuid;

use super::naming::presentation_filename;
use super::package_validation::media_archive_path;
use crate::propresenter::arrangement::has_selectable_arrangement;
use crate::propresenter::deserialize::{decode_presentation_bytes, ProPresenterError};
use crate::propresenter::generated::rv_data;

/// A field in the native presentation-item contract owned by a [`PlaylistEntry`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaylistItemContractField {
    /// Operator-visible playlist item name.
    Name,
    /// Platform attached to the presentation document URL.
    DocumentPlatform,
    /// Absolute `file://` URL attached to the presentation document.
    AbsoluteFileUrl,
    /// Storage oneof must not contain a relative path.
    StorageRelativePath,
    /// Show-relative presentation path.
    LocalRelativePath,
    /// Root used by the show-relative presentation path.
    LocalRelativeRoot,
    /// External-volume relative path.
    ExternalRelativePath,
    /// Selected arrangement UUID.
    ArrangementUuid,
    /// Selected arrangement display name.
    ArrangementName,
    /// User-selected music key and scale.
    UserMusicKey,
    /// Layer destination used when presenting the item.
    ContentDestination,
}

impl fmt::Display for PlaylistItemContractField {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Name => "name",
            Self::DocumentPlatform => "document platform",
            Self::AbsoluteFileUrl => "absolute file URL",
            Self::StorageRelativePath => "storage relative path",
            Self::LocalRelativePath => "local relative path",
            Self::LocalRelativeRoot => "local relative root",
            Self::ExternalRelativePath => "external relative path",
            Self::ArrangementUuid => "arrangement UUID",
            Self::ArrangementName => "arrangement name",
            Self::UserMusicKey => "user music key",
            Self::ContentDestination => "content destination",
        })
    }
}

/// Errors that can occur when writing playlist files.
#[derive(Debug, thiserror::Error)]
pub enum PlaylistError {
    /// A checked playlist set could not be constructed.
    #[error(transparent)]
    Set(#[from] PlaylistSetError),

    /// A playlist entry could not be constructed without violating its invariants.
    #[error(transparent)]
    Entry(#[from] PlaylistEntryError),

    /// An I/O error occurred during file operations.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Failed to encode the protobuf playlist data.
    #[error("Encoding error: {0}")]
    Encode(String),

    /// A zip archive error occurred.
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

    /// One presentation item field disagreed with its checked package entry.
    #[error(
        "Playlist presentation item {index} ({name:?}) does not match its package entry's {field}"
    )]
    PackageItemMismatch {
        /// Zero-based presentation item index in canonical document order.
        index: usize,
        /// Checked operator-visible entry name.
        name: String,
        /// Exact native field that violated the entry contract.
        field: PlaylistItemContractField,
    },

    /// Embedded bytes were not a decodable `ProPresenter` presentation.
    #[error("Embedded presentation {index} ({name:?}) is invalid: {reason}")]
    InvalidEmbeddedPresentation {
        /// Zero-based playlist item index.
        index: usize,
        /// Playlist item display name.
        name: String,
        /// Native presentation decoding or identity failure.
        #[source]
        reason: ProPresenterError,
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

    /// A discovered native dependency was assigned a non-native archive identity.
    #[error(
        "Media dependency {path:?} in presentation {name:?} must use its canonical native archive path, not {archive_path:?}"
    )]
    MediaDependencyArchiveOverride {
        /// Playlist item display name.
        name: String,
        /// Canonical dependency source path.
        path: PathBuf,
        /// Conflicting caller-supplied archive member path.
        archive_path: String,
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

    /// Two distinct presentation sources would occupy the same native archive member.
    #[error(
        "Presentation sources {first_presentation_path:?} and {conflicting_presentation_path:?} both require embedded basename {basename:?}"
    )]
    DuplicateEmbeddedPresentationBasename {
        /// Case-preserving basename claimed by the first source.
        basename: String,
        /// First presentation source path claiming the basename.
        first_presentation_path: String,
        /// Conflicting presentation source path claiming the basename.
        conflicting_presentation_path: String,
    },
}

/// Errors raised while capturing immutable native playlist metadata.
#[derive(Debug, thiserror::Error)]
pub enum PlaylistMetadataError {
    /// The live playlist library could not be read.
    #[error("Could not read playlist library {path:?}: {reason}")]
    Read {
        /// Native playlist library path.
        path: PathBuf,
        /// Filesystem failure.
        #[source]
        reason: std::io::Error,
    },

    /// The live playlist library was not decodable.
    #[error("Could not decode playlist library {path:?}: {reason}")]
    Decode {
        /// Native playlist library path.
        path: PathBuf,
        /// Protobuf decoding failure.
        #[source]
        reason: prost::DecodeError,
    },

    /// A playlist document omitted producer metadata required for a native file.
    #[error("Playlist document has no application metadata")]
    MissingApplicationInfo,

    /// Playlist item URLs and package semantics are implemented for macOS only.
    #[error("Playlist producer platform {platform} is not supported; expected ProPresenter/macOS")]
    UnsupportedPlatform {
        /// Raw protobuf enum value supplied by the document.
        platform: i32,
    },

    /// This product reconstructs `ProPresenter` documents, not sibling Renewed Vision formats.
    #[error(
        "Playlist producer application {application} is not supported; expected ProPresenter/macOS"
    )]
    UnsupportedApplication {
        /// Raw protobuf enum value supplied by the document.
        application: i32,
    },
}

/// Immutable producer metadata captured from the live `Playlists/Library`
/// document once at process startup.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaylistMetadata {
    pub(super) application_info: rv_data::ApplicationInfo,
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
        if application_info.platform != rv_data::application_info::Platform::Macos as i32 {
            return Err(PlaylistMetadataError::UnsupportedPlatform {
                platform: application_info.platform,
            });
        }
        if application_info.application
            != rv_data::application_info::Application::Propresenter as i32
        {
            return Err(PlaylistMetadataError::UnsupportedApplication {
                application: application_info.application,
            });
        }
        Ok(Self { application_info })
    }

    /// Read `Playlists/Library` below an explicitly checked `ProPresenter` root.
    ///
    /// Root discovery belongs to `BuildLocations`; this format boundary never
    /// infers installation ownership independently.
    pub fn read_from_propresenter_root(
        root: impl AsRef<Path>,
    ) -> Result<Self, PlaylistMetadataError> {
        let root = root.as_ref();
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

    /// Canonical metadata for hermetic unit tests with no installed runtime.
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

/// Errors raised while constructing a selected playlist arrangement.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SelectedArrangementError {
    /// A selected arrangement must carry its exact native display name.
    #[error("selected arrangement name cannot be empty")]
    EmptyName,
    /// The exact native name cannot contain padding or control characters.
    #[error("selected arrangement name is not a valid exact identity")]
    InvalidName,
}

/// The exact native arrangement selected for a playlist item.
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
        if name.trim() != name || name.chars().any(char::is_control) {
            return Err(SelectedArrangementError::InvalidName);
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

/// Errors raised while constructing a checked playlist entry.
#[derive(Debug, thiserror::Error)]
pub enum PlaylistEntryError {
    /// The operator-visible name was empty, padded, or contained control characters.
    #[error("playlist entry name is not a valid exact identity")]
    InvalidName,
    /// The presentation path was not an exact path to a `.pro` document.
    #[error("playlist presentation path is invalid: {0:?}")]
    InvalidPresentationPath(String),
    /// Embedded bytes were not a decodable `ProPresenter` presentation.
    #[error("embedded presentation {name:?} is invalid: {reason}")]
    InvalidEmbeddedPresentation {
        /// Playlist item display name.
        name: String,
        /// Protobuf decoding failure.
        #[source]
        reason: ProPresenterError,
    },
    /// Selected-arrangement metadata did not resolve uniquely in the embedded presentation.
    #[error(
        "embedded presentation {entry_name:?} has no selectable arrangement {arrangement_name:?} ({arrangement_uuid})"
    )]
    EmbeddedArrangementUnavailable {
        /// Playlist item display name.
        entry_name: String,
        /// Exact selected arrangement UUID.
        arrangement_uuid: Uuid,
        /// Exact selected arrangement display name.
        arrangement_name: String,
    },
}

#[derive(Debug, Clone)]
struct PresentationPath {
    value: String,
    filename: String,
}

impl PresentationPath {
    fn new(value: String) -> Result<Self, PlaylistEntryError> {
        if value.trim() != value || value.chars().any(char::is_control) {
            return Err(PlaylistEntryError::InvalidPresentationPath(value));
        }
        let filename = presentation_filename(&value)
            .ok_or_else(|| PlaylistEntryError::InvalidPresentationPath(value.clone()))?;
        Ok(Self { value, filename })
    }
}

#[derive(Debug, Clone)]
enum PlaylistEntryContent {
    Linked,
    Embedded(Vec<u8>),
}

/// A checked native playlist presentation reference.
///
/// Linked and embedded entries are distinct construction states. Every entry
/// has an exact display name and a path whose final component is a `.pro`
/// document; embedded bytes additionally carry native document identity.
#[derive(Debug, Clone)]
pub struct PlaylistEntry {
    name: String,
    presentation_path: PresentationPath,
    content: PlaylistEntryContent,
    selected_arrangement: Option<SelectedArrangement>,
    user_music_key: Option<rv_data::MusicKeyScale>,
}

impl PlaylistEntry {
    /// Construct a reference to a presentation that is not embedded.
    pub fn linked(
        name: impl Into<String>,
        presentation_path: impl Into<String>,
    ) -> Result<Self, PlaylistEntryError> {
        Self::new(
            name.into(),
            presentation_path.into(),
            PlaylistEntryContent::Linked,
        )
    }

    /// Construct a reference whose checked native presentation bytes are embedded.
    pub fn embedded(
        name: impl Into<String>,
        presentation_path: impl Into<String>,
        data: Vec<u8>,
    ) -> Result<Self, PlaylistEntryError> {
        let name = name.into();
        decode_presentation_bytes(data.as_slice(), &name).map_err(|reason| {
            PlaylistEntryError::InvalidEmbeddedPresentation {
                name: name.clone(),
                reason,
            }
        })?;
        Self::new(
            name,
            presentation_path.into(),
            PlaylistEntryContent::Embedded(data),
        )
    }

    fn new(
        name: String,
        presentation_path: String,
        content: PlaylistEntryContent,
    ) -> Result<Self, PlaylistEntryError> {
        if name.trim().is_empty() || name.trim() != name || name.chars().any(char::is_control) {
            return Err(PlaylistEntryError::InvalidName);
        }
        Ok(Self {
            name,
            presentation_path: PresentationPath::new(presentation_path)?,
            content,
            selected_arrangement: None,
            user_music_key: None,
        })
    }

    /// Attach exact selected-arrangement metadata.
    ///
    /// Embedded entries prove the UUID/name pair and complete group/cue
    /// traversal against their native bytes. Linked entries retain the exact
    /// reference because their target presentation is outside this boundary.
    pub fn with_selected_arrangement(
        mut self,
        selected_arrangement: Option<SelectedArrangement>,
    ) -> Result<Self, PlaylistEntryError> {
        if let (PlaylistEntryContent::Embedded(data), Some(selected)) =
            (&self.content, &selected_arrangement)
        {
            let presentation = decode_presentation_bytes(data, &self.name).map_err(|reason| {
                PlaylistEntryError::InvalidEmbeddedPresentation {
                    name: self.name.clone(),
                    reason,
                }
            })?;
            if !has_selectable_arrangement(&presentation, selected.uuid(), selected.name()) {
                return Err(PlaylistEntryError::EmbeddedArrangementUnavailable {
                    entry_name: self.name,
                    arrangement_uuid: *selected.uuid(),
                    arrangement_name: selected.name().to_string(),
                });
            }
        }
        self.selected_arrangement = selected_arrangement;
        Ok(self)
    }

    /// Attach a source-supplied music key.
    #[must_use]
    pub const fn with_user_music_key(
        mut self,
        user_music_key: Option<rv_data::MusicKeyScale>,
    ) -> Self {
        self.user_music_key = user_music_key;
        self
    }

    /// Convert this checked entry into a linked presentation reference.
    ///
    /// This drops only the embedded presentation bytes. The presentation path,
    /// operator-visible name, selected arrangement, and user music key retain
    /// their exact checked values.
    #[must_use]
    pub fn into_linked(mut self) -> Self {
        self.content = PlaylistEntryContent::Linked;
        self
    }

    /// Operator-visible playlist item name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Exact native presentation path.
    #[must_use]
    pub fn presentation_path(&self) -> &str {
        &self.presentation_path.value
    }

    pub(super) fn embedded_filename(&self) -> &str {
        &self.presentation_path.filename
    }

    /// Embedded native presentation bytes, when this is an embedded entry.
    #[must_use]
    pub fn embedded_data(&self) -> Option<&[u8]> {
        match &self.content {
            PlaylistEntryContent::Linked => None,
            PlaylistEntryContent::Embedded(data) => Some(data),
        }
    }

    /// Exact selected arrangement, when one was chosen.
    #[must_use]
    pub const fn selected_arrangement(&self) -> Option<&SelectedArrangement> {
        self.selected_arrangement.as_ref()
    }

    /// Source-supplied music key, when present.
    #[must_use]
    pub const fn user_music_key(&self) -> Option<&rv_data::MusicKeyScale> {
        self.user_music_key.as_ref()
    }
}

/// Errors raised while constructing a bundle with named playlists.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlaylistSetError {
    /// A native playlist document must contain at least one child playlist.
    #[error("playlist set must contain at least one playlist")]
    Empty,
    /// A child playlist needs an operator-visible name.
    #[error("playlist name cannot be empty")]
    EmptyName,
    /// A child name cannot contain padding or control characters.
    #[error("playlist name is not a valid exact identity")]
    InvalidName,
}

/// One named child in a multi-playlist bundle.
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
        if name.trim() != name || name.chars().any(char::is_control) {
            return Err(PlaylistSetError::InvalidName);
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
pub(super) struct PlaylistChild {
    pub(super) name: String,
    pub(super) entries: Range<usize>,
}

/// A checked one-level native playlist bundle.
///
/// This type is the single owner of the flattened order used by both the
/// protobuf document and embedded presentation members.
#[derive(Debug, Clone)]
pub struct PlaylistSet {
    pub(super) children: Vec<PlaylistChild>,
    pub(super) entries: Vec<PlaylistEntry>,
}

impl PlaylistSet {
    /// Build the common one-playlist package without exposing the intermediate
    /// child representation to callers.
    pub fn single(
        name: impl Into<String>,
        entries: Vec<PlaylistEntry>,
    ) -> Result<Self, PlaylistSetError> {
        Self::new(vec![NamedPlaylist::new(name, entries)?])
    }

    /// Normalize named playlists into canonical package order.
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
    /// Optional confined archive entry path.
    pub archive_path: Option<String>,
}

impl PlaylistMediaAsset {
    /// Use the canonical absolute source path as native archive identity.
    pub fn new(source_path: impl Into<PathBuf>) -> Self {
        Self {
            source_path: source_path.into(),
            archive_path: None,
        }
    }

    /// Resolve the exact member identity used by a portable native archive.
    ///
    /// Keeping this checked translation beside the package writer prevents
    /// workflow evidence from independently reimplementing `ProPresenter`'s
    /// native absolute-path naming rule.
    pub(crate) fn resolved_archive_path(&self) -> Result<String, PlaylistError> {
        media_archive_path(self)
    }

    /// Bind this reviewed archive identity to bytes captured at preview time.
    pub(crate) fn bind_reviewed<'a>(
        &self,
        data: &'a [u8],
    ) -> Result<ReviewedPlaylistMediaAsset<'a>, PlaylistError> {
        Ok(ReviewedPlaylistMediaAsset {
            archive_path: self.resolved_archive_path()?,
            data: Cow::Borrowed(data),
        })
    }
}

/// Portable media whose identity and bytes were bound during preview approval.
#[derive(Debug)]
pub struct ReviewedPlaylistMediaAsset<'a> {
    pub(super) archive_path: String,
    pub(super) data: Cow<'a, [u8]>,
}

/// Requested behavior when writing a playlist package.
///
/// This is intentionally distinct from
/// [`crate::propresenter::package::PlaylistArchiveShape`], which describes an
/// archive after it has been read. Portable imports always discover media
/// referenced by their embedded presentations; callers cannot accidentally
/// request a nominally portable package while disabling that work.
#[derive(Debug, Clone)]
pub enum PlaylistExportIntent {
    /// Write presentation links only, suitable for the current local library.
    LibraryLinks,
    /// Embed available presentations and every resolvable native media dependency.
    PortableImport {
        /// Additional media members beyond dependencies discovered from presentations.
        additional_media_assets: Vec<PlaylistMediaAsset>,
    },
}

impl PlaylistExportIntent {
    /// Construct a local-library link package.
    #[must_use]
    pub const fn library_links() -> Self {
        Self::LibraryLinks
    }

    /// Construct a portable import whose presentation dependencies are discovered.
    #[must_use]
    pub const fn portable_import(additional_media_assets: Vec<PlaylistMediaAsset>) -> Self {
        Self::PortableImport {
            additional_media_assets,
        }
    }

    /// Operator-facing mode represented by this complete export intent.
    #[must_use]
    pub const fn mode(&self) -> PlaylistExportMode {
        match self {
            Self::LibraryLinks => PlaylistExportMode::LibraryLinks,
            Self::PortableImport { .. } => PlaylistExportMode::PortableImport,
        }
    }

    /// Additional or discovered media owned by a portable import.
    #[must_use]
    pub fn media_assets(&self) -> &[PlaylistMediaAsset] {
        match self {
            Self::LibraryLinks => &[],
            Self::PortableImport {
                additional_media_assets,
            } => additional_media_assets,
        }
    }
}

impl Default for PlaylistExportIntent {
    fn default() -> Self {
        Self::portable_import(Vec::new())
    }
}

/// Export behavior whose portable media bytes were captured at review time.
#[derive(Clone, Copy)]
pub enum ReviewedPlaylistExportIntent<'a> {
    LibraryLinks,
    PortableImport(&'a [ReviewedPlaylistMediaAsset<'a>]),
}

/// Operator-facing playlist export behavior.
///
/// This is write intent, not an inference about an archive that has already
/// been read. The serialized names remain compatible with the existing MCP
/// and CLI contract.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    rmcp::schemars::JsonSchema,
)]
pub enum PlaylistExportMode {
    /// Reference presentations already installed in the selected library.
    #[serde(rename = "library_local", alias = "library_links")]
    LibraryLinks,
    /// Embed presentations and every reviewed, resolvable media dependency.
    #[default]
    #[serde(rename = "export_portable", alias = "portable_import")]
    PortableImport,
}
