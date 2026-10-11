# Playlist document

A versioned, read-only JSON description of one existing ProPresenter playlist,
meant to be read by an LLM. This is the read side only. Nothing accepts a
document as build input; whether the document becomes an authoring format is
undecided (see "Next steps").

## What it is

`PlaylistDocument` (crate `proflow-propresenter`, module `playlist_document`):

```text
schema_version   always PLAYLIST_DOCUMENT_SCHEMA_VERSION (1); a newtype that can hold no other value
source           package { file_name } | live_library { selected }
playlists[]      id, name, folders[], items[]
  items[]        index, id, name, hidden, kind + the fields of that kind
```

`kind` is an enum: `presentation`, `header`, `cue`, `planning_center`,
`placeholder`, `untyped`. Only `presentation` carries more:

- `source`: how the file recorded the link (`show_relative`, `other_relative`,
  `external`, `absolute`, `unspecified`), with the decoded path.
- `arrangement`: `presentation_default` or `selected { id, name }`.
- `content`: `available { name, id, slide_count, scripture, cue_groups[],
  arrangements[], media[], structure_problems[] }` or `unavailable { reason }`.
- `problems[]`: disagreements between item and presentation (a selected
  arrangement the presentation does not define).

Cue groups carry name, slide count and distinct slide labels. Scripture carries
book, chapter range, verse range and translation. Media is the list of
background file names.

The schema is generated from these types with schemars and checked in at
`crates/propresenter/tests/fixtures/playlist-document.schema.json`. A test
fails when the generated schema differs from that file, so a shape change is a
reviewed diff. Bump the version for any change an existing reader could
misread (removed or retyped field, changed meaning); additive optional fields
need no bump. Regenerate with
`PROFLOW_BLESS_PLAYLIST_DOCUMENT_SCHEMA=1 cargo test -p proflow-propresenter --lib playlist_document`.

## Ownership

The document describes a native playlist, so the types and readers live in the
format crate. They depend on the existing package reader, presentation
inspection and native URL resolution, and on nothing from workflow or project
policy. Facts the format cannot know (classification status, reasons, Planning
Center keys, output keys) are a separate layer owned by the host crate; that
layer would wrap or extend this document and the format crate would not learn
of it.

The type is named `PlaylistDocument` like the raw protobuf message, which is
reachable only through `unstable_native::rv_data`; the two do not meet in the
public API.

## Reading

- `read_package_playlist_document(path)`: a `.proplaylist` package. Each item
  resolves to the embedded `.pro` named by its link (exact name, then a unique
  case-insensitive match).
- `read_library_playlist_document(root, name)`: one named playlist or folder
  from `<root>/Playlists/Library`. Each item resolves through the native URL
  locator (show-relative first) to a file that must canonicalize inside `root`.
  A link outside the root, including through a symlink, is not read.

Failure handling follows rule 5. If the playlist itself cannot be read or the
name does not select exactly one node, the call returns a typed
`PlaylistDocumentError`. If one linked presentation cannot be read, the item
stays in the document with `content: unavailable` and a typed reason
(`no_locator`, `embedded_file_missing`, `embedded_file_ambiguous`,
`file_missing`, `file_outside_root`, `unreadable`, `not_a_presentation`). A
package whose embedded `.pro` is invalid is rejected by the existing package
reader as a whole.

## Deliberately omitted

Pixel-level styling, fonts, colors and layout; slide text; raw protobuf; ZIP
metadata; music keys, tags and content destinations; the stale absolute path
when a relative one exists. Slide text is the obvious candidate for an opt-in
later (a hymn's full lyrics would dominate the document, so it is not
default).

## Relation to `PreviewEntry`

`PreviewEntry` (`src/workflow/classify_preview.rs`) describes a proposal:
what the classifier decided for each Planning Center item, with free-string
fields and no version. The document describes a playlist file. They share no
type. Overlap is conceptual: item name, source path, arrangement. A future
preview would carry the proposed playlist in document form plus a host-owned
section keyed by item index holding the workflow facts.

## MCP

- `playlist_document_schema`: returns the JSON Schema.
- `read_playlist { name | path }`: exactly one of `name` (a playlist or folder
  in the configured ProPresenter data root's live library) or `path` (a
  `.proplaylist` file inside the configured playlist output directory or data
  root; relative paths are taken from the playlist output directory). Both,
  neither, an escaping path or an unknown name are `invalid_params`.

## Next steps

- `preview_playlist` does not embed a document yet. A preview's items are
  mostly generated presentations that exist only as sealed in-memory bytes
  (rendering needs macOS), so a document of the proposed playlist would need
  either a reader over sealed bytes or a mapping from the preview, and the
  response gains a new top-level shape. Stating only the schema version in the
  response would announce a document that is not there, so it is not done.
- Decide whether the document is an authoring format. If so, the write side
  needs a deserializable input type (fields that must be present are already
  non-optional), an item-to-entry mapping, and Mac verification.
- Optional slide text, behind an argument.
