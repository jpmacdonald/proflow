# Changelog and worklog

This file is the resumable record of where proflow stands. Every change to
`main` updates it: what landed, what is unfinished, and what was dropped. A
new session should be able to continue from this file alone.

Dates are the day the work landed on `main`.

## Unreleased

### State as of 2026-10-10

- `main` is the only branch. Everything described below is on `origin/main`.
- Git history was rewritten on 2026-10-10 to remove `data/` (licensed Bible
  text and a real project config) from every commit. A bundle of every ref
  from before the rewrite lives outside the repository at
  `~/Projects/archive/pre-cleanup-20261010/proflow.bundle` on Jimmy's laptop.
- `data/` is the maintainer's live `PROFLOW_DATA` root and is ignored. Tests
  that read it are `#[ignore]`d and run with `just live-data`.
- Verification on this state: `just local` (734 tests), `just live-data`
  (7 tests), `just doctest`, `just doc`, `just audit`, `just deny`,
  `just machete`, `just parity`, `just invariants`, and `just proto` pass on
  macOS. `just prop` (10000 cases) and `just mutants` pass on the server;
  a mutation run scoped to the 2026-10-10 audit fixes caught every mutant
  but the MCP `preview_playlist` handler stub, which needs a live
  Planning Center workstation. Not run: `just deep` as a whole,
  `just schema-audit`, `just release-audit`.
- The server checkout (`~/Projects/proflow` on the home server, reached as
  `server-ts`) has no global Rust toolchain; `nix develop
  ~/Projects/nix-config#rust` provides one. The Swift text-fit helper only
  builds on macOS; off macOS the crate compiles without it and text-fit
  measurement is a typed unavailable error. See "Server" below for what has
  been verified there and what still fails.

### Added

- Cargo workspace. `crates/propresenter` is the `proflow-propresenter` crate:
  native documents, playlists, packages, RTF, media, arrangements, themes,
  macros, audience resolution, the Swift text-fit helper, the protobuf schema
  and generated bindings, the native fixture corpus, and the codec and export
  integration tests. It cannot depend on workflow or project policy. The two
  tool crates under `tools/` are workspace members.
- `src/workstation.rs`: the one composition root. Loads credentials, project
  config, locations, Bible corpora, library catalog, playlist metadata, cue
  groups, and render assets as a unit, and performs the Planning Center reads
  around the executor.
- `tools/schema-audit` and `just schema-audit`: compares the checked-in protos
  against the descriptors embedded in a local ProPresenter binary
  (`docs/schema-audit.md`).
- `just live-data`: runs the ignored tests that need the live data root; part
  of `just deep`; skips loudly without a root.
- `docs/semantic-resolution.md` (translation, song identity, arrangement
  contracts) and `docs/text-fit-strategy.md` (proposal).
- Playlist document (`docs/playlist-document.md`): `PlaylistDocument`, in
  `proflow-propresenter`, is a versioned, schemars-described JSON
  view of an existing playlist for LLM readers (items with a `kind` enum,
  source link, arrangement, and a compact summary of cue groups, scripture
  and media). It is built from a `.proplaylist` package or from a named
  playlist in the live `Playlists/Library`; an unreadable linked presentation
  stays in the document with a typed reason. Read side only: nothing accepts
  a document as build input, and `preview_playlist` does not embed one yet.
  The schema is checked in as a golden
  (`crates/propresenter/tests/fixtures/playlist-document.schema.json`) and
  every fixture playlist is validated against it. New MCP tools:
  `playlist_document_schema` and `read_playlist { name | path }` (path
  confined to the playlist output directory and ProPresenter root). New
  dev-dependency in the format crate: `jsonschema`.

- Build directories (`src/workflow/build_dir.rs`), phase 0 of the
  file-shaped pipeline. A build lives at
  `<data root>/builds/<plan id>/<input hash>/`. `InputHash` covers five
  inputs, each bound to its name and length: the plan snapshot, the
  render-asset fingerprint (which includes the config), the Bible corpus
  fingerprints, the library (every `.pro` candidate with its content hash),
  and the operator's decisions in canonical order. Stages (`snapshot`,
  `classified`, `rendered`, `verified`, `committed`) are subdirectories
  written once through a temporary directory and a rename, and sealed with a
  `stage.json` of per-file SHA-256; a stage whose files differ from its seal
  is refused on resume. `BuildDir<S>` carries the stage in its type, so a
  stage can only be written from the one before it. Mutation run on the
  module: 32 mutants, 20 caught, 10 unviable, 2 missed, both the `fsync`
  helpers, whose effect a test cannot observe.
- Phase 1, slice 1 (`src/workflow/execute/build_stages.rs`): every review
  opens the build directory its inputs name and writes `snapshot/` (the five
  inputs, one file each: `plan.json`, `render-assets.json`,
  `bible-corpora.json`, `catalog.json`, `decisions.json`) and `classified/`
  (`plans.json`, the effective plan), including reviews that still need
  decisions. An equal review reopens the same directory. Review now reads
  every library presentation a second time to hash it.
- Phase 1, slice 2 (`src/workflow/execute/rendered_stage.rs`): prepared
  state is the build directory, not a value in memory. A finished render
  writes `rendered/` (every artifact, the bytes each one replaces,
  `manifest.json` with the source and font fingerprints, and `result.json`)
  and `verified/` (`checks.json`), then removes the copies staged beside the
  live targets. `PreparedBuildRequest` names that directory and remembers
  the seal of the rendered stage it was made from. Commit re-reads
  everything from disk: it checks the seals, that the snapshot stage hashes
  to the directory name, that the recorded render assets and Bible corpora
  are the committing runtime's, and that the library candidates, sources and
  fonts are unchanged; it then stages the artifacts beside their targets
  again through the existing reviewed-output checks, installs them, and
  writes `committed/targets.json`. A fully resolved review always renders:
  an earlier render of the same inputs is deleted, or moved to
  `retired/<n>/` if it was committed. `PreparedRuntimeMismatch` now comes
  from comparing the recorded inputs, and the in-memory `RuntimeEvidence`
  and `PreparedService` are gone. Mutation run over the four pipeline
  modules: 81 mutants, 42 caught, 36 unviable, 3 missed (two `fsync`
  helpers and one digest-length guard, the last now covered by a test).

### Changed

- The build executor is synchronous and has no Planning Center client. It
  emits a `PlanLookup`, consumes a captured `PlanSnapshot`, and verifies a
  refreshed snapshot before commit. The async boundary is in `workstation`.
- The library catalog mutex is a `std` mutex; a poisoned lock is the typed
  `BuildServiceError::LibraryCatalogPoisoned`.
- Errors: the stringly crate-wide `Error` is gone. Planning Center, Bible
  lookup, config loading, and build request validation each own a typed error
  (`PlanningCenterError`, `ScriptureLookupError`, `ConfigError`,
  `RequestError`). MCP reports request errors as `invalid_params`.
- Report: `BuildServiceEntry.action` is the `BuildEntryAction` enum and paths
  are `PathBuf`. A skipped entry's reason is a structured field instead of a
  `"skipped: ..."` string.
- `RenderRole` carries a compiled `CueRoleId`; the renderer no longer re-parses
  role names at execution time.
- Domain types derive `schemars::JsonSchema` directly instead of through
  `rmcp`. Raw protobuf access from the host crate goes through
  `unstable_native` only.
- Protobuf schema fixes against ProPresenter 20.0.1: `RenderLayer` field
  numbers, `Slide.Element.DataLink` key-value and zone members, `Timer`
  style enum name.
- Text fit: a text box set to "Scale Font Down" in ProPresenter is measured
  at its authored font size and must fit there (owner decision,
  2026-10-13: shrinking is a manual safety net; generated output uses the
  theme's font size). It was refused outright before. Font-growing
  behaviours are still refused.
- Songs: a Planning Center arrangement name the file does not have no
  longer needs a decision when the file defines exactly one arrangement;
  that arrangement is used and the plan's reason says so (owner decision,
  2026-10-13, after the first real build asked this for every song whose
  Planning Center arrangement is named after an artist). A file with several
  arrangements still asks. A configured arrangement name is never replaced,
  and an omitted request still does not select a sole arrangement with
  another name. `docs/semantic-resolution.md` is updated.
- Scripture: `BibleVersion` is typed end to end; unsupported or conflicting
  explicit translations require review; Planning Center selected-verse
  excerpts are reconciled against local text.
- Songs: exact song-title matches own identity; ambiguous or conflicting
  titles require an explicit file selection.
- README condensed around setup, configuration, and operation.
- `proflow --configured-workspace` trusts the configured ProPresenter
  root without consulting the active macOS show.
- Typed identities in the compiled config (`project_config/runtime/
  identity.rs`). `PresentationTypeKey` is minted only when a presentation
  type compiles to a policy, so compiled rules, required items, and plans
  that carry one name a type that exists. `MacroName` and `ThemeSlideName`
  replace the raw strings in `CueMacro`, `RestyleMacroRegion`, and
  `RenderRole`. The wire model, the JSON schema, serialized plans and
  previews, and every validation message are unchanged; the names become
  plain strings again only where the format crate is called. Service group
  names needed nothing: compilation already reduces them to service scopes.

- Phase 1, slice 3 (`src/workflow/execute/decisions.rs`): a plan's
  decisions can be recorded in `<data root>/builds/<plan id>/review.toml`.
  The file carries what a request carries: `playlist_name`, `package_mode`,
  `[[media]]`, and one `[[decision]]` per item, which either sets
  `skip = true` or gives a `playlist_name`, a `slide_type`, or a
  `[decision.action]` with the same `kind`s the MCP override takes.
  `question`, `options`, `decided_by` and `at` are kept for the reader and do
  not change what is built or which build directory is used.
  `ServiceBuildExecutor::recorded_request` and
  `Workstation::review_recorded_decisions` review a plan from that file; a
  plan with no file has no decisions. `BuildReview::open_decisions` lists
  every item still waiting, and `into_prepared` now fails with
  `UnansweredDecisions` naming all of them instead of `UnresolvedPlan` for
  the first. `EntryOverrideAction` and its resolution moved out of `src/mcp`
  so the file and the MCP override read one definition. New dependency:
  `toml`. The format is documented in `docs/review-decisions.md`.
- Phase 2: the command line is the boundary and the MCP tools call the same
  functions.
  - `proflow build --plan <id|next> [--service <type>]` reviews the plan with
    its `review.toml` and renders and verifies it. With open decisions it
    lists every one and exits 2. `proflow commit --plan ...` installs the
    verified build. Exit 0 is done, 1 is an error; neither prompts
    (`src/cli.rs`, `src/bin/proflow.rs`). `--plan next` is the soonest
    upcoming plan, refused when two plans share that time unless `--service`
    names one.
  - Commit no longer needs the process that built. `Workstation::
    commit_recorded` fetches the plan again, recomputes the five inputs, and
    commits the verified build in the directory they name. A plan,
    configuration, library or decision that changed since the build names a
    directory with no build, so nothing is installed (`NoVerifiedBuild`).
    This replaces the separate Planning Center freshness comparison. A build
    is committed once (`BuildAlreadyCommitted`).
  - MCP `preview_playlist` writes its arguments to the plan's `review.toml`,
    replacing what was there, then builds from it. `preview_revision` is now
    the identity of the rendered stage and survives a server restart;
    `build_service` commits by plan and refuses a revision that is not the
    build on disk. The response gains `open_decisions`. The process-local
    preview store is deleted.
- A target that changed between preview and commit is now reported as the
  typed `OutputReviewError` (`Changed`, `Appeared`, `Disappeared`) naming
  the path, instead of an I/O error reading "changed concurrently". The
  check itself is unchanged; it now runs when commit stages the artifacts
  again.
- The build report types (`BuildServiceResult` and what it contains,
  `ExpectedPresentationContract`, the presentation structure summary)
  derive `Deserialize` so a committed result can be read back from
  `rendered/result.json`.

- Answerable decisions. Every open decision now carries `options`: the
  library files the review could not choose between (an ambiguous song title,
  conflicting titles, or an ambiguous configured or required file), the
  arrangements the proposed file defines when the requested one is
  unavailable, or the review's own proposal, and always leaving the item out.
  An option is offered only if applying it to the plan is accepted
  (`src/workflow/execute/decision_options.rs`). `proflow build` prints each
  option as the command that records it, and `proflow decide --plan <id>
  --item <item> <decision>` writes that decision to the plan's `review.toml`
  (`--skip`, `--use`, `--arrangement`, `--generate`, `--edit`,
  `--background`). MCP `preview_playlist` returns the same options inside
  `open_decisions`. An item with no proposal and no candidate file still
  offers only leaving it out; choosing a file for it is `--use <file>`.
- Bible translations are data. `BibleVersion` is a translation code (2 to 16
  ASCII letters and digits, case-insensitive), not a fixed list, and its
  corpus is `<data root>/bibles/<CODE>.json`. Installing `ESV.json` makes
  `ESV` a translation a plan may name; a name with no installed corpus that
  is not one of the standard seven still goes to review as an unsupported
  version. The recognized set (`BibleTranslations`: the standard seven plus
  every installed corpus) is read when the configuration is loaded and
  carried by `ProjectConfig`. `NRSVue` keeps its spelling and its
  `NRSVUE.json` file name. `defaults.bible_version` in the config schema is
  now a string, not a seven-value enum. `BibleVersion::from_text` and
  `BibleVersion::all` are gone.
- An image for a configured background, for one plan. `proflow background
  --plan <id> --id <background> --file <image>` (or `--reset`) records a
  `[backgrounds]` entry in the plan's `review.toml`; every item whose
  background is that id uses the file in that plan only. The mapping is part
  of the decisions that name the build directory, applies to proposals still
  under review, and is refused for an id the configuration does not define.
  MCP `preview_playlist` takes it as `background_files`.
- Remembered choices and defaults (`src/workflow/decision_memory.rs`,
  `<data root>/decision-memory.toml`). A question is an item title and a
  `DecisionKind` (`which_file`, `which_arrangement`, `confirm_proposal`,
  `unresolved`). A decision that answers a question the plan's latest build
  asked is remembered, by `proflow decide` and by MCP `preview_playlist`
  alike; later builds list earlier choices first, most recent first, with a
  count and date, and still ask. `proflow decide --default` also makes the
  choice the question's default: a later build records it in that plan's
  `review.toml` as `decided_by = "default"`, reviews again, and reports it
  (`applied_defaults` in the MCP response). A default the plan refuses is
  removed from `review.toml` again and reported (`refused_defaults`). The
  questions a build leaves open are kept in
  `builds/<plan id>/open-questions.toml` so a later decision can be matched
  to its question without fetching the plan.
- Handler-level tests for `proflow build` / `commit` and for the MCP
  `preview_playlist`, `build_service` and `read_playlist` tools. They run a
  whole `Workstation` over a temporary directory against a local stand-in
  for Planning Center (`src/workstation/test_support.rs`), from open
  decision through build to one install. This replaces the "needs a live
  Planning Center" gap. A mutation run over the handler layer (49 mutants:
  32 caught, 15 unviable) leaves 2 missed, the MCP server-info text and a
  generic error helper.

### Fixed

- A scripture title that continues in the same book, such as "2 Corinthians
  8:1-9; 9:6-11", is two passages of that book; the second part was rejected
  as an invalid reference. Found by the first real build on macOS.
- A possible song match is offered as a choice when the song's policy
  restyles the file; it was listed in the question but not among the
  choices. Found by the same build.
- A presentation whose own name ends in a space ("Offertory Nametag ") no
  longer fails the build: a playlist entry keeps the native name as spelled,
  and an item's name taken from a file name is trimmed. Found by the real
  build on macOS.
- `just live-data` failed on macOS (bash 3.2) when its skip list was empty.
- A preview whose decisions are refused (for example media with a
  linked-library package) no longer overwrites the plan's `review.toml`
  before failing. Decisions are checked before they are recorded.
- A presentation path that starts at `/Libraries/...` no longer infers the
  filesystem root as the show root. Show-relative media was being resolved
  against `/`, so a missing file was reported at the wrong path (and a file
  that happened to exist there would have been packaged). The existing
  missing-media test only passed on macOS because temporary directories sit
- Architecture audit of 24e2246 (all four findings):
  - A prepared build carries the render-asset and Bible-corpus fingerprints it
    was prepared from; committing it through a runtime holding different
    evidence is `BuildServiceError::PreparedRuntimeMismatch`. This is a
    runtime check, not a type-level binding: a lifetime brand would force
    every holder of a prepared build (the MCP server keeps them between
    calls) to borrow the workstation.
  - `ServiceBuildExecutor::build_prepared_request`, which takes the caller's
    "current" snapshot, is `pub(crate)`. `workstation::build_prepared_request`
    (refresh, then commit) is the only public commit; a `compile_fail` doctest
    on `ServiceBuildExecutor` pins it.
  - `preview_playlist` returns `invalid_params` for `media_assets` combined
    with `package_mode` `library_local` instead of dropping the media.
  - `review_snapshot` rebuilds the library catalog before classifying, so a
    duplicate title added outside the workflow is reviewed as ambiguous.
    Prepared state records the library's `.pro` candidate set (taken when the
    build is prepared) and commit refuses with `LibraryCandidatesChanged` if it
    differs. The set is scanned before the rebuild and preparation refuses a
    library that no longer matches it, so a file added between the rebuild
    and preparation of the same review is caught too.
  behind the `/var` symlink there.

### Unfinished or deferred

- `BuildServiceError` is still one flat enum of about fifty variants after the
  request variants were extracted. A full split by phase was considered and
  not done: freshness and Planning Center failures occur at both review and
  commit, so the phase grouping is not clean, and nested enums add a match
  level everywhere. Revisit only with a concrete consumer that needs it.
- Phase 1 of the file-shaped pipeline differs from its plan
  (`.agents/plans/file-shaped-pipeline.md`) in four places. `review.toml`
  sits beside the hashed build directories, not inside one, and has no
  on-disk "reviewed" stage. The library is a hash input. A decision is
  `skip` or an action table rather than one `answer` field, so it shares the
  MCP override schema. Decisions made by rules are not written to
  `review.toml`, since writing them would change the hash that names the
  directory holding the result; they are in `classified/plans.json`.
  Phase 2 has no separate `proflow review`: a review of a fully decided plan
  already renders, so `build` is the one command, and it prints the open
  decisions, with a `decide` command for each choice, instead of writing
  them into `review.toml`. There are no golden
  files; the three outputs are asserted verbatim in `src/cli/tests.rs`.
- Build directories are never pruned. Every committed build keeps its
  artifacts and the bytes they replaced under `<data root>/builds/`, in
  place or in `retired/<n>/`. Nothing deletes old builds yet.
- Two processes that review the same plan with the same inputs at the same
  time share one build directory and one `<stage>.tmp`; nothing locks it.
- A build directory is trusted as much as the data root it lives in. Commit
  checks every stage against its seal and the snapshot against the
  directory name, which catches accidents and stale builds, not someone who
  can write the data root and reseal a stage. The MCP revision narrows that:
  it must match the rendered stage.
- Unverified against the real data root: corpus loading now reads every
  `bibles/<CODE>.json`, not only the seven standard names. A JSON file there
  whose name is a valid code but whose content is not a corpus now stops the
  workstation from loading. Check `data/bibles/` on the Mac.
- A translation outside the standard seven is recorded in ProPresenter under
  its code as both full name and abbreviation; there is nowhere to give it a
  full name. Nothing converts a Bible from another format into the corpus
  JSON.
- The mutation pass over the decision, memory and background code was
  stopped unfinished on 2026-10-12 and not rerun; the owner judged broad
  mutation runs not worth their time. These slices are gated on `just ci`.
- Defaults have no helper yet. Nothing lists them, clears them, or reviews
  whether one still deserves to be a default (how often it is overridden or
  refused); `decision-memory.toml` is edited by hand. Remembered answers are
  never pruned. A build with several defaults reviews the plan once more per
  round of defaults, each a Planning Center fetch.
- A question is matched across plans by item title and kind only. Two
  different songs with the same title are one question.
- `proflow build` and MCP `preview_playlist` share one `review.toml` per
  plan. A preview replaces the file with its arguments, including with no
  decisions when it is given none.
- The compiled config still looks macros and theme slides up by name in the
  loaded native assets (`MacroCache::find`, `slide_template`), and those
  lookups can still miss. `MacroName` and `ThemeSlideName` say which kind of
  name is being looked up; they do not prove the name is installed. Removing
  the misses would mean resolving names against the render assets once and
  carrying handles, which is a separate change. Cue-role ids, text-slot
  names, arrangement names, and rule ids in the compiled config are still
  strings.
- The remaining items of `docs/1.0-roadmap.md` section 4 (release audit of an
  imported playlist) and section 5 (the shared format library) are unchanged.
  Section 5 no longer extracts the crate to its own repository: the owner
  prefers a monorepo (2026-10-11), so `proflow-propresenter` stays a
  workspace crate here. How Pew consumes it is undecided.
- Verified on macOS, 2026-10-13 (run over SSH from the server): `just local`
  (849 tests at c1a8487) and `just live-data` (7 tests) pass, and a real
  `proflow build` of plan 91172659 against Planning Center and a local copy
  of the church library reaches "built and verified" in 40 s, with native
  text measurement, at 080fb10. `proflow commit` has still never run for
  real. The Mac's configured ProPresenter root is a local copy, not the
  active show (the shared Dropbox library), so builds there need
  `--configured-workspace`.
- A render or packaging failure does not name the item that caused it.
- Text boxes that shrink or grow their font have no configurable limits. A
  shrink-to-fit box must fit at its authored size; the owner wants a
  minimum (and maximum) font scale to be a project-config setting if anyone
  needs shrinking to count. Not built.

### Dropped

- The `build_service` binary and its `--skip`, `--decisions <json>`,
  `--override-file`, `--portable` and `--library-local` arguments. `proflow
  build` and `proflow commit` replace it, and every decision it took as an
  argument is written in the plan's `review.toml`.
- MCP `preview_playlist`'s `superseded_preview_revision` field, with the
  process-local preview store it described. A revision that no longer names
  the build on disk is refused at `build_service`.
- Restart-safe previews as a separate item: phase 2 delivers it. The build
  is on disk and the revision is derived from it.
- `Workstation::build_service` and `build_prepared_request`, and the
  executor's commit that took a refreshed snapshot from its caller.

- `origin/claude/code-review-YH3Xk` (five commits from 2026-02-18: Planning
  Center credential error messages and loading-spinner fixes for the
  since-removed TUI, plus tokio feature flags). Superseded by
  `PlanningCenterError::MissingCredentials` and the MCP-only operator
  boundary. The branch is on the pre-rewrite history and conflicts with
  `main`. Deleted from `origin` on 2026-10-10 after confirming six of its nine
  touched files no longer exist; it is preserved in the archive bundle above.

### Server

- 2026-10-10: clone at `~/Projects/proflow` on the home server (Linux
  x86_64, reached as `server-ts`), fast-forwarded to `30f637e`.
- 2026-10-10: `cargo check --workspace --all-targets --all-features` passes
  there under `nix develop ~/Projects/nix-config#rust` (rustc 1.97.1, 48 s
  cold). `just` is installed on the server.
- 2026-10-10: `just fmt`, `just check`, and `just clippy` pass on the server.
  Getting there took more than the four dead-code warnings first recorded:
  the helper-process plumbing in `text_fit/client.rs`, the macOS-test-only
  evidence accessors, `interpret_active_show_preference`, and two test
  helpers are now gated to the hosts that use them; `HelperFrame` and the
  non-macOS `active_macos_show_directory` stub carry a scoped `expect` with a
  reason; and clippy 0.1.97 required `sort_by_key` in
  `planning_center/api.rs` and dropping a `return` in `start_bundled`. None
  of this has been compiled on macOS yet; run `just local` there before
  relying on it.
- 2026-10-10: `just local`, `just doctest`, `just doc`, `just audit`,
  `just deny`, and `just machete` pass on the server (447 host unit tests,
  281 format-crate unit tests). `cargo-audit`, `cargo-deny`, and
  `cargo-machete` are not installed there; they ran from
  `nix shell nixpkgs#cargo-audit nixpkgs#cargo-deny nixpkgs#cargo-machete`.
  `just prop` and `just mutants` also pass there (2026-10-11). Not run on
  the server: the rest of `just deep`. Nothing from the server commits has
  been compiled or tested on macOS; run `just local` there.
- Off macOS the executor tests run against `NativeTextFitOracle::diagnostic()`:
  fixed one-line, always-fitting, font-free evidence, built only under
  `cfg(test)` or the dev-dependency `diagnostics` feature and selected by
  `render_plans` only in this crate's tests off macOS. Production off macOS
  still fails with `TextFitError::UnsupportedPlatform`; macOS tests still use
  the real helper. 23 of the 24 formerly ignored executor tests run on the
  server, covering prepare-then-commit freshness, refusal, catalog, drift and
  override invariants. One stays macOS-only
  (`receipt_commits_exact_source_artifact_and_structure_evidence`, it asserts
  resolved-font evidence). The diagnostic oracle proves nothing about real
  text metrics, text splitting, or overflow: goldens and fit sizes remain
  macOS-only.
- 2026-10-11: `just deep` members pass on the server, each run on its own:
  `just ci` (764 tests under nextest), `just parity`, `just proto`,
  `just prop`, `just mutants` (49 mutants: 39 caught, 10 unviable, 0
  missed), `just invariants`. `just live-data` skipped: no data root there.
  The `#rust` dev shell in `~/Projects/nix-config` now carries
  `cargo-audit`, `cargo-deny`, `cargo-machete`, `cargo-mutants`, and
  `cargo-nextest` (nix-config `205f2b8`, local to the server, not pushed),
  so `just tools-check` passes and no `nix shell` wrapper is needed.
- 2026-10-11: with the playlist document, `just local` (455 host and 298
  format-crate unit tests), `just doctest`, `just doc`, `just invariants`,
  `just parity`, `just machete`, and `just deny` pass on the server.
