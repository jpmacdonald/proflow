#![allow(clippy::expect_used)]

use super::config::{backup_config_path, candidate_locations, write_config_reviewed};
use super::review::{bounded_days, bounded_usize, recorded_decisions};
use super::{BuildServiceArgs, EntryOverride, ProFlowServer};
use crate::paths::{BuildLocationInputs, BuildLocations, BuildLocationsError, PROJECT_CONFIG_FILE};
use crate::project_config::{LibraryName, ProjectConfig, RawProjectConfig};
use crate::workflow::classify::{PreviewResult, PreviewSummary};
use crate::workflow::execute::{DecisionError, OverrideAction};
use proflow_propresenter::playlist::PlaylistExportMode;
use std::path::Path;

fn config_test_locations(root: &Path) -> BuildLocations {
    let data = root.join("data");
    let propresenter = root.join("ProPresenter");
    let library = propresenter.join("Libraries/Default");
    std::fs::create_dir_all(&data).expect("project data directory");
    std::fs::create_dir_all(&library).expect("current presentation library");
    BuildLocations::from_inputs(BuildLocationInputs {
        project_data_root: data,
        presentation_library: library,
        playlist_output: root.join("playlist-output"),
        propresenter_root: propresenter.clone(),
        themes: propresenter.join("Themes"),
        macros: propresenter.join("Configuration/Macros"),
    })
    .expect("current checked locations")
}

#[test]
fn tool_router_exposes_exactly_the_supported_surface() {
    let names: Vec<_> = ProFlowServer::tool_router()
        .list_all()
        .into_iter()
        .map(|tool| tool.name.to_string())
        .collect();

    assert_eq!(
        names,
        [
            "build_service",
            "catalog_assets",
            "explain_rule_match",
            "fetch_plan",
            "playlist_document_schema",
            "preview_playlist",
            "project_config_schema",
            "read_playlist",
            "search_library",
            "show_effective_config",
            "write_project_config",
        ]
    );
}

#[test]
fn project_config_schema_exposes_the_complete_v4_surface() {
    let schema = serde_json::to_value(schemars::schema_for!(RawProjectConfig))
        .expect("project config schema should serialize");
    let properties = schema["properties"]
        .as_object()
        .expect("root config properties");
    assert_eq!(
        properties.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            "backgrounds",
            "cue_roles",
            "defaults",
            "item_rules",
            "library_identities",
            "metadata",
            "overrides",
            "people",
            "presentation_types",
            "required_playlist_items",
            "service_groups",
            "version",
        ]
    );
    assert_eq!(schema["required"], serde_json::json!(["version"]));
    assert_eq!(properties["version"]["minimum"], 4);
    assert_eq!(properties["version"]["maximum"], 4);
    assert!(schema["$defs"]["ItemRuleConfig"].is_object());
    assert!(schema["$defs"]["PresentationTypeConfig"].is_object());
}

#[test]
fn bounded_arguments_reject_zero_and_oversized_values() {
    assert!(bounded_usize("max_results", Some(0), 10, 100).is_err());
    assert!(bounded_usize("max_results", Some(101), 10, 100).is_err());
    assert!(bounded_days(Some(0), crate::planning_center::PlanLookaheadDays::DEFAULT).is_err());
    assert!(bounded_days(
        Some(366),
        crate::planning_center::PlanLookaheadDays::DEFAULT
    )
    .is_err());
}

#[test]
fn bounded_arguments_accept_defaults_and_limits() {
    assert_eq!(bounded_usize("max_results", None, 10, 100).ok(), Some(10));
    assert_eq!(
        bounded_usize("max_results", Some(100), 10, 100).ok(),
        Some(100)
    );
    assert_eq!(
        bounded_days(None, crate::planning_center::PlanLookaheadDays::DEFAULT)
            .map(crate::planning_center::PlanLookaheadDays::get)
            .ok(),
        Some(30)
    );
    assert_eq!(
        bounded_days(
            Some(365),
            crate::planning_center::PlanLookaheadDays::DEFAULT
        )
        .map(crate::planning_center::PlanLookaheadDays::get)
        .ok(),
        Some(365)
    );
}

#[test]
fn preview_without_background_or_media_defaults_portable_and_preserves_explicit_local() {
    let default_args = serde_json::from_value::<super::PreviewPlaylistArgs>(serde_json::json!({
        "plan_id": "plan-1"
    }))
    .expect("minimal MCP preview arguments");
    assert_eq!(
        default_args.package_mode.unwrap_or_default(),
        PlaylistExportMode::PortableImport
    );

    let local_args = serde_json::from_value::<super::PreviewPlaylistArgs>(serde_json::json!({
        "plan_id": "plan-1",
        "package_mode": "library_local"
    }))
    .expect("explicit local MCP preview arguments");
    assert_eq!(
        local_args.package_mode.unwrap_or_default(),
        PlaylistExportMode::LibraryLinks
    );
}

#[test]
fn config_backups_never_reuse_a_path() {
    let live = Path::new("/tmp/proflow/proflow.config.json");
    let first = backup_config_path(live);
    let second = backup_config_path(live);

    assert_ne!(first, second);
    assert_eq!(
        first.parent(),
        Some(Path::new("/tmp/proflow/config-backups"))
    );
}

#[test]
fn candidate_config_locations_are_selected_by_the_candidate_library() {
    let root = tempfile::tempdir().expect("temporary root");
    let current = config_test_locations(root.path());
    let reviewed_library = current
        .propresenter_root()
        .join("Libraries/Reviewed Library");
    std::fs::create_dir(&reviewed_library).expect("reviewed presentation library");

    let mut raw = RawProjectConfig::default();
    raw.defaults.library =
        LibraryName::new("Reviewed Library").expect("valid candidate library name");
    let candidate = ProjectConfig::try_from(raw).expect("valid candidate config");

    let resolved = candidate_locations(&candidate, &current).expect("candidate locations");
    assert_eq!(resolved.presentation_library(), reviewed_library);

    std::fs::remove_dir(&reviewed_library).expect("remove candidate library");
    let error = candidate_locations(&candidate, &current)
        .expect_err("the current Default library must not mask a missing candidate library");
    assert!(matches!(
        error,
        BuildLocationsError::NotDirectory { path, .. } if path == reviewed_library
    ));
}

#[test]
fn missing_candidate_library_is_rejected_before_candidate_or_activation_writes() {
    for activate in [false, true] {
        let root = tempfile::tempdir().expect("temporary root");
        let current = config_test_locations(root.path());
        let live_path = current.project_data_root().join(PROJECT_CONFIG_FILE);
        let original = b"reviewed live config\n";
        std::fs::write(&live_path, original).expect("live config fixture");

        let mut raw = RawProjectConfig::default();
        raw.defaults.library =
            LibraryName::new("Missing Library").expect("valid candidate library name");
        let candidate = ProjectConfig::try_from(raw).expect("valid candidate config");

        let result = write_config_reviewed(&candidate, &current, activate, Some("reviewed"));

        assert!(result.is_err());
        assert_eq!(
            std::fs::read(&live_path).expect("live config remains readable"),
            original
        );
        assert!(!current
            .project_data_root()
            .join("config-candidates")
            .exists());
        assert!(!current.project_data_root().join("config-backups").exists());
    }
}

#[test]
fn build_tool_rejects_output_options_that_were_not_bound_by_preview() {
    let error = serde_json::from_value::<BuildServiceArgs>(serde_json::json!({
        "plan_id": "plan-1",
        "preview_revision": "revision-1",
        "playlist_name": "Changed after preview"
    }))
    .expect_err("build_service must not accept new output-affecting options");

    assert!(error.to_string().contains("unknown field `playlist_name`"));
}

#[test]
fn unresolved_preview_serializes_without_an_executable_revision() {
    let response = super::schema::ReviewedPreviewResponse {
        preview_revision: None,
        open_decisions: vec![crate::workflow::execute::OpenDecision {
            item: "pco:1:main".to_string(),
            title: "Welcome".to_string(),
            question: "no rule matches".to_string(),
            options: Vec::new(),
        }],
        playlist_name: "Needs decisions".to_string(),
        package_mode: PlaylistExportMode::LibraryLinks,
        media_assets: Vec::new(),
        materialized: None,
        preview: PreviewResult {
            plan_title: "Sunday".to_string(),
            service_name: "Sunday Morning".to_string(),
            date: "2026-07-19".to_string(),
            entries: Vec::new(),
            summary: PreviewSummary::from_entries(&[]),
        },
    };

    let serialized = serde_json::to_value(response).expect("serialize unresolved preview");
    assert!(serialized.get("preview_revision").is_none());
    assert_eq!(serialized["open_decisions"][0]["item"], "pco:1:main");
}

#[test]
fn preview_entry_override_rejects_misspelled_decisions() {
    let error = serde_json::from_value::<EntryOverride>(serde_json::json!({
        "output_key": "pco:item-1:main",
        "playist_name": "Misspelled"
    }))
    .expect_err("unknown override fields must fail instead of being ignored");

    assert!(error.to_string().contains("unknown field `playist_name`"));
}

#[test]
fn preview_arguments_become_the_recorded_decisions_and_then_the_same_request() {
    let args = serde_json::from_value::<super::PreviewPlaylistArgs>(serde_json::json!({
        "plan_id": "plan-1",
        "service_name": "Sunday Morning",
        "playlist_name": "Reviewed",
        "skip_output_keys": ["pco:1:main"],
        "overrides": [{
            "output_key": "pco:2:main",
            "playlist_name": "Hymn",
            "slide_type": "lyrics",
            "action": {"kind": "select_arrangement", "arrangement": "Short"}
        }],
        "media_assets": [{"path": "/media/loop.mp4"}]
    }))
    .expect("preview arguments");

    let decisions = recorded_decisions(args);
    let written = decisions.to_toml().expect("decisions serialize");
    assert!(
        !written.contains("Sunday Morning"),
        "an assertion is not a decision"
    );
    let config = ProjectConfig::try_from(RawProjectConfig::default()).expect("config");
    let request = decisions
        .into_request("plan-1", &config)
        .expect("recorded decisions are a request");

    assert_eq!(request.playlist_name.as_deref(), Some("Reviewed"));
    assert_eq!(request.skip_output_keys, ["pco:1:main"]);
    assert_eq!(
        request.playlist_export.mode(),
        PlaylistExportMode::PortableImport
    );
    assert_eq!(request.playlist_export.media_assets().len(), 1);
    let [changed] = request.overrides.as_slice() else {
        panic!("one override");
    };
    assert_eq!(changed.output_key, "pco:2:main");
    assert_eq!(changed.playlist_name.as_deref(), Some("Hymn"));
    assert!(changed.slide_type.is_some());
    assert!(matches!(
        &changed.action,
        Some(OverrideAction::SelectArrangement { arrangement }) if arrangement == "Short"
    ));
}

#[test]
fn media_for_a_linked_library_preview_is_the_callers_error() {
    let args = serde_json::from_value::<super::PreviewPlaylistArgs>(serde_json::json!({
        "plan_id": "plan-1",
        "package_mode": "library_local",
        "media_assets": [{"path": "/media/loop.mp4"}]
    }))
    .expect("preview arguments");
    let config = ProjectConfig::try_from(RawProjectConfig::default()).expect("config");

    let error = recorded_decisions(args)
        .into_request("plan-1", &config)
        .expect_err("library links cannot carry portable media; it must not be dropped");

    assert!(matches!(error, DecisionError::MediaWithoutPortablePackage));
    assert_eq!(
        super::build_err(error.into()).code,
        rmcp::model::ErrorCode::INVALID_PARAMS
    );
}

#[test]
fn a_build_that_is_not_there_to_install_is_the_callers_error() {
    use crate::workflow::execute::BuildServiceError;

    for stale in [
        BuildServiceError::NoVerifiedBuild {
            plan_id: "plan-1".to_string(),
        },
        BuildServiceError::BuildAlreadyCommitted {
            plan_id: "plan-1".to_string(),
        },
        BuildServiceError::PreparedBuildSuperseded,
    ] {
        assert_eq!(
            super::build_err(stale).code,
            rmcp::model::ErrorCode::INVALID_PARAMS
        );
    }
    assert_eq!(
        super::build_err(BuildServiceError::LibraryCatalogPoisoned).code,
        rmcp::model::ErrorCode::INTERNAL_ERROR
    );
}
