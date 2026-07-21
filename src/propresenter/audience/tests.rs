#![allow(clippy::expect_used, clippy::panic)]

use std::path::Path;

use prost::Message;
use sha2::Digest;
use uuid::Uuid;

use super::*;
use crate::propresenter::generated::rv_data::{
    self, action, action::ActionTypeData, macros_document, pro_audience_look::ProScreenLook,
    pro_presenter_screen, template, Action, CollectionElementType, ProAudienceLook,
    ProPresenterScreen, ProPresenterWorkspace, Url,
};

const SCREEN_UUID: &str = "11111111-1111-4111-8111-111111111111";
const LOOK_UUID: &str = "22222222-2222-4222-8222-222222222222";
const SLIDE_UUID: &str = "33333333-3333-4333-8333-333333333333";

#[test]
fn resolves_macro_uuid_to_checked_theme_destination() {
    let root = tempfile::tempdir().expect("temporary show");
    let theme_path = root.path().join("Themes/VPC Theme/Theme");
    std::fs::create_dir_all(theme_path.parent().expect("theme parent"))
        .expect("create theme directory");
    write_theme(&theme_path, &[SLIDE_UUID]);
    let workspace = workspace_with_look(vec![screen_look(
        SCREEN_UUID,
        true,
        Some(file_url(&theme_path)),
        Some(native_uuid(SLIDE_UUID)),
    )]);
    let mut resolver = AudienceDestinationResolver::from_workspace(workspace, root.path());

    let look = resolver
        .resolve_macro(&macro_for_look(LOOK_UUID, "Lyrics"))
        .expect("resolve macro");

    assert_eq!(look.uuid(), Uuid::parse_str(LOOK_UUID).expect("look UUID"));
    assert_eq!(look.name(), "Lyrics");
    assert_eq!(look.screens().len(), 1);
    let screen = &look.screens()[0];
    assert_eq!(screen.screen_name(), "Projectors");
    assert_eq!(
        screen.screen_uuid(),
        Uuid::parse_str(SCREEN_UUID).expect("screen UUID")
    );
    let PresentationDestination::ThemeOverride(theme) = screen.presentation() else {
        panic!("expected theme override");
    };
    assert_eq!(theme.document_path(), theme_path);
    assert_eq!(
        theme.slide_uuid(),
        Uuid::parse_str(SLIDE_UUID).expect("slide UUID")
    );
    assert_eq!(
        template::Slide::decode(theme.template_bytes()).expect("captured template"),
        *theme.template()
    );
    assert_eq!(
        Some(theme.base_slide()),
        theme.template().base_slide.as_ref()
    );
    let expected_document_digest: [u8; 32] =
        sha2::Sha256::digest(std::fs::read(theme.document_path()).expect("read theme")).into();
    assert_eq!(theme.document_sha256(), expected_document_digest);
}

#[test]
fn no_theme_pair_means_source_presentation() {
    let root = tempfile::tempdir().expect("temporary show");
    let workspace = workspace_with_look(vec![screen_look(SCREEN_UUID, true, None, None)]);
    let mut resolver = AudienceDestinationResolver::from_workspace(workspace, root.path());
    let look = resolver
        .resolve_macro(&macro_for_look(LOOK_UUID, "Lyrics"))
        .expect("resolve macro");

    assert_eq!(
        look.screens()[0].presentation(),
        &PresentationDestination::SourcePresentation
    );
}

#[test]
fn disabled_foreground_is_not_a_text_destination() {
    let root = tempfile::tempdir().expect("temporary show");
    let workspace = workspace_with_look(vec![screen_look(SCREEN_UUID, false, None, None)]);
    let mut resolver = AudienceDestinationResolver::from_workspace(workspace, root.path());
    let look = resolver
        .resolve_macro(&macro_for_look(LOOK_UUID, "Lyrics"))
        .expect("resolve macro");

    assert!(look.screens().is_empty());
}

#[test]
fn unrelated_invalid_look_does_not_block_selected_macro() {
    let root = tempfile::tempdir().expect("temporary show");
    let mut workspace = workspace_with_look(vec![screen_look(SCREEN_UUID, true, None, None)]);
    workspace.audience_looks.push(ProAudienceLook {
        uuid: Some(native_uuid("AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA")),
        name: "Unused broken Look".to_string(),
        screen_looks: vec![screen_look(
            "BBBBBBBB-BBBB-4BBB-8BBB-BBBBBBBBBBBB",
            true,
            None,
            None,
        )],
        ..ProAudienceLook::default()
    });
    let mut resolver = AudienceDestinationResolver::from_workspace(workspace, root.path());

    let selected = resolver
        .resolve_macro(&macro_for_look(LOOK_UUID, "Lyrics"))
        .expect("configured Look alone is validated");

    assert_eq!(selected.name(), "Lyrics");
}

#[test]
fn missing_macro_look_is_a_dangling_reference() {
    let root = tempfile::tempdir().expect("temporary show");
    let mut resolver = resolver(root.path());

    let error = resolver
        .resolve_macro(&macro_for_look(
            "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA",
            "Missing",
        ))
        .expect_err("missing Look must fail");

    assert!(matches!(
        error,
        AudienceDestinationError::DanglingAudienceLook { .. }
    ));
}

#[test]
fn duplicate_look_uuid_is_ambiguous() {
    let root = tempfile::tempdir().expect("temporary show");
    let mut workspace = workspace_with_look(vec![]);
    workspace.audience_looks.push(ProAudienceLook {
        uuid: Some(native_uuid(LOOK_UUID)),
        name: "Duplicate".to_string(),
        ..ProAudienceLook::default()
    });

    let mut resolver = AudienceDestinationResolver::from_workspace(workspace, root.path());
    let error = resolver
        .resolve_macro(&macro_for_look(LOOK_UUID, "Lyrics"))
        .expect_err("duplicate Look must fail");

    assert!(matches!(
        error,
        AudienceDestinationError::Graph(AudienceWorkspaceError::DuplicateLookUuid { .. })
    ));
}

#[test]
fn duplicate_screen_mapping_is_ambiguous() {
    let root = tempfile::tempdir().expect("temporary show");
    let workspace = workspace_with_look(vec![
        screen_look(SCREEN_UUID, true, None, None),
        screen_look(SCREEN_UUID, true, None, None),
    ]);

    let mut resolver = AudienceDestinationResolver::from_workspace(workspace, root.path());
    let error = resolver
        .resolve_macro(&macro_for_look(LOOK_UUID, "Lyrics"))
        .expect_err("duplicate screen mapping must fail");

    assert!(matches!(
        error,
        AudienceDestinationError::Graph(AudienceWorkspaceError::DuplicateLookScreen { .. })
    ));
}

#[test]
fn incomplete_theme_override_fails() {
    let root = tempfile::tempdir().expect("temporary show");
    let workspace = workspace_with_look(vec![screen_look(
        SCREEN_UUID,
        true,
        Some(file_url(&root.path().join("Themes/VPC Theme/Theme"))),
        None,
    )]);

    let mut resolver = AudienceDestinationResolver::from_workspace(workspace, root.path());
    let error = resolver
        .resolve_macro(&macro_for_look(LOOK_UUID, "Lyrics"))
        .expect_err("incomplete override must fail");

    assert!(matches!(
        error,
        AudienceDestinationError::Graph(AudienceWorkspaceError::IncompleteThemeOverride { .. })
    ));
}

#[test]
fn missing_and_duplicate_theme_slides_are_distinct_failures() {
    let root = tempfile::tempdir().expect("temporary show");
    let theme_path = root.path().join("Themes/VPC Theme/Theme");
    std::fs::create_dir_all(theme_path.parent().expect("theme parent"))
        .expect("create theme directory");
    let workspace = workspace_with_look(vec![screen_look(
        SCREEN_UUID,
        true,
        Some(file_url(&theme_path)),
        Some(native_uuid(SLIDE_UUID)),
    )]);

    write_theme(&theme_path, &["AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA"]);
    let mut resolver = AudienceDestinationResolver::from_workspace(workspace.clone(), root.path());
    let missing = resolver
        .resolve_macro(&macro_for_look(LOOK_UUID, "Lyrics"))
        .expect_err("missing slide must fail");
    assert!(matches!(
        missing,
        AudienceDestinationError::Graph(AudienceWorkspaceError::DanglingThemeSlide { .. })
    ));

    write_theme(&theme_path, &[SLIDE_UUID, SLIDE_UUID]);
    let mut resolver = AudienceDestinationResolver::from_workspace(workspace, root.path());
    let duplicate = resolver
        .resolve_macro(&macro_for_look(LOOK_UUID, "Lyrics"))
        .expect_err("ambiguous slide must fail");
    assert!(matches!(
        duplicate,
        AudienceDestinationError::Graph(AudienceWorkspaceError::AmbiguousThemeSlide {
            count: 2,
            ..
        })
    ));
}

#[test]
fn theme_override_without_base_slide_is_rejected_during_graph_compilation() {
    let root = tempfile::tempdir().expect("temporary show");
    let theme_path = root.path().join("Themes/VPC Theme/Theme");
    std::fs::create_dir_all(theme_path.parent().expect("theme parent"))
        .expect("create theme directory");
    std::fs::write(
        &theme_path,
        template::Document {
            slides: vec![template::Slide::default()],
            ..template::Document::default()
        }
        .encode_to_vec(),
    )
    .expect("write baseless theme");
    let workspace = workspace_with_look(vec![screen_look(
        SCREEN_UUID,
        true,
        Some(file_url(&theme_path)),
        Some(native_uuid(SLIDE_UUID)),
    )]);
    let mut resolver = AudienceDestinationResolver::from_workspace(workspace, root.path());

    let error = resolver
        .resolve_macro(&macro_for_look(LOOK_UUID, "Lyrics"))
        .expect_err("baseless theme override must fail during graph compilation");

    assert!(matches!(
        error,
        AudienceDestinationError::Graph(AudienceWorkspaceError::DanglingThemeSlide { .. })
    ));
}

#[test]
fn multiple_enabled_look_actions_are_ambiguous() {
    let root = tempfile::tempdir().expect("temporary show");
    let mut resolver = resolver(root.path());
    let mut native_macro = macro_for_look(LOOK_UUID, "Lyrics");
    native_macro
        .actions
        .push(macro_for_look(LOOK_UUID, "Lyrics").actions.remove(0));

    let error = resolver
        .resolve_macro(&native_macro)
        .expect_err("multiple actions must fail");

    assert!(matches!(
        error,
        AudienceDestinationError::AmbiguousAudienceLookActions { count: 2, .. }
    ));
}

#[test]
fn enabled_nested_macro_action_is_rejected_before_look_resolution() {
    let root = tempfile::tempdir().expect("temporary show");
    let mut resolver = resolver(root.path());
    let mut native_macro = macro_for_look(LOOK_UUID, "Lyrics");
    native_macro.actions.push(Action {
        is_enabled: true,
        r#type: action::ActionType::Macro as i32,
        action_type_data: Some(ActionTypeData::Macro(action::MacroType {
            identification: Some(CollectionElementType {
                parameter_uuid: Some(native_uuid("AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA")),
                parameter_name: "Nested".to_string(),
                parent_collection: None,
            }),
        })),
        ..Action::default()
    });

    let error = resolver
        .resolve_macro(&native_macro)
        .expect_err("nested macro effects must fail closed");

    assert!(matches!(
        error,
        AudienceDestinationError::NestedMacroAction {
            action_index: 1,
            ..
        }
    ));
}

#[test]
fn disabled_nested_macro_action_does_not_execute() {
    let root = tempfile::tempdir().expect("temporary show");
    let mut resolver = resolver(root.path());
    let mut native_macro = macro_for_look(LOOK_UUID, "Lyrics");
    native_macro.actions.push(Action {
        is_enabled: false,
        r#type: action::ActionType::Macro as i32,
        action_type_data: Some(ActionTypeData::Macro(action::MacroType::default())),
        ..Action::default()
    });

    let look = resolver
        .resolve_macro(&native_macro)
        .expect("disabled actions have no Look effect");

    assert_eq!(look.name(), "Lyrics");
}

#[test]
fn nested_macro_payload_cannot_hide_behind_a_mismatched_declared_type() {
    let root = tempfile::tempdir().expect("temporary show");
    let mut resolver = resolver(root.path());
    let mut native_macro = macro_for_look(LOOK_UUID, "Lyrics");
    native_macro.actions.push(Action {
        is_enabled: true,
        r#type: action::ActionType::Clear as i32,
        action_type_data: Some(ActionTypeData::Macro(action::MacroType::default())),
        ..Action::default()
    });

    let error = resolver
        .resolve_macro(&native_macro)
        .expect_err("nested payload effects must fail closed");

    assert!(matches!(
        error,
        AudienceDestinationError::NestedMacroAction {
            action_index: 1,
            ..
        }
    ));
}

fn resolver(root: &Path) -> AudienceDestinationResolver {
    AudienceDestinationResolver::from_workspace(
        workspace_with_look(vec![screen_look(SCREEN_UUID, true, None, None)]),
        root,
    )
}

fn workspace_with_look(screen_looks: Vec<ProScreenLook>) -> ProPresenterWorkspace {
    ProPresenterWorkspace {
        pro_screens: vec![ProPresenterScreen {
            name: "Projectors".to_string(),
            screen_type: pro_presenter_screen::ScreenType::Audience as i32,
            uuid: Some(native_uuid(SCREEN_UUID)),
            ..ProPresenterScreen::default()
        }],
        audience_looks: vec![ProAudienceLook {
            uuid: Some(native_uuid(LOOK_UUID)),
            name: "Lyrics".to_string(),
            screen_looks,
            ..ProAudienceLook::default()
        }],
        ..ProPresenterWorkspace::default()
    }
}

fn screen_look(
    screen_uuid: &str,
    foreground: bool,
    document: Option<Url>,
    slide_uuid: Option<rv_data::Uuid>,
) -> ProScreenLook {
    ProScreenLook {
        pro_screen_uuid: Some(native_uuid(screen_uuid)),
        presentation_foreground_enabled: foreground,
        template_document_file_path: document,
        template_slide_uuid: slide_uuid,
        ..ProScreenLook::default()
    }
}

fn macro_for_look(uuid: &str, name: &str) -> macros_document::Macro {
    macros_document::Macro {
        name: "Song".to_string(),
        actions: vec![Action {
            is_enabled: true,
            r#type: action::ActionType::AudienceLook as i32,
            action_type_data: Some(ActionTypeData::AudienceLook(action::AudienceLookType {
                identification: Some(CollectionElementType {
                    parameter_uuid: Some(native_uuid(uuid)),
                    parameter_name: name.to_string(),
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

fn write_theme(path: &Path, slide_uuids: &[&str]) {
    let document = template::Document {
        slides: slide_uuids
            .iter()
            .map(|uuid| template::Slide {
                base_slide: Some(rv_data::Slide {
                    uuid: Some(native_uuid(uuid)),
                    ..rv_data::Slide::default()
                }),
                ..template::Slide::default()
            })
            .collect(),
        ..template::Document::default()
    };
    std::fs::write(path, document.encode_to_vec()).expect("write theme");
}
