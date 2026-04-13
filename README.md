# ProFlow

ProFlow is a headless Planning Center to ProPresenter workflow engine with an MCP server as the primary interface.

The intended product shape is:

- deterministic runtime engine
- church-specific config
- MCP tools for discovery, setup, validation, preview, and build

It is not a TUI app anymore, and it is not built around a user-facing CLI.

## What Problem It Solves

Churches using Planning Center and ProPresenter usually end up with a repetitive weekly workflow:

- inspect upcoming plans in Planning Center
- find matching ProPresenter files
- regenerate weekly liturgy or scripture slides
- build playlists in the correct order
- notice edge cases too late

ProFlow moves that work into a config-driven engine. The engine decides what to do for each Planning Center item based on explicit rules, then generates or reuses ProPresenter files and writes playlists.

The MCP layer is how an LLM operates that engine.

## Scope

Current scope:

- macOS only
- Planning Center plans as the source of service structure
- ProPresenter files and playlists as the output
- MCP-first operation
- config-driven classification and build behavior

Explicit non-goals for now:

- Windows support
- interactive terminal UI
- ad hoc runtime heuristics hidden in the build path

## Product Shape

There are three distinct layers in this repo:

1. Runtime engine
   - deterministic
   - takes Planning Center items + project config
   - produces typed plans, presentations, and playlists

2. Project config
   - defines church-specific behavior
   - maps Planning Center item patterns to presentation behavior
   - defines service groups and optional profiles

3. MCP setup and operations tools
   - discover local ProPresenter assets
   - inspect recent plans
   - draft starter config
   - suggest config patches
   - validate config
   - preview and build services

That separation matters. The LLM should help author and improve config, but the actual build path should stay deterministic.

## Repository Status

Current direction:

- MCP-only product surface
- v2 config only
- typed workflow plan model
- shared workflow core, no UI-owned business logic
- setup tooling built directly into MCP

This means the repo is now organized around:

- `src/bin/proflow_mcp.rs`: MCP server entrypoint
- `src/mcp/`: MCP adapter layer
- `src/workflow/`: shared runtime planning and execution
- `src/setup/`: setup and config-authoring helpers
- `src/project_config.rs`: config schema and validation
- `src/planning_center/`: PCO client and types
- `src/propresenter/`: ProPresenter rendering and playlist generation

## Examples

The `examples/` directory contains starter material for new installs and for LLM-assisted setup.

- `examples/starter-config.json`: a conservative MCP-first v2 config scaffold
- `examples/onboarding-workflow.md`: a short setup loop for cataloging assets, drafting config, and tightening it with patches

## Requirements

- macOS
- Rust toolchain
- Planning Center Services API credentials
- access to a ProPresenter library
- a usable ProPresenter theme and optional macros if you want those referenced in config

## Environment

ProFlow reads configuration from environment variables and `.env`.

Important variables:

- `PCO_APP_ID`
- `PCO_SECRET`
- `DAYS_AHEAD`
- `PROPRESENTER_PATH`
- `LIBRARY_DIR`
- `PROFLOW_DATA`
- `HYMNAL_PATH`

Typical `.env`:

```bash
PCO_APP_ID=your_app_id
PCO_SECRET=your_secret
LIBRARY_DIR=~/Documents/ProPresenter/Libraries/Default
DAYS_AHEAD=30
```

Notes:

- `LIBRARY_DIR` is the clearest way to point ProFlow at the right ProPresenter library.
- If `LIBRARY_DIR` is not set, ProFlow will try the default ProPresenter library path.
- `data/proflow.config.json` is the project config file the runtime loads.

## Running The MCP Server

```bash
cargo run --bin proflow_mcp
```

The server runs over stdio and exposes the ProFlow MCP tools.

## Core Runtime Model

At runtime, each Planning Center item is classified into an explicit action:

- `use_existing`
- `edit_in_place`
- `generate_new`
- `skip`
- `needs_review`

That action is produced from config, not inferred from random fallback behavior.

The runtime also tracks:

- content source
  - `static`
  - `description`
  - `scripture`
  - `song`
- presentation kind
- target file behavior
- rendering style
  - template
  - title template
  - background
  - macros
  - arrangement

## Config Model

ProFlow uses a v2 project config.

Top-level shape:

```json
{
  "version": 2,
  "metadata": {},
  "defaults": {},
  "service_groups": {},
  "profiles": {},
  "presentation_types": {},
  "item_rules": [],
  "people": {},
  "overrides": []
}
```

### `metadata`

Descriptive project information.

Example:

```json
{
  "name": "Village Presbyterian Church",
  "timezone": "America/New_York"
}
```

### `defaults`

Project-wide runtime defaults.

Typical fields:

- `theme`
- `days_ahead`
- `review_policy`
- `plan_sort`

### `service_groups`

Named reusable sets of Planning Center service types.

Example:

```json
{
  "all_services": {
    "service_types": ["9:00am contemporary", "10:30am traditional"]
  }
}
```

### `profiles`

Optional named build presets.

Profiles are config-defined. They are not hardcoded in Rust.

Example:

```json
{
  "weekly": {
    "description": "Primary recurring services",
    "service_groups": ["all_services"],
    "days_ahead": 14,
    "review_policy": "ask"
  }
}
```

### `presentation_types`

Named output behavior definitions.

Each one should explicitly declare:

- `kind`
- `content_source`
- `output_strategy`
- optional `template`
- optional `title_template`
- optional `background`
- optional `macro`
- optional `content_macro`
- optional `arrangement`

Example:

```json
{
  "scripture": {
    "kind": "scripture",
    "content_source": "scripture",
    "output_strategy": "generate_new",
    "template": "Scripture (Projectors)",
    "title_template": "Information (Projectors)",
    "macro": "Name Tag/Title",
    "content_macro": "Scripture/Prayer"
  }
}
```

### `item_rules`

Ordered matching rules for Planning Center items.

This is the heart of the system.

Each rule can:

- match by title prefix
- match by title contains
- match by category
- require or forbid scripture references
- limit itself to specific service types
- use a presentation type
- skip explicitly
- mark for review explicitly
- expand into multiple outputs
- point at a specific library file

Example:

```json
{
  "id": "call_to_worship",
  "match": {
    "title_prefix": ["call to worship"]
  },
  "use_type": "liturgical_weekly",
  "target": {
    "library_file": "Call to Worship.pro"
  }
}
```

### `people`

Known people metadata for speaker resolution and future nametag workflows.

### `overrides`

Structured service-specific or type-specific overrides for things like:

- arrangement
- background
- template

## MCP Tool Categories

### Discovery And Setup

Use these when onboarding a new install or new church:

- `get_context`
- `list_profiles`
- `catalog_assets`
- `analyze_recent_plans`
- `draft_project_config`
- `suggest_config_patch`
- `write_project_config`
- `apply_config_patch`
- `validate_config`
- `show_effective_config`

### Plan Inspection

Use these to inspect Planning Center data and current classification:

- `fetch_plan`
- `preview_playlist`
- `explain_rule_match`
- `find_unmapped_items`
- `search_library`

### Generation And Build

Use these when generating outputs:

- `generate_slides`
- `build_playlist`
- `build_service`

## Recommended Onboarding Workflow

For a brand new church/install:

1. Start the MCP server.
2. Run `catalog_assets`.
   This shows:
   - theme slides
   - macros
   - library folders
   - sample library files
   - current service groups, profiles, and presentation types if any exist
3. Run `analyze_recent_plans`.
   This shows:
   - service type breakdown
   - category breakdown
   - recurring titles
   - normalized recurring patterns
   - scripture patterns
   - speaker candidates
   - candidate rule hints
4. Run `draft_project_config`.
   This produces a conservative starter v2 config scaffold.
5. Review the draft.
6. Run `write_project_config`.
   Use `activate: false` to save a candidate file, or `activate: true` to promote the reviewed draft to the live `data/proflow.config.json`.
7. Run `validate_config`.
8. Run `preview_playlist` or `find_unmapped_items` on a few real plans.
9. Run `suggest_config_patch` to tighten the config based on unresolved items.
10. Review the suggested patch, then run `apply_config_patch`.
11. Repeat until previews are consistently clean.

That is the intended setup loop.

## Recommended Weekly Workflow

Once config is stable:

1. `fetch_plan`
2. `preview_playlist`
3. review uncertain items
4. optionally `find_unmapped_items`
5. `build_service`

This is the intended operations loop.

When correcting a multi-output service preview, use the `output_key` returned by `preview_playlist`.
That key is the stable identifier for:

- `build_service.skip_output_keys`
- `build_service.overrides[].output_key`

Do not rely on `position` alone for expanded items like nametag + liturgy bundles.

## What `draft_project_config` Does

`draft_project_config` is the blank-slate scaffold tool.

It currently drafts:

- `metadata`
- `defaults`
- `service_groups`
- `profiles`
- starter `presentation_types`
- starter `item_rules`

It intentionally prefers conservative scaffolding over aggressive guessing.

Examples of what it will infer:

- a `song` type when song items are present
- a `scripture` type when scripture patterns are present
- a generic description-driven type when recurring non-song items carry description content
- a generic static/library-backed type when recurring non-song items appear static
- starter song/scripture rules
- starter recurring item rules for repeated titles

It will also emit assumptions and review notes where manual follow-up is required.

## What `suggest_config_patch` Does

`suggest_config_patch` is for tightening an existing config, not starting from scratch.

It looks at unresolved preview items and proposes:

- missing `presentation_types`
- missing `item_rules`
- deterministic `target.library_file` values when an exact library file can be justified

It avoids inventing runtime behavior beyond what can be inferred safely from:

- recent plans
- current config
- exact library matches
- available templates and macros

## What `write_project_config` And `apply_config_patch` Do

These tools close the setup loop.

- `write_project_config` validates a full reviewed v2 config and writes either:
  - the live `data/proflow.config.json`, or
  - a candidate config under `config-candidates/`
- `apply_config_patch` merges a reviewed `suggest_config_patch.patch` into the current live config, validates the result, and writes either:
  - the live config, or
  - a candidate config

When either tool activates a live config, ProFlow writes a timestamped backup first.

## Current Setup Strategy

The intended progression is:

1. `draft_project_config` creates a starter scaffold.
2. You review it and write it with `write_project_config`.
3. Real plans get previewed against it.
4. `suggest_config_patch` closes the remaining gaps.
5. `apply_config_patch` promotes reviewed fixes.

This keeps the runtime deterministic while still making setup practical.

## Current Limitations

The setup layer is useful, but not magical.

Current limitations:

- It does not auto-approve suggested rules or patches.
- It does not fully infer multi-step expansion rules.
- It does not infer every church’s naming convention perfectly.
- It does not attempt to generate unsupported behavior combinations.
- It still expects a human or LLM operator to review draft output before it becomes canonical config.

That is intentional.

## Architecture Walkthrough

### `src/project_config.rs`

Owns:

- v2 config schema
- parsing
- validation

This is the runtime contract.

### `src/setup/`

Owns:

- asset cataloging
- recent-plan pattern analysis
- starter config drafting
- config patch suggestion

This is the onboarding and config-authoring layer.

### `src/workflow/`

Owns:

- typed plan model
- item classification
- description parsing
- build execution
- build reporting

This is the deterministic runtime engine.

### `src/mcp/`

Owns:

- MCP argument types
- MCP responses
- server wiring
- thin adapter calls into `setup` and `workflow`

This should not grow its own business logic.

### `src/planning_center/`

Owns:

- PCO API client
- pagination
- retry/backoff behavior
- plan and item parsing

### `src/propresenter/`

Owns:

- theme/template loading
- macro loading
- presentation generation
- playlist generation
- serialization/deserialization

### `src/utils/file_index.rs`

Owns:

- library indexing
- fuzzy search
- exact file lookup helpers
- cache persistence

## Development Notes

Useful checks:

```bash
cargo fmt --all
cargo test
```

The codebase currently relies on fixture-driven tests for setup and workflow logic. That is intentional. The setup and config-authoring path needs real examples to stay honest.

## How To Think About Changes

Good changes:

- make config behavior more explicit
- improve deterministic matching
- add better MCP diagnostics
- improve starter config drafting without hiding uncertainty
- add fixture coverage for real services

Bad changes:

- adding hidden fallbacks in runtime classification
- letting the MCP layer shell out to a second interface unnecessarily
- hardcoding church-specific assumptions in Rust
- bypassing config review by turning LLM suggestions into runtime behavior

## Roadmap Direction

The next high-value improvements are:

- richer `draft_project_config` inference
- better expansion-rule drafting
- improved speaker/nametag setup flows
- more fixture sets from different church workflows

The long-term goal is straightforward:

- another church points ProFlow at Planning Center and a ProPresenter library
- MCP tools inspect assets and recent plans
- ProFlow drafts a starter config
- the LLM tightens that config with patch suggestions
- weekly builds become deterministic

## License

MIT License. See `LICENSE`.
