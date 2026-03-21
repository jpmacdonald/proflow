//! Per-item persistent state management.
//!
//! This module consolidates all per-item state into a single store, replacing
//! the multiple `HashMap`s that previously tracked item completion, ignored status,
//! matched files, editor state, and slide types.

use crate::app::EditorState;
use crate::planning_center::types::ItemId;
use crate::propresenter::SlideType;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// All persistent state for a single `Planning Center` item.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ItemState {
    /// Item marked complete (matched to file or custom created).
    #[serde(default)]
    pub completed: bool,

    /// Item excluded from playlist generation.
    #[serde(default)]
    pub ignored: bool,

    /// Path to matched .pro file, if any.
    #[serde(default)]
    pub matched_file: Option<String>,

    /// Custom editor content for slide creation.
    #[serde(default)]
    pub editor: Option<EditorState>,

    /// Slide type override.
    #[serde(default)]
    pub slide_type: Option<SlideType>,
}

/// Thread-safe item state store with persistence support.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ItemStateStore {
    states: HashMap<ItemId, ItemState>,
}

impl ItemStateStore {
    /// Create a new empty store.
    pub fn new() -> Self {
        Self {
            states: HashMap::new(),
        }
    }

    /// Get the state for an item, if it exists.
    pub fn get(&self, id: &ItemId) -> Option<&ItemState> {
        self.states.get(id)
    }

    /// Get a mutable reference to the state for an item, creating it if needed.
    pub fn get_mut(&mut self, id: &ItemId) -> &mut ItemState {
        self.states.entry(id.clone()).or_default()
    }

    /// Check if an item is completed.
    pub fn is_completed(&self, id: &ItemId) -> bool {
        self.get(id).is_some_and(|s| s.completed)
    }

    /// Set the completed status for an item.
    pub fn set_completed(&mut self, id: &ItemId, completed: bool) {
        self.get_mut(id).completed = completed;
    }

    /// Check if an item is ignored.
    pub fn is_ignored(&self, id: &ItemId) -> bool {
        self.get(id).is_some_and(|s| s.ignored)
    }

    /// Set the ignored status for an item.
    pub fn set_ignored(&mut self, id: &ItemId, ignored: bool) {
        self.get_mut(id).ignored = ignored;
    }

    /// Get the matched file path for an item.
    pub fn get_matched_file(&self, id: &ItemId) -> Option<&str> {
        self.get(id).and_then(|s| s.matched_file.as_deref())
    }

    /// Set the matched file path for an item.
    pub fn set_matched_file(&mut self, id: &ItemId, path: Option<String>) {
        self.get_mut(id).matched_file = path;
    }

    /// Get the editor state for an item.
    pub fn get_editor(&self, id: &ItemId) -> Option<&EditorState> {
        self.get(id).and_then(|s| s.editor.as_ref())
    }

    /// Set the editor state for an item.
    pub fn set_editor(&mut self, id: &ItemId, editor: Option<EditorState>) {
        self.get_mut(id).editor = editor;
    }

    /// Get the slide type for an item.
    pub fn get_slide_type(&self, id: &ItemId) -> Option<SlideType> {
        self.get(id).and_then(|s| s.slide_type)
    }

    /// Set the slide type for an item.
    pub fn set_slide_type(&mut self, id: &ItemId, slide_type: Option<SlideType>) {
        self.get_mut(id).slide_type = slide_type;
    }

    /// Clear all state (for reload).
    pub fn clear(&mut self) {
        self.states.clear();
    }

    /// Application cache directory (`~/Library/Application Support/proflow/` on macOS).
    /// Creates the directory if it does not exist.
    pub fn cache_dir() -> Option<PathBuf> {
        let dir = dirs::data_dir()?.join("proflow");
        std::fs::create_dir_all(&dir).ok()?;
        Some(dir)
    }

    /// Load persisted item states from `{cache_dir}/item_states.json`.
    /// Returns a default (empty) store if the file is missing or corrupt.
    pub fn load(cache_dir: &Path) -> Self {
        let path = cache_dir.join("item_states.json");
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|data| serde_json::from_str(&data).ok())
            .unwrap_or_default()
    }

    /// Persist item states to `{cache_dir}/item_states.json`.
    /// Silently ignores write errors (same strategy as `FileIndex`).
    pub fn persist(&self, cache_dir: &Path) {
        let path = cache_dir.join("item_states.json");
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_item_state_default() {
        let state = ItemState::default();
        assert!(!state.completed);
        assert!(!state.ignored);
        assert!(state.matched_file.is_none());
        assert!(state.editor.is_none());
        assert!(state.slide_type.is_none());
    }

    #[test]
    fn test_store_get_mut_creates_entry() {
        let mut store = ItemStateStore::new();
        let id = ItemId::new("test-item");

        // Should not exist yet
        assert!(store.get(&id).is_none());

        // get_mut should create it
        store.get_mut(&id).completed = true;

        // Should exist now
        assert!(store.get(&id).is_some());
        assert!(store.is_completed(&id));
    }
}
