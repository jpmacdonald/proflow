# ProFlow TUI Todo List

## Recently Completed
- [x] Vertical stacked Items/Matching Files layout with 50/50 split
- [x] Sophisticated file matcher (normalization, liturgical boosts, hymn numbers, selection memory)
- [x] Ignore toggle for items (Space key) with visuals and completion interplay
- [x] Loading spinner and error modal overlays
- [x] Playlist generation (`g` key) - produces `.proplaylist` files
- [x] ProPresenter export - `:export` command in editor writes `.pro` files
- [x] Help modal (`F1` or `?`) with context-sensitive keybindings
- [x] API retry/backoff for Planning Center failures (exponential backoff, 3 retries)
- [x] Open existing .pro files in editor (`e` key extracts RTF text from slides)
- [x] Scripture auto-fetch with version picker (NRSVue, NRSV, NIV, KJV)
- [x] Bundled Bible JSON files from jadenzaleski/bible-translations
- [x] Scripture reference parsing with superscript verse numbers
- [x] Fix: Focus now returns to items list after matching last item
- [x] Template-based slide generation system (TemplateCache, TemplateType)
- [x] Embed .pro files directly into .proplaylist zip bundles
- [x] Created `src/constants.rs` with centralized magic numbers
- [x] Added module documentation to config.rs, error.rs, lyrics/mod.rs, planning_center/mod.rs, ui/mod.rs, utils/mod.rs, app.rs
- [x] Removed dead code: `App::is_scripture_item()`, `count_visual_lines_for_content()`

---

## Code Review Findings (2024)

### CRITICAL Priority

#### God Object: `App` Struct (src/app.rs:169-214)

The `App` struct has **44 public fields** - textbook god object.

- [ ] Extract `ModalManager` to handle modal state (show_help, version_picker_active, pending_playlist_confirmation, error_message, status_message)
- [ ] Extract `EditorManager` to handle editor state persistence across item_states, file_index, and in-memory editor
- [ ] Extract `ListNavigator` helper for 62+ list state selection patterns
- [ ] Extract `FileSearchService` from update_matching_files() (217 lines)
- [ ] Extract `PlaylistService` from generate_playlist() (138 lines)
- [ ] Create `VersionPickerState` struct (version_picker_active + version_picker_selection)

#### Modal State Explosion (src/app.rs:182-211)

7 independent modal states with unclear precedence:
- `show_help: bool`
- `version_picker_active: bool`
- `editor_side_pane_focused: bool`
- `file_search_active: bool`
- `is_global_command_mode: bool`
- `is_command_mode` in EditorState
- `pending_playlist_confirmation: Option<usize>`

- [ ] Consolidate into single `ModalState` enum with proper state machine
- [ ] Document modal precedence rules

#### Dead Code in Planning Center Types (src/planning_center/types.rs)

13 fields marked `#[allow(dead_code)]` with leading underscores:

**Plan struct:**
- [ ] Remove or use `_service_name: String` (line 20)
- [ ] Remove or use `_items: Vec<Item>` (line 26)

**Item struct:**
- [ ] Remove or use `_position: usize` (line 35)
- [ ] Remove or use `_description: Option<String>` (line 38)
- [ ] Remove or use `_note: Option<String>` (line 41)
- [ ] Remove or use `_scripture: Option<Scripture>` (line 44)

**Song struct:**
- [ ] Remove or use `_copyright: Option<String>` (line 64)
- [ ] Remove or use `_ccli: Option<String>` (line 66)
- [ ] Remove or use `_themes: Option<Vec<String>>` (line 68)
- [ ] Remove or use `_arrangement: Option<String>` (line 71)

**Scripture struct:**
- [ ] Remove or use `_reference: String` (line 78)
- [ ] Remove or use `_text: Option<String>` (line 80)
- [ ] Remove or use `_translation: Option<String>` (line 82)

---

### HIGH Priority

#### Functions Over 50 Lines - Extraction Needed

| Function | File | Lines | Size | Action |
|----------|------|-------|------|--------|
| `new()` | app.rs | 217-359 | 142 | Split into builder methods |
| `handle_key()` | app.rs | 369-446 | 77 | Extract modal guard logic |
| `handle_service_list_input()` | app.rs | 548-648 | 100 | Extract navigation helpers |
| `handle_item_list_input()` | app.rs | 650-776 | 126 | Extract item/file selection |
| `handle_editor_normal_input()` | app.rs | 887-1056 | 169 | Extract clipboard/editing ops |
| `update_matching_files()` | app.rs | 1685-1902 | 217 | Extract search term builder |
| `generate_playlist()` | app.rs | 1992-2130 | 138 | Extract entry collection |
| `convert_slide_to_rv_data()` | convert.rs | 711-960 | 250 | Extract builder methods |
| `build_presentation_from_template_with_options()` | template.rs | 242-354 | 113 | Split into smaller functions |

#### UI Code Duplication

- [ ] Extract `Color::Rgb(80, 80, 120)` selection color to constants (used in editor.rs:259, item_list.rs:26, service_list.rs:46, service_list.rs:105)
- [ ] Create unified color theme in `src/constants.rs` under `ui` module

#### UI Bug: Unreachable Code (src/ui/mod.rs:122-133)

```rust
if app.mode == AppMode::Editor && app.editor.is_command_mode {
    // Lines 123-126: Draw command input
} else if app.mode == AppMode::Editor && app.editor.is_command_mode {  // DUPLICATE!
    // Lines 128-133: UNREACHABLE
}
```

- [ ] Fix duplicate condition - second block never executes

#### File-Wide Dead Code Suppressions

| File | Line | Current |
|------|------|---------|
| rtf.rs | 6 | `#![allow(dead_code, clippy::unwrap_used)]` |
| data_model.rs | 6 | `#![allow(dead_code)]` |
| convert.rs | 6 | `#![allow(dead_code)]` |
| builder.rs | 6 | `#![allow(dead_code)]` |
| serialize.rs | 5 | `#![allow(dead_code)]` |
| parser.rs | 3 | `#![allow(dead_code)]` |

- [ ] Audit each file for actual dead code
- [ ] Replace file-wide allows with targeted item-level allows

#### Cache Performance Issue (src/utils/file_matcher.rs)

`persist()` called after every single interaction:
- Line 239: Called in `record_selection()`
- Line 250: Called in `save_editor_state()`
- Line 261: Called in `save_item_completion()`
- Line 272: Called in `save_item_ignored()`

- [ ] Implement debounced/batched cache writes
- [ ] Add periodic flush instead of per-action persistence

---

### MEDIUM Priority

#### Type Safety Issues

**String used where PathBuf should be:**
- [ ] `config.rs:22` - Change `propresenter_path: Option<String>` to `Option<PathBuf>`
- [ ] `item_state.rs:25` - Change `matched_file: Option<String>` to `Option<PathBuf>`
- [ ] `services/playlist.rs:11` - Change `file_path: Option<String>` to `Option<PathBuf>`

**Newtype IDs not used consistently:**
- [ ] `planning_center/types.rs:10` - Change `Service.id: String` to `ServiceId`
- [ ] `planning_center/types.rs:17` - Change `Plan.id: String` to `PlanId`
- [ ] `planning_center/types.rs:18` - Change `Plan.service_id: String` to `ServiceId`
- [ ] `app.rs:173` - Change `active_service_id: Option<String>` to `Option<ServiceId>`

**Unstructured error types:**
- [ ] `input.rs:20` - Change `Error(String)` to structured error type
- [ ] `input.rs:22` - Change `Status(String)` to `Status(StatusMessage)` enum

#### UX Issues

**Inconsistent border color logic (src/ui/editor.rs:139):**
- [ ] Fix backwards logic: focused should be Yellow, unfocused should be DarkGray
- [ ] Make consistent with side pane (lines 351-352)

**Focus indicators unclear:**
- [ ] `service_list.rs:40-44` - Replace "(focused)" text with visual border indicator
- [ ] `item_list.rs:22-23` - Add border color change when switching between items/files lists

**Missing scroll indicators:**
- [ ] `editor.rs:173` - Add visual indication of scroll position
- [ ] `editor.rs:411` - Add indicator when markers extend beyond viewport

**Hardcoded layout values:**
- [ ] `editor.rs:116` - Make side pane width (22) configurable
- [ ] `service_list.rs:84` - Make title width (30) dynamic
- [ ] `mod.rs:30` - Make command bar height responsive
- [ ] Modal dimensions (mod.rs:325, 364, 472, 601) - Calculate dynamically

#### Magic Numbers in Scoring (src/utils/file_matcher.rs)

20+ hardcoded score values:

| Line | Value | Purpose |
|------|-------|---------|
| 345 | 25000 | Reverse containment score |
| 349 | 22000 | Normalized reverse containment |
| 355, 358 | 20000, 19000 | Exact match scores |
| 361, 364 | 15000, 14000 | Prefix match scores |
| 367, 370 | 8000/800, 6000/600 | Query length conditional |
| 379, 383, 386 | 20000, 15000, 6000 | Composite query scores |
| 405, 415, 419 | 9000, 10000, 8000 | Special case scores |
| 546, 552, 555 | 3000, 2000 | Token scoring |
| 569-571 | 500, 300, 100 | Threshold values |

- [ ] Create `ScoringWeights` struct with named fields
- [ ] Document scoring strategy

#### Configuration Issues (src/config.rs)

- [ ] Add `validate()` method that fails early with actionable messages
- [ ] Add more ProPresenter installation paths (support PP8+, custom installs)
- [ ] Log warning for invalid DAYS_AHEAD instead of silent default
- [ ] Add doc comments with examples to each Config field

#### Missing Abstractions in ProPresenter Module

- [ ] Create `fn new_uuid() -> rv_data::Uuid` helper (repeated dozens of times)
- [ ] Extract `fn extract_regex_field<T>()` helper for rtf.rs:192-230
- [ ] Create `ProPresenterDefaults` struct for version/platform values

#### Error Handling Inconsistencies

**app.rs mixed patterns:**
- [ ] Line 463: Uses `format!` for error
- [ ] Line 1410: Returns early silently on `.is_none()`
- [ ] Line 1416: Sets error_message on `.is_none()`
- [ ] Standardize error handling pattern

**Silent failures:**
- [ ] `file_matcher.rs:179-180` - Log warnings for cache read/parse failures
- [ ] `config.rs:44` - Warn if `.env` exists but has parse errors

**Placeholder error handlers (app.rs:641-642):**
- [ ] Implement actual error handling for `{ /* ... error ... */ }` placeholders

---

### LOW Priority

#### Documentation Gaps

**Missing function documentation:**
- [ ] `convert.rs:711` - Document `convert_slide_to_rv_data()`
- [ ] `convert.rs:19-108` - Document `From` implementations
- [ ] `template.rs:208` - Document `estimate_visual_lines()` algorithm
- [ ] `template.rs:148` - Explain wrap_column in `split_content_for_slides()`

**Missing comments on magic numbers:**
- [ ] `convert.rs:878` - What does `info: 3` mean?
- [ ] `convert.rs:658-670` - Why these specific version numbers?
- [ ] `rtf.rs:70` - Add comment: RTF uses half-points

#### Dead/Commented Code to Remove

- [ ] `app.rs:218-221` - Remove commented debug eprintln statements
- [ ] `app.rs:1173-1175` - Remove commented verse marker commands
- [ ] `convert.rs:782` - Remove dead variable `_text_element_color`

#### Keyboard Shortcut Discoverability

- [ ] `editor.rs:411` - Add Tab/arrow hints for markers pane navigation
- [ ] `mod.rs:152, 576, 572-573` - Make command syntax documentation consistent
- [ ] `mod.rs:527-534` - Make F1/? more prominent in help modal

#### Performance Optimizations

- [ ] `file_matcher.rs:298-309` - Use adaptive threshold for parallelization (only if >5000 entries)
- [ ] `file_matcher.rs:314-317` - Return references instead of cloning FileEntry
- [ ] `file_matcher.rs:427` - Pre-convert paths to strings for HashMap lookup

#### Test Brittleness

- [ ] Replace `expect()` calls in tests with proper assertions
- [ ] Add setup/teardown for test fixtures
- [ ] Tests panic on missing test data files

#### Unsafe Index Access (app.rs)

- [ ] Line 1117: Add bounds check (`content.len() - 1` underflows if empty)
- [ ] Line 1139: Same issue
- [ ] Line 1162: Validate cursor_y before indexing

#### Minor Issues

- [ ] `config.rs:14-16` - Remove or use `_app_name` and `_app_version`
- [ ] `file_matcher.rs:30-39` - Consider computing lowercase variants on-demand
- [ ] `file_matcher.rs:134` - Add max depth to prevent infinite symlink loops
- [ ] `file_matcher.rs:25` - Document `.proflow_cache.json` in README

---

## Architecture Recommendations

### Suggested New Modules/Structs

1. **`src/modal.rs`** - Modal state machine
   - `ModalState` enum: None, Help, Error(String), Status(String), VersionPicker, PlaylistConfirm(usize)
   - `ModalManager` with `show()`, `dismiss()`, `is_blocking()` methods

2. **`src/navigation.rs`** - List navigation helpers
   - `ListNavigator` trait with `up()`, `down()`, `select()`, `clear()`
   - Generic implementation for ListState

3. **`src/services/file_search.rs`** - Extract from app.rs
   - Move `update_matching_files()` logic
   - Move liturgical mappings
   - Move search term generation

4. **`src/services/playlist_builder.rs`** - Extract from app.rs
   - Move `generate_playlist()` logic
   - Move entry collection methods

5. **`src/propresenter/defaults.rs`** - ProPresenter constants
   - Version strings, UUIDs, platform info
   - Default element values

### State Management Refactor

Current: 5 separate data stores for item state
- `item_states: ItemStateStore`
- `file_index.item_file_selections`
- `file_index.editor_states`
- `file_index.item_completion`
- `file_index.item_ignored`

Target: Single source of truth
- Move all persistence to ItemStateStore
- FileIndex only handles file indexing, not state

---

## Original High Priority Items

### Error Handling
- [x] Expand `Error` enum with specific variants (network, parse, file, config)
- [x] Add retry/backoff for Planning Center API failures
- [x] Clearer error messages with actionable context
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
- [x] Embed .pro presentations directly into .proplaylist zip (bundled like sample playlists)
- [x] Template-based slide generation with style injection
- [ ] Validate generated files against real ProPresenter imports
- [ ] Hex/proto diff tool to compare generated vs known-good .pro files

### UI / UX
- [x] Command/help modal with key cheatsheet per mode
- [ ] Add "Create New File" entry in matching files list (parity with `c`)
- [ ] Item filtering toggles for boilerplate "Other" items (configurable list, show/hide)
- [ ] Better empty-state/missing-match guidance (modal or inline message)
- [ ] Richer status bar (active library path, API state, current plan info)

---

## Template System Notes

- Template files named: `__template_<type>__.pro`
- Supported types: Scripture, Song, Info
- Templates in library path or `data/templates/`

## Debug Tools

- `cargo run --bin dump_pro -- <file.pro>` - Dump presentation structure
- `cargo run --bin dump_pro -- <file.pro> --json` - Output as JSON
- `cargo run --bin dump_pro -- <file1.pro> <file2.pro> --diff` - Compare two files
- `cargo run --bin test_template` - Test template-based generation
