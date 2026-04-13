# MCP-First Refactor Plan

## Status

The core refactor is complete.

ProFlow now has:

- an MCP-only product surface
- no TUI or user-facing CLI
- a shared workflow core for preview and build
- v2 project config as the runtime contract
- typed workflow plans instead of implicit fallback behavior
- setup tools for cataloging assets, analyzing plans, drafting config, and suggesting patches

## What Remains

The remaining work is productization, not architecture:

- reviewed config write/apply tooling
- stronger expansion and nametag drafting for multi-output rules
- one real onboarding smoke test against a fresh or semi-fresh install
- fixture capture from that smoke test
- a final docs/examples pass so the setup story is concrete for new installs

## Product Direction

- Do not hardcode church-specific workflows in Rust.
- Keep runtime behavior deterministic.
- Let config describe the project.
- Let MCP and setup tools discover, explain, and improve that config.

## Current Core Shape

Main layers:

1. `project_config`
2. `setup`
3. `planning_center`
4. `workflow`
5. `mcp`

Suggested modules:

- `src/project_config.rs`
- `src/setup/`
- `src/planning_center/`
- `src/workflow/`
- `src/mcp/`

## Acceptance Criteria

The repo is in the intended shape when:

- another church can start from a starter config and use MCP to adapt it
- config changes are validated before build time
- preview and build use the same typed workflow model
- setup tools can explain mismatches and propose patches
- example config and workflow artifacts show the intended setup loop

## Historical Context

The original extraction plan focused on:

- shared path helpers
- typed plans
- moving preview/build logic out of MCP
- removing the TUI

Those steps are complete and should stay stable.
