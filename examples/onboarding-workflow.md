# Onboarding Workflow

This is the intended setup loop for a new church or a fresh install.

1. Start the MCP server.
2. Run `catalog_assets` to inspect exact theme-slide names, installed macros, configured backgrounds, and library files. If the starter config has no theme, pass the intended exact `theme_name`; this loads it only for that discovery call and does not change runtime state.
3. Run `fetch_plan` for representative services and inspect their real titles, categories, scripture metadata, speakers, and service-type names.
4. Copy `starter-config.json` and author one complete v4 config from those facts.
5. Register each reusable image under `backgrounds` and each semantic slide/macro pair under `cue_roles`. Use `arrangement` only on `use_existing` presentations; rendered and edited presentations do not accept it.
6. Write a candidate with `write_project_config` and `activate: false`. The tool reloads the configured theme and installed macros and validates every cue-role slide, macro, and background before any file is written.
7. Review the complete candidate and activate it with `write_project_config` and `activate: true`.
8. Restart the MCP server so config and validated ProPresenter asset-name caches share one runtime snapshot.
9. Run `preview_playlist` on representative plan IDs and use `explain_rule_match` or `search_library` for unresolved entries. Preview title, date, and service name come from Planning Center; a supplied `service_name` is only an exact assertion.
10. Run `build_service` only with the exact confirmed `preview_revision`. The matching revision is consumed before build side effects and cannot be retried, even after a failed build; re-preview first.

Setup tools report facts; they never infer or patch runtime policy. The portable
data bundle contains config, backgrounds, and Bibles. Theme slides and macros
remain machine-installed dependencies and are validated on the target
workstation. The goal is to get from a real church's assets and plans to one
deterministic, reviewable config without editing Rust code.
