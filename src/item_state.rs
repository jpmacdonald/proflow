//! Per-item persistent state management.
//!
//! This module consolidates all per-item state into a single store, replacing
//! the multiple `HashMap`s that previously tracked item completion, ignored status,
//! matched files, editor state, and slide types.

use crate::app::EditorState;
use crate::types::{ItemId, SlideType};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

impl ItemState {
    /// Create a new empty `ItemState`.
    pub const fn new() -> Self {
        Self {
            completed: false,
            ignored: false,
            matched_file: None,
            editor: None,
            slide_type: None,
        }
    }

    /// Check if this item has any meaningful state that should be persisted.
    pub const fn has_content(&self) -> bool {
        self.completed
            || self.ignored
            || self.matched_file.is_some()
            || self.editor.is_some()
            || self.slide_type.is_some()
    }

    /// Reset all state for this item.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
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

    /// Update state for an item using a closure.
    pub fn update<F>(&mut self, id: &ItemId, f: F)
    where
        F: FnOnce(&mut ItemState),
    {
        f(self.get_mut(id));
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

    /// Remove items that have no meaningful state.
    pub fn compact(&mut self) {
        self.states.retain(|_, state| state.has_content());
    }

    /// Get an iterator over all item IDs with state.
    pub fn item_ids(&self) -> impl Iterator<Item = &ItemId> {
        self.states.keys()
    }

    /// Get the number of items with state.
    pub fn len(&self) -> usize {
        self.states.len()
    }

    /// Check if the store is empty.
    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }

    /// Remove state for a specific item.
    pub fn remove(&mut self, id: &ItemId) -> Option<ItemState> {
        self.states.remove(id)
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
        assert!(!state.has_content());
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

    #[test]
    fn test_store_update() {
        let mut store = ItemStateStore::new();
        let id = ItemId::new("test-item");

        store.update(&id, |state| {
            state.completed = true;
            state.ignored = true;
        });

        assert!(store.is_completed(&id));
        assert!(store.is_ignored(&id));
    }

    #[test]
    fn test_store_compact() {
        let mut store = ItemStateStore::new();
        let id1 = ItemId::new("item1");
        let id2 = ItemId::new("item2");

        // id1 has content
        store.set_completed(&id1, true);

        // id2 was created but has no meaningful state
        let _ = store.get_mut(&id2);

        assert_eq!(store.len(), 2);

        store.compact();

        assert_eq!(store.len(), 1);
        assert!(store.get(&id1).is_some());
        assert!(store.get(&id2).is_none());
    }
}
