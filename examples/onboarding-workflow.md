# Onboarding Workflow

This is the intended setup loop for a new church or a fresh install.

1. Start the MCP server.
2. Run `catalog_assets` to inspect local templates, macros, and library files.
3. Run `analyze_recent_plans` to find recurring titles and patterns in real service plans.
4. Run `draft_project_config` to generate a conservative starter config.
5. Review the draft and write it with `write_project_config`.
6. Run `validate_config`.
7. Run `preview_playlist` on a few real plans.
8. Run `find_unmapped_items` and `suggest_config_patch` until the unresolved items are explained or fixed.
9. Review the patch and promote it with `apply_config_patch`.
10. Run `build_service` once the preview is clean enough to trust.

The goal is not to guess perfectly on the first pass. The goal is to get from a real church's assets and plans to a deterministic config without editing Rust code.
