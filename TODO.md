# ProFlow TUI Todo List

## Recently Completed
- [x] Vertical stacked Items/Matching Files layout with 50/50 split
- [x] Sophisticated file matcher (normalization, liturgical boosts, hymn numbers, selection memory)
- [x] Ignore toggle for items (Delete/Backspace) with visuals and completion interplay
- [x] Loading spinner and error modal overlays
- [x] Playlist generation (`g` key) - produces `.proplaylist` files
- [x] ProPresenter export - `:export` command in editor writes `.pro` files
- [x] Help modal (`F1` or `?`) with context-sensitive keybindings
- [x] API retry/backoff for Planning Center failures (exponential backoff, 3 retries)

## High Priority

### Error Handling
- [x] Expand `Error` enum with specific variants (network, parse, file, config)
- [x] Add retry/backoff for Planning Center API failures
- [x] Clearer error messages with actionable context (e.g., "Check PCO_APP_ID env var")
- [ ] Propagate errors idiomatically with `?` instead of silent `.ok()` swallows

### Fuzzy Search & Ranking
- [ ] Persist file index to disk (SQLite or bincode cache) to avoid cold-start rescans
- [ ] Store item→file selection history in cache so previously matched files rank first
- [ ] Boost files that have been matched to *any* item with similar title across sessions
- [ ] Add configurable ranking weights (exact match, prefix, fuzzy, frequency)
- [ ] Async index building so UI doesn't block on large libraries

### Playlist & Export
- [x] Playlist generation flow (`g`): confirm modal, respect ignored/completed, produce `.proplaylist`
- [x] ProPresenter export pipeline: wire builder/convert to write `.pro` files from editor data
- [ ] Validate generated files against real ProPresenter imports

### UI / UX
- [x] Command/help modal with key cheatsheet per mode
- [ ] Add "Create New File" entry in matching files list (parity with `c`)
- [ ] Item filtering toggles for boilerplate "Other" items (configurable list, show/hide)
- [ ] Better empty-state/missing-match guidance (modal or inline message)
- [ ] Richer status bar (active library path, API state, current plan info)

## Code Quality

### Refactoring
- [ ] Modularize `app.rs` (~2000 lines) into focused submodules (input, state, commands)
- [ ] Replace `match _ => {}` arms with `if let` or early returns where appropriate
- [ ] Reduce `.clone()` calls by using references and borrowing where possible
- [ ] Use newtypes for IDs (`ServiceId`, `PlanId`, `ItemId`) instead of raw `String`
- [ ] Remove leading underscores on genuinely unused struct fields; gate with `#[allow(dead_code)]` if intentional

### Editor
- [ ] Smart title template insertion when creating Title/Other items (cursor placement)
- [ ] Additional templates by category (song/scripture/graphic)
- [ ] Wrap guide presets or per-item wrap defaults

### Planning Center
- [ ] Tests around item parsing (scripture + arrangement lyrics)
- [ ] Manual reclassification of items (hotkey to change Category)

## Performance
- [ ] Async/index caching layer
- [ ] Large-library scan metrics and logging
- [ ] Avoid rebuilding index when unchanged across runs

## Bugs & Issues
- [ ] Track and list known bugs here (none recorded)

## Notes
- Keep backward compatibility for existing libraries
- Document new features in README as they land
