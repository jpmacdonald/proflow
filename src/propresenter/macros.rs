//! Macro support for `ProPresenter` presentations.
//!
//! Loads the user's macro definitions from `ProPresenter`'s config, then injects
//! macro actions at caller-supplied cue-region boundaries.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use prost::Message;
use serde::Serialize;

use super::generated::rv_data::{self, action, CollectionElementType, Uuid};

/// Cached macro name → native collection identification loaded from
/// `ProPresenter`'s config.
pub struct MacroCache {
    macros: HashMap<String, InstalledMacro>,
}

struct InstalledMacro {
    identification: CollectionElementType,
    actions: Vec<MacroActionSummary>,
}

/// Read-only description of one installed macro.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct MacroSummary {
    pub name: String,
    /// Actions remain in native execution order.
    pub actions: Vec<MacroActionSummary>,
}

/// Read-only description of one action in an installed macro.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct MacroActionSummary {
    pub action_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

/// Failure to read or decode a configured `ProPresenter` macro document.
#[derive(Debug, thiserror::Error)]
pub enum MacroCacheLoadError {
    /// The macro document could not be read.
    #[error("failed to read macro document at {path}: {source}")]
    Read {
        /// Macro document path.
        path: PathBuf,
        /// Filesystem failure.
        source: std::io::Error,
    },
    /// The macro document was not valid `ProPresenter` protobuf data.
    #[error("failed to decode macro document at {path}: {source}")]
    Decode {
        /// Macro document path.
        path: PathBuf,
        /// Protobuf decoding failure.
        source: prost::DecodeError,
    },
    /// Two installed macros have the same canonical name.
    #[error("macro document at {path} contains ambiguous names '{first}' and '{duplicate}'")]
    DuplicateName {
        /// Macro document path.
        path: PathBuf,
        /// First installed spelling.
        first: String,
        /// Conflicting installed spelling.
        duplicate: String,
    },
}

/// Failure to apply a configured macro at rendered cue boundaries.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MacroApplyError {
    /// The requested macro is not installed.
    #[error("macro '{0}' is not available")]
    Unavailable(String),
    /// Render metadata referenced a cue that is not in its presentation.
    #[error("rendered cue entry {index} is outside the presentation's {cue_count} cues")]
    CueUnavailable {
        /// Invalid cue index.
        index: usize,
        /// Number of cues in the rendered presentation.
        cue_count: usize,
    },
}

impl MacroCache {
    /// Load macros from the default `ProPresenter` configuration path.
    ///
    /// A missing default file means macros are not installed. An existing but
    /// unreadable or malformed document is an error.
    pub fn load_default() -> Result<Self, MacroCacheLoadError> {
        let path = get_macros_path();
        path.map_or_else(|| Ok(Self::empty()), |path| Self::load_from(&path))
    }

    /// Load macros from a specific file path.
    pub fn load_from(path: &Path) -> Result<Self, MacroCacheLoadError> {
        load_macro_map(path).map(|macros| Self { macros })
    }

    /// Create an empty cache (no macros available).
    #[must_use]
    pub fn empty() -> Self {
        Self {
            macros: HashMap::new(),
        }
    }

    /// Look up a macro's native identification by its exact installed name.
    #[must_use]
    pub fn find(&self, name: &str) -> Option<(&str, &CollectionElementType)> {
        self.macros
            .get_key_value(name)
            .map(|(installed_name, installed)| (installed_name.as_str(), &installed.identification))
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

    /// Return installed macros and their native actions, sorted by exact name.
    #[must_use]
    pub(crate) fn summaries(&self) -> Vec<MacroSummary> {
        let mut summaries: Vec<_> = self
            .macros
            .iter()
            .map(|(name, installed)| MacroSummary {
                name: name.clone(),
                actions: installed.actions.clone(),
            })
            .collect();
        summaries.sort_by(|left, right| left.name.cmp(&right.name));
        summaries
    }
}

/// Resolve the default macros file path.
fn get_macros_path() -> Option<PathBuf> {
    if let Some(root) = std::env::var_os("PROPRESENTER_DIR") {
        let root = root.to_string_lossy();
        if !root.trim().is_empty() {
            let path = macros_path_for_root(&PathBuf::from(shellexpand::tilde(&root).to_string()));
            return path.is_file().then_some(path);
        }
    }
    let home = dirs::home_dir()?;
    let path = macros_path_for_root(&home.join("Documents/ProPresenter"));
    path.is_file().then_some(path)
}

fn macros_path_for_root(root: &Path) -> PathBuf {
    root.join("Configuration/Macros")
}

/// Load installed macro identity and action summaries from one protobuf read.
fn load_macro_map(path: &Path) -> Result<HashMap<String, InstalledMacro>, MacroCacheLoadError> {
    let mut map = HashMap::new();
    let mut canonical_names = HashMap::<String, String>::new();
    let data = std::fs::read(path).map_err(|source| MacroCacheLoadError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let doc = rv_data::MacrosDocument::decode(data.as_slice()).map_err(|source| {
        MacroCacheLoadError::Decode {
            path: path.to_path_buf(),
            source,
        }
    })?;
    let parent_collections = doc
        .macro_collections
        .iter()
        .flat_map(|collection| {
            collection.items.iter().filter_map(move |item| {
                let rv_data::macros_document::macro_collection::item::ItemType::MacroId(macro_id) =
                    item.item_type.as_ref()?;
                Some((
                    macro_id.string.as_str(),
                    CollectionElementType {
                        parameter_uuid: collection.uuid.clone(),
                        parameter_name: collection.name.clone(),
                        parent_collection: None,
                    },
                ))
            })
        })
        .collect::<HashMap<_, _>>();

    for m in &doc.macros {
        if let Some(ref uuid) = m.uuid {
            if !m.name.is_empty() {
                let canonical = m.name.to_lowercase();
                if let Some(first) = canonical_names.insert(canonical, m.name.clone()) {
                    return Err(MacroCacheLoadError::DuplicateName {
                        path: path.to_path_buf(),
                        first,
                        duplicate: m.name.clone(),
                    });
                }
                map.insert(
                    m.name.clone(),
                    InstalledMacro {
                        identification: CollectionElementType {
                            parameter_uuid: Some(uuid.clone()),
                            parameter_name: m.name.clone(),
                            parent_collection: parent_collections
                                .get(uuid.string.as_str())
                                .cloned()
                                .map(Box::new),
                        },
                        actions: m.actions.iter().map(summarize_macro_action).collect(),
                    },
                );
            }
        }
    }
    Ok(map)
}

fn summarize_macro_action(native: &rv_data::Action) -> MacroActionSummary {
    match native.action_type_data.as_ref() {
        Some(action::ActionTypeData::Stage(stage)) => MacroActionSummary {
            action_type: "stage_layout".to_string(),
            target: stage_layout_target(stage),
        },
        Some(action::ActionTypeData::AudienceLook(look)) => MacroActionSummary {
            action_type: "audience_look".to_string(),
            target: identification_name(look.identification.as_ref()),
        },
        Some(action::ActionTypeData::ClearGroup(group)) => MacroActionSummary {
            action_type: "clear_group".to_string(),
            target: identification_name(group.identification.as_ref()),
        },
        Some(data) => MacroActionSummary {
            action_type: action_data_name(data).to_string(),
            target: None,
        },
        None => MacroActionSummary {
            action_type: declared_action_type_name(native.r#type),
            target: None,
        },
    }
}

fn stage_layout_target(stage: &action::StageLayoutType) -> Option<String> {
    let assignments: Vec<_> = stage
        .stage_screen_assignments
        .iter()
        .filter_map(|assignment| {
            let screen = identification_name(assignment.screen.as_ref());
            let layout = identification_name(assignment.layout.as_ref());
            match (screen, layout) {
                (Some(screen), Some(layout)) => Some(format!("{screen} → {layout}")),
                (Some(screen), None) => Some(screen),
                (None, Some(layout)) => Some(layout),
                (None, None) => None,
            }
        })
        .collect();
    (!assignments.is_empty()).then(|| assignments.join(", "))
}

fn identification_name(identification: Option<&CollectionElementType>) -> Option<String> {
    identification
        .map(|identification| identification.parameter_name.trim())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

const fn action_data_name(data: &action::ActionTypeData) -> &'static str {
    match data {
        action::ActionTypeData::CollectionElement(_) => "collection_element",
        action::ActionTypeData::PlaylistItem(_) => "playlist_item",
        action::ActionTypeData::BlendMode(_) => "blend_mode",
        action::ActionTypeData::Transition(_) => "transition",
        action::ActionTypeData::Media(_) => "media",
        action::ActionTypeData::DoubleItem(_) => "double_item",
        action::ActionTypeData::Effects(_) => "effects",
        action::ActionTypeData::Slide(_) => "slide",
        action::ActionTypeData::Background(_) => "background",
        action::ActionTypeData::Timer(_) => "timer",
        action::ActionTypeData::Clear(_) => "clear",
        action::ActionTypeData::Stage(_) => "stage_layout",
        action::ActionTypeData::Prop(_) => "prop",
        action::ActionTypeData::Mask(_) => "mask",
        action::ActionTypeData::Message(_) => "message",
        action::ActionTypeData::Communication(_) => "communication",
        action::ActionTypeData::MultiScreen(_) => "multi_screen",
        action::ActionTypeData::PresentationDocument(_) => "presentation_document",
        action::ActionTypeData::ExternalPresentation(_) => "external_presentation",
        action::ActionTypeData::AudienceLook(_) => "audience_look",
        action::ActionTypeData::AudioInput(_) => "audio_input",
        action::ActionTypeData::SlideDestination(_) => "slide_destination",
        action::ActionTypeData::Macro(_) => "macro",
        action::ActionTypeData::ClearGroup(_) => "clear_group",
        action::ActionTypeData::TransportControl(_) => "transport_control",
        action::ActionTypeData::Capture(_) => "capture",
    }
}

fn declared_action_type_name(value: i32) -> String {
    action::ActionType::try_from(value).map_or_else(
        |_| format!("unknown({value})"),
        |action_type| {
            action_type
                .as_str_name()
                .strip_prefix("ACTION_TYPE_")
                .unwrap_or("UNKNOWN")
                .to_ascii_lowercase()
        },
    )
}

/// Create a macro action that triggers the named macro.
///
/// `ProPresenter` represents this as an `Action` with type `Macro` containing
/// a `CollectionElementType` that identifies the macro by UUID and name.
pub fn make_macro_action(macro_name: &str, macro_uuid: &str) -> rv_data::Action {
    make_macro_action_from_identification(CollectionElementType {
        parameter_uuid: Some(Uuid {
            string: macro_uuid.to_string(),
        }),
        parameter_name: macro_name.to_string(),
        parent_collection: None,
    })
}

fn make_macro_action_from_identification(identification: CollectionElementType) -> rv_data::Action {
    rv_data::Action {
        uuid: Some(Uuid {
            string: uuid::Uuid::new_v4().to_string(),
        }),
        name: identification.parameter_name.clone(),
        label: None,
        delay_time: 0.0,
        old_type: None,
        is_enabled: true,
        layer_identification: None,
        duration: 0.0,
        r#type: action::ActionType::Macro as i32,
        action_type_data: Some(action::ActionTypeData::Macro(action::MacroType {
            identification: Some(identification),
        })),
    }
}

/// Add a macro action to each rendered cue-region entry.
///
/// The renderer owns role detection and supplies exact cue indices. Every index
/// is validated before any cue is changed, so stale render metadata cannot
/// partially mutate a presentation.
pub fn add_macro_to_cue_entries(
    presentation: &mut rv_data::Presentation,
    cue_indices: &[usize],
    macro_name: &str,
    cache: &MacroCache,
) -> Result<(), MacroApplyError> {
    let Some((installed_name, identification)) = cache.find(macro_name) else {
        return Err(MacroApplyError::Unavailable(macro_name.to_string()));
    };
    let cue_count = presentation.cues.len();
    if let Some(&index) = cue_indices.iter().find(|&&index| index >= cue_count) {
        return Err(MacroApplyError::CueUnavailable { index, cue_count });
    }

    for &index in cue_indices {
        let cue = &mut presentation.cues[index];
        if !cue_has_macro_named(cue, installed_name) {
            debug_assert_eq!(installed_name, identification.parameter_name);
            cue.actions.push(make_macro_action_from_identification(
                identification.clone(),
            ));
        }
    }
    Ok(())
}

/// Return the macro name referenced by an action, if it is a macro action.
#[must_use]
pub fn macro_action_name(action: &rv_data::Action) -> Option<&str> {
    let Some(action::ActionTypeData::Macro(macro_type)) = &action.action_type_data else {
        return None;
    };
    macro_type
        .identification
        .as_ref()
        .map(|id| id.parameter_name.as_str())
}

/// Return whether a cue already has a macro with the exact configured name.
#[must_use]
pub fn cue_has_macro_named(cue: &rv_data::Cue, macro_name: &str) -> bool {
    cue.actions
        .iter()
        .any(|action| macro_action_name(action).is_some_and(|name| name == macro_name))
}

/// Return whether a cue already has any macro whose name begins with `prefix`.
#[must_use]
pub fn cue_has_macro_prefix(cue: &rv_data::Cue, prefix: &str) -> bool {
    let prefix = prefix.to_lowercase();
    cue.actions.iter().any(|action| {
        macro_action_name(action).is_some_and(|name| name.to_lowercase().starts_with(&prefix))
    })
}

/// Remove all macro actions from a cue.
pub fn remove_macro_actions(cue: &mut rv_data::Cue) {
    cue.actions.retain(|action| {
        !matches!(
            &action.action_type_data,
            Some(action::ActionTypeData::Macro(_))
        )
    });
}

/// Remove macro actions whose name begins with `prefix`.
pub fn remove_macro_prefix_actions(cue: &mut rv_data::Cue, prefix: &str) -> bool {
    let prefix = prefix.to_lowercase();
    let before = cue.actions.len();
    cue.actions.retain(|action| {
        !macro_action_name(action).is_some_and(|name| name.to_lowercase().starts_with(&prefix))
    });
    cue.actions.len() != before
}

/// Ensure a cue has the named macro, preserving any existing macro actions.
/// Returns true when a macro action was added.
pub fn ensure_macro_on_cue(cue: &mut rv_data::Cue, macro_name: &str, cache: &MacroCache) -> bool {
    if cue_has_macro_named(cue, macro_name) {
        return false;
    }
    let Some((name, identification)) = cache.find(macro_name) else {
        return false;
    };
    debug_assert_eq!(name, identification.parameter_name);
    cue.actions.push(make_macro_action_from_identification(
        identification.clone(),
    ));
    true
}

/// Replace any macro actions on a cue with exactly the named macro.
pub fn replace_macro_on_cue(cue: &mut rv_data::Cue, macro_name: &str, cache: &MacroCache) -> bool {
    if cue_has_macro_named(cue, macro_name)
        && cue
            .actions
            .iter()
            .filter(|action| macro_action_name(action).is_some())
            .count()
            == 1
    {
        return false;
    }
    let Some((name, identification)) = cache.find(macro_name) else {
        return false;
    };
    remove_macro_actions(cue);
    debug_assert_eq!(name, identification.parameter_name);
    cue.actions.push(make_macro_action_from_identification(
        identification.clone(),
    ));
    true
}

/// Ensure a cue has a macro matching a prefix, adding `macro_name` only if none exists.
pub fn ensure_macro_prefix_on_cue(
    cue: &mut rv_data::Cue,
    prefix: &str,
    macro_name: &str,
    cache: &MacroCache,
) -> bool {
    if cue_has_macro_prefix(cue, prefix) {
        return false;
    }
    ensure_macro_on_cue(cue, macro_name, cache)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn identification(name: &str, id: &str) -> CollectionElementType {
        CollectionElementType {
            parameter_uuid: Some(Uuid {
                string: id.to_string(),
            }),
            parameter_name: name.to_string(),
            parent_collection: None,
        }
    }

    fn installed_macro(name: &str, id: &str) -> InstalledMacro {
        InstalledMacro {
            identification: identification(name, id),
            actions: Vec::new(),
        }
    }

    #[test]
    fn macro_is_added_to_each_explicit_region_entry() {
        let cache = MacroCache {
            macros: HashMap::from([(
                "Scripture/Prayer".to_string(),
                installed_macro("Scripture/Prayer", "00000000-0000-0000-0000-000000000001"),
            )]),
        };
        let mut presentation = rv_data::Presentation {
            cues: vec![
                rv_data::Cue::default(),
                rv_data::Cue::default(),
                rv_data::Cue::default(),
            ],
            ..rv_data::Presentation::default()
        };

        add_macro_to_cue_entries(&mut presentation, &[0, 2], "Scripture/Prayer", &cache)
            .expect("valid cue entries");

        assert_eq!(presentation.cues[0].actions.len(), 1);
        assert!(presentation.cues[1].actions.is_empty());
        assert_eq!(presentation.cues[2].actions.len(), 1);
    }

    #[test]
    fn title_and_content_macros_follow_rendered_role_boundaries() {
        let cache = MacroCache {
            macros: HashMap::from([
                (
                    "Title".to_string(),
                    installed_macro("Title", "00000000-0000-0000-0000-000000000001"),
                ),
                (
                    "Content".to_string(),
                    installed_macro("Content", "00000000-0000-0000-0000-000000000002"),
                ),
            ]),
        };
        let mut presentation = rv_data::Presentation {
            // title, content, divider, title, content
            cues: vec![rv_data::Cue::default(); 5],
            ..rv_data::Presentation::default()
        };

        add_macro_to_cue_entries(&mut presentation, &[0, 3], "Title", &cache)
            .expect("valid title entries");
        add_macro_to_cue_entries(&mut presentation, &[1, 4], "Content", &cache)
            .expect("valid content entries");

        assert!(cue_has_macro_named(&presentation.cues[0], "Title"));
        assert!(cue_has_macro_named(&presentation.cues[1], "Content"));
        assert!(presentation.cues[2].actions.is_empty());
        assert!(cue_has_macro_named(&presentation.cues[3], "Title"));
        assert!(cue_has_macro_named(&presentation.cues[4], "Content"));
    }

    #[test]
    fn invalid_region_entry_does_not_partially_apply_macros() {
        let cache = MacroCache {
            macros: HashMap::from([("Title".to_string(), installed_macro("Title", "macro-id"))]),
        };
        let mut presentation = rv_data::Presentation {
            cues: vec![rv_data::Cue::default()],
            ..rv_data::Presentation::default()
        };

        let error = add_macro_to_cue_entries(&mut presentation, &[0, 1], "Title", &cache)
            .expect_err("stale cue metadata must fail");

        assert_eq!(
            error,
            MacroApplyError::CueUnavailable {
                index: 1,
                cue_count: 1
            }
        );
        assert!(presentation.cues[0].actions.is_empty());
    }

    #[test]
    fn malformed_macro_document_is_an_error() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("Macros");
        std::fs::write(&path, b"not a protobuf document").expect("write malformed document");

        assert!(matches!(
            MacroCache::load_from(&path),
            Err(MacroCacheLoadError::Decode { .. })
        ));
    }

    #[test]
    fn duplicate_canonical_macro_names_are_rejected() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("Macros");
        let installed_macro = |name: &str, id: &str| rv_data::macros_document::Macro {
            uuid: Some(rv_data::Uuid {
                string: id.to_string(),
            }),
            name: name.to_string(),
            color: None,
            actions: Vec::new(),
            trigger_on_startup: false,
            image_type: 0,
            image_data: Vec::new(),
        };
        let document = rv_data::MacrosDocument {
            application_info: None,
            macros: vec![
                installed_macro("Scripture/Prayer", "one"),
                installed_macro("scripture/prayer", "two"),
            ],
            macro_collections: Vec::new(),
        };
        std::fs::write(&path, document.encode_to_vec()).expect("write macro document");

        assert!(matches!(
            MacroCache::load_from(&path),
            Err(MacroCacheLoadError::DuplicateName { .. })
        ));
    }

    #[test]
    fn macro_lookup_requires_the_exact_installed_name() {
        let cache = MacroCache {
            macros: HashMap::from([("Title".to_string(), installed_macro("Title", "one"))]),
        };

        assert!(cache.find("Title").is_some());
        assert!(cache.find("title").is_none());

        let mut cue = rv_data::Cue {
            actions: vec![make_macro_action("title", "lowercase")],
            ..rv_data::Cue::default()
        };
        assert!(ensure_macro_on_cue(&mut cue, "Title", &cache));
        assert!(cue_has_macro_named(&cue, "Title"));
    }

    #[test]
    fn loaded_macro_action_preserves_collection_and_native_name() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("Macros");
        let macro_id = rv_data::Uuid {
            string: "macro-id".to_string(),
        };
        let document = rv_data::MacrosDocument {
            application_info: None,
            macros: vec![rv_data::macros_document::Macro {
                uuid: Some(macro_id.clone()),
                name: "Scripture/Prayer".to_string(),
                color: None,
                actions: Vec::new(),
                trigger_on_startup: false,
                image_type: 0,
                image_data: Vec::new(),
            }],
            macro_collections: vec![rv_data::macros_document::MacroCollection {
                uuid: Some(rv_data::Uuid {
                    string: "collection-id".to_string(),
                }),
                name: "Default Collection".to_string(),
                items: vec![rv_data::macros_document::macro_collection::Item {
                    item_type: Some(
                        rv_data::macros_document::macro_collection::item::ItemType::MacroId(
                            macro_id,
                        ),
                    ),
                }],
            }],
        };
        std::fs::write(&path, document.encode_to_vec()).expect("write macro document");
        let cache = MacroCache::load_from(&path).expect("load native macro document");
        let mut cue = rv_data::Cue::default();

        assert!(ensure_macro_on_cue(&mut cue, "Scripture/Prayer", &cache));
        let action = cue.actions.first().expect("macro action");
        assert_eq!(action.name, "Scripture/Prayer");
        assert!(matches!(
            action.action_type_data,
            Some(action::ActionTypeData::Macro(_))
        ));
        let Some(action::ActionTypeData::Macro(macro_type)) = &action.action_type_data else {
            return;
        };
        let parent = macro_type
            .identification
            .as_ref()
            .and_then(|identification| identification.parent_collection.as_deref())
            .expect("parent collection");
        assert_eq!(parent.parameter_name, "Default Collection");
        assert_eq!(
            parent
                .parameter_uuid
                .as_ref()
                .map(|uuid| uuid.string.as_str()),
            Some("collection-id")
        );
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "the native-order macro fixture stays beside its complete expected action summary"
    )]
    fn loaded_summaries_preserve_action_order_targets_and_unknown_types() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("Macros");
        let named = |name: &str| CollectionElementType {
            parameter_uuid: None,
            parameter_name: name.to_string(),
            parent_collection: None,
        };
        let stage = rv_data::Action {
            r#type: action::ActionType::StageLayout as i32,
            action_type_data: Some(action::ActionTypeData::Stage(action::StageLayoutType {
                stage_screen_assignments: vec![rv_data::stage::ScreenAssignment {
                    screen: Some(named("Stage Display")),
                    layout: Some(named("Song Stage")),
                }],
                slide_target: action::stage_layout_type::SlideTarget::NoChange as i32,
            })),
            ..rv_data::Action::default()
        };
        let audience_look = rv_data::Action {
            r#type: action::ActionType::AudienceLook as i32,
            action_type_data: Some(action::ActionTypeData::AudienceLook(
                action::AudienceLookType {
                    identification: Some(named("Song Look")),
                },
            )),
            ..rv_data::Action::default()
        };
        let clear_group = rv_data::Action {
            r#type: action::ActionType::ClearGroup as i32,
            action_type_data: Some(action::ActionTypeData::ClearGroup(action::ClearGroupType {
                identification: Some(named("Video")),
            })),
            ..rv_data::Action::default()
        };
        let unknown = rv_data::Action {
            r#type: 997,
            ..rv_data::Action::default()
        };
        let transport = rv_data::Action {
            action_type_data: Some(action::ActionTypeData::TransportControl(
                action::TransportControlType::default(),
            )),
            ..rv_data::Action::default()
        };
        let macro_definition =
            |name: &str, id: &str, actions: Vec<rv_data::Action>| rv_data::macros_document::Macro {
                uuid: Some(Uuid {
                    string: id.to_string(),
                }),
                name: name.to_string(),
                color: None,
                actions,
                trigger_on_startup: false,
                image_type: 0,
                image_data: Vec::new(),
            };
        let document = rv_data::MacrosDocument {
            application_info: None,
            macros: vec![
                macro_definition("Zulu", "z", vec![]),
                macro_definition(
                    "Alpha",
                    "a",
                    vec![stage, audience_look, clear_group, unknown, transport],
                ),
            ],
            macro_collections: Vec::new(),
        };
        std::fs::write(&path, document.encode_to_vec()).expect("write macro document");

        let summaries = MacroCache::load_from(&path)
            .expect("load native macro document")
            .summaries();

        assert_eq!(
            summaries
                .iter()
                .map(|summary| summary.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Alpha", "Zulu"]
        );
        assert_eq!(
            summaries[0].actions,
            vec![
                MacroActionSummary {
                    action_type: "stage_layout".to_string(),
                    target: Some("Stage Display → Song Stage".to_string()),
                },
                MacroActionSummary {
                    action_type: "audience_look".to_string(),
                    target: Some("Song Look".to_string()),
                },
                MacroActionSummary {
                    action_type: "clear_group".to_string(),
                    target: Some("Video".to_string()),
                },
                MacroActionSummary {
                    action_type: "unknown(997)".to_string(),
                    target: None,
                },
                MacroActionSummary {
                    action_type: "transport_control".to_string(),
                    target: None,
                },
            ]
        );
    }

    #[test]
    fn macro_document_path_is_relative_to_propresenter_root() {
        assert_eq!(
            macros_path_for_root(Path::new("/custom/ProPresenter")),
            Path::new("/custom/ProPresenter/Configuration/Macros")
        );
    }
}
