# Lessons

## Silent config deserialization kills everything
A `_comment` key with a String value inside a `HashMap<String, Vec<String>>` caused serde
to fail the entire config parse. The v2-only parser now returns hard errors instead of
silently falling back, but the principle remains: never swallow config parse failures.
**Prevention:** The v2 parser rejects unknown fields and returns `Result`. Keep it that way.

## Stale MCP binary
The MCP server runs from `target/release/proflow_mcp`. After code changes, must rebuild
AND `/mcp` reconnect. Check binary timestamp vs source timestamps before debugging.

## FileIndex normalizes bracket prefixes away
`normalize_name()` strips `[Hymn]`, `[Anthem]`, etc. from filenames. Searching WITH
brackets against the normalized index fails containment checks. Search bare titles instead.

## content_source and output_strategy replace the "edited" flag
`content_source: description` + `output_strategy: edit_in_place` means inject content into
an existing library file (preserving its styling). `output_strategy: generate_new` means
build a new file from a template. These must be separate code paths in the executor.

## Config is the single source of truth for ProPresenter names
Template names, macro names, title templates, and content macros all live in
`proflow.config.json`. Never hardcode ProPresenter theme/macro names in Rust — when they
change in Pro, only the config should need updating.

## New config fields must be wired through the full chain
Adding a field to the config schema requires threading it through: `PresentationTypeConfig`
→ `PresentationStyle` (plan.rs) → `resolve_style()` (classify.rs) → executor methods.
Missing any link means the field is silently ignored with no compile-time warning.

## Setup tools stay out of the runtime path
`catalog_assets`, `analyze_recent_plans`, `draft_project_config`, and `suggest_config_patch`
are onboarding helpers. They should explain and propose config, not act as hidden runtime
fallbacks. The runtime engine must stay deterministic and config-driven.

## Multi-output workflow needs stable output keys
Expanded items cannot be targeted safely by PCO `position` alone. Preview/build flows now
surface a per-output `output_key`, and any skip/override operation must use that key for
bundles like speaker nametag + liturgy. Keep that contract stable across MCP tools.
