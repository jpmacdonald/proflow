#![allow(clippy::expect_used)]

use super::config::{backup_config_path, candidate_locations, write_config_reviewed};
use super::review::{
    bounded_days, bounded_usize, consume_reviewed_plan, replace_prepared_snapshot,
    resolve_entry_override, PreparedPlanSnapshot, ReviewedPlanError,
};
use super::{BuildServiceArgs, EntryOverride, EntryOverrideAction, ProFlowServer};
use crate::paths::{BuildLocationInputs, BuildLocations, BuildLocationsError, PROJECT_CONFIG_FILE};
use crate::project_config::{
    BackgroundAssetPath, BackgroundId, LibraryName, ProjectConfig, RawProjectConfig,
};
use crate::propresenter::playlist::PlaylistExportMode;
use crate::workflow::classify::{PreviewResult, PreviewSummary};
use crate::workflow::execute::{OverrideAction, PreparedBuildRequest};
use std::collections::HashMap;
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
            "preview_playlist",
            "project_config_schema",
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
fn reviewed_revision_is_consumed_once_and_stale_calls_preserve_current_preview() {
    let mut snapshots = HashMap::from([(
        "plan-1".to_string(),
        PreparedPlanSnapshot {
            revision: "revision-2".to_string(),
            prepared: PreparedBuildRequest::offline_test(
                "plan-1",
                "Sunday Morning",
                "May 24, 2026 - Sunday Morning",
            )
            .expect("empty reviewed request should capture"),
        },
    )]);

    let stale = consume_reviewed_plan(
        &mut snapshots,
        "plan-1",
        "revision-1",
        Some("Sunday Morning"),
    );
    assert_eq!(
        stale.err(),
        Some(ReviewedPlanError::RevisionMismatch {
            plan_id: "plan-1".to_string(),
        })
    );
    assert!(snapshots.contains_key("plan-1"));

    let mismatch = consume_reviewed_plan(
        &mut snapshots,
        "plan-1",
        "revision-2",
        Some("Christmas Eve"),
    );
    assert_eq!(
        mismatch.err(),
        Some(ReviewedPlanError::ServiceNameMismatch {
            actual: "Sunday Morning".to_string(),
        })
    );
    assert!(snapshots.contains_key("plan-1"));

    let consumed = consume_reviewed_plan(
        &mut snapshots,
        "plan-1",
        "revision-2",
        Some("Sunday Morning"),
    )
    .expect("matching revision should be consumed");
    assert_eq!(consumed.prepared.service_name(), "Sunday Morning");
    assert!(!snapshots.contains_key("plan-1"));

    let reused = consume_reviewed_plan(
        &mut snapshots,
        "plan-1",
        "revision-2",
        Some("Sunday Morning"),
    );
    assert_eq!(
        reused.err(),
        Some(ReviewedPlanError::Missing {
            plan_id: "plan-1".to_string(),
        })
    );
}

#[test]
fn unresolved_repreview_invalidates_an_older_prepared_revision() {
    let mut snapshots = HashMap::from([(
        "plan-1".to_string(),
        PreparedPlanSnapshot {
            revision: "stale-revision".to_string(),
            prepared: PreparedBuildRequest::offline_test(
                "plan-1",
                "Sunday Morning",
                "Old prepared playlist",
            )
            .expect("old prepared snapshot"),
        },
    )]);

    let revision = replace_prepared_snapshot(&mut snapshots, "plan-1".to_string(), None);

    assert!(revision.is_none());
    assert!(!snapshots.contains_key("plan-1"));
}

#[test]
fn build_override_resolves_registered_background_once() {
    let id = BackgroundId::new("sermon").expect("valid background id");
    let path = BackgroundAssetPath::new("backgrounds/sermon.png").expect("valid background path");
    let mut raw = RawProjectConfig::default();
    raw.backgrounds.insert(id.clone(), path.clone());
    let config = ProjectConfig::try_from(raw).expect("valid runtime config");

    let resolved = resolve_entry_override(
        &config,
        EntryOverride {
            output_key: "item-1:0".to_string(),
            playlist_name: None,
            slide_type: None,
            action: Some(EntryOverrideAction::SetBackground {
                background: id.clone(),
            }),
        },
    )
    .expect("registered background should resolve");

    let background = resolved
        .action
        .and_then(|action| match action {
            OverrideAction::SetBackground { background } => Some(background),
            _ => None,
        })
        .expect("resolved override should carry a background action");
    assert_eq!(background.id(), &id);
    assert_eq!(background.file(), &path);
}

#[test]
fn build_override_rejects_unknown_background_id() {
    let result = resolve_entry_override(
        &ProjectConfig::try_from(RawProjectConfig::default()).expect("valid empty runtime config"),
        EntryOverride {
            output_key: "item-1:0".to_string(),
            playlist_name: None,
            slide_type: None,
            action: Some(EntryOverrideAction::SetBackground {
                background: BackgroundId::new("missing").expect("valid background id"),
            }),
        },
    );

    assert!(result.is_err());
}
