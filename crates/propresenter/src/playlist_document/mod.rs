//! Versioned, LLM-readable view of a native `ProPresenter` playlist.
//!
//! A `PlaylistDocument` is output-only: it describes what a playlist file
//! contains (ordered items, the presentation each one links to, the selected
//! arrangement, and a compact content summary) in a stable JSON shape with a
//! published schema. It carries no workflow policy and no raw protobuf, and it
//! cannot be turned back into a playlist file. See `docs/playlist-document.md`.
//!
//! Readers return a typed error when the playlist itself cannot be read and a
//! typed `PresentationContent::Unavailable` when one referenced presentation
//! cannot be read, so a missing file is never an omitted item.

mod model;
mod read;

pub use model::{
    ArrangementChoice, ArrangementSummary, CueGroupSummary, DocumentSource, ItemDetail,
    ItemProblem, NativeId, NumberRange, Playlist, PlaylistDocument, PlaylistItem,
    PresentationContent, PresentationItem, PresentationLocator, PresentationSummary, SchemaVersion,
    ScriptureReference, UnavailableReason, PLAYLIST_DOCUMENT_SCHEMA_VERSION,
};
pub use read::{
    playlist_document_from_package, read_library_playlist_document, read_package_playlist_document,
    PlaylistDocumentError,
};

/// JSON Schema of `PlaylistDocument`, as pretty-printed JSON.
///
/// The checked-in golden copy of this text is the schema's change control:
/// any change to the document's wire shape must update it deliberately.
#[must_use]
pub fn playlist_document_schema() -> schemars::Schema {
    schemars::schema_for!(PlaylistDocument)
}

#[cfg(test)]
mod tests;
