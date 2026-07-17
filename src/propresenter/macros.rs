//! Macro support for `ProPresenter` presentations.
//!
//! Loads the user's macro definitions from `ProPresenter`'s config, then injects
//! macro actions at caller-supplied cue-region boundaries.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use prost::Message;
use serde::Serialize;

use super::generated::rv_data::{self, action, CollectionElementType, Uuid};
use crate::workflow::plan::{RestyleMacroPolicy, RestyleMacroSelector};

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
    /// The configured path exists but is not a regular file.
    #[error("macro document path is not a regular file: {path}")]
    NotRegular {
        /// Configured macro document path.
        path: PathBuf,
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
    /// The presentation has no operator-visible cue on which a macro can run.
    #[error("presentation has no operator-visible cue")]
    MissingOperatorCue,
    /// A configured native region could not be resolved.
    #[error("macro region {region} is unavailable: {selector}")]
    RegionUnavailable {
        /// Zero-based configured region occurrence.
        region: usize,
        /// Human-readable selector that failed to resolve.
        selector: String,
    },
    /// A selected arrangement group had an unexpected exact native name.
    #[error(
        "macro region {region} expected arrangement group {index} to be one of {allowed:?}, found '{actual}'"
    )]
    UnexpectedGroup {
        /// Zero-based configured region occurrence.
        region: usize,
        /// Zero-based selected-arrangement group occurrence.
        index: usize,
        /// Exact native group name found in the presentation.
        actual: String,
        /// Exact native group names accepted by the policy.
        allowed: Vec<String>,
    },
    /// Two configured regions resolved to the same native cue.
    #[error("macro regions resolve more than once to cue {cue_index}")]
    DuplicateRegionTarget {
        /// Native presentation cue index targeted more than once.
        cue_index: usize,
    },
}

impl MacroCache {
    /// Load one explicitly selected macro document, or an empty catalog when
    /// the workstation has no macro file.
    ///
    /// The caller owns path discovery. A path that exists but cannot be read or
    /// decoded remains an error; absence is the only empty-catalog state.
    pub fn load_optional(path: &Path) -> Result<Self, MacroCacheLoadError> {
        let metadata = match std::fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::empty());
            }
            Err(source) => {
                return Err(MacroCacheLoadError::Read {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };
        if metadata.file_type().is_symlink() {
            return match std::fs::metadata(path) {
                Ok(target) if target.is_file() => Self::load_from(path),
                Ok(_) => Err(MacroCacheLoadError::NotRegular {
                    path: path.to_path_buf(),
                }),
                Err(source) => Err(MacroCacheLoadError::Read {
                    path: path.to_path_buf(),
                    source,
                }),
            };
        }
        if !metadata.is_file() {
            return Err(MacroCacheLoadError::NotRegular {
                path: path.to_path_buf(),
            });
        }
        Self::load_from(path)
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

/// Replace all macro actions on one native entry cue with the configured macro.
///
/// A matching native action keeps its wrapper identity. The configured macro
/// is placed immediately before the background action so cue entry ordering is
/// stable and no stale inherited macro can also fire.
pub(crate) fn replace_entry_macro(
    cue: &mut rv_data::Cue,
    macro_name: &str,
    cache: &MacroCache,
) -> Result<(), MacroApplyError> {
    let Some((installed_name, identification)) = cache.find(macro_name) else {
        return Err(MacroApplyError::Unavailable(macro_name.to_string()));
    };
    let existing = cue
        .actions
        .iter()
        .find(|action| macro_action_name(action) == Some(installed_name))
        .cloned()
        .or_else(|| {
            cue.actions
                .iter()
                .find(|action| is_macro_action(action))
                .cloned()
        });
    let retained = existing.map_or_else(
        || make_macro_action_from_identification(identification.clone()),
        |existing| {
            let mut canonical = make_macro_action_from_identification(identification.clone());
            canonical.uuid = existing.uuid;
            canonical
        },
    );
    cue.actions.retain(|action| !is_macro_action(action));
    let insertion = cue
        .actions
        .iter()
        .position(crate::propresenter::background::is_background_media_action)
        .unwrap_or(cue.actions.len());
    cue.actions.insert(insertion, retained);
    Ok(())
}

/// Enforce the configured macro sequence on the selected operator traversal.
///
/// Every selector and installed macro is resolved before mutation. Existing
/// macro actions in the selected traversal are then removed and the configured
/// region transitions are applied atomically in config order.
pub(crate) fn apply_operator_macro_policy(
    presentation: &mut rv_data::Presentation,
    policy: &RestyleMacroPolicy,
    cache: &MacroCache,
) -> Result<bool, MacroApplyError> {
    let traversal = crate::propresenter::arrangement::operator_cue_indices(presentation);
    if traversal.is_empty() {
        return Err(MacroApplyError::MissingOperatorCue);
    }
    let selected_groups = crate::propresenter::arrangement::selected_group_entries(presentation);
    let mut targets = Vec::with_capacity(policy.regions().len());
    let mut target_indexes = std::collections::HashSet::new();
    for (region_index, region) in policy.regions().iter().enumerate() {
        if cache.find(region.enter_macro()).is_none() {
            return Err(MacroApplyError::Unavailable(
                region.enter_macro().to_string(),
            ));
        }
        let cue_index = match region.selector() {
            RestyleMacroSelector::OperatorCue { index } => traversal
                .get(*index)
                .copied()
                .ok_or_else(|| MacroApplyError::RegionUnavailable {
                    region: region_index,
                    selector: format!("operator cue {index}"),
                })?,
            RestyleMacroSelector::ArrangementGroup {
                index,
                allowed_names,
            } => {
                let group = selected_groups
                    .as_ref()
                    .and_then(|groups| groups.get(*index))
                    .ok_or_else(|| MacroApplyError::RegionUnavailable {
                        region: region_index,
                        selector: format!("selected arrangement group {index}"),
                    })?;
                if !allowed_names.contains(group.name) {
                    return Err(MacroApplyError::UnexpectedGroup {
                        region: region_index,
                        index: *index,
                        actual: group.name.to_string(),
                        allowed: allowed_names.iter().cloned().collect(),
                    });
                }
                group.cue_index
            }
        };
        if !target_indexes.insert(cue_index) {
            return Err(MacroApplyError::DuplicateRegionTarget { cue_index });
        }
        targets.push((cue_index, region.enter_macro()));
    }

    let mut transformed = presentation.clone();
    let mut visited = std::collections::HashSet::new();
    for &index in &traversal {
        if !visited.insert(index) {
            continue;
        }
        let cue_count = transformed.cues.len();
        let Some(cue) = transformed.cues.get_mut(index) else {
            return Err(MacroApplyError::CueUnavailable { index, cue_count });
        };
        if !target_indexes.contains(&index) {
            cue.actions.retain(|action| !is_macro_action(action));
        }
    }
    for (index, macro_name) in targets {
        let cue_count = transformed.cues.len();
        let cue = transformed
            .cues
            .get_mut(index)
            .ok_or(MacroApplyError::CueUnavailable { index, cue_count })?;
        replace_entry_macro(cue, macro_name, cache)?;
    }

    if transformed == *presentation {
        Ok(false)
    } else {
        *presentation = transformed;
        Ok(true)
    }
}

const fn is_macro_action(action: &rv_data::Action) -> bool {
    action.r#type == action::ActionType::Macro as i32
        || matches!(
            action.action_type_data,
            Some(action::ActionTypeData::Macro(_))
        )
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

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::workflow::plan::RestyleMacroRegion;

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

    fn macro_cache() -> MacroCache {
        MacroCache {
            macros: HashMap::from([
                (
                    "Name Tag/Title".to_string(),
                    installed_macro("Name Tag/Title", "name-tag-macro"),
                ),
                ("Song".to_string(), installed_macro("Song", "song-macro")),
                ("Wrong".to_string(), installed_macro("Wrong", "wrong-macro")),
            ]),
        }
    }

    fn arrangement_group_policy(regions: &[(usize, &[&str], &str)]) -> RestyleMacroPolicy {
        RestyleMacroPolicy::new(
            regions
                .iter()
                .map(|(index, names, macro_name)| {
                    RestyleMacroRegion::new(
                        RestyleMacroSelector::arrangement_group(
                            *index,
                            names.iter().map(|name| (*name).to_string()).collect(),
                        )
                        .expect("valid exact group names"),
                        (*macro_name).to_string(),
                    )
                    .expect("valid exact macro name")
                })
                .collect(),
        )
        .expect("nonempty macro policy")
    }

    fn native_macro(name: &str) -> rv_data::Action {
        make_macro_action_from_identification(identification(name, &format!("{name}-id")))
    }

    fn presentation_with_selected_groups(
        groups: &[(&str, &str, &[&str])],
    ) -> rv_data::Presentation {
        let cue_ids = groups
            .iter()
            .flat_map(|(_, _, cue_ids)| cue_ids.iter().copied())
            .collect::<Vec<_>>();
        rv_data::Presentation {
            selected_arrangement: Some(Uuid {
                string: "selected-arrangement".to_string(),
            }),
            arrangements: vec![rv_data::presentation::Arrangement {
                uuid: Some(Uuid {
                    string: "selected-arrangement".to_string(),
                }),
                name: "Default".to_string(),
                group_identifiers: groups
                    .iter()
                    .map(|(id, _, _)| Uuid {
                        string: (*id).to_string(),
                    })
                    .collect(),
            }],
            cues: cue_ids
                .iter()
                .map(|id| rv_data::Cue {
                    uuid: Some(Uuid {
                        string: (*id).to_string(),
                    }),
                    actions: vec![native_macro("Wrong"), native_macro("Wrong")],
                    ..rv_data::Cue::default()
                })
                .collect(),
            cue_groups: groups
                .iter()
                .map(|(id, name, cue_ids)| rv_data::presentation::CueGroup {
                    group: Some(rv_data::Group {
                        uuid: Some(Uuid {
                            string: (*id).to_string(),
                        }),
                        name: (*name).to_string(),
                        ..rv_data::Group::default()
                    }),
                    cue_identifiers: cue_ids
                        .iter()
                        .map(|id| Uuid {
                            string: (*id).to_string(),
                        })
                        .collect(),
                })
                .collect(),
            ..rv_data::Presentation::default()
        }
    }

    fn assert_only_macro(cue: &rv_data::Cue, expected: Option<&str>) {
        let macros = cue
            .actions
            .iter()
            .filter(|action| is_macro_action(action))
            .collect::<Vec<_>>();
        match expected {
            Some(name) => {
                assert_eq!(macros.len(), 1, "entry cue must contain exactly one macro");
                assert_eq!(macro_action_name(macros[0]), Some(name));
            }
            None => assert!(macros.is_empty(), "non-entry cue must not retain a macro"),
        }
    }

    #[test]
    fn contemporary_song_starts_with_one_song_macro() {
        let cache = macro_cache();
        let mut presentation = presentation_with_selected_groups(&[
            ("background", "Background", &["blank"]),
            ("verse", "Verse 1", &["verse-1", "verse-2"]),
        ]);
        let policy = arrangement_group_policy(&[(0, &["Background", "Blank"], "Song")]);

        assert!(
            apply_operator_macro_policy(&mut presentation, &policy, &cache)
                .expect("selected contemporary arrangement")
        );

        assert_only_macro(&presentation.cues[0], Some("Song"));
        assert_only_macro(&presentation.cues[1], None);
        assert_only_macro(&presentation.cues[2], None);
    }

    #[test]
    fn hymn_transitions_from_title_macro_to_song_macro_once() {
        let cache = macro_cache();
        let mut presentation = presentation_with_selected_groups(&[
            ("title", "Title", &["title"]),
            ("verse", "Verse 1", &["verse-1", "verse-2"]),
            ("blank", "Blank", &["blank"]),
        ]);
        let policy = arrangement_group_policy(&[
            (0, &["Background", "Title"], "Name Tag/Title"),
            (1, &["Verse", "Verse 1"], "Song"),
        ]);

        assert!(
            apply_operator_macro_policy(&mut presentation, &policy, &cache)
                .expect("selected hymn arrangement")
        );

        assert_only_macro(&presentation.cues[0], Some("Name Tag/Title"));
        assert_only_macro(&presentation.cues[1], Some("Song"));
        assert_only_macro(&presentation.cues[2], None);
        assert_only_macro(&presentation.cues[3], None);
    }

    #[test]
    fn doxology_transitions_from_title_macro_to_song_macro_once() {
        let cache = macro_cache();
        let mut presentation = presentation_with_selected_groups(&[
            ("title", "Group", &["title"]),
            ("verse", "Verse", &["verse-1", "verse-2"]),
            ("blank", "Blank", &["blank"]),
        ]);
        let policy =
            arrangement_group_policy(&[(0, &["Group"], "Name Tag/Title"), (1, &["Verse"], "Song")]);

        assert!(
            apply_operator_macro_policy(&mut presentation, &policy, &cache)
                .expect("selected doxology arrangement")
        );

        assert_only_macro(&presentation.cues[0], Some("Name Tag/Title"));
        assert_only_macro(&presentation.cues[1], Some("Song"));
        assert_only_macro(&presentation.cues[2], None);
        assert_only_macro(&presentation.cues[3], None);
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
    fn entry_macro_replaces_stale_macros_before_background() {
        let cache = MacroCache {
            macros: HashMap::from([
                (
                    "Song".to_string(),
                    installed_macro("Song", "00000000-0000-0000-0000-000000000001"),
                ),
                (
                    "Wrong".to_string(),
                    installed_macro("Wrong", "00000000-0000-0000-0000-000000000002"),
                ),
            ]),
        };
        let mut cue = rv_data::Cue {
            actions: vec![
                make_macro_action_from_identification(identification(
                    "Wrong",
                    "00000000-0000-0000-0000-000000000002",
                )),
                rv_data::Action {
                    r#type: action::ActionType::BackgroundMedia as i32,
                    ..rv_data::Action::default()
                },
            ],
            ..rv_data::Cue::default()
        };

        replace_entry_macro(&mut cue, "Song", &cache).expect("installed macro");

        assert_eq!(macro_action_name(&cue.actions[0]), Some("Song"));
        assert_eq!(
            cue.actions[1].r#type,
            action::ActionType::BackgroundMedia as i32
        );
        assert_eq!(
            cue.actions
                .iter()
                .filter(|action| macro_action_name(action).is_some())
                .count(),
            1
        );
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
    fn optional_macro_catalog_rejects_existing_directory() {
        let directory = tempfile::tempdir().expect("tempdir");

        assert!(matches!(
            MacroCache::load_optional(directory.path()),
            Err(MacroCacheLoadError::NotRegular { .. })
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

        let mut presentation = rv_data::Presentation {
            cues: vec![rv_data::Cue {
                actions: vec![make_macro_action_from_identification(identification(
                    "title",
                    "lowercase",
                ))],
                ..rv_data::Cue::default()
            }],
            ..rv_data::Presentation::default()
        };
        add_macro_to_cue_entries(&mut presentation, &[0], "Title", &cache)
            .expect("installed exact macro");
        assert!(cue_has_macro_named(&presentation.cues[0], "Title"));
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
        let mut presentation = rv_data::Presentation {
            cues: vec![rv_data::Cue::default()],
            ..rv_data::Presentation::default()
        };

        add_macro_to_cue_entries(&mut presentation, &[0], "Scripture/Prayer", &cache)
            .expect("installed exact macro");
        let action = presentation.cues[0].actions.first().expect("macro action");
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
}
