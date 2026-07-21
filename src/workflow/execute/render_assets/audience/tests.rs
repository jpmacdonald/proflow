#![allow(clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::Path;

use prost::Message;
use sha2::{Digest, Sha256};

use super::*;
use crate::paths::{BuildLocationInputs, BuildLocations};
use crate::project_config::{
    BackgroundAssetPath, BackgroundId, ContentSourceKind, CueRoleConfig, ItemKind, OutputStrategy,
    PresentationTypeConfig, ProjectConfig, RawProjectConfig, RestyleMacroConfig,
    RestyleMacroRegionConfig, RestyleMacroSelectorConfig,
};
use crate::propresenter::audience::PresentationDestination;
use crate::propresenter::generated::rv_data::{
    self, action, action::ActionTypeData, macros_document, pro_audience_look::ProScreenLook,
    pro_presenter_screen, template, Action, CollectionElementType, ProAudienceLook,
    ProPresenterScreen, ProPresenterWorkspace, Url,
};
use crate::propresenter::theme::ThemeCache;

const SCREEN_UUID: &str = "11111111-1111-4111-8111-111111111111";
const LOOK_UUID: &str = "22222222-2222-4222-8222-222222222222";
const SLIDE_UUID: &str = "33333333-3333-4333-8333-333333333333";

#[test]
fn configured_macros_capture_one_exact_workspace_and_unique_theme_document() {
    let root = tempfile::tempdir().expect("temporary root");
    let locations = locations(root.path());
    let theme_path = locations
        .propresenter_root()
        .join("Themes/Audience Theme/Theme");
    std::fs::create_dir_all(theme_path.parent().expect("theme parent"))
        .expect("create theme directory");
    let theme_bytes = theme_document().encode_to_vec();
    std::fs::write(&theme_path, &theme_bytes).expect("write theme");

    let macros_document = rv_data::MacrosDocument {
        macros: vec![
            macro_for_look("Song", LOOK_UUID, "Lyrics"),
            macro_for_look("Song Alt", LOOK_UUID, "Lyrics"),
        ],
        ..rv_data::MacrosDocument::default()
    };
    std::fs::create_dir_all(
        locations
            .macros()
            .parent()
            .expect("macro configuration directory"),
    )
    .expect("create configuration directory");
    std::fs::write(locations.macros(), macros_document.encode_to_vec()).expect("write macros");

    let workspace_bytes = workspace(&theme_path).encode_to_vec();
    std::fs::write(locations.workspace(), &workspace_bytes).expect("write workspace");

    let config = config_with_restyle_macros(&["Song", "Song Alt"]);
    assert!(config.cue_roles().is_empty());
    assert_eq!(
        config.referenced_macro_names(),
        ["Song", "Song Alt"].into_iter().collect()
    );
    let macros = MacroCache::load_from(locations.macros()).expect("load macros");
    let mut issues = Vec::new();
    let destinations =
        ConfiguredAudienceDestinations::capture(&config, &locations, &macros, &mut issues)
            .expect("load configured destinations");

    assert!(issues.is_empty());
    let song = destinations.for_macro("Song").expect("Song destination");
    assert_eq!(song.name(), "Lyrics");
    let PresentationDestination::ThemeOverride(theme) = song.screens()[0].presentation() else {
        panic!("expected alternate theme");
    };
    assert_eq!(theme.document_path(), theme_path);
    assert_eq!(
        theme.template_bytes(),
        theme_document().slides[0].encode_to_vec()
    );
    assert_eq!(
        destinations.workspace_source(),
        Some((
            locations.workspace(),
            Sha256::digest(&workspace_bytes).into()
        ))
    );
    assert_eq!(destinations.theme_sources().count(), 1);

    let themes = ThemeCache::load_from_dir(None, locations.themes()).expect("empty main theme");
    let fingerprint = super::super::fingerprint::RenderAssetFingerprint::capture(
        &config,
        &themes,
        &macros,
        &destinations,
    )
    .expect("fingerprint exact assets");
    assert_eq!(fingerprint.schema, "proflow.render-assets.v2");
    assert_eq!(
        fingerprint
            .audience_workspace
            .as_ref()
            .expect("workspace digest")
            .sha256,
        digest_hex(&workspace_bytes)
    );
    assert_eq!(fingerprint.audience_themes.len(), 1);
    assert_eq!(
        fingerprint.audience_themes[0].sha256,
        digest_hex(&theme_bytes)
    );
}

#[test]
fn configured_dangling_look_becomes_one_typed_render_asset_issue() {
    let root = tempfile::tempdir().expect("temporary root");
    let locations = locations(root.path());
    std::fs::create_dir_all(
        locations
            .macros()
            .parent()
            .expect("macro configuration directory"),
    )
    .expect("create configuration directory");
    let native_macros = rv_data::MacrosDocument {
        macros: vec![macro_for_look(
            "Song",
            "CCCCCCCC-CCCC-4CCC-8CCC-CCCCCCCCCCCC",
            "Missing",
        )],
        ..rv_data::MacrosDocument::default()
    };
    std::fs::write(locations.macros(), native_macros.encode_to_vec()).expect("write macros");
    let theme_path = locations
        .propresenter_root()
        .join("Themes/Audience Theme/Theme");
    std::fs::write(
        locations.workspace(),
        workspace(&theme_path).encode_to_vec(),
    )
    .expect("write workspace");

    let macros = MacroCache::load_from(locations.macros()).expect("load macros");
    let mut issues = Vec::new();
    let destinations = ConfiguredAudienceDestinations::capture(
        &config_with_macros(&["Song"]),
        &locations,
        &macros,
        &mut issues,
    )
    .expect("workspace itself is readable");

    assert!(destinations.for_macro("Song").is_none());
    assert!(matches!(
        issues.as_slice(),
        [super::super::RenderAssetIssue::AudienceDestination {
            name,
            source: crate::propresenter::audience::AudienceDestinationError::DanglingAudienceLook { .. },
        }] if name == "Song"
    ));
}

#[test]
fn missing_restyle_only_macro_becomes_one_typed_render_asset_issue() {
    let root = tempfile::tempdir().expect("temporary root");
    let locations = locations(root.path());
    let macros = MacroCache::load_optional(locations.macros()).expect("empty macro catalog");
    let mut issues = Vec::new();

    let destinations = ConfiguredAudienceDestinations::capture(
        &config_with_restyle_macros(&["Song"]),
        &locations,
        &macros,
        &mut issues,
    )
    .expect("no workspace is needed when every configured macro is absent");

    assert!(destinations.for_macro("Song").is_none());
    assert!(matches!(
        issues.as_slice(),
        [super::super::RenderAssetIssue::MissingMacro { name }] if name == "Song"
    ));
}

#[test]
fn implicit_role_rejects_an_audience_theme_without_one_text_destination() {
    let config = config_with_macros(&["Song"]);
    let destinations = capture_theme_destinations(&config, &theme_document());
    let mut issues = Vec::new();

    super::super::validation::validate_audience_text_bindings(&config, &destinations, &mut issues);

    assert!(matches!(
        issues.as_slice(),
        [super::super::RenderAssetIssue::AudienceTextBinding {
            role,
            name,
            screen_name,
            source: crate::propresenter::render::TemplateSlotError::AmbiguousDefaultSlot {
                count: 0
            },
            ..
        }] if role == "role-0" && name == "Song" && screen_name == "Stream"
    ));
}

#[test]
fn single_field_role_uses_the_unique_audience_text_destination_regardless_of_name() {
    let compatible = config_with_named_macro("Song", "Source Body");
    let destinations =
        capture_theme_destinations(&compatible, &theme_document_with_slots(&["Audience Body"]));
    let mut issues = Vec::new();
    super::super::validation::validate_audience_text_bindings(
        &compatible,
        &destinations,
        &mut issues,
    );
    assert!(issues.is_empty());

    let ambiguous_document = theme_document_with_slots(&["Audience Body", "Audience Detail"]);
    let ambiguous_destinations = capture_theme_destinations(&compatible, &ambiguous_document);
    super::super::validation::validate_audience_text_bindings(
        &compatible,
        &ambiguous_destinations,
        &mut issues,
    );
    assert!(matches!(
        issues.as_slice(),
        [super::super::RenderAssetIssue::AudienceTextBinding {
            role,
            name,
            source: crate::propresenter::render::TemplateSlotError::AmbiguousDefaultSlot {
                count: 2
            },
            ..
        }] if role == "role" && name == "Song"
    ));
}

#[test]
fn render_asset_snapshot_rejects_an_incompatible_audience_text_binding() {
    let root = tempfile::tempdir().expect("temporary snapshot root");
    let locations = locations(root.path());
    write_audience_environment(&locations, &theme_document());
    let source_theme_path = locations.themes().join("Source Theme/Theme");
    std::fs::create_dir_all(source_theme_path.parent().expect("source theme parent"))
        .expect("create source theme directory");
    std::fs::write(&source_theme_path, source_theme_document().encode_to_vec())
        .expect("write source theme");

    let mut raw = config_with_macros(&["Song"]).into_raw();
    raw.defaults.theme = Some("Source Theme".to_string());
    let config = ProjectConfig::try_from(raw).expect("valid source-theme config");
    let Err(error) = super::super::RenderAssetSnapshot::load(config, locations) else {
        panic!("audience destination cannot bind the configured role");
    };

    assert!(matches!(
        error,
        super::super::RenderAssetSnapshotError::Unresolved(issues)
            if matches!(
                issues.issues(),
                [super::super::RenderAssetIssue::AudienceTextBinding {
                    role,
                    name,
                    source: crate::propresenter::render::TemplateSlotError::AmbiguousDefaultSlot {
                        count: 0
                    },
                    ..
                }] if role == "role-0" && name == "Song"
            )
    ));
}

fn locations(root: &Path) -> BuildLocations {
    let data = root.join("data");
    let library = root.join("library");
    let show = root.join("ProPresenter");
    std::fs::create_dir_all(&data).expect("create data");
    std::fs::create_dir_all(&library).expect("create library");
    std::fs::create_dir_all(&show).expect("create show");
    BuildLocations::from_inputs(BuildLocationInputs {
        project_data_root: data,
        presentation_library: library.clone(),
        playlist_output: library,
        propresenter_root: show.clone(),
        themes: show.join("Themes"),
        macros: show.join("Configuration/Macros"),
    })
    .expect("checked locations")
}

fn config_with_macros(names: &[&str]) -> ProjectConfig {
    let mut raw = RawProjectConfig::default();
    for (index, name) in names.iter().enumerate() {
        raw.cue_roles.insert(
            format!("role-{index}"),
            CueRoleConfig {
                slide: "Content".to_string(),
                text_slots: BTreeMap::new(),
                enter_macro: Some((*name).to_string()),
                leader_enter_macro: None,
                speaker_colors: None,
            },
        );
    }
    ProjectConfig::try_from(raw).expect("valid configured macros")
}

fn config_with_named_macro(macro_name: &str, native_slot: &str) -> ProjectConfig {
    let mut raw = RawProjectConfig::default();
    raw.cue_roles.insert(
        "role".to_string(),
        CueRoleConfig {
            slide: "Content".to_string(),
            text_slots: BTreeMap::from([("body".to_string(), native_slot.to_string())]),
            enter_macro: Some(macro_name.to_string()),
            leader_enter_macro: None,
            speaker_colors: None,
        },
    );
    ProjectConfig::try_from(raw).expect("valid named macro role")
}

fn config_with_restyle_macros(names: &[&str]) -> ProjectConfig {
    let mut raw = RawProjectConfig::default();
    let background = BackgroundId::new("default").expect("valid background id");
    raw.backgrounds.insert(
        background.clone(),
        BackgroundAssetPath::new("backgrounds/default.png").expect("valid background path"),
    );
    raw.presentation_types.insert(
        "song".to_string(),
        PresentationTypeConfig {
            kind: ItemKind::Song,
            content_source: ContentSourceKind::Song,
            output_strategy: OutputStrategy::RestyleExisting,
            background: Some(background),
            macro_transitions: Some(RestyleMacroConfig {
                regions: names
                    .iter()
                    .enumerate()
                    .map(|(index, name)| RestyleMacroRegionConfig {
                        selector: RestyleMacroSelectorConfig::OperatorCue { index },
                        enter_macro: (*name).to_string(),
                    })
                    .collect(),
            }),
            ..PresentationTypeConfig::default()
        },
    );
    ProjectConfig::try_from(raw).expect("valid restyle-only macro config")
}

fn workspace(theme_path: &Path) -> ProPresenterWorkspace {
    ProPresenterWorkspace {
        pro_screens: vec![ProPresenterScreen {
            uuid: Some(native_uuid(SCREEN_UUID)),
            name: "Stream".to_string(),
            screen_type: pro_presenter_screen::ScreenType::Audience as i32,
            ..ProPresenterScreen::default()
        }],
        audience_looks: vec![
            ProAudienceLook {
                uuid: Some(native_uuid(LOOK_UUID)),
                name: "Lyrics".to_string(),
                screen_looks: vec![ProScreenLook {
                    pro_screen_uuid: Some(native_uuid(SCREEN_UUID)),
                    presentation_foreground_enabled: true,
                    template_document_file_path: Some(file_url(theme_path)),
                    template_slide_uuid: Some(native_uuid(SLIDE_UUID)),
                    ..ProScreenLook::default()
                }],
                ..ProAudienceLook::default()
            },
            ProAudienceLook {
                uuid: Some(native_uuid("AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA")),
                name: "Unconfigured broken Look".to_string(),
                screen_looks: vec![ProScreenLook {
                    pro_screen_uuid: Some(native_uuid("BBBBBBBB-BBBB-4BBB-8BBB-BBBBBBBBBBBB")),
                    presentation_foreground_enabled: true,
                    ..ProScreenLook::default()
                }],
                ..ProAudienceLook::default()
            },
        ],
        ..ProPresenterWorkspace::default()
    }
}

fn theme_document() -> template::Document {
    template::Document {
        slides: vec![template::Slide {
            base_slide: Some(rv_data::Slide {
                uuid: Some(native_uuid(SLIDE_UUID)),
                ..rv_data::Slide::default()
            }),
            ..template::Slide::default()
        }],
        ..template::Document::default()
    }
}

fn theme_document_with_slots(names: &[&str]) -> template::Document {
    let mut document = theme_document();
    document.slides[0]
        .base_slide
        .as_mut()
        .expect("theme base slide")
        .elements = names
        .iter()
        .map(|name| rv_data::slide::Element {
            element: Some(rv_data::graphics::Element {
                name: (*name).to_string(),
                text: Some(rv_data::graphics::Text {
                    rtf_data: br"{\rtf1\ansi Body}".to_vec(),
                    ..rv_data::graphics::Text::default()
                }),
                ..rv_data::graphics::Element::default()
            }),
            ..rv_data::slide::Element::default()
        })
        .collect();
    document
}

fn source_theme_document() -> template::Document {
    let mut document = theme_document_with_slots(&["Body"]);
    document.slides[0].name = "Content".to_string();
    let base_slide = document.slides[0]
        .base_slide
        .as_mut()
        .expect("source theme base slide");
    base_slide.size = Some(rv_data::graphics::Size {
        width: 1920.0,
        height: 1080.0,
    });
    document
}

fn capture_theme_destinations(
    config: &ProjectConfig,
    document: &template::Document,
) -> ConfiguredAudienceDestinations {
    let root = tempfile::tempdir().expect("temporary destination root");
    let locations = locations(root.path());
    write_audience_environment(&locations, document);
    let macros = MacroCache::load_from(locations.macros()).expect("load macros");
    let mut issues = Vec::new();
    let destinations =
        ConfiguredAudienceDestinations::capture(config, &locations, &macros, &mut issues)
            .expect("capture configured destination");
    assert!(issues.is_empty());
    destinations
}

fn write_audience_environment(locations: &BuildLocations, document: &template::Document) {
    let theme_path = locations
        .propresenter_root()
        .join("Themes/Audience Theme/Theme");
    std::fs::create_dir_all(theme_path.parent().expect("theme parent"))
        .expect("create theme directory");
    std::fs::write(&theme_path, document.encode_to_vec()).expect("write destination theme");
    std::fs::create_dir_all(
        locations
            .macros()
            .parent()
            .expect("macro configuration directory"),
    )
    .expect("create configuration directory");
    let native_macros = rv_data::MacrosDocument {
        macros: vec![macro_for_look("Song", LOOK_UUID, "Lyrics")],
        ..rv_data::MacrosDocument::default()
    };
    std::fs::write(locations.macros(), native_macros.encode_to_vec()).expect("write macros");
    std::fs::write(
        locations.workspace(),
        workspace(&theme_path).encode_to_vec(),
    )
    .expect("write workspace");
}

fn macro_for_look(name: &str, look_uuid: &str, look_name: &str) -> macros_document::Macro {
    macros_document::Macro {
        uuid: Some(native_uuid(&uuid::Uuid::new_v4().to_string())),
        name: name.to_string(),
        actions: vec![Action {
            is_enabled: true,
            r#type: action::ActionType::AudienceLook as i32,
            action_type_data: Some(ActionTypeData::AudienceLook(action::AudienceLookType {
                identification: Some(CollectionElementType {
                    parameter_uuid: Some(native_uuid(look_uuid)),
                    parameter_name: look_name.to_string(),
                    parent_collection: None,
                }),
            })),
            ..Action::default()
        }],
        ..macros_document::Macro::default()
    }
}

fn native_uuid(value: &str) -> rv_data::Uuid {
    rv_data::Uuid {
        string: value.to_string(),
    }
}

fn file_url(path: &Path) -> Url {
    Url {
        storage: Some(rv_data::url::Storage::AbsoluteString(format!(
            "file://{}",
            path.display()
        ))),
        ..Url::default()
    }
}

fn digest_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(digest.len().saturating_mul(2));
    for byte in digest {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}
