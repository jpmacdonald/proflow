//! Macro support for `ProPresenter` presentations.
//!
//! Loads the user's macro definitions from `ProPresenter`'s config, then injects
//! a macro action on the first cue of generated presentations — same pattern as
//! background images.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use prost::Message;

use super::generated::rv_data::{self, action, CollectionElementType, Uuid};

/// Cached macro name → UUID mapping loaded from `ProPresenter`'s config.
pub struct MacroCache {
    macros: HashMap<String, String>,
}

impl MacroCache {
    /// Load macros from the default `ProPresenter` configuration path.
    ///
    /// Returns an empty cache if the file doesn't exist or can't be decoded.
    pub fn load_default() -> Self {
        let path = get_macros_path();
        path.map_or_else(Self::empty, |p| Self::load_from(&p))
    }

    /// Load macros from a specific file path.
    pub fn load_from(path: &Path) -> Self {
        let macros = load_macro_map(path);
        Self { macros }
    }

    /// Create an empty cache (no macros available).
    #[must_use]
    pub fn empty() -> Self {
        Self {
            macros: HashMap::new(),
        }
    }

    /// Look up a macro UUID by name (case-insensitive).
    #[must_use]
    pub fn find(&self, name: &str) -> Option<(&str, &str)> {
        let target = name.to_lowercase();
        self.macros
            .iter()
            .find(|(k, _)| k.to_lowercase() == target)
            .map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Return the number of loaded macros.
    #[must_use]
    pub fn len(&self) -> usize {
        self.macros.len()
    }

    /// Whether the cache is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.macros.is_empty()
    }

    /// Return all macro names (sorted).
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.macros.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    }
}

/// Resolve the default macros file path.
fn get_macros_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let path = home.join("Documents/ProPresenter/Configuration/Macros");
    path.exists().then_some(path)
}

/// Load macro name → UUID map from a `MacrosDocument` protobuf file.
fn load_macro_map(path: &Path) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Ok(data) = std::fs::read(path) else {
        return map;
    };
    let Ok(doc) = rv_data::MacrosDocument::decode(data.as_slice()) else {
        return map;
    };
    for m in &doc.macros {
        if let Some(ref uuid) = m.uuid {
            if !m.name.is_empty() {
                map.insert(m.name.clone(), uuid.string.clone());
            }
        }
    }
    map
}

/// Create a macro action that triggers the named macro.
///
/// `ProPresenter` represents this as an `Action` with type `Macro` containing
/// a `CollectionElementType` that identifies the macro by UUID and name.
pub fn make_macro_action(macro_name: &str, macro_uuid: &str) -> rv_data::Action {
    rv_data::Action {
        uuid: Some(Uuid {
            string: uuid::Uuid::new_v4().to_string(),
        }),
        name: String::new(),
        label: None,
        delay_time: 0.0,
        old_type: None,
        is_enabled: true,
        layer_identification: None,
        duration: 0.0,
        r#type: action::ActionType::Macro as i32,
        action_type_data: Some(action::ActionTypeData::Macro(action::MacroType {
            identification: Some(CollectionElementType {
                parameter_uuid: Some(Uuid {
                    string: macro_uuid.to_string(),
                }),
                parameter_name: macro_name.to_string(),
                parent_collection: None,
            }),
        })),
    }
}

/// Add a macro action to the first cue of a presentation.
///
/// Looks up the macro by name in the cache. No-op if the macro isn't found
/// or the presentation has no cues.
pub fn add_macro_to_first_cue(
    presentation: &mut rv_data::Presentation,
    macro_name: &str,
    cache: &MacroCache,
) {
    let Some((name, uuid)) = cache.find(macro_name) else {
        return;
    };
    let Some(first_cue) = presentation.cues.first_mut() else {
        return;
    };
    first_cue.actions.push(make_macro_action(name, uuid));
}
