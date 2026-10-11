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
  macOS. Not run: `just prop`, `just mutants`, `just deep` as a whole,
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
- MCP `preview_playlist` returns `superseded_preview_revision` when a new
  preview replaces an earlier executable one.
- `docs/semantic-resolution.md` (translation, song identity, arrangement
  contracts) and `docs/text-fit-strategy.md` (proposal).

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
- Scripture: `BibleVersion` is typed end to end; unsupported or conflicting
  explicit translations require review; Planning Center selected-verse
  excerpts are reconciled against local text.
- Songs: exact song-title matches own identity; ambiguous or conflicting
  titles require an explicit file selection.
- README condensed around setup, configuration, and operation.
- `build_service --configured-workspace` trusts the configured ProPresenter
  root without consulting the active macOS show.

### Fixed

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
    differs. A file added between the catalog rebuild and preparation of the
    same review is not caught.
  behind the `/var` symlink there.

### Unfinished or deferred

- `BuildServiceError` is still one flat enum of about fifty variants after the
  request variants were extracted. A full split by phase was considered and
  not done: freshness and Planning Center failures occur at both review and
  commit, so the phase grouping is not clean, and nested enums add a match
  level everywhere. Revisit only with a concrete consumer that needs it.
- Preview revisions are process-local. Restarting the MCP server, including
  the restart that activates a new project config, discards every pending
  preview. Persisting sealed previews under their revision would make
  `build_service` restart-safe; this is a product decision, not started.
- Presentation type keys, service group names, theme slide names, and macro
  names are still raw strings in the compiled config. Cross-references are
  validated at load; typed identifiers would remove the remaining
  `Option` lookups. Not started.
- The remaining items of `docs/1.0-roadmap.md` section 4 (release audit of an
  imported playlist) and section 5 (extracting the format crate to its own
  repository) are unchanged. The workspace split is the in-repo step toward
  section 5.
- `just prop` and `just mutants` have not been run since the workspace split.

### Dropped

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
  `just deny`, and `just machete` pass on the server (417 host unit tests,
  279 format-crate unit tests). `cargo-audit`, `cargo-deny`, and
  `cargo-machete` are not installed there; they ran from
  `nix shell nixpkgs#cargo-audit nixpkgs#cargo-deny nixpkgs#cargo-machete`.
  Not run on the server: `just deep` and its heavy members. Nothing from
  this day's server commits has been compiled or tested on macOS; run
  `just local` there.
- Off macOS, 24 `workflow::execute::tests::*` tests are ignored with the
  reason "requires the native TextKit oracle". Each renders through the
  build executor, which cannot start the oracle on Linux. They stay compiled
  and run unchanged on macOS. Gating them with `cfg` instead would have left
  about thirty shared helpers dead off macOS. The executor's render path
  therefore has no test coverage on the server.
