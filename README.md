# ProFlow

ProFlow turns a Planning Center service plan into deterministic ProPresenter
presentations and playlists. The runtime is configured per church, and MCP is
the primary operator interface.

The central workflow is:

```text
Planning Center plan
    -> v4 rules classify each item
    -> resolved / needs-review / skip
    -> materialize exact native artifacts into a sealed preview transaction
    -> operator resolves uncertainty and confirms the preview
    -> revalidate sources, outputs, and staged bytes, then commit playlist-last
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
PROPRESENTER_DIR=~/Documents/ProPresenter
THEMES_DIR=~/Documents/ProPresenter/Themes
PLAYLIST_DIR=~/Documents/ProPresenter/Playlists/ProFlow
PROFLOW_DATA=/absolute/path/to/a/proflow-data-bundle
```

Important variables:

- `PCO_APP_ID` and `PCO_SECRET`: Planning Center credentials.
- `PROPRESENTER_DIR`: optional ProPresenter user-data directory override. This
  is the active show workspace, not the installed application bundle. On macOS
  ProFlow also reads ProPresenter's `applicationShowDirectory` preference and
  rejects a conflicting override.
- `THEMES_DIR`: optional direct override for the installed Themes directory.
- `PLAYLIST_DIR`: optional playlist output directory. When omitted, playlist
  packages go to `<PROPRESENTER_DIR>/Playlists/ProFlow`, outside the
  presentation library.
- `PROFLOW_DATA`: optional project data root.

The exact registered presentation library is durable project policy at
`defaults.library` in `proflow.config.json`. ProFlow reads and writes canonical
presentation names in `<PROPRESENTER_DIR>/Libraries/<library>`. A safe copied
show should therefore be active during generation and QA; a live Dropbox show
remains untouched until an approved portable playlist is imported.

## The Project Data Bundle

One directory is the canonical local project snapshot:

```text
$PROFLOW_DATA/
├── proflow.config.json
├── backgrounds/
│   ├── default.png
│   └── communion.jpg
└── bibles/
    ├── NRSVUE.json
    └── ...
```

If `PROFLOW_DATA` is set, it is authoritative even when it is incomplete. If it
is not set, development prefers the repository's `data/` directory; installed
builds use the first available application data bundle. ProFlow selects one
whole bundle and does not silently combine config, backgrounds, Bibles, or
other data from different installations.

This layout is appropriate for reuse because the config stores background paths
relative to the bundle. Config, backgrounds, and Bibles can move together.
ProPresenter theme slides, macros, and cue-group definitions are not embedded
in this bundle: they are dependencies installed on the target workstation.
Standardized workstations can share one config directly; heterogeneous
workstations need the same named assets or separately reviewed machine-specific
config. Startup validates the installed theme, macros, and cue-group catalog on
the current machine. Config writes validate every dependency the v4 config can
currently reference: theme slides, macros, backgrounds, and Bible corpora.

The weekly product targets one configured workstation. Exact-name startup
validation and immutable in-process theme/macro/group catalogs are the current
reproducibility boundary. Do not add a second registry, database, or asset-lock
configuration source without a demonstrated multi-workstation requirement.

Independent native exports and an installed library can be re-audited without
writing to them:

```bash
just parity-corpus ~/Desktop
just parity-library ~/Documents/ProPresenter/Libraries/Default
```

Bible filenames are translation identities, not aliases. Startup parses every
installed corpus and rejects byte-identical files under different translation
names. A requested translation whose corpus is not installed requires review;
ProFlow never substitutes a similarly named translation. The former
`NRSV.json` was removed because it was byte-identical to the NRSVue corpus and
therefore could not truthfully represent NRSV.

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
    "library": "Default",
    "theme": "Example Church Theme",
    "background": "default",
    "days_ahead": 14,
    "bible_version": "NRSVue",
    "speaker_fallback_rule": "lords_prayer",
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
      "text_slots": { "body": "Text" },
      "enter_macro": "Name Tag/Title"
    },
    "scripture_prayer": {
      "slide": "Scripture (Projectors)",
      "text_slots": { "body": "Scripture" },
      "enter_macro": "Scripture/Prayer"
    },
    "responsive_scripture_prayer": {
      "slide": "Scripture (Projectors) (Responsive)",
      "text_slots": { "body": "Scripture" },
      "enter_macro": "Scripture/Prayer",
      "leader_enter_macro": "Scripture/Prayer (Highlighted)",
      "speaker_colors": {
        "leader": "#FEDB4F",
        "audience": "#FFFFFF"
      }
    }
  },
  "presentation_types": {
    "static_graphic": {
      "kind": "graphic",
      "content_source": "static",
      "output_strategy": "preserve_existing"
    },
    "song": {
      "kind": "song",
      "content_source": "song",
      "output_strategy": "restyle_existing",
      "background": "default",
      "macro_transitions": {
        "regions": [
          {
            "selector": {
              "kind": "arrangement_group",
              "index": 0,
              "names": ["Background", "Blank"]
            },
            "enter_macro": "Song"
          }
        ]
      }
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
  "library_identities": [
    {
      "id": "g2g_840_it_is_well",
      "match": {
        "kind": "title_prefix",
        "values": ["g2g #840 it is well with my soul"]
      },
      "use_type": "song",
      "library_file": "[Hymn] It Is Well With My Soul (G2G).pro",
      "notes": "G2G wording is distinct from the default hymnal"
    }
  ],
  "item_rules": [
    {
      "id": "all_songs",
      "tier": "catch_all",
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
- `defaults`: exact registered library, default theme, background ID,
  lookahead window, expected presentation size, and optional Bible translation
  for scripture items that do not name one. `speaker_fallback_rule` optionally
  names the item rule whose explicit speaker is used only after an item supplies
  no speaker. Without an explicit item translation or `defaults.bible_version`,
  scripture requires review.
- `service_groups`: reusable sets of Planning Center service types.
- `required_playlist_items`: exact reusable presentations that must occur once
  at the configured start or end of each matching service group. If Planning
  Center already names the same presentation elsewhere, the required policy
  owns it and moves it to the configured edge.
- `backgrounds`: named, project-relative image files.
- `cue_roles`: semantic cue regions bound to exact ProPresenter assets.
- `presentation_types`: content source, output strategy, display binding, and
  optional style values.
- `library_identities`: exact recurring title aliases bound to one canonical
  library file and the presentation type required by that file. These matches
  outrank generic item rules and never depend on array position.
- `item_rules`: deterministic classification rules with exactly one outcome
  each. Rules use explicit `primary` (default), `fallback`, or `catch_all`
  tiers. The highest matching tier wins; multiple matches in that tier require
  review. Array position never decides classification.
- `people`: known-person and nametag metadata.
- `overrides`: service-group, service-type, or presentation-type style
  overrides. Overlapping rules may set different fields or the same value, but
  conflicting values for one field are rejected; array order is not policy.

### What Belongs In Config

`proflow.config.json` remains the durable church policy. It should contain
facts expected to survive many service weeks: service groups, semantic cue
roles, installed macro names, reusable backgrounds, recurring item rules,
required playlist items, and canonical `library_identities`.

An exact `library_file` is intentional for a stable identity such as
`Apostles Creed.pro`, `Call to Worship.pro`, a person nametag, or a
wording-sensitive G2G/HWC hymn edition. Reusable title aliases belong in
`library_identities`; behavioral classification remains in `item_rules`. An
exact filename is too specific when it encodes one week's passage or a
temporary musical choice.
Those choices belong in the reviewed preview override, which can select an
exact file for that build without teaching the base config a historical
exception. Unknowns and weak matches remain `needs_review`; ProFlow does not
grow filename rules merely to avoid asking a human once.

Object-valued config maps serialize in sorted key order. Candidate and backup
diffs therefore reflect policy changes, not randomized map iteration.

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

Matching config overrides must agree when they set the same field. Conflicting
overlaps are invalid config rather than a hidden "last item wins" rule.

The plan resolves the chosen ID to one relative file path before execution, so
a second config lookup cannot change the policy decision. Preview binds the
canonical image identity and its bytes. Rendering derives image metadata from
those reviewed bytes and never follows the configured path or symlink again.
The canonical source is still reverified before commit; a changed source aborts
the reviewed build.

Background IDs are lowercase ASCII identifiers and may also contain digits,
`_`, and `-`. Paths must be normal relative paths with a `.jpg`, `.jpeg`,
`.png`, `.tif`, or `.tiff` extension.

The project bundle registers the approved lyrics/default and sermon images by
stable IDs. Presentation policies choose those IDs; weekly plans never contain
workstation-specific source paths.

At startup and execution, a configured image must:

- remain inside the canonical data root, including after symlink resolution;
- be a regular, non-empty file;
- fully decode as PNG, JPEG, or TIFF using the codec selected by its extension;
  and
- declare nonzero natural dimensions.

A missing, empty, escaped, or mislabeled image is an error. ProFlow does not
fall back to a similarly named file.

MCP and the direct build CLI always default to a portable playlist package.
Portable output contains the exact reviewed presentations and every local media
dependency they still reference. `--library-local` is an explicit diagnostic
choice for a same-workstation package containing only the playlist document and
links to presentations already installed under the reviewed ProPresenter root.

The normal release flow is: build and inspect canonical files in a safe copied
show, then switch ProPresenter to the live show and import the approved portable
playlist using its overwrite option. The package embeds those exact reviewed
`.pro` bytes under their canonical filenames, so the import is the deliberate
promotion step. `--library-local` is only for viewing a playlist against files
already written into the currently active show.

Portable review also inspects media inherited from every selected theme slide.
Those files are canonicalized and their bytes are bound to the immutable
preview revision before rendering; a missing, relative, or changed theme-media
reference fails during preview instead of producing a late package surprise.

## Choosing Slides And Macros

A `cue_role` is the one reusable display contract:

```json
"scripture_prayer": {
  "slide": "Scripture (Projectors)",
  "text_slots": { "body": "Scripture" },
  "enter_macro": "Scripture/Prayer",
  "leader_enter_macro": "Scripture/Prayer (Highlighted)",
  "speaker_colors": {
    "leader": "#FEDB4F",
    "audience": "#FFFFFF"
  }
}
```

- `slide` is the exact theme-slide name used to render that region.
- `text_slots` maps semantic fields to exact names of native text graphics.
  The standard weekly composers write `body`; lower-level presentation specs
  may bind any declared fields.
- `enter_macro` is an optional exact installed macro name and runs when the
  operator enters the region.
- `leader_enter_macro` is the exact alternate macro used when the first
  semantic text run on a generated cue is leader/liturgist content. Audience-
  first cues use `enter_macro`. It requires `enter_macro` and
  `speaker_colors`.
- `speaker_colors` records the editor colors for leader and audience text.
  Macro choice comes from semantic speaker roles, never by comparing RGB
  values. The production policy uses yellow for a leader and white for the
  congregation. The two configured colors must differ so ProPresenter can
  preserve the mixed-style distinction used by its Looks/themes behavior.

Description parsing assigns those roles before layout: `LEADER` and ordinary
leader-read prose are leader content; `ALL`, `PEOPLE`, and `UNISON` are audience
content. Responsive blocks stay together with one blank line between responses
when capacity allows. A catechism question and answer may share one content cue
when both fit; otherwise the question is kept separate and the answer is packed
with the normal text-flow policy. Fragmentation preserves speaker identity, so
later cue macros remain deterministic even when one source paragraph spans
several slides.

A role without `text_slots` is the concise single-field form: its slide must
have exactly one meaningful text destination. Empty unnamed helper elements are
ignored, but two plausible destinations are ambiguous and fail. A role with
`text_slots` can use a multi-field template safely because every semantic field
targets one unique, exact native element name; element order and UUIDs are not
used. The configured slide must also have no embedded theme actions. Cue actions
remain explicit through the macro and background contracts.

For example, a custom nametag template can expose independently named fields:

```json
"speaker_nametag": {
  "slide": "Name Tag",
  "text_slots": {
    "body": "Speaker Name",
    "role": "Speaker Role"
  }
}
```

The template must actually name those graphics elements in ProPresenter. ProFlow
will not guess that element 2 means a person's name. The built-in title/nametag
composer replaces `body`; an additional field such as `role` remains
template-preserved until a source compiler explicitly supplies it through the
same `PresentationSpec` boundary.

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
That is normally one title and one content entry. Responsive content may add a
new transition when a later slide changes between leader-first and audience-
first content. A combined scripture reading gets a fresh title/content
transition for each passage. If rendering produces no title cue, no title
macro is attached and the content macro starts on cue one. Macros represent
region-entry state transitions, so they are not copied onto every slide in a
region.

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
presentation is audited during preview. A uniform legacy canvas with the same
aspect ratio is normalized by scaling slide bounds, element geometry, text
metrics, and RTF font sizes together. Mixed, missing, invalid, or different-
aspect-ratio canvases stop for review because they require layout judgment.

One-off existing-file overrides cross the same boundary: their paths become
canonical absolute `.pro` identities, their bytes must have native presentation
identity, and a requested arrangement must match exactly one native
arrangement. These checks happen before a preview revision is issued.

`arrangement` is valid for operations with an existing native source.
`preserve_existing` links the reviewed native source unchanged.
`restyle_existing` captures the source bytes in the reviewed transaction,
preserves its document identity, and applies a checked combination of three
independent transforms: an explicit `background`, optional
`macro_transitions`, and optional `operator_cue_limit`. At least one must be
configured. Omitting one preserves that native property; a default background
is never inherited implicitly at this boundary. This lets a graphic enforce
its `Graphics` macro without replacing media, a baptism replace its background
without destroying native speaker transitions, and a greeting retain only its
first operator cue. A unique native `Default` is selected for songs when
Planning Center does not supply a usable arrangement; ambiguity remains a
review item.
For songs, a presentation-type setting or matching service override wins;
otherwise ProFlow first uses an exact, case-insensitive Planning Center
arrangement with complete native identity. If that request is absent or
unavailable, one complete native `Default` is the deterministic fallback; a
song with no native arrangements carries no selection. Explicitly configured,
duplicate, ambiguous, or incomplete arrangements require human review and list
the available names. Restyling does not regenerate song text or alternate
arrangements. Cue trimming prunes dangling group and arrangement references
atomically. Background and macro transforms otherwise leave cue/group
structure intact. The reviewed arrangement is selected, and enforced macro
actions replace macro actions only on the configured region entries in that
selected operator traversal.

`macro_transitions.regions` is an ordered, exact policy for existing files. Each
region names the first cue where an installed macro must take effect:

- `{"kind":"operator_cue","index":0}` addresses a cue by its zero-based
  operator-visible position. Use this for a static presentation whose cue order
  is the stable contract. When `operator_cue_limit` is configured, every
  operator-cue index must be smaller than that limit because cue pruning runs
  before macro enforcement.
- `{"kind":"arrangement_group","index":1,"names":["Verse","Verse 1"]}`
  addresses the first cue of the second selected-arrangement group occurrence.
  Its native group name must exactly match one configured name. Use this for
  songs, hymns, and other arrangement-driven presentations.

The ordered regions express transitions, not per-slide decoration. A
contemporary song therefore has one `Song` transition on its initial
`Background` or `Blank` group. A hymn has `Name Tag/Title` on its initial
`Background` or `Title` group and `Song` on its first `Verse` group. A mismatch,
missing cue, repeated target, or unavailable installed macro stops for review;
there is no visible-text or filename inference in the presentation rewrite.

## Scripture Packing And Labels

Scripture rendering keeps the source verse number beside every text fragment.
A deterministic global partitioner evaluates every fitting boundary, caps each
slide at the configured line maximum, and prioritizes sentence, clause, and
verse boundaries over a mid-sentence word break. It rejects tiny tails, favors
nonincreasing front-loaded word counts, and may use an extra slide when that
avoids an unnatural split. Production fit decisions use a persistent native
macOS TextKit oracle over the exact final RTF, text-box bounds, margins,
paragraph settings, fonts, and supported scale behavior. Every generated text
cue is measured again after rendering, for both its source theme and every
macro-selected Audience Look screen theme, before it can be staged. Restyled
existing text cues are likewise measured from their exact retained RTF and
source geometry, then against every active macro destination. Paint-only run
changes such as leader/audience color cannot change textbox fit. Multiple
metric-affecting runs—font, bold/italic, superscript, kerning, baseline, or
paragraph geometry—require review when an Audience Look replaces the source
theme, because `ProPresenter`'s private inline-style remapping cannot be proved
by flattening them. An ambiguous source or destination text-slot mapping
likewise requires review instead of being guessed. Source character order and
verse provenance remain independently regression-tested.

A single terminal partial reference such as `Exodus 16:1-4a` may use the
Planning Center description to prove its exact endpoint. ProFlow compares the
normalized numbered description against the selected local translation,
accepts only one strict prefix that reaches the final requested verse, and then
truncates the local verse at that matched token boundary. Missing text, changed
wording, an earlier cutoff, the complete final verse, or non-prefix suffixes
remain human review. The same predicate runs during review and again against
captured Bible bytes during rendering.

Only scripture content cues receive native slide-action labels. Labels include
book, chapter, and the exact verses represented on that slide, for example
`Ephesians 4:4-6`; a continuation keeps the source verse label. Passage-title
and blank-divider cues remain unlabeled, and combined readings use the correct
book/chapter prefix for each passage.

The fit oracle is compiled into the ProFlow binary and materialized from
content-addressed bytes at runtime; an installed build does not depend on a
development `OUT_DIR`. Missing fonts, malformed RTF, unsupported native text
modes, helper failure, physical overflow, or a line-count violation are typed
build failures. There is deliberately no production fallback to character
counts. The oracle also reports metric-style boundaries, the exact CoreText
font programs, and the operating-system/AppKit/CoreText versions that shaped
the text. Every font explicitly selected by a visible RTF run is preflighted;
unused font-table entries and normal glyph-level system fallback are not
mistaken for authored dependencies. A visible superscript run from an older
document must carry ProPresenter's standardized-superscript marker, because an
unstandardized run may be migrated when the application opens it. TextKit
proves the native attributed-text layout that ProFlow can
reproduce; ProPresenter itself remains the independent oracle for any
proprietary compositor behavior. ProFlow intentionally does not synthesize
ProPresenter Bible-UI metadata: scripture identity is owned by the reviewed
source request, native cue labels, preserved verse provenance, and the rendered
superscript verse numbers.

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
- Default playlist names use `<Month> <day>, <year> - <service>`. Service times
  are compacted without punctuation: `9:00am contemporary` becomes
  `July 12, 2026 - 9am Contemporary`, and `10:30am traditional` becomes
  `July 12, 2026 - 1030am Traditional`.

Rendered files use a small hybrid boundary. Installed theme slides supply the
visual slide—geometry, text box, font, color, and layout. ProFlow constructs the
fresh native Presentation/Cue/Action/Group protobuf envelope, replaces the text,
and adds explicit configured macros and backgrounds. Checked-in files under
`tests/fixtures/propresenter/native/templates` are test fixtures only. Existing-
source assets retain their approved native structure except for the explicit
checked transforms selected by `restyle_existing`. Song restyling preserves
document identity, lyrics, groups, and alternate arrangements while selecting
the reviewed arrangement and applying only its configured background, macro,
or cue operations.

Internally, content compiles to a renderer-independent `PresentationSpec`: a
nonempty sequence of groups and cues, where each cue names a semantic role,
text-field bindings, and an optional label. Immutable render assets resolve each
role to one reviewed theme slide and its named fields. The pure renderer then
creates the native envelope and reports the actual role transitions used for
macro placement. This is the extensibility point for new presentation styles;
new source compilers produce specs instead of adding another protobuf-building
path.

The built-in weekly source compilers cover description/liturgy, title/nametag,
and scripture content. The specification and renderer already support static
cues, named groups, arbitrary semantic fields, multiple cue roles, and labels;
adding another content family means compiling it into that same checked model,
not adding another native serialization path.

## Song Groups And Macro Definitions

Existing song presentations retain their cue groups and repeated arrangement
group order. When a `restyle_existing` policy enforces macro transitions, it
canonicalizes macro actions only at configured region boundaries and removes
stale macro actions from the selected operator traversal; unselected
arrangement-only cues remain untouched. For new named groups,
ProFlow loads the workstation's native `Configuration/Groups` catalog and
copies the exact installed color, hot key, application-group UUID, and name into
a fresh presentation-local group. A named group absent from that exact catalog
is an error; the renderer never emits a plausible-looking group with incomplete
metadata. `catalog_assets` reports the installed names.
It also reports each theme slide's exact named text slots, canvas size, default
text-slot candidate count, embedded-action count, and any issue that prevents
safe generation, so onboarding does not require protobuf inspection or guesses.

Automatic song creation still needs a reviewed source compiler that turns the
church's Planning Center lyric notation into explicit section assignments and
arrangement order. The right next corpus example is one song with every section
type the church uses and at least two arrangements containing repeated groups.
That proves the source-to-spec mapping; native group metadata itself already has
one owner in the installed catalog. Unknown section names remain review items
instead of being guessed from lyrics.

Macro definitions remain owned by ProPresenter. Config references exact
installed names, and presentations contain only native macro references. The
asset catalog now reports each installed macro's actions in native execution
order—including Stage Layout, Audience Look, and Clear Group targets—so an
operator can inspect behavior without ProFlow duplicating macro authoring.

At startup, every macro that configuration can apply—cue-role macros and
presentation-type transition macros alike—is resolved through the native
Audience Look graph by UUID. Each enabled audience screen then resolves either
to the source presentation or to one exact theme document and slide UUID.
ProFlow does not guess from Look, screen, theme, or file names. Snapshot loading
also proves that every configured cue role can bind its text fields on every
macro-selected destination theme. The Workspace, macro document, source theme,
and destination theme documents are parsed once and included in the immutable
render-asset fingerprint; their exact bytes are rehashed before review, after
materialization, and immediately before commit. A dangling Look, theme, screen,
or text-slot reference therefore fails before preview instead of producing a
plausible editor view whose projector or stream output is wrong.

## Output Strategies

| Strategy | Presentation behavior | Style contract |
|---|---|---|
| `preserve_existing` | Reuse an explicitly exempt native presentation unchanged | Existing content is read-only. `display`, `background`, `macro_transitions`, `operator_cue_limit`, and line limits are invalid; an arrangement may be selected. |
| `restyle_existing` | Atomically apply one checked native transform in the staging library | At least one of explicit `background`, `macro_transitions`, or `operator_cue_limit` is required; omitted dimensions are preserved. `display` and line limits are invalid; an arrangement may be selected. |
| `edit_in_place` | Rebuild weekly content into an existing target | `display` is required, so rendering always uses an explicit installed-theme cue role. Background and line limit may be set. Arrangements are invalid. |
| `generate_new` | Create a new presentation | `display` is required. Background and line limit may be set. Arrangements are invalid. |
| `skip` | Produce no output | No rendering occurs. |
| `needs_review` | Stop automatic resolution | The operator must choose an explicit outcome. |

This separation is intentional: an explicit `preserve_existing` exemption
cannot appear to accept a macro or background that the runtime would ignore.

## Fail-Fast Config And Asset Validation

Config parsing rejects versions other than 4, unknown fields, invalid IDs and
paths, contradictory rule outcomes, unknown background or cue-role references,
and invalid output-strategy/style combinations.

Installed macro and theme-slide names are exact, case-sensitive contracts.
ProFlow rejects installed names that differ only by case because such assets
would be ambiguous to humans and across filesystems.

The MCP server then loads one immutable runtime snapshot and verifies every
configured cue-role slide, macro, and background against the local
ProPresenter/data assets. A successfully constructed runtime always owns a
complete library index; there is no later "index not initialized" state. If
activation writes a new live config, restart the server so config, theme cache,
macro cache, group catalog, and file index all come from the same snapshot.

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
   text-slot/canvas/action facts, macros and their actions, cue groups,
   configured backgrounds, and library files.
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
and a mismatch is rejected. Because Planning Center offers no transactional
snapshot endpoint, ProFlow requires two consecutive direct normalized reads to
match before it accepts either the preview source or the pre-commit freshness
check. A changing plan fails for retry rather than producing a torn snapshot.
A fully resolved preview renders every presentation,
constructs the playlist package, prepares the future library-catalog update,
and seals the exact staged bytes before it returns a revision. Source payloads
exist only during this preparation; the revision retains their hashes, the
classified plans, final playlist identity, package policy, and sealed native
artifacts. An unresolved preview retains only diagnostic plans and cannot be
executed.

`build_service` consumes a matching revision exactly once, then performs a
checked process transaction. A revision is therefore one-time even when commit
fails: run `preview_playlist`
again before every retry. Missing, stale, and mismatched revisions are rejected
without consuming the current preview. Immediately before commit, ProFlow
revalidates source hashes, the reviewed present/absent state of every output,
and the hashes of all staged artifacts. A file that appears, disappears, or
changes is not overwritten. Preview also rejects duplicate physical write
targets, symlink outputs, and any cross-entry source/output overlap after
one-off overrides have been applied. Use the stable `output_key` from preview
results for skips and overrides. Keys are derived from
the Planning Center item ID plus the expansion step, never its mutable service
position.

Every successful build also commits
`<playlist>.proflow-build.json` beside the playlist. This deterministic receipt
records the complete normalized Planning Center snapshot and revision, exact
playlist-producer metadata, render-asset fingerprint, reviewed source digests,
final artifact digests, effective playlist arrangement traversal, presentation
structure, every portable media reference/member/unresolved decision and
warning, and native text-fit evidence for generated text cues and safely
measurable restyled text cues. Font evidence includes the exact CoreText-resolved
local program path and SHA-256; those bytes are rehashed at the commit boundary.
The receipt is reviewed and committed in the same process transaction as the
presentations and playlist; the playlist is the final commit artifact, so a
reported commit failure rolls the installed prefix back before returning.

Each individual replacement is an atomic filesystem rename, but the complete
multi-file build is not power-loss atomic or durable. ProFlow has no journal or
startup recovery protocol. The playlist-last commit marker prevents a normal
process failure from advertising an incomplete build; power-loss guarantees
would require filesystem sync plus a recovery journal.

Planning Center item order comes from each item's required `attributes.sequence`,
not HTTP response or pagination order. Missing, invalid, or duplicate sequences
reject the source plan instead of producing a plausibly ordered playlist.

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
      "action": {
        "kind": "set_background",
        "background": "communion"
      }
    },
    {
      "output_key": "pco:12347:main",
      "action": {
        "kind": "select_arrangement",
        "arrangement": "Christmas Eve"
      }
    },
    {
      "output_key": "pco:12348:main",
      "action": {
        "kind": "use_existing",
        "file_path": "/reviewed/library/Jonah 4.pro"
      },
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

The unattended-rendering acceptance criteria and delivery order live in the
[rendering reliability roadmap](docs/reliability-roadmap.md).

The code follows compiler-like, one-directional boundaries:

```text
RawProjectConfig -> validate/compile ProjectConfig ┐
Planning Center JSON -> strict domain normalization ├-> classify semantic actions
installed assets -> immutable exact-name catalogs  ┘
    -> bind one reviewed request/source/output snapshot
    -> compile PresentationSpec or reuse approved native bytes
    -> render/package -> validate -> seal exact native artifacts
    -> operator approval -> revalidate -> transactional commit
```

- `src/project_config/` owns the v4 wire schema and its immutable checked
  runtime form; `src/project_config.rs` is only its public facade. Storage
  deserializes editable `RawProjectConfig`, validation compiles it into
  `ProjectConfig`, and workflow code accepts only the latter.
- `src/workflow/` owns classification, semantic actions, reviewed state,
  presentation-spec compilation, and execution.
- `src/propresenter/` owns ProPresenter parsing, rendering, arrangements,
  checked text flow, macros, backgrounds, and serialization.
- `src/setup/` reports installed and configured asset facts; it does not analyze
  plans or author hidden runtime behavior.
- `src/mcp/` is a thin operator adapter over config and workflow.
- `src/planning_center/` separates HTTP/retry/pagination behavior from strict
  JSON-to-domain normalization. Missing identities, titles, dates, or declared
  song relationships are errors rather than invented placeholder values.

The important state has one owner: the project config names reusable policy and
assets; source payloads belong to preview preparation and become a hash-only
manifest afterward; the reviewed filesystem transaction owns both original
output state and sealed staged artifacts; and the executor alone commits file
side effects. Rendering and packaging consume captured source bytes directly
before approval. Host layers do not maintain a shadow copy of runtime state.

Filesystem discovery also occurs once. `BuildLocations` resolves the project
bundle, library, outputs, theme directory, macro and cue-group documents, and
ProPresenter root at process startup. `RenderAssetSnapshot` then binds those
locations, the compiled project config, and the exact loaded theme/macro
catalogs into one value. Middle phases receive that checked snapshot and never
fall back to the current directory, reread environment variables, or combine
assets loaded for a different config.

Native producer metadata has one source as well. At startup, ProFlow reads the
current `Playlists/Library` document under the active ProPresenter show. New
playlist documents and newly saved presentations receive that captured
application and platform metadata; its exact protobuf bytes and digest are
recorded in the build receipt. A later library edit does not silently mutate the
active runtime snapshot, and an older theme file is never treated as the
producer.

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
their UUID and exact native display name. Portable dependency discovery uses one
visitor across cue actions, presentation and prop slides, nested playback-marker
actions, chord charts, graphics and text-run media fills, data-link files,
external-presentation actions, media file properties, and both legacy and v2
timeline actions. Intentional remote web/RSS content is not mistaken for a local
package asset. The reviewed workflow packages captured bytes, not a second read
of live media paths. Its media set is derived from the final embedded
presentation bytes, so an entry background removed by restyling is not
exported. Shared final media is embedded once, and explicitly requested media
that no final presentation uses is rejected instead of becoming unexplained
package state.

Nested playlist exports use the same `.proplaylist` package format as a single
playlist. `PlaylistSet` owns one or more checked `NamedPlaylist` children and a
single canonical flattened presentation order, so the protobuf document and ZIP
members cannot be supplied in conflicting orders. Shared presentations are
deduplicated across children; the new Desktop golden demonstrates 36 references
to 26 embedded `.pro` files.

The reconstructed April playlist fixtures are labelled as ProFlow
materializations, not independent native exports. They remain diagnostic and
expose legacy defects such as incomplete arrangement metadata and pre-native ZIP
ordering; they are not counted as parity proof. The checked-in
`tests/fixtures/propresenter/native/corpus/playlists/native-template-library.proplaylist`
package is the independent native export reconstructed through the production
`PlaylistSet` writer. The separate
`native-easter-portable-media.proplaylist` fixture is an independent native
portable export with actual media; the parity gate checks its package links,
dependency coverage, member shape, and protobuf round trips without depending
on the original workstation paths.
Portable media packaging uses the canonical absolute source path observed in
native exports. It embeds every available reviewed dependency, while preserving
unresolved external references and sealing their warnings into the build
receipt. Relocatable import behavior remains experimental until an
import/save-back round trip in ProPresenter proves that URL contract. Native
exports also show that older presentations may retain Windows or another user's
absolute URL while the package rebases the media entry by filename. ProFlow does
not guess that mapping: faithful portable export of such presentations needs an
explicit, uniquely reviewed media catalog/relink contract.

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
just parity-library ~/Documents/ProPresenter/Libraries/Default
just pco-smoke
```

Use the narrowest relevant command while debugging, then rerun the enclosing
`just` target. Invariant-heavy changes should finish with `just deep`; the
focused `just parity` gate covers byte-exact codecs and reconstructs the
independent native template-library package through the production writer. The
separate committed Easter export is an archive, link, and media-dependency shape
oracle; it does not by itself prove exact production-writer reconstruction,
relocation, application import, or pixel output. `just deep` also verifies that
checked-in protobuf bindings are fresh against the authoritative schema.
`just parity-corpus <directory>` additionally audits a local, read-only directory
of independent exports; it is intentionally not part of `just deep` because
that corpus is machine-specific.
`just parity-library` does the same for an installed presentation library and
raw playlist document. `just pco-smoke` is the explicit live-network gate: it
runs the Planning Center integration tests serially and requires valid
credentials, while the deterministic gates only compile those tests.

## License

MIT License. See `LICENSE`.
