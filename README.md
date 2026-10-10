# ProFlow

ProFlow compiles Planning Center service plans into ProPresenter presentations
and playlists. Project-specific behavior is defined in configuration; the Rust
core provides deterministic classification, rendering, validation, packaging,
and filesystem transactions.

```text
Planning Center
    -> normalize
    -> classify from project rules
    -> resolve or request review
    -> render and package
    -> validate
    -> commit approved artifacts
```

ProFlow does not silently guess when a rule, asset, arrangement, or source is
ambiguous. Unresolved items remain reviewable and cannot cross the build
boundary.

## Requirements

- macOS
- Rust stable
- Planning Center Services API credentials
- ProPresenter with the assets referenced by the project configuration

## Setup

1. Copy the starter configuration:

   ```bash
   mkdir -p /path/to/proflow-data
   cp examples/starter-config.json /path/to/proflow-data/proflow.config.json
   ```

2. Add project assets under the same directory:

   ```text
   proflow-data/
   ├── proflow.config.json
   ├── backgrounds/
   └── bibles/
   ```

3. Configure the environment:

   ```bash
   PCO_APP_ID=your_app_id
   PCO_SECRET=your_secret
   PROFLOW_DATA=/absolute/path/to/proflow-data
   ```

   Optional overrides:

   ```bash
   PROPRESENTER_DIR=/path/to/ProPresenter
   THEMES_DIR=/path/to/ProPresenter/Themes
   PLAYLIST_DIR=/path/to/output/playlists
   ```

4. Start the MCP server:

   ```bash
   cargo run --bin proflow_mcp
   ```

   To create a repository-local MCP client configuration:

   ```bash
   ./tools/setup_mcp.sh
   ```

ProFlow reads these variables from the process environment and `.env`.
`PROPRESENTER_DIR` identifies the active ProPresenter data directory, not the
application bundle. When `PLAYLIST_DIR` is omitted, output goes to
`<PROPRESENTER_DIR>/Playlists/ProFlow`.

## Project Configuration

`proflow.config.json` is the complete project policy. Version 4 is the only
supported schema.

Configuration can define:

- service groups and Planning Center matching rules;
- presentation types and output strategies;
- installed themes, cue roles, macros, and text-slot bindings;
- reusable backgrounds and Bible corpora;
- library identities, required playlist items, people, and scoped overrides.

The starter configuration is intentionally asset-neutral. It leaves operations
in `needs_review` until the required local assets and policies are configured.
Use [examples/starter-config.json](examples/starter-config.json) as the schema
starting point.

### Classification

Rules produce exactly one typed outcome. Rule tiers determine precedence;
array order does not.

- `primary` rules override lower tiers.
- `fallback` rules apply when no primary rule matches.
- `catch_all` rules handle broad categories.
- Multiple matching rules in the winning tier require review.

Recurring aliases for a specific native file belong in `library_identities`.
General behavioral rules belong in `item_rules`. One-build exceptions belong
in preview decisions rather than durable configuration.

The [semantic resolution contracts](docs/semantic-resolution.md) define authority,
defaults, and review requirements for translations, song identities, and arrangements.

### Presentation Types

A presentation type combines content semantics with an output strategy:

| Strategy | Behavior |
|---|---|
| `preserve_existing` | Reuse an existing native presentation without modification. |
| `restyle_existing` | Apply configured, checked transforms to an existing presentation. |
| `edit_in_place` | Render current content into a configured existing target. |
| `generate_new` | Create a new native presentation. |
| `skip` | Produce no output. |
| `needs_review` | Require an explicit operator decision. |

Invalid combinations are rejected while loading the configuration. For
example, a read-only presentation cannot declare transforms that would be
ignored.

### Assets

Backgrounds use stable configuration IDs and project-relative paths. Themes,
macros, cue groups, and presentation-library files use exact installed names.
ProFlow validates referenced assets before a build can be approved.

The data directory is loaded as one unit. ProFlow does not combine configuration,
backgrounds, or Bible data from unrelated installations.

The repository's own `data/` directory is a live data root and is not tracked.
Bible corpora carry licenses that do not permit redistribution, and a real
project config names real people. Running from the repository uses `./data`
when `PROFLOW_DATA` is unset. Tests that depend on that root are `#[ignore]`d
in the deterministic gates and run with `just live-data`.

## Operation

The MCP server exposes eight tools:

| Purpose | Tools |
|---|---|
| Configuration and discovery | `catalog_assets`, `show_effective_config`, `write_project_config` |
| Plan inspection | `fetch_plan`, `preview_playlist`, `explain_rule_match`, `search_library` |
| Build | `build_service` |

A normal build is:

1. Fetch the Planning Center plan.
2. Preview the classified playlist.
3. Resolve any review items.
4. Approve the exact preview revision.
5. Build that revision once.

Preview materializes and validates the future output before returning its
revision. Build revalidates inputs and staged bytes, then commits presentations,
the build receipt, and finally the playlist. A changed source or destination
invalidates the revision.

Every successful build writes a deterministic
`<playlist>.proflow-build.json` receipt beside the playlist. It records the
normalized plan, decisions, source and artifact digests, render assets,
presentation structure, media dependencies, and text-fit evidence.

### Diagnostic CLI

The direct build command is available for development and diagnostics:

```bash
cargo run --bin build_service -- \
  <plan_id> <service_name> [playlist_name] \
  [--skip <output_key> ...] \
  [--decisions decisions.json] \
  [--portable | --library-local]
```

Portable packages embed reviewed presentations and available local media.
Library-local packages reference presentations already installed in the
configured ProPresenter library.

Additional inspection binaries are available with the `dev-tools` feature.

## Architecture

The code follows compiler-style phase boundaries:

```text
parse -> normalize -> plan -> render -> validate -> export
```

- `src/project_config/` parses and validates project policy.
- `src/planning_center/` retrieves and normalizes source plans.
- `src/workflow/` classifies items, creates reviewed plans, and coordinates
  execution.
- `src/propresenter/` owns native formats, rendering, layout, arrangements,
  media discovery, and package serialization.
- `src/mcp/` exposes the workflow without owning domain policy.
- `src/setup/` reports installed asset facts.

Existing presentation bytes remain opaque unless a configured operation owns a
specific transformation. Generated presentations use the same checked native
serialization path regardless of their content source.

## Native Fidelity

The fidelity contract covers:

1. Byte-exact decoding and encoding of supported ProPresenter protobuf
   documents.
2. Native playlist and package structure based on independent ProPresenter
   exports.
3. Preservation of unknown or unowned presentation state by reusing original
   bytes.
4. Semantic validation of presentation links, arrangements, media, macros,
   geometry, and text fit.

Machine-specific native export corpora can be audited without modifying them:

```bash
just parity-corpus /path/to/exports
just parity-library /path/to/ProPresenter/Libraries/Default
```

For an import and save-back audit:

```bash
just release-audit generated.proplaylist saved-back.proplaylist
```

See [docs/release-audit.md](docs/release-audit.md) for the evidence and release
procedure.

## Development

Use the repository verification interface:

```bash
just local
just ci
just deep
```

- `just local` runs formatting, compilation, clippy, and tests.
- `just ci` adds documentation and dependency checks.
- `just deep` adds native parity, protobuf freshness, property tests, mutation
  tests, and project invariants.

Other useful targets:

```bash
just parity
just pco-smoke
just tools-check
```

Roadmaps:

- [1.0 roadmap](docs/1.0-roadmap.md)
- [rendering reliability roadmap](docs/reliability-roadmap.md)

## License

MIT License. See [LICENSE](LICENSE).
