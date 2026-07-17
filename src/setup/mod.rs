//! Read-only discovery helpers for project config authoring.
#![allow(clippy::redundant_pub_crate)]
//!
//! These routines expose installed `ProPresenter` assets and the current
//! project contract. They never infer, invent, or mutate runtime policy.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use crate::project_config::{
    ContentSourceKind, DisplayBindingConfig, ItemKind, OutputStrategy, PresentationTypeConfig,
    ProjectConfig, ServiceGroupConfig, SpeakerColorConfig,
};
use crate::propresenter::groups::GroupCatalog;
use crate::propresenter::library::LibraryCatalog;
use crate::propresenter::macros::{MacroCache, MacroSummary};
use crate::propresenter::theme::ThemeCache;

#[derive(Debug, Serialize)]
pub(crate) struct AssetCatalog {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme_name: Option<String>,
    pub library_path: String,
    /// Exact installed names retained for existing catalog consumers.
    pub theme_slides: Vec<String>,
    pub theme_slide_summaries: Vec<ThemeSlideSummary>,
    /// Exact installed names retained for existing catalog consumers.
    pub macros: Vec<String>,
    pub macro_summaries: Vec<MacroSummary>,
    /// Exact names from the installed `ProPresenter` cue-group catalog.
    pub groups: Vec<String>,
    pub backgrounds: Vec<BackgroundSummary>,
    pub cue_roles: Vec<CueRoleSummary>,
    pub library: LibrarySummary,
    pub service_groups: Vec<ServiceGroupSummary>,
    pub presentation_types: Vec<PresentationTypeSummary>,
}

#[derive(Debug, Serialize)]
pub(crate) struct BackgroundSummary {
    pub id: String,
    pub path: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct CueRoleSummary {
    pub name: String,
    pub slide: String,
    pub text_slots: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enter_macro: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub leader_enter_macro: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker_colors: Option<SpeakerColorConfig>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ThemeSlideSummary {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canvas_size: Option<ThemeSlideCanvasSummary>,
    pub named_text_slots: Vec<String>,
    pub default_text_slot_candidates: usize,
    pub embedded_action_count: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub generation_issues: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ThemeSlideCanvasSummary {
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Serialize)]
pub(crate) struct LibrarySummary {
    pub file_count: usize,
    pub top_level_folders: Vec<NamedCount>,
    pub sample_files: Vec<LibraryFileSummary>,
}

#[derive(Debug, Serialize)]
pub(crate) struct LibraryFileSummary {
    pub file_name: String,
    pub relative_path: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct NamedCount {
    pub name: String,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct ServiceGroupSummary {
    pub name: String,
    pub service_types: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PresentationTypeSummary {
    pub name: String,
    pub kind: String,
    pub content_source: String,
    pub output_strategy: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display: Option<DisplayBindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arrangement: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_lines_per_slide: Option<usize>,
}

pub(crate) fn catalog_assets(
    config: &ProjectConfig,
    theme_cache: &ThemeCache,
    macro_cache: &MacroCache,
    group_catalog: &GroupCatalog,
    file_index: &LibraryCatalog,
    library_path: &Path,
    sample_limit: usize,
) -> AssetCatalog {
    let raw = config.as_raw();
    let mut service_groups: Vec<_> = raw
        .service_groups
        .iter()
        .map(|(name, group)| summarize_service_group(name, group))
        .collect();
    service_groups.sort_by(|a, b| a.name.cmp(&b.name));

    let mut presentation_types: Vec<_> = raw
        .presentation_types
        .iter()
        .map(|(name, ptype)| summarize_presentation_type(name, ptype))
        .collect();
    presentation_types.sort_by(|a, b| a.name.cmp(&b.name));

    let theme_slides = theme_cache
        .theme_slide_names()
        .into_iter()
        .map(str::to_string)
        .collect();
    let theme_slide_summaries = theme_cache
        .theme_slide_facts()
        .into_iter()
        .map(|facts| ThemeSlideSummary {
            name: facts.name,
            canvas_size: facts.canvas_size.map(|size| ThemeSlideCanvasSummary {
                width: size.width,
                height: size.height,
            }),
            named_text_slots: facts.named_text_slots,
            default_text_slot_candidates: facts.default_text_slot_candidates,
            embedded_action_count: facts.embedded_action_count,
            generation_issues: facts.generation_issues,
        })
        .collect();

    let macro_summaries = macro_cache.summaries();
    let macros = macro_summaries
        .iter()
        .map(|summary| summary.name.clone())
        .collect();

    let mut backgrounds: Vec<_> = raw
        .backgrounds
        .iter()
        .map(|(id, path)| BackgroundSummary {
            id: id.to_string(),
            path: path.as_path().display().to_string(),
        })
        .collect();
    backgrounds.sort_by(|a, b| a.id.cmp(&b.id));

    let mut cue_roles: Vec<_> = raw
        .cue_roles
        .iter()
        .map(|(name, role)| CueRoleSummary {
            name: name.clone(),
            slide: role.slide.clone(),
            text_slots: role.text_slots.clone(),
            enter_macro: role.enter_macro.clone(),
            leader_enter_macro: role.leader_enter_macro.clone(),
            speaker_colors: role.speaker_colors,
        })
        .collect();
    cue_roles.sort_by(|a, b| a.name.cmp(&b.name));

    AssetCatalog {
        project_name: raw.metadata.name.clone(),
        theme_name: theme_cache.theme_name().map(str::to_string),
        library_path: library_path.display().to_string(),
        theme_slides,
        theme_slide_summaries,
        macros,
        macro_summaries,
        groups: group_catalog
            .names()
            .into_iter()
            .map(str::to_string)
            .collect(),
        backgrounds,
        cue_roles,
        library: summarize_library(file_index, sample_limit),
        service_groups,
        presentation_types,
    }
}

fn summarize_library(index: &LibraryCatalog, sample_limit: usize) -> LibrarySummary {
    let mut folder_counts: BTreeMap<String, usize> = BTreeMap::new();
    for entry in index.entries() {
        let folder = entry
            .relative_path()
            .split_once('/')
            .map_or_else(|| "(root)".to_string(), |(segment, _)| segment.to_string());
        *folder_counts.entry(folder).or_default() += 1;
    }

    let mut sample_files: Vec<_> = index
        .entries()
        .iter()
        .map(|entry| LibraryFileSummary {
            file_name: entry.file_name().to_string(),
            relative_path: entry.relative_path().to_string(),
        })
        .collect();
    sample_files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    sample_files.truncate(sample_limit);

    LibrarySummary {
        file_count: index.entries().len(),
        top_level_folders: sort_named_counts(folder_counts),
        sample_files,
    }
}

fn summarize_service_group(name: &str, group: &ServiceGroupConfig) -> ServiceGroupSummary {
    ServiceGroupSummary {
        name: name.to_string(),
        service_types: group.service_types.clone(),
    }
}

fn summarize_presentation_type(
    name: &str,
    ptype: &PresentationTypeConfig,
) -> PresentationTypeSummary {
    PresentationTypeSummary {
        name: name.to_string(),
        kind: item_kind_name(ptype.kind).to_string(),
        content_source: content_source_name(ptype.content_source).to_string(),
        output_strategy: output_strategy_name(ptype.output_strategy).to_string(),
        display: ptype.display.clone(),
        background: ptype
            .background
            .as_ref()
            .map(std::string::ToString::to_string),
        arrangement: ptype.arrangement.clone(),
        max_lines_per_slide: ptype.max_lines_per_slide.map(std::num::NonZeroUsize::get),
    }
}

fn sort_named_counts(counts: impl IntoIterator<Item = (String, usize)>) -> Vec<NamedCount> {
    let mut values: Vec<_> = counts
        .into_iter()
        .map(|(name, count)| NamedCount { name, count })
        .collect();
    values.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
    values
}

const fn item_kind_name(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Song => "song",
        ItemKind::Scripture => "scripture",
        ItemKind::Liturgy => "liturgy",
        ItemKind::Nametag => "nametag",
        ItemKind::Announcement => "announcement",
        ItemKind::Graphic => "graphic",
        ItemKind::Other => "other",
    }
}

const fn content_source_name(kind: ContentSourceKind) -> &'static str {
    match kind {
        ContentSourceKind::Static => "static",
        ContentSourceKind::Description => "description",
        ContentSourceKind::Scripture => "scripture",
        ContentSourceKind::Song => "song",
    }
}

const fn output_strategy_name(strategy: OutputStrategy) -> &'static str {
    match strategy {
        OutputStrategy::Skip => "skip",
        OutputStrategy::PreserveExisting => "preserve_existing",
        OutputStrategy::RestyleExisting => "restyle_existing",
        OutputStrategy::EditInPlace => "edit_in_place",
        OutputStrategy::GenerateNew => "generate_new",
        OutputStrategy::NeedsReview => "needs_review",
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::project_config::parse_project_config_str;
    use crate::propresenter::generated::rv_data::{self, action, CollectionElementType, Uuid};
    use prost::Message;
    use std::path::{Path, PathBuf};

    fn fixture_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/workflow")
    }

    #[test]
    fn catalog_assets_reports_only_discovered_and_configured_facts() {
        let config =
            parse_project_config_str(include_str!("../../tests/fixtures/workflow/v4_config.json"))
                .expect("fixture config should parse");
        let index = LibraryCatalog::build(&fixture_root().join("library"))
            .expect("fixture library should index");
        let theme_cache = ThemeCache::empty();
        let macro_cache = MacroCache::empty();
        let group_catalog = GroupCatalog::default();

        let catalog = catalog_assets(
            &config,
            &theme_cache,
            &macro_cache,
            &group_catalog,
            &index,
            Path::new("/tmp/library"),
            3,
        );

        assert_eq!(catalog.project_name.as_deref(), Some("Fixture Church"));
        assert_eq!(catalog.library.file_count, index.entries().len());
        assert_eq!(
            catalog.library.sample_files.len(),
            index.entries().len().min(3)
        );
        assert!(catalog
            .presentation_types
            .iter()
            .any(|entry| entry.name == "scripture"));
        assert!(catalog
            .backgrounds
            .iter()
            .any(|background| background.id == "scripture"
                && background.path == "backgrounds/default.png"));
        assert!(catalog.cue_roles.iter().any(|role| role.name == "scripture"
            && role.slide == "Scripture"
            && role.enter_macro.as_deref() == Some("Scripture/Prayer")));
        let scripture = catalog
            .presentation_types
            .iter()
            .find(|entry| entry.name == "scripture")
            .expect("scripture presentation type should be summarized");
        assert!(matches!(
            scripture.display.as_ref(),
            Some(DisplayBindingConfig::Single { role }) if role == "scripture"
        ));
    }

    #[test]
    fn catalog_assets_exposes_macro_actions_without_replacing_name_list() {
        let directory = tempfile::tempdir().expect("tempdir");
        let macro_path = directory.path().join("Macros");
        let document = rv_data::MacrosDocument {
            application_info: None,
            macros: vec![rv_data::macros_document::Macro {
                uuid: Some(Uuid {
                    string: "macro-id".to_string(),
                }),
                name: "Song".to_string(),
                color: None,
                actions: vec![rv_data::Action {
                    r#type: action::ActionType::AudienceLook as i32,
                    action_type_data: Some(action::ActionTypeData::AudienceLook(
                        action::AudienceLookType {
                            identification: Some(CollectionElementType {
                                parameter_uuid: None,
                                parameter_name: "Song Look".to_string(),
                                parent_collection: None,
                            }),
                        },
                    )),
                    ..rv_data::Action::default()
                }],
                trigger_on_startup: false,
                image_type: 0,
                image_data: Vec::new(),
            }],
            macro_collections: Vec::new(),
        };
        std::fs::write(&macro_path, document.encode_to_vec()).expect("write macro document");
        let macro_cache = MacroCache::load_from(&macro_path).expect("load macro document");
        let config =
            parse_project_config_str(include_str!("../../tests/fixtures/workflow/v4_config.json"))
                .expect("fixture config should parse");
        let theme_cache = ThemeCache::empty();
        let group_catalog = GroupCatalog::default();
        let library = directory.path().join("library");
        std::fs::create_dir(&library).expect("create empty library");
        let index = LibraryCatalog::build(&library).expect("index empty library");

        let catalog = catalog_assets(
            &config,
            &theme_cache,
            &macro_cache,
            &group_catalog,
            &index,
            &library,
            0,
        );
        let serialized = serde_json::to_value(&catalog).expect("serialize asset catalog");

        assert_eq!(catalog.macros, vec!["Song"]);
        assert_eq!(catalog.macro_summaries[0].name, "Song");
        assert_eq!(
            catalog.macro_summaries[0].actions[0],
            crate::propresenter::macros::MacroActionSummary {
                action_type: "audience_look".to_string(),
                target: Some("Song Look".to_string()),
            }
        );
        assert_eq!(
            serialized["macro_summaries"][0]["actions"][0]["target"],
            "Song Look"
        );
    }

    #[test]
    fn catalog_assets_exposes_exact_configured_text_slot_bindings() {
        let directory = tempfile::tempdir().expect("tempdir");
        let config = parse_project_config_str(
            r#"{
              "version": 4,
              "cue_roles": {
                "nametag": {
                  "slide": "Name Tag",
                  "text_slots": {
                    "body": "Speaker Name",
                    "role": "Speaker Role"
                  }
                }
              }
            }"#,
        )
        .expect("config should parse");
        let index = LibraryCatalog::build(directory.path()).expect("index empty directory");

        let catalog = catalog_assets(
            &config,
            &ThemeCache::empty(),
            &MacroCache::empty(),
            &GroupCatalog::default(),
            &index,
            directory.path(),
            0,
        );

        assert_eq!(
            catalog.cue_roles[0].text_slots,
            BTreeMap::from([
                ("body".to_string(), "Speaker Name".to_string()),
                ("role".to_string(), "Speaker Role".to_string()),
            ])
        );
        assert!(catalog.theme_slide_summaries.is_empty());
    }
}
