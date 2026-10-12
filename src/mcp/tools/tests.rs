#![allow(clippy::expect_used)]

use std::sync::Arc;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ErrorCode};
use serde_json::{json, Value};

use crate::mcp::ProFlowServer;
use crate::workstation::test_support::{TestSite, TestWorkstation, ITEM, PLAN_ID};

fn server() -> (ProFlowServer, TestSite) {
    let (workstation, site) = TestWorkstation::new().into_parts();
    (
        ProFlowServer {
            workstation: Arc::new(workstation),
        },
        site,
    )
}

fn arguments<T: serde::de::DeserializeOwned>(value: Value) -> Parameters<T> {
    Parameters(serde_json::from_value(value).expect("tool arguments"))
}

fn body(result: &CallToolResult) -> Value {
    let text = result.content[0]
        .as_text()
        .expect("text content")
        .text
        .as_str();
    serde_json::from_str(text).expect("tool results are JSON")
}

#[tokio::test]
async fn preview_records_decisions_and_build_installs_only_the_previewed_revision() {
    let (server, station) = server();

    let open = body(
        &server
            .preview_playlist(arguments(json!({ "plan_id": PLAN_ID })))
            .await
            .expect("preview"),
    );
    assert!(open.get("preview_revision").is_none());
    assert_eq!(open["open_decisions"][0]["item"], ITEM);
    assert!(
        station.decisions_file().is_file(),
        "a preview records its decisions, even none"
    );

    let resolved = body(
        &server
            .preview_playlist(arguments(json!({
                "plan_id": PLAN_ID,
                "playlist_name": "Reviewed",
                "skip_output_keys": [ITEM]
            })))
            .await
            .expect("preview with decisions"),
    );
    let revision = resolved["preview_revision"]
        .as_str()
        .expect("a resolved preview names its build");
    assert!(resolved.get("open_decisions").is_none());
    assert_eq!(resolved["playlist_name"], "Reviewed");
    let recorded = std::fs::read_to_string(station.decisions_file()).expect("decisions");
    assert!(recorded.contains("playlist_name = \"Reviewed\"") && recorded.contains(ITEM));
    assert_eq!(station.installed_playlists(), 0, "preview installs nothing");

    let other_revision = server
        .build_service(arguments(json!({
            "plan_id": PLAN_ID,
            "preview_revision": "0".repeat(revision.len())
        })))
        .await
        .expect_err("another revision is not the previewed build");
    assert_eq!(other_revision.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(station.installed_playlists(), 0);

    let built = body(
        &server
            .build_service(arguments(json!({
                "plan_id": PLAN_ID,
                "preview_revision": revision
            })))
            .await
            .expect("build the previewed revision"),
    );
    assert!(built["playlist_path"]
        .as_str()
        .is_some_and(|path| path.ends_with("Reviewed.proplaylist")));
    assert_eq!(station.installed_playlists(), 1);

    let twice = server
        .build_service(arguments(json!({
            "plan_id": PLAN_ID,
            "preview_revision": revision
        })))
        .await
        .expect_err("a build installs once");
    assert_eq!(twice.code, ErrorCode::INVALID_PARAMS);

    let document = body(
        &server
            .read_playlist(arguments(json!({ "path": "Reviewed.proplaylist" })))
            .await
            .expect("read the installed playlist"),
    );
    assert_eq!(document["schema_version"], 1);
}

#[tokio::test]
async fn a_preview_with_no_decisions_replaces_decisions_recorded_earlier() {
    let (server, station) = server();
    station.skip_the_item();

    let preview = body(
        &server
            .preview_playlist(arguments(json!({ "plan_id": PLAN_ID })))
            .await
            .expect("preview"),
    );

    assert_eq!(preview["open_decisions"][0]["item"], ITEM);
    assert!(!std::fs::read_to_string(station.decisions_file())
        .expect("decisions")
        .contains(ITEM));
}

#[tokio::test]
async fn a_malformed_preview_is_the_callers_error_and_records_nothing() {
    let (server, station) = server();
    station.skip_the_item();
    let recorded = std::fs::read_to_string(station.decisions_file()).expect("decisions");

    let error = server
        .preview_playlist(arguments(json!({
            "plan_id": PLAN_ID,
            "package_mode": "library_local",
            "media_assets": [{ "path": "/media/loop.mp4" }]
        })))
        .await
        .expect_err("library links carry no media");

    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(
        std::fs::read_to_string(station.decisions_file()).expect("decisions"),
        recorded,
        "refused decisions leave the recorded ones as they were"
    );
}

#[tokio::test]
async fn a_decision_given_to_a_preview_is_remembered_when_it_answers_an_open_question() {
    let (server, station) = server();
    server
        .preview_playlist(arguments(json!({ "plan_id": PLAN_ID })))
        .await
        .expect("preview that leaves the item open");
    server
        .preview_playlist(arguments(json!({
            "plan_id": PLAN_ID,
            "skip_output_keys": [ITEM]
        })))
        .await
        .expect("preview that answers it");

    let asked_again = body(
        &server
            .preview_playlist(arguments(json!({ "plan_id": PLAN_ID })))
            .await
            .expect("preview with no decisions"),
    );

    let first = &asked_again["open_decisions"][0]["options"][0];
    assert_eq!(first["choice"], "skip");
    assert_eq!(first["remembered"]["times"], 1);
    assert!(station.memory_file().is_file());
}
