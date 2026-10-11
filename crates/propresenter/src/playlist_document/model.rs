//! Wire types of the playlist document.

use std::borrow::Cow;

use schemars::{json_schema, JsonSchema, Schema, SchemaGenerator};
use serde::Serialize;

/// Schema version written into every document.
///
/// Bump on any change to the wire shape that an existing reader could
/// misinterpret (removed or retyped field, changed meaning). Additive optional
/// fields do not require a bump.
pub const PLAYLIST_DOCUMENT_SCHEMA_VERSION: u32 = 1;

/// The schema version of a document, fixed to the current constant.
///
/// The field can only hold the version this build writes, so a document can
/// never claim a version the code did not produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct SchemaVersion(u32);

impl SchemaVersion {
    /// The version this build writes.
    #[must_use]
    pub const fn current() -> Self {
        Self(PLAYLIST_DOCUMENT_SCHEMA_VERSION)
    }

    /// The numeric version.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl JsonSchema for SchemaVersion {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("SchemaVersion")
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Playlist document schema version. Always the constant shown here.",
            "type": "integer",
            "const": PLAYLIST_DOCUMENT_SCHEMA_VERSION,
        })
    }
}

/// A UUID exactly as the native file stores it (uppercase, hyphenated).
///
/// Native identifiers are volatile across rewrites of a playlist; use them to
/// correlate within one document, not across documents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct NativeId(pub(super) String);

impl NativeId {
    /// The identifier text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A versioned description of one playlist file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[schemars(title = "ProPresenter playlist document")]
pub struct PlaylistDocument {
    /// Schema version of this document.
    pub schema_version: SchemaVersion,
    /// Where the playlist was read from.
    pub source: DocumentSource,
    /// Playlists in file order. A `.proplaylist` package usually holds one; the
    /// live library read selects one named playlist (and any playlists nested
    /// below it when the name is a folder).
    pub playlists: Vec<Playlist>,
}

/// Where a document's playlist data came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DocumentSource {
    /// An exported `.proplaylist` package; presentations are its embedded files.
    Package {
        /// File name of the package.
        file_name: String,
    },
    /// The live `Playlists/Library` document of a `ProPresenter` data root;
    /// presentations are read from the library files it links to.
    LiveLibrary {
        /// Name of the playlist (or folder) that was selected.
        selected: String,
    },
}

/// One playlist: an ordered list of items.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Playlist {
    /// Native playlist identifier, when the file records one.
    pub id: Option<NativeId>,
    /// Operator-visible playlist name.
    pub name: String,
    /// Names of the folders enclosing the playlist, outermost first, excluding
    /// the unnamed root.
    pub folders: Vec<String>,
    /// Items in operator order.
    pub items: Vec<PlaylistItem>,
}

/// One entry in a playlist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PlaylistItem {
    /// Zero-based position within its playlist.
    pub index: usize,
    /// Native item identifier, when the file records one.
    pub id: Option<NativeId>,
    /// Operator-visible item name (may differ from the presentation's name).
    pub name: String,
    /// Whether the operator has hidden the item.
    pub hidden: bool,
    /// What kind of item this is, with the fields that kind carries.
    #[serde(flatten)]
    pub detail: ItemDetail,
}

/// The kind of a playlist item and the data specific to that kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[allow(
    clippy::large_enum_variant,
    reason = "a document is built once and read once; boxing only the presentation variant would complicate every consumer match"
)]
pub enum ItemDetail {
    /// A presentation (slides) the operator can trigger.
    Presentation(PresentationItem),
    /// A section header; its name is the heading text.
    Header,
    /// A single cue stored directly in the playlist.
    Cue,
    /// An item linked to a Planning Center plan item.
    PlanningCenter,
    /// A placeholder for content that is not yet attached.
    Placeholder,
    /// The file records no item type.
    Untyped,
}

/// A presentation item: its source file, arrangement and content summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PresentationItem {
    /// The file the item links to.
    pub source: PresentationLocator,
    /// Which arrangement the item plays.
    pub arrangement: ArrangementChoice,
    /// What was found in the presentation, or why nothing could be read.
    pub content: PresentationContent,
    /// Disagreements between the item and the presentation it links to.
    pub problems: Vec<ItemProblem>,
}

/// How a presentation item locates its file, as recorded in the playlist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "locator", rename_all = "snake_case")]
pub enum PresentationLocator {
    /// Path relative to the `ProPresenter` data root (for example
    /// `Libraries/Default/Song.pro`).
    ShowRelative {
        /// The relative path.
        path: String,
    },
    /// Path relative to a user folder (home, documents, ...) rather than the
    /// data root.
    OtherRelative {
        /// The relative path.
        path: String,
    },
    /// Absolute path or `file://` URL on the machine that wrote the playlist.
    Absolute {
        /// The decoded absolute path.
        path: String,
    },
    /// Path on an external volume.
    External {
        /// The path.
        path: String,
    },
    /// The item records no file location.
    Unspecified,
}

/// The arrangement an item selects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "choice", rename_all = "snake_case")]
pub enum ArrangementChoice {
    /// No arrangement selected; the presentation plays in its own cue order.
    PresentationDefault,
    /// A specific arrangement of the presentation.
    Selected {
        /// Arrangement identifier inside the presentation.
        id: NativeId,
        /// Arrangement name recorded on the item (may be empty in the file).
        name: String,
    },
}

/// What could be read from a linked presentation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PresentationContent {
    /// The presentation was read.
    Available(PresentationSummary),
    /// The presentation could not be read; no content is known.
    Unavailable {
        /// Why it could not be read.
        reason: UnavailableReason,
    },
}

/// Why a linked presentation could not be summarized.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum UnavailableReason {
    /// The item records no usable file location.
    NoLocator,
    /// The package has no embedded file for the item.
    EmbeddedFileMissing {
        /// The file name the item links to.
        file_name: String,
    },
    /// Several embedded files match the item's file name ignoring case.
    EmbeddedFileAmbiguous {
        /// Every matching archive member.
        candidates: Vec<String>,
    },
    /// The linked library file does not exist.
    FileMissing {
        /// Where the file was expected, relative to the data root when known.
        path: String,
    },
    /// The linked file resolves outside the data root and was not read.
    FileOutsideRoot {
        /// The resolved location.
        path: String,
    },
    /// The file exists but could not be read.
    Unreadable {
        /// Operating-system error text.
        detail: String,
    },
    /// The file was read but is not a native `ProPresenter` presentation.
    NotAPresentation {
        /// Decoder error text.
        detail: String,
    },
}

/// A disagreement between a playlist item and its presentation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "problem", rename_all = "snake_case")]
pub enum ItemProblem {
    /// The item selects an arrangement the presentation does not define.
    ArrangementNotInPresentation {
        /// The selected arrangement identifier.
        id: NativeId,
    },
}

/// Compact content summary of one presentation. Slide text, styling and
/// layout are deliberately not included.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PresentationSummary {
    /// Presentation name stored in the file.
    pub name: String,
    /// Presentation identifier, when recorded.
    pub id: Option<NativeId>,
    /// Number of slides (cues).
    pub slide_count: usize,
    /// Scripture metadata attached to the presentation, when present.
    pub scripture: Option<ScriptureReference>,
    /// Cue groups (for example Verse 1, Chorus) in file order.
    pub cue_groups: Vec<CueGroupSummary>,
    /// Arrangements the presentation defines.
    pub arrangements: Vec<ArrangementSummary>,
    /// File names of background media used by the slides, first use first.
    pub media: Vec<String>,
    /// Broken or ambiguous internal references found in the presentation.
    pub structure_problems: Vec<crate::inspection::PresentationReferenceDiagnostic>,
}

/// A cue group and its slides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct CueGroupSummary {
    /// Group name.
    pub name: String,
    /// Number of slides in the group.
    pub slide_count: usize,
    /// Distinct slide labels in the group, in slide order.
    pub labels: Vec<String>,
}

/// An arrangement: an ordered selection of cue groups.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ArrangementSummary {
    /// Arrangement identifier, when recorded.
    pub id: Option<NativeId>,
    /// Arrangement name.
    pub name: String,
    /// Cue group names in play order (a group may repeat).
    pub groups: Vec<String>,
}

/// Scripture metadata of a presentation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ScriptureReference {
    /// Book name as shown to the operator.
    pub book: String,
    /// Inclusive chapter range, when recorded.
    pub chapters: Option<NumberRange>,
    /// Inclusive verse range, when recorded.
    pub verses: Option<NumberRange>,
    /// Translation abbreviation shown to the operator.
    pub translation: String,
}

/// Inclusive integer range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
pub struct NumberRange {
    /// First value.
    pub start: i32,
    /// Last value.
    pub end: i32,
}
