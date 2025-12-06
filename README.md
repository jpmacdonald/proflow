# ProFlow

Terminal UI to map Planning Center plans to existing ProPresenter library files and prep slide content quickly from the keyboard.

## Current Capabilities

- Splash screen → Services/Plans → Items/Matching Files → Editor flow.
- Planning Center: fetches service types and plans when `PCO_APP_ID`/`PCO_SECRET` are set; otherwise uses built-in dummy data so the UI can be exercised offline. Includes retry/backoff for API failures.
- ProPresenter library discovery: auto-detects `Documents/ProPresenter/Libraries/Default`, `PROPRESENTER_PATH`, or `LIBRARY_DIR`. Builds a `.pro` index on first entry past the splash.
- **Persistent file index caching**: saves index and selection history to `.proflow_cache.json` in the library directory, avoiding cold-start rescans and remembering previously matched files across sessions.
- File matching: normalization + fuzzy scoring with hymn-number detection, composite title handling, liturgical boosts, and selection frequency boosting.
- Item actions: mark complete, ignore (Delete/Backspace), select a matching file, or open an editor buffer (`c`) with optional preloaded song lyrics.
- **Playlist generation** (`g`): generates `.proplaylist` files from matched items, respecting ignored items.
- **ProPresenter export** (`:export` in editor): converts editor content with verse markers to `.pro` files.
- Editor: basic text editing, selection, clipboard, wrap guide (Alt+←/→), verse markers via `:` commands, wrap/split helpers, and export.
- **Help modal** (`F1` or `?`): context-sensitive keybinding reference for each mode.
- Status overlays: loading spinner and dismissible error modal.

## Known Gaps

- No mouse support.
- Validation of exported `.pro` files against real ProPresenter imports pending.

## Setup

1. **Prerequisites**  
   - Rust 1.70+  
   - macOS or Windows with ProPresenter library available (optional)

2. **Environment**  
   Create `.env` with any of:  
   - `PCO_APP_ID`, `PCO_SECRET` – enable Planning Center fetching.  
   - `DAYS_AHEAD` – override default 30-day plan window.  
   - `PROPRESENTER_PATH` or `LIBRARY_DIR` – point to your ProPresenter install or library.

3. **Run**  
   ```bash
   cargo run
   ```
   Press any key on the splash screen to begin. If PCO credentials are missing, dummy services/plans/items are used.

## UI & Keys (quick reference)

- **Navigation**: arrows / `h` `j` `k` `l`, `Tab` to switch panes.
- **Global**: `F1` or `?` for help modal; `:` enters command mode; `:q` quit, `:reload` refresh data.
- **Service/Plans**: Enter to drill into a plan.
- **Items pane**: Enter/Tab to focus files; Delete/Backspace toggles ignore; `c` open editor; `g` generate playlist.
- **Files pane**: Enter selects file for the current item (marks complete, records preference for future ranking).
- **Editor**: Esc back; Shift+arrows for selection; Ctrl/Cmd+C/X/V clipboard; Alt+←/→ wrap column; `:split`, `:wrap`/`wrap 90`, verse markers like `:v1`, `:c`; `:export` or `:save` to write `.pro` file.

## Architecture Notes

- `src/main.rs` boots the TUI, cleans up terminal state, and runs the async event loop.
- `src/app.rs` holds all state, keyboard handling, async Planning Center fetches, file indexing, match selection, and editor logic.
- `src/ui/` renders splash, services/plans, items/files (vertical 50/50), editor, loading and error overlays, and the command bar.
- `src/planning_center/` provides the API client and data models with improved error types.
- `src/utils/file_matcher.rs` indexes `.pro` files and performs scoring/boosting with persistent selection history.
- `src/propresenter/` contains the data model, conversions, and builder for future export support.
- `src/error.rs` provides rich error types with actionable context and hints.

## Contributing

Focus areas that will unlock the workflow:
- Implement playlist generation and ProPresenter export wiring to the builder/convert layer.
- Add item filtering toggles and help modal.
- Improve error surfacing in the UI.

## License

MIT License. See `LICENSE`.
