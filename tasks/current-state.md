# Current State

Date: 2026-07-13

## Product Shape

ProFlow is an MCP-first weekly service compiler:

```text
Planning Center -> classify -> review -> render/reuse -> validate -> export
```

The Rust core owns deterministic classification, reviewed source/output state,
native ProPresenter construction, and transactional file writes. MCP owns the
human review conversation. Diagnostic CLIs call the same workflow boundary; a
Ratatui client is not a separate product architecture.

## Durable And Weekly State

- `data/proflow.config.json` is durable church policy: service groups, required
  items, cue roles, macro names, backgrounds, presentation types, people, and
  recurring classification rules.
- The project data bundle owns config-relative backgrounds and Bible data.
- Installed ProPresenter themes, macros, and library presentations remain
  workstation dependencies validated by exact identity.
- Weekly exceptions belong to preview overrides and the one-time reviewed build
  revision. They do not become permanent config rules without evidence that the
  association recurs.

## Current ProPresenter Contract

- Existing presentations are decoded for audit and otherwise byte-preserved.
- Generated description and scripture slides take their geometry, text box,
  font, color, and layout from exact installed-theme cue roles.
- ProFlow builds the native Presentation/Cue/Action/Group envelope and adds
  explicit macro and background actions.
- The configured 1920x1080 size is checked for theme slides, selected existing
  files, and rendered output. Legacy files require operator correction in
  ProPresenter: set output size, then reapply the theme.
- Required pre/post-service and Stephen Minister presentations occur once at
  their configured edge, even if Planning Center placed the same file elsewhere.
- Mutable target collisions stop in preview; repeated read-only playlist
  references remain valid.
- Managed backgrounds cause portable package export by default so reviewed image
  bytes are included.

## Native Fidelity Evidence

- Supported `.pro`, playlist-document, and package codecs have byte-exact
  round-trip coverage.
- Independent native packages cover single playlists and nested playlist sets.
- Package reconstruction tests cover member identity/order, ZIP64 envelope,
  selected-arrangement metadata, shared-presentation deduplication, and portable
  media discovery.
- Machine-local Desktop and Documents corpora are audited with `just
  parity-corpus`; they are reference-only and are never mutated.

## Remaining Product Inputs

- Confirm the durable default/lyrics and sermon background assets. The current
  data bundle has a generic blue placeholder; Documents contains the recent
  Jonah lyrics and sermon candidates, but asset identity is a church decision.
- `Marilyn Nametag.pro` is configured but absent from the Documents library.
- Decide whether the exact Come Thou Fount and Tunny song rules are permanent
  aliases. Otherwise remove them and make those choices in weekly review.
- Sermon decks remain deliberately manual, so a sermon-specific background is
  not used until sermon generation is intentionally added.
- Capture reviewed native song fixtures before adding song/group authoring.

## Consolidation Plan

The next structural pass should replace the cross-product workflow plan with
semantic variants:

```text
UseExisting | EditDescription | GenerateDescription | GenerateScripture
| Skip | NeedsReview
```

Each executable variant should carry exactly the path, content, and style it
requires. This makes contradictory `PlanAction + Option<path> + Option<content>
+ style` combinations unrepresentable and removes the second execution-time
shadow action/checking layer. Normalize the config wire schema into the same
semantic variants once, then split the oversized files only along the real
phase boundaries exposed by that deletion. Do not add generic managers,
registries, or a second asset database.

Project presentation size is now one required reviewed-build value rather than
an optional field copied into every item. After the plan model change:

1. Resolve all build locations once into a checked value; remove ambient `.` and
   internal `data_root()` fallbacks.
2. Separate config wire parsing, normalized policy, and persistence.
3. Split classification into rule matching, item resolution, and required-item
   policy; split execution into approval, rendering, and transaction phases.
4. Keep `just deep` plus native corpus parity as the completion gate.
