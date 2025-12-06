# ProFlow TUI Todo List

## Recently Completed
- [x] Vertical stacked Items/Matching Files layout with 50/50 split
- [x] Sophisticated file matcher (normalization, liturgical boosts, hymn numbers, selection memory)
- [x] Ignore toggle for items (Delete/Backspace) with visuals and completion interplay
- [x] Loading spinner and error modal overlays

## High Priority
- [ ] Playlist generation flow (`g`): confirm modal, respect ignored/completed, produce playlist output
- [ ] ProPresenter export pipeline: wire builder/convert to write `.pro` / `.proplaylist` from item/editor data
- [ ] Cache/persist file index & match results to avoid cold-start rescans
- [ ] Command/help modal with key cheatsheet per mode
- [ ] Add “Create New File” entry in matching files list (parity with `c`)
- [ ] Item filtering toggles for boilerplate “Other” items (configurable list, show/hide)

## UI / UX
- [ ] Better empty-state/missing-match guidance (modal or inline message)
- [ ] Richer status bar (active library path, API state, current plan info)
- [ ] Optional modal file picker when many matches

## Editor
- [ ] Smart title template insertion when creating Title/Other items (cursor placement)
- [ ] Additional templates by category (song/scripture/graphic)
- [ ] Wrap guide presets or per-item wrap defaults

## Planning Center
- [ ] Retry/backoff and clearer error messages for API failures
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