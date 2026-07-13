# ProFlow

ProFlow turns a Planning Center service plan into deterministic ProPresenter
presentations and playlists. The runtime is configured per church, and MCP is
the primary operator interface.

The central workflow is:

```text
Planning Center plan
    -> v4 rules classify each item
    -> resolved / needs-review / skip
    -> preview resolved build decisions and source paths
    -> operator resolves uncertainty and confirms the preview
    -> render/reuse presentations and export the playlist
```

There is no hidden runtime inference. If an item, background, theme slide,
macro, or arrangement cannot be resolved, ProFlow reports the problem instead
of guessing.

## Why MCP, Not A Ratatui App

The workflow needs judgment while onboarding a church and reviewing uncertain
plan items, but presentation generation itself must remain deterministic. MCP
fits that split: an assistant can inspect assets, explain matches, and present a
preview while the Rust core owns classification and output.

A Ratatui client would duplicate the review flow, config editor, and state
management without improving the engine. If a terminal UI is useful later, it
should be a thin client over the same preview/build boundary. The CLI binaries
in this repository are development and diagnostic entrypoints, not a second
product architecture.

## Requirements

- macOS
- Rust toolchain
- Planning Center Services API credentials
- access to the target ProPresenter library
- a ProPresenter theme and macros for any configured cue roles

## Environment

ProFlow reads environment variables directly and from `.env`:

```bash
PCO_APP_ID=your_app_id
PCO_SECRET=your_secret
LIBRARY_DIR=~/Documents/ProPresenter/Libraries/Default
PROPRESENTER_DIR=~/Documents/ProPresenter
THEMES_DIR=~/Documents/ProPresenter/Themes
PROFLOW_DATA=/absolute/path/to/a/proflow-data-bundle
```

Important variables:

- `PCO_APP_ID` and `PCO_SECRET`: Planning Center credentials.
- `LIBRARY_DIR`: target ProPresenter library. When omitted, ProFlow tries the
  default ProPresenter library path.
- `PROPRESENTER_DIR`: optional ProPresenter user-data directory override. This
  is not the installed application bundle; theme and macro discovery use it.
- `THEMES_DIR`: optional direct override for the installed Themes directory.
- `PLAYLIST_DIR`: optional playlist output directory.
- `GENERATED_PRESENTATIONS_DIR`: optional generated-presentation directory.
- `PROFLOW_DATA`: optional project data root.

## The Project Data Bundle

One directory is the canonical local project snapshot:

```text
$PROFLOW_DATA/
├── proflow.config.json
├── backgrounds/
│   ├── default.png
│   └── communion.jpg
└── bibles/
```

If `PROFLOW_DATA` is set, it is authoritative even when it is incomplete. If it
is not set, development prefers the repository's `data/` directory; installed
builds use the first available application data bundle. ProFlow selects one
whole bundle and does not silently combine config, backgrounds, Bibles, or
other data from different installations.

This layout is appropriate for reuse because the config stores background paths
relative to the bundle. Config, backgrounds, and Bibles can move together.
ProPresenter theme slides and macros are not embedded in this bundle: they are
dependencies installed on the target workstation.
Standardized workstations can share one config directly; heterogeneous
workstations need the same named assets or separately reviewed machine-specific
config. Startup and config writes validate those dependencies on the current
machine.

The weekly product targets one configured workstation. Exact-name startup
validation and immutable in-process theme/macro caches are the current
reproducibility boundary. Do not add a second registry, database, or asset-lock
configuration source without a demonstrated multi-workstation requirement.

## Config v4

Version 4 is the only supported project-config schema. A representative core
configuration looks like this:

```json
{
  "version": 4,
  "metadata": {
    "name": "Example Church",
    "timezone": "America/New_York"
  },
  "defaults": {
    "theme": "Example Church Theme",
    "background": "default",
    "days_ahead": 14,
    "bible_version": "NRSVue",
    "presentation_size": { "width": 1920, "height": 1080 }
  },
  "backgrounds": {
    "default": "backgrounds/default.png",
    "communion": "backgrounds/communion.jpg"
  },
  "service_groups": {
    "weekly": { "service_types": ["Sunday Morning"] }
  },
  "required_playlist_items": [
    {
      "id": "pre_post_service",
      "use_type": "static_graphic",
      "library_file": "Pre-Service, Post-Service Slides.pro",
      "placement": "start",
      "service_group": "weekly"
    }
  ],
  "cue_roles": {
    "title": {
      "slide": "Information (Projectors)",
      "enter_macro": "Name Tag/Title"
    },
    "scripture_prayer": {
      "slide": "Scripture (Projectors)",
      "enter_macro": "Scripture/Prayer"
    },
    "responsive_scripture_prayer": {
      "slide": "Scripture (Projectors) (Responsive)",
      "enter_macro": "Scripture/Prayer",
      "all_content_colored_macro": "Scripture/Prayer (Highlighted)"
    }
  },
  "presentation_types": {
    "static_graphic": {
      "kind": "graphic",
      "content_source": "static",
      "output_strategy": "use_existing"
    },
    "song": {
      "kind": "song",
      "content_source": "song",
      "output_strategy": "use_existing"
    },
    "weekly_liturgy": {
      "kind": "liturgy",
      "content_source": "description",
      "description_parser": "liturgical",
      "output_strategy": "edit_in_place",
      "display": {
        "kind": "split",
        "title": "title",
        "content": "responsive_scripture_prayer"
      },
      "max_lines_per_slide": 8
    },
    "scripture": {
      "kind": "scripture",
      "content_source": "scripture",
      "output_strategy": "generate_new",
      "display": {
        "kind": "split",
        "title": "title",
        "content": "scripture_prayer"
      }
    }
  },
  "item_rules": [
    {
      "id": "all_songs",
      "match": { "category": "song" },
      "use_type": "song"
    },
    {
      "id": "scripture",
      "match": {
        "title_prefix": ["scripture"]
      },
      "use_type": "scripture"
    }
  ],
  "overrides": [
    {
      "when": { "service_type": "Christmas Eve" },
      "background": "communion"
    }
  ]
}
```

The complete top-level contract contains:

- `metadata`: descriptive project information.
- `defaults`: default theme, background ID, lookahead window, expected
  presentation size, and optional Bible translation for scripture items that
  do not name one. Without an explicit item translation or
  `defaults.bible_version`, scripture requires review.
- `service_groups`: reusable sets of Planning Center service types.
- `required_playlist_items`: exact reusable presentations that must occur once
  at the configured start or end of each matching service group. If Planning
  Center already names the same presentation elsewhere, the required policy
  owns it and moves it to the configured edge.
- `backgrounds`: named, project-relative image files.
- `cue_roles`: semantic cue regions bound to exact ProPresenter assets.
- `presentation_types`: content source, output strategy, display binding, and
  optional style values.
- `item_rules`: ordered classification rules with exactly one outcome each;
  manual-only material is an ordinary explicit `skip` rule placed before
  broader matches.
- `people`: known-person and nametag metadata.
- `overrides`: service-group, service-type, or presentation-type style
  overrides.

### What Belongs In Config

`proflow.config.json` remains the durable church policy. It should contain
facts expected to survive many service weeks: service groups, semantic cue
roles, installed macro names, reusable backgrounds, recurring item rules,
required playlist items, and exact identities for canonical static files or
edit-in-place slots.

An exact `library_file` is intentional for a stable identity such as
`Apostles Creed.pro`, `Call to Worship.pro`, or a person nametag. It is too
specific when it encodes one week's passage or a temporary musical choice.
Those choices belong in the reviewed preview override, which can select an
exact file for that build without teaching the base config a historical
exception. Unknowns and weak matches remain `needs_review`; ProFlow does not
grow filename rules merely to avoid asking a human once.

The live config therefore uses ordinary scripture generation as its durable
rule. The former Jonah 4 exact-file exception was removed. A reviewed operator
can still select an existing Jonah presentation for a particular build.

`examples/starter-config.json` is intentionally asset-neutral. It can start
before church-specific theme slides and macros have been chosen; generation
stays in `needs_review` until those bindings are explicit.

## Supplying Backgrounds

Backgrounds have two parts: a stable ID in config and an image inside the data
bundle.

```json
{
  "defaults": { "background": "default" },
  "backgrounds": {
    "default": "backgrounds/default.png",
    "sermon_series": "backgrounds/sermon-series.jpg"
  }
}
```

Use the ID, never a raw path, from `presentation_types`, config `overrides`, or
a one-off build override:

```json
{
  "kind": "scripture",
  "content_source": "scripture",
  "output_strategy": "generate_new",
  "display": { "kind": "single", "role": "scripture_prayer" },
  "background": "sermon_series"
}
```

Background precedence is:

```text
project default < presentation type < matching config override < one-off build override
```

The plan resolves the chosen ID to one relative file path before execution, so
a second config lookup cannot change the policy decision. Preview binds the
canonical image identity and its bytes. Rendering derives image metadata from
those reviewed bytes and never follows the configured path or symlink again.
The canonical source is still reverified before commit; a changed source aborts
the reviewed build.

Background IDs are lowercase ASCII identifiers and may also contain digits,
`_`, and `-`. Paths must be normal relative paths with a `.jpg`, `.jpeg`,
`.png`, `.tif`, or `.tiff` extension.

At startup and execution, a configured image must:

- remain inside the canonical data root, including after symlink resolution;
- be a regular, non-empty file;
- have an image signature matching its extension.

A missing, empty, escaped, or mislabeled image is an error. ProFlow does not
fall back to a similarly named file.

When a reviewed plan uses a managed background, MCP and the direct build CLI
default to a portable playlist package so the exact reviewed image bytes are
included. `--library-local` is an explicit diagnostic choice for a package
that depends on files already installed at their absolute local paths. This is
why merely pointing at an image in config is not sufficient for a package that
must move between workstations.

## Choosing Slides And Macros

A `cue_role` is the one reusable display contract:

```json
"scripture_prayer": {
  "slide": "Scripture (Projectors)",
  "enter_macro": "Scripture/Prayer",
  "all_content_colored_macro": "Scripture/Prayer (Highlighted)"
}
```

- `slide` is the exact theme-slide name used to render that region.
- `enter_macro` is an optional exact installed macro name and runs when the
  operator enters the region.
- `all_content_colored_macro` is an optional alternate selected when every
  generated content segment is colored. It requires `enter_macro`.

A configured role slide must contain exactly one text-bearing graphics element
and no embedded theme actions. Multiple text targets are ambiguous, while
silently inheriting a theme action would make macro and media behavior invisible
in config. Both conditions fail validation; cue actions must be named explicitly
through the role and background contracts.

A presentation type then chooses how roles map to cues.

Single-role display:

```json
"display": {
  "kind": "single",
  "role": "scripture_prayer"
}
```

Every generated cue uses that role's theme slide. Its macro is attached only to
the first operator-visible cue.

Split display:

```json
"display": {
  "kind": "split",
  "title": "title",
  "content": "responsive_scripture_prayer"
}
```

The title cue uses the `title` role; subsequent content cues use the `content`
role. A macro is attached to each cue where its actual rendered role begins.
That is normally one title and one content entry; a combined scripture reading
gets a fresh title/content transition for each passage. If rendering produces
no title cue, no title macro is attached and the content macro starts on cue
one. Macros represent region-entry state transitions, so they are not copied
onto every slide in a region.

For newly rendered or edited presentations, the background is attached to the
first operator-visible cue and macros mark the first cue entering each role.
Those presentations do not accept an arrangement setting.

Within a cue, the serialized action order is the rendered slide, then its role
macro when that cue enters a role, then the background media action when that is
the first operator-visible cue. This is ProFlow's canonical generated order.
ProPresenter may retain a different order after manual edits, so action
semantics—not historical edit order—remain the stable contract.

`defaults.presentation_size` is the project output invariant, normally
1920×1080. Cue-role theme slides are checked against it at startup, every
generated presentation is checked after rendering, and every selected existing
presentation is audited during preview. A legacy 1280×720 file is not silently
resized because changing only its slide bounds does not reproduce
ProPresenter's UI behavior. Preview instead asks the operator to set the
expected output first and then reapply the theme, in that order, before the
file can be approved again.

`arrangement` is only valid for `use_existing`: it selects a named arrangement
from a read-only library presentation when that file is placed in the playlist.
For songs, a presentation-type setting or matching service override wins;
otherwise ProFlow uses the nonempty arrangement supplied by Planning Center.
If none is supplied, the playlist item carries no selected arrangement. Once a
song file is confidently matched, classification requires an exact,
case-insensitive, unique native arrangement name with a valid UUID and retains
the native casing. Missing, duplicate, or incomplete metadata requires human
review and lists the available names instead of silently selecting a different
arrangement. Because the existing file is not restyled, its arrangement does
not participate in generated background or macro placement.

## Scripture Packing And Labels

Scripture rendering keeps the source verse number beside every text fragment.
It greedily fills each slide to the configured or template-derived line budget,
preferring punctuation boundaries (`; , . ? ! : —`) on the last usable visual
line and falling back to the latest word boundary when a sentence has no useful
break. Every produced slide satisfies the same estimated geometry bound used to
pack it; source character order and verse provenance are regression-tested.

Only scripture content cues receive native slide-action labels. Labels include
book, chapter, and the exact verses represented on that slide, for example
`Ephesians 4:4-6`; a continuation keeps the source verse label. Passage-title
and blank-divider cues remain unlabeled, and combined readings use the correct
book/chapter prefix for each passage.

The line model intentionally remains an approximation based on text-box bounds
and font metrics rather than a second font-shaping engine. ProFlow intentionally
does not synthesize ProPresenter Bible-UI metadata: scripture identity is owned
by the reviewed source request, native cue labels, preserved verse provenance,
and the rendered superscript verse numbers.

## File Naming And Rendering Ownership

Naming is deliberately conventional at the human boundary and deterministic at
the generated-file boundary; filenames are not a second configuration system.

- Existing songs use the canonical native song title. `[Hymn]`, `[Anthem]`,
  and `[Youth Choir]` are optional role hints; a hymn number is matching
  metadata, and an arranger/tune suffix denotes a genuinely distinct version.
  Untargeted songs are matched from Planning Center title data. A tie or merely
  plausible match requires review.
- Generated scripture uses `<Reference> <Translation>.pro`, with a verse colon
  converted to `v`, for example `Ephesians 4v4-6 NRSVue.pro`. It does not add a
  date, speaker, or `Scripture -` prefix.
- A recurring mutable role such as Call to Worship uses one exact canonical
  edit target. A true one-off generated item uses its normalized Planning
  Center title. Normalization collisions require review rather than an invented
  `(2)` suffix.

Rendered files use a small hybrid boundary. Installed theme slides supply the
visual slide—geometry, text box, font, color, and layout. ProFlow constructs the
fresh native Presentation/Cue/Action/Group protobuf envelope, replaces the text,
and adds explicit configured macros and backgrounds. Checked-in files under
`data/templates` are test fixtures only. Songs and other `use_existing` assets
are not regenerated or restyled; their approved native bytes, groups, and
arrangements are preserved.

## Song Groups And Macro Definitions

Existing song presentations remain byte-preserved, including their cue groups,
repeated arrangement group order, and macro actions. ProFlow does not currently
create or rewrite song groups: the unused generic repair routine lacks the
installed group's color, hotkey, and application-group binding and is therefore
not part of the production workflow.

Creating songs safely needs one deliberately reviewed native fixture containing
every group type the church wants plus two arrangements with repeated groups.
At that point a small installed-group catalog can copy exact local metadata and
require explicit section assignments; unknown section names should require
review rather than be guessed from lyrics.

Macro definitions remain owned by ProPresenter. Config references exact
installed names, and presentations contain only native macro references. The
asset catalog now reports each installed macro's actions in native execution
order—including Stage Layout, Audience Look, and Clear Group targets—so an
operator can inspect behavior without ProFlow duplicating macro authoring.

## Output Strategies

| Strategy | Presentation behavior | Style contract |
|---|---|---|
| `use_existing` | Reuse a library presentation | Existing visuals are read-only. `display`, `background`, and `max_lines_per_slide` are invalid; an arrangement may be selected. |
| `edit_in_place` | Rebuild weekly content into an existing target | `display` is required, so rendering always uses an explicit installed-theme cue role. Background and line limit may be set. Arrangements are invalid. |
| `generate_new` | Create a new presentation | `display` is required. Background and line limit may be set. Arrangements are invalid. |
| `skip` | Produce no output | No rendering occurs. |
| `needs_review` | Stop automatic resolution | The operator must choose an explicit outcome. |

This separation is intentional: a `use_existing` song cannot appear to accept
a macro or background that the runtime would ignore.

## Fail-Fast Config And Asset Validation

Config parsing rejects versions other than 4, unknown fields, invalid IDs and
paths, contradictory rule outcomes, unknown background or cue-role references,
and invalid output-strategy/style combinations.

Installed macro and theme-slide names are exact, case-sensitive contracts.
ProFlow rejects installed names that differ only by case because such assets
would be ambiguous to humans and across filesystems.

The MCP server then loads one immutable runtime snapshot and verifies every
configured cue-role slide, macro, and background against the local
ProPresenter/data assets. If activation writes a new live config, restart the
server so config, theme cache, macro cache, and file index all come from the same
snapshot.

## MCP Server

Run the stdio server with:

```bash
cargo run --bin proflow_mcp
```

For a repository-local MCP client configuration:

```bash
./tools/setup_mcp.sh
```

The generated `.mcp.json` is ignored by Git. Keep Planning Center credentials
in `.env` or the MCP host environment.

### The 8-Tool Surface

Discovery and config:

- `catalog_assets`
- `show_effective_config`
- `write_project_config`

Plan inspection:

- `fetch_plan`
- `preview_playlist`
- `explain_rule_match`
- `search_library`

Production build:

- `build_service`

Setup tools return facts; they do not infer runtime configuration. The former
analysis/draft/suggest/patch tools were removed, as were single-presentation and
raw playlist/package build bypasses. `write_project_config` accepts a complete,
reviewed v4 document. Before it writes either a candidate or live file—and
before it creates a backup—it reloads the configured theme and installed macros
and validates every cue-role slide, macro, and background. Diagnostic package
inspection remains available through developer CLI binaries outside the MCP
operator surface.

### Onboarding A Project

1. Put the asset-neutral starter config at
   `$PROFLOW_DATA/proflow.config.json` and start the MCP server.
2. Run `catalog_assets`. If the starter has no theme yet, pass the exact intended
   `theme_name`; the tool loads that theme only for this discovery call and does
   not mutate the running snapshot. Review its exact slide names, installed
   macros, configured backgrounds, and library files.
3. Run `fetch_plan` for representative services and inspect the real titles,
   categories, scripture metadata, speakers, and service-type names.
4. Author one complete v4 config from those facts. Give each reusable image a
   background ID and each semantic cue region a cue role.
5. Call `write_project_config` with `activate: false`. Asset validation happens
   before the candidate is written; then review the complete candidate.
6. Call `write_project_config` with the reviewed document and `activate: true`.
   A live replacement creates a timestamped backup.
7. Restart the MCP server, then use `show_effective_config` and
   `preview_playlist` on representative plans.
8. Refine explicit rules until every intended item resolves cleanly.

### Weekly Operation

1. `fetch_plan`
2. `preview_playlist`, including any proposed playlist name, skips, overrides,
   package mode, or explicit media.
3. Review every entry. Use `explain_rule_match` and `search_library` for any
   uncertainty.
4. Confirm the exact preview with the user.
5. Call `build_service` with only the plan identity and returned
   `preview_revision`. If any output choice changes, preview again.

`preview_playlist` resolves the plan title, date, and service type from Planning
Center; an optional caller-supplied `service_name` is only an exact assertion
and a mismatch is rejected. `build_service` atomically consumes a matching
revision before it performs build side effects. A revision is therefore
one-time even when the build fails: run `preview_playlist` again before every
retry. Missing, stale, and mismatched revisions are rejected without consuming
the current preview. The revision privately owns the final playlist identity,
effective skips/overrides, package mode, media set, classified plans, and source
snapshot; `build_service` cannot add replacements afterward. Approved
presentation, Bible, background, and portable-media bytes are carried into
rendering and packaging. Every presentation target and the final playlist are
also reviewed as exactly present-with-bytes or absent; a file that appears,
disappears, or changes before staging is not overwritten. Sources and targets
are checked again before commit, so drift requires a new preview. Use the stable
`output_key` from preview results for skips and overrides. Keys are derived from
the Planning Center item ID plus the expansion step, never its mutable service
position.

The filesystem does not offer a portable compare-and-rename primitive. ProFlow
rechecks each target immediately before replacement and rolls back its own
installed prefix, but another writer can still race in the final check/rename
interval. Do not build while ProPresenter or another process is writing the same
library. A ProFlow-only advisory lock would not be honored by ProPresenter and
would not close that external race.

## Diagnostic CLI

The direct build CLI exists for local debugging. The supported weekly operator
flow is MCP preview followed by the one-time reviewed build revision.

```bash
cargo run --bin build_service -- \
  <plan_id> <service_name> [playlist_name] \
  [--skip <output_key> ...] \
  [--decisions decisions.json] \
  [--portable | --library-local]
```

Decision files use registered background IDs, not categories or file paths:

```json
{
  "skip_output_keys": ["pco:12345:main"],
  "overrides": [
    {
      "output_key": "pco:12346:main",
      "background": "communion"
    },
    {
      "output_key": "pco:12347:main",
      "arrangement": "Christmas Eve"
    },
    {
      "output_key": "pco:12348:main",
      "action": "use_existing",
      "file_path": "/reviewed/library/Jonah 4.pro",
      "playlist_name": "Jonah 4",
      "slide_type": "scripture"
    }
  ]
}
```

The background example applies to a rendered entry; the arrangement example
applies to a `use_existing` entry. The final example is a weekly, reviewed
selection of an existing scripture file; it does not belong in durable config.
The CLI rejects an override whose background ID is absent from `backgrounds`;
execution then validates the resolved file with the same rules as MCP builds.

## Architecture

The code follows one directional boundary:

```text
Planning Center IO
    -> project_config + workflow classification
    -> resolved plan decisions and source paths
    -> ProPresenter render/reuse
    -> validation + playlist/package export
```

- `src/project_config.rs` owns the single v4 contract and semantic validation.
- `src/workflow/` owns classification, preview state, and execution.
- `src/propresenter/` owns ProPresenter parsing, rendering, arrangements,
  macros, backgrounds, and serialization.
- `src/setup/` reports installed and configured asset facts; it does not analyze
  plans or author hidden runtime behavior.
- `src/mcp/` is a thin operator adapter over config and workflow.
- `src/planning_center/` owns Planning Center API behavior.

The important state has one owner: the project config names reusable policy and
assets, the reviewed plan freezes classification decisions, source bytes,
canonical backgrounds, portable-media bytes, and present/absent output state,
and the executor owns file side effects. Rendering and packaging consume those
reviewed bytes directly. Host layers do not maintain a shadow copy of runtime
state.

Native producer metadata has one source as well. At startup, ProFlow reads the
current `Playlists/Library` document associated with `LIBRARY_DIR`. New playlist
documents and newly saved presentations receive that current application and
platform metadata; an older theme file is never treated as the producer.

## Native Fidelity Contract

There are two distinct promises:

1. Decoding and re-encoding a supported native protobuf document must reproduce
   its bytes exactly. Unknown schema fields cannot be preserved by `prost`, so
   checked-in and optional live-corpus round-trip tests act as schema-drift
   alarms.
2. New playlist packages must reproduce the evidenced native structure. The
   playlist document and embedded presentation entries are cross-validated;
   archive member identity comes from the source presentation path, independently
   of its display label; members are stored in global lexicographic order using
   the forced ZIP64 shape emitted by current ProPresenter exports.

Native exports leave ZIP's UTF-8 filename flag unset even when member-name bytes
are valid UTF-8. The writer reproduces that envelope, while the reader prefers
valid raw UTF-8 bytes before falling back to ZIP's legacy filename decoding.

Repeated playlist items that reference the same source share one embedded
presentation member, matching native packages. Selected arrangements carry both
their UUID and exact native display name. Portable media discovery traverses
cue actions, prop slides, nested playback-marker actions, chord charts, and both
legacy and v2 timeline actions; the reviewed workflow packages captured bytes,
not a second read of live media paths.

Nested playlist exports use the same `.proplaylist` package format as a single
playlist. `PlaylistSet` owns one or more checked `NamedPlaylist` children and a
single canonical flattened presentation order, so the protobuf document and ZIP
members cannot be supplied in conflicting orders. Shared presentations are
deduplicated across children; the new Desktop golden demonstrates 36 references
to 26 embedded `.pro` files.

The reconstructed April playlist fixtures are labelled as ProFlow
materializations, not independent native exports. They remain diagnostic and
expose legacy defects such as incomplete arrangement metadata and pre-native ZIP
ordering; they are not counted as parity proof. `data/test.proplaylist` is the
checked-in independent native package used to prove reconstruction compatibility,
and the parity smoke harness must reconstruct it successfully.
Portable media packaging uses the canonical absolute source path observed in
native exports and rejects unresolved dependencies. Relocatable import behavior
remains experimental until an import/save-back round trip in ProPresenter proves
that URL contract. Native exports also show that older presentations may retain
Windows or another user's absolute URL while the package rebases the media entry
by filename. ProFlow does not guess that mapping: faithful portable export of
such presentations needs an explicit, uniquely reviewed media catalog/relink
contract.

Current native exports deliberately use a nonstandard forced-ZIP64 envelope.
The writer reproduces it for ProPresenter fidelity; some generic ZIP tools warn
about the extra 98 bytes or reject the container. The Rust package reader and
ProPresenter accept the evidenced shape.

## Verification

Use the repository's verification interface:

```bash
just local
just ci
just deep
just parity
just parity-corpus ~/Desktop
just pco-smoke
```

Use the narrowest relevant command while debugging, then rerun the enclosing
`just` target. Invariant-heavy changes should finish with `just deep`; the
focused `just parity` gate covers byte-exact codecs and independent native
package reconstruction. `just parity-corpus <directory>` additionally audits a
local, read-only directory of independent exports; it is intentionally not part
of `just deep` because that corpus is machine-specific. `just pco-smoke` is the
explicit live-network gate: it runs the Planning Center integration tests
serially and requires valid credentials, while the deterministic gates only
compile those tests.

## License

MIT License. See `LICENSE`.
