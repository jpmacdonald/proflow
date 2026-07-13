# ProPresenter 1:1 Parity Goal Set

Date: 2026-05-08

This is the follow-on goal set after the first ProPresenter parity-hardening pass.
The intent is to stop approximating ProPresenter output and make ProFlow support
real playlist, presentation, media, theme, macro, and identity behavior directly
from the existing corpus of real files.

## Goal

Make ProFlow generate and inspect ProPresenter playlists the way ProPresenter
normally builds them, with stable linked presentations, reusable identities,
correct package shape, correct theme/macro behavior, and fixture coverage from
real local `.proplaylist` and `.pro` files.

## Workstreams

### 1. Expand The Real Corpus

Use the existing local ProPresenter library as the source of truth instead of
hand-picked examples.

Deliverables:

- catalog all available local `.proplaylist` and `.pro` files into a manifest
- include multiple Sundays, both 9am and 10:30am services, and common one-off
  service shapes
- tag fixtures by role: songs, scripture, responsive liturgy, confession,
  catechism, announcements, pre-service, post-service, nametags, manual-only
  sermon decks/placeholders, media-heavy files, arrangements
- track fixture source path, copied fixture path, cue counts, groups,
  arrangements, macros, themes, media dependencies, and package mode
- add a fixture refresh script that copies selected real files reproducibly

Acceptance:

- the corpus is large enough that one broken assumption shows up in tests
- every config rule we rely on has at least one real fixture exercising it

### 2. Preserve ProPresenter Data 1:1

Close the gap between semantic compatibility and true ProPresenter-compatible
serialization.

Deliverables:

- audit generated protobuf structs for unknown or currently ignored fields
- decide whether `prost` is sufficient or whether we need raw field preservation
  for selected messages
- compare archive entry names, entry order, compression, timestamps where useful,
  embedded data payloads, UUID fields, paths, arrangements, folders, and links
- add normalized comparison modes:
  - semantic compatibility
  - strict package shape
  - byte-sensitive diagnostics
- document which volatile fields are intentionally normalized

Acceptance:

- a generated package can be explained field-by-field against a real
  ProPresenter package
- differences are either fixed or explicitly documented as volatile/irrelevant

### 3. Fix Identity And Reuse Semantics

Treat generated presentations as shared library files, not duplicate same-named
copies.

Deliverables:

- formal identity policy for every generated type:
  - scripture
  - responsive readings
  - call to worship
  - confessions
  - catechism
  - announcement bundles
  - nametags
  - generated text slides
- reuse existing generated files by canonical output path before creating a new
  presentation
- ensure the same generated file is referenced by every playlist that needs it
- prevent duplicate same-name presentation files from being produced for adjacent
  services
- expose created/reused/edited identity decisions in preview, build, inspect,
  and MCP responses

Acceptance:

- editing one generated shared presentation updates both services that reference
  it in ProPresenter
- no duplicate exact-name generated files appear unless explicitly requested

### 4. Match Theme And Macro Behavior

Use macros as ProPresenter display toggles while keeping slide edit themes
understandable.

Deliverables:

- inspect real files to map macro UUIDs/names to expected slide themes
- ensure each slide type only applies the needed first-slide macro toggles
- use projector/editor theme styles matching the macro purpose:
  - lyrics macro -> lyrics theme
  - scripture/content macro -> content theme
  - nametag/title macro -> information projectors title theme
- verify highlight colors, responsive text packing, and title styles against
  real files
- add tests for first-slide-only macro application and repeated slide behavior

Acceptance:

- generated slides open in ProPresenter with the same editing expectations as
  manually built slides
- macro/theme mismatches are caught by fixture comparison or smoke inspection

### 5. Match ProPresenter Playlist Linking

Model playlist item links the way ProPresenter does.

Deliverables:

- preserve local root, storage type, relative paths, and absolute path fallback
  semantics
- support both library-local playlists and portable export packages
- preserve embedded presentation metadata needed for linked editing
- compare folder/library paths and package links in inspect output
- test duplicated service references to the same underlying presentation file

Acceptance:

- ProPresenter opens generated playlists with linked presentations, not isolated
  duplicate files
- playlist links survive across 9am/10:30am generated service sets

### 6. Complete Media Dependency Handling

Make portable exports and media-heavy presentations reliable.

Deliverables:

- discover media dependencies from actions, slide fills, text fills, notes,
  audio/video objects, thumbnails, and any remaining real-file fields
- resolve local paths from file URLs, relative paths, and ProPresenter root
  metadata
- report missing media as build warnings instead of silent omissions
- include discovered assets in portable exports when package mode requires it
- add media-heavy real fixtures to the corpus

Acceptance:

- media-heavy announcements and background slides package correctly
- missing media is visible in the build report and MCP response

### 7. Mine Real Services Into Config

Use the real corpus to harden config instead of relying on ad hoc fixes.

Deliverables:

- scan real plans and library files for recurring aliases, typos, and naming
  drift
- add exact config mappings for confirmed recurring cases
- keep uncertain cases as warnings rather than guesses
- produce a report of:
  - matched library files
  - missing library files
  - skipped service items
  - generated shared presentations
  - duplicate-risk names

Acceptance:

- the May service builds require config choices, not Rust changes
- unknown items are reported clearly after build

### 8. Add A Live MCP Parity Harness

Turn preview/build/inspect/compare into a repeatable operator workflow.

Deliverables:

- one command or MCP workflow that runs:
  - preview
  - build
  - inspect generated playlists
  - compare against fixture or baseline
  - report created/reused/edited/skipped/missing items
- JSON output suitable for regression fixtures
- classify sermon decks as expected manual additions, not missing generated
  output
- optional ProPresenter binary/app inspection if protobuf/package comparison is
  insufficient

Acceptance:

- we can rerun one command after config changes and know whether playlist parity
  improved or regressed
- any remaining differences are listed as concrete findings

## Definition Of Done

- expanded real fixture manifest exists and is covered by tests
- semantic and strict package comparisons pass for selected real services
- generated 9am and 10:30am playlists reuse shared generated presentations
- call to worship/responsive readings pack text like real ProPresenter slides
- macros and slide themes match the intended ProPresenter display/editing model
- portable exports include discovered media or report missing media
- MCP preview/build/inspect responses expose enough identity and package detail
  to debug without opening binary files first
- `cargo test`, `cargo clippy --all-targets -- -D warnings`, release build, and
  the parity harness all pass
