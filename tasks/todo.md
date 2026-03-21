# Replace Template System with ProPresenter Theme Files

## Step 1: ThemeCache in template.rs ✅
- [x] Add `ThemeCache` struct keyed by slide name
- [x] Add `load_theme()` and `theme_slide_to_presentation_slide()` conversion
- [x] Keep `TemplateType` for backwards compat fallback
- [x] Change `build_presentation_from_template_with_options` to take `&PresentationSlide`
- [x] Remove `extract_template_slide()` from public API (now private, legacy-only)

## Step 2: Update callers ✅
- [x] `src/mcp/mod.rs` — use slide name from config, pass `&PresentationSlide`
- [x] `src/playlist_gen.rs` — update template lookup
- [x] `src/app.rs` — update `ThemeCache::new()` init + export logic

## Step 3: Config update ✅
- [x] Add `theme` field to `ItemMappings` in `preview.rs`
- [x] Add `template_name` field to `PreviewEntry` for type→slide resolution
- [x] Update `data/item_mappings.json` with theme name and slide names
- [x] Update `render_context()` to show theme info

## Step 4: Update bins ✅
- [x] `src/bin/test_template.rs` — use theme system
- [x] `src/bin/dump_theme.rs` — clean up clippy warnings

## Step 5: Verify ✅
- [x] `cargo build` clean
- [x] `cargo clippy` clean
- [x] `cargo test` — 78 passed, 0 failed
- [x] VPC Theme loads all 13 slides with correct styling
- [x] Legacy `.pro` template path still works as fallback
