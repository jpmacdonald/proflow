# Backlog Completion Plan

This plan covers the remaining product work after the MCP-first refactor.

The core runtime is done. The remaining work is about:

- better multi-output authoring
- one real onboarding proving pass
- fixture capture from that pass
- parser and matching polish
- final sign-off and cleanup

## Definition Of Done

ProFlow is done enough to call complete when all of the following are true:

- a new or semi-new church setup can be onboarded through MCP without Rust changes
- `draft_project_config`, `write_project_config`, `preview_playlist`, `suggest_config_patch`, and `apply_config_patch` work as a practical loop on real plans
- multi-output bundles like nametag + liturgy or nametag + announcements can be drafted or patched without manual runtime hacks
- at least one real onboarding pass is captured as fixtures and covered by tests
- remaining misses are church-specific config choices, not engine gaps

## Workstreams

### 1. Multi-Output Drafting

Goal: make setup tooling infer and patch bundle rules instead of leaving them as manual config work.

Deliverables:

- `draft_project_config` can emit starter `expand` steps for obvious bundles
- `suggest_config_patch` can propose `expand` rules when unresolved patterns strongly imply multiple outputs
- speaker-aware bundle suggestions can target `person_nametag` plus a second type
- candidate rule output includes clear notes when confidence is not high enough

Implementation steps:

1. Extend recent-plan analysis to surface bundle signals:
   - recurring titles with speaker names
   - recurring titles that consistently produce both a content slide and a nametag
   - repeated title prefixes like `welcome`, `call to worship`, `moment for mission`
2. Add setup heuristics for starter expansion rules:
   - if a recurring pattern has a known speaker signal and a known content pattern, suggest `expand`
   - if the second output maps to a known library file, attach `target.library_file`
   - if the nametag filename is not obvious, prefer review notes over guessing
3. Update patch suggestion logic to emit:
   - new `presentation_types` when missing
   - new `item_rules.expand` arrays when the pattern is strong
   - target hints and notes for review
4. Add fixture-backed tests for:
   - welcome bundle
   - call to worship bundle
   - speaker-driven non-staff cases
   - duplicate-output-key stability

Acceptance criteria:

- setup tools can draft at least one multi-output rule from real plan patterns
- setup tools can patch at least one existing config with an `expand` rule
- bundle suggestions never silently invent unsupported runtime behavior

### 2. Real Onboarding Smoke Test

Goal: prove the full operator loop on a realistic install.

Deliverables:

- one recorded onboarding run from recent plans and a real library
- one reviewed starter config candidate
- one reviewed patch candidate
- one clean preview/build result for a representative service

Execution steps:

1. Choose a fresh or semi-fresh target config state.
2. Run the real MCP loop:
   - `catalog_assets`
   - `analyze_recent_plans`
   - `draft_project_config`
   - `write_project_config`
   - `validate_config`
   - `preview_playlist`
   - `find_unmapped_items`
   - `suggest_config_patch`
   - `apply_config_patch`
   - `preview_playlist` again
   - `build_service`
3. Record where the operator had to intervene:
   - config review calls
   - unclear rule matches
   - naming misses
   - bundle inference misses
4. Turn each intervention into either:
   - a code fix
   - a fixture
   - an explicit documented limitation

Acceptance criteria:

- one real service can be onboarded and built through MCP
- unresolved items are explainable and few
- no manual Rust edits are required during onboarding

### 3. Fixture Capture

Goal: lock the real onboarding path into tests.

Deliverables:

- one fixture set for starter config drafting
- one fixture set for patch suggestions
- one fixture set for multi-output bundles
- one fixture set for preview/build after config promotion

Implementation steps:

1. Save sanitized Planning Center item fixtures from the smoke test.
2. Save expected outputs for:
   - `analyze_recent_plans`
   - `draft_project_config`
   - `suggest_config_patch`
   - `preview_playlist`
3. Add assertions for:
   - generated `presentation_types`
   - generated `item_rules`
   - generated `expand` rules
   - stable `output_key` behavior
4. Keep fixtures small and representative, not exhaustive.

Acceptance criteria:

- regressions in setup suggestions show up in tests
- regressions in bundle handling show up in tests
- tests cover both scaffold generation and patch tightening

### 4. Parser And Matching Polish

Goal: reduce friction from church-specific text patterns without adding fallbacks.

Deliverables:

- better description parsing for common liturgy and nametag patterns
- better matching diagnostics for songs, scripture, and static titles
- clearer review reasons when config is insufficient

Implementation steps:

1. Review unresolved items from the smoke test and categorize misses:
   - parse miss
   - match miss
   - naming miss
   - config-gap miss
2. Tighten parser behavior where it can be deterministic:
   - type-aware description parsing
   - better speaker extraction
   - clearer marker handling
3. Tighten matching where it improves portability:
   - title normalization edge cases
   - bracketed prefix handling
   - stable exact-match versus uncertain-match behavior
4. Prefer explicit review output over hidden fallback logic.

Acceptance criteria:

- common real-world misses from the smoke test are either fixed or clearly surfaced
- parser improvements are backed by focused tests
- matching changes do not widen hidden inference

### 5. Final Cleanup And Sign-Off

Goal: leave the repo in a state that is easy to understand and ship.

Deliverables:

- docs aligned with the actual MCP loop
- task files reflect only real remaining backlog
- final review with no blocking findings

Implementation steps:

1. Update:
   - `README.md`
   - `examples/`
   - `tasks/todo.md`
   - `tasks/lessons.md`
2. Make sure the backlog reflects only post-launch improvements.
3. Run:
   - `cargo fmt --all`
   - `cargo test`
4. Do one final review pass focused on:
   - correctness
   - determinism
   - operator clarity
   - setup loop completeness

Acceptance criteria:

- docs match the implemented tools
- tests pass
- final review finds no blockers

## Parallelization Plan

These can run in parallel:

- Workstream 1: multi-output drafting heuristics and tests
- Workstream 2: real onboarding smoke test and issue capture
- Workstream 4: parser and matching polish, using the smoke-test misses as input as they appear

These should follow after the first two start:

- Workstream 3: fixture capture, because it depends on smoke-test artifacts
- Workstream 5: final cleanup and sign-off, because it depends on the previous outputs

Recommended parallel split:

- Agent A: setup heuristics for `draft_project_config` and `suggest_config_patch`
- Agent B: onboarding smoke test execution and issue log
- Agent C: parser and matching fixes from observed misses
- Main thread: integrate changes, add fixtures, update docs, run final review

## Recommended Order

1. Start multi-output drafting heuristics.
2. In parallel, run the real onboarding smoke test.
3. Use smoke-test misses to drive parser and matching polish.
4. Capture fixtures from the smoke test and the new setup outputs.
5. Update docs and prune the remaining backlog.
6. Run final review and sign off.

## Final Sign-Off Checklist

- `draft_project_config` produces a usable starter config from real data
- `suggest_config_patch` can tighten unresolved items from real previews
- `write_project_config` and `apply_config_patch` are the normal reviewed promotion path
- multi-output bundles are representable, previewable, and overridable with `output_key`
- one real onboarding pass succeeds without Rust edits
- smoke-test artifacts are captured as fixtures
- `cargo test` passes
- final review has no blocking findings
