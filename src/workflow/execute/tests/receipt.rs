use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::*;

#[tokio::test]
async fn receipt_commits_exact_source_artifact_and_structure_evidence() {
    let root = tempfile::tempdir().expect("temporary root");
    let mut runtime = TestRuntime::new(root.path());
    install_fixture_theme(&mut runtime);
    let (source, arrangement_uuid, library) = arranged_library_fixture(root.path());
    let mut generated = generate_title_plan("pco:item:generated", test_style(None));
    generated.playlist_name = "Generated Evidence".to_string();
    let result = runtime
        .executor()
        .build_prepared_request(expect_prepared(
            runtime
                .executor()
                .review_build_request(
                    reviewed_request("Receipt Evidence"),
                    &[library, generated],
                    crate::propresenter::PresentationSize::FULL_HD,
                )
                .await
                .expect("prepare reviewed build"),
        ))
        .await
        .expect("commit build and receipt");

    assert_eq!(
        result.receipt_path,
        runtime
            .locations()
            .playlist_output()
            .join("Receipt Evidence.proplaylist.proflow-build.json")
            .display()
            .to_string()
    );
    let receipt_bytes = std::fs::read(&result.receipt_path).expect("read committed receipt");
    let receipt: Value = serde_json::from_slice(&receipt_bytes).expect("parse receipt JSON");
    assert_receipt_identity(&receipt, &result, &runtime);
    assert_receipt_entries(&receipt, &result, arrangement_uuid);
    assert_receipt_files(&receipt, &result, &source);
}

fn arranged_library_fixture(root: &Path) -> (PathBuf, uuid::Uuid, ResolvedItemPlan) {
    let source = root.join("library-source.pro");
    let mut presentation = presentation_with_size("Reviewed Library", 1920.0, 1080.0);
    let arrangement_uuid = uuid::Uuid::new_v4();
    presentation.arrangements = vec![rv_data::presentation::Arrangement {
        uuid: Some(rv_data::Uuid {
            string: arrangement_uuid.to_string().to_uppercase(),
        }),
        name: "Default".to_string(),
        group_identifiers: vec![presentation.cue_groups[0]
            .group
            .as_ref()
            .and_then(|group| group.uuid.clone())
            .expect("fixture group identity")],
    }];
    std::fs::write(&source, presentation.encode_to_vec()).expect("write reviewed presentation");
    let mut plan = test_plan(
        "pco:item:library",
        PlanDisposition::Ready(ReadyAction::UseExisting {
            file_path: source.clone(),
            arrangement: Some("Default".to_string()),
        }),
    );
    plan.playlist_name = "Reviewed Library".to_string();
    (source, arrangement_uuid, plan)
}

fn assert_receipt_identity(
    receipt: &Value,
    result: &crate::workflow::report::BuildServiceResult,
    runtime: &TestRuntime,
) {
    assert_eq!(receipt["schema"], "proflow.build-receipt.v3");
    assert_eq!(receipt["revision"], result.receipt_revision);
    assert_eq!(
        receipt["playlist_export"],
        serde_json::json!({
            "warnings": [],
            "media_manifest": {
                "references": [],
                "members": [],
                "unresolved": []
            }
        })
    );
    let planning_center = crate::planning_center::PlanSnapshot::offline("plan-1", "Sunday Morning");
    assert_eq!(receipt["planning_center"]["plan_id"], "plan-1");
    assert_eq!(receipt["planning_center"]["service_id"], "offline-service");
    assert_eq!(receipt["planning_center"]["items"], serde_json::json!([]));
    assert_eq!(
        receipt["planning_center"]["revision"],
        planning_center
            .revision()
            .expect("offline plan revision")
            .to_string()
    );
    assert_eq!(
        receipt["render_assets"],
        serde_json::to_value(runtime.render_assets.fingerprint()).expect("render fingerprint JSON")
    );
    let producer = &receipt["playlist_producer"];
    let application_info_bytes = runtime.playlist_metadata.application_info().encode_to_vec();
    assert_eq!(
        producer["application_info_sha256"],
        digest_hex(&Sha256::digest(&application_info_bytes).into())
    );
    assert_eq!(
        producer["application_info_protobuf_hex"],
        hex_bytes(&application_info_bytes)
    );
    assert_eq!(
        receipt["text_fit_contract"],
        serde_json::to_value(&result.text_fit_contract).expect("text-fit contract JSON")
    );
    assert_eq!(
        receipt["text_fit_contract"]["schema"],
        "proflow.text-fit.v3"
    );
    assert_eq!(receipt["text_fit_contract"]["protocol_version"], 5);
    assert_eq!(
        receipt["text_fit_contract"]["helper_sha256"]
            .as_str()
            .map(str::len),
        Some(64)
    );
}

fn assert_receipt_entries(
    receipt: &Value,
    result: &crate::workflow::report::BuildServiceResult,
    arrangement_uuid: uuid::Uuid,
) {
    assert_eq!(
        receipt["entries"],
        serde_json::to_value(&result.entries).expect("result entry JSON")
    );
    assert_eq!(receipt["entries"].as_array().map(Vec::len), Some(2));
    assert!(receipt["entries"]
        .as_array()
        .expect("receipt entries")
        .iter()
        .all(|entry| entry["presentation_structure"].is_object()));
    assert!(result
        .entries
        .iter()
        .all(|entry| entry.presentation_structure.is_some()));
    let library_selection = receipt["entries"]
        .as_array()
        .expect("receipt entries")
        .iter()
        .find(|entry| entry["output_key"] == "pco:item:library")
        .map(|entry| &entry["playlist_selection"])
        .expect("library selection evidence");
    assert_eq!(
        library_selection["arrangement_uuid"],
        arrangement_uuid.to_string()
    );
    assert_eq!(library_selection["arrangement_name"], "Default");
    assert_eq!(
        library_selection["operator_cue_indexes"],
        serde_json::json!([0])
    );
    let generated_evidence = receipt["entries"]
        .as_array()
        .expect("receipt entries")
        .iter()
        .find(|entry| entry["output_key"] == "pco:item:generated")
        .and_then(|entry| entry["text_fit_evidence"].as_array())
        .expect("generated native text-fit evidence");
    assert_eq!(generated_evidence.len(), 1);
    let source_destination = &generated_evidence[0]["destinations"][0];
    assert_eq!(source_destination["destination"]["kind"], "source_theme");
    assert_eq!(source_destination["destination"]["cue_role"], "content");
    assert!(source_destination["fits_bounds"]
        .as_bool()
        .is_some_and(|fits| fits));
    assert!(source_destination["line_count"].as_u64().is_some());
    assert!(source_destination["metric_style_run_count"]
        .as_u64()
        .is_some());
    assert!(source_destination["used_x"].as_f64().is_some());
    assert!(source_destination["used_y"].as_f64().is_some());
    assert!(source_destination["native_layout_runtime"]
        .as_object()
        .is_some_and(
            |runtime| ["operating_system", "appkit", "core_text"]
                .into_iter()
                .all(|field| runtime
                    .get(field)
                    .and_then(Value::as_str)
                    .is_some_and(|value| !value.is_empty()))
        ));
    assert!(source_destination["input_utf16_length"].as_u64().is_some());
    assert!(source_destination["resolved_fonts"]
        .as_array()
        .is_some_and(|fonts| !fonts.is_empty()
            && fonts.iter().all(|font| {
                font["font_program_path"]
                    .as_str()
                    .is_some_and(|path| std::path::Path::new(path).is_absolute())
                    && font["font_program_sha256"]
                        .as_str()
                        .is_some_and(|digest| digest.len() == 64)
            })));
}

fn assert_receipt_files(
    receipt: &Value,
    result: &crate::workflow::report::BuildServiceResult,
    source: &Path,
) {
    let sources = receipt["reviewed_sources"]
        .as_array()
        .expect("reviewed sources");
    let source_evidence = sources
        .iter()
        .find(|entry| entry["path"] == source.display().to_string())
        .expect("source digest evidence");
    assert_eq!(
        source_evidence["sha256"],
        sha256_file(source).expect("source digest")
    );

    let artifacts = receipt["artifacts"].as_array().expect("artifact evidence");
    assert!(artifacts.len() >= 2);
    assert!(artifacts
        .iter()
        .all(|artifact| artifact["path"] != result.receipt_path));
    for artifact in artifacts {
        let path = PathBuf::from(artifact["path"].as_str().expect("artifact path"));
        let bytes = std::fs::read(&path).expect("committed artifact");
        assert_eq!(
            artifact["length"].as_u64(),
            Some(u64::try_from(bytes.len()).expect("artifact length fits u64"))
        );
        assert_eq!(
            artifact["sha256"],
            digest_hex(&Sha256::digest(&bytes).into())
        );
    }
    let playlist = artifacts.last().expect("playlist artifact");
    assert_eq!(playlist["kind"], "playlist");
    assert_eq!(playlist["path"], result.playlist_path);
    assert!(artifacts
        .iter()
        .any(|artifact| artifact["kind"] == "presentation"));
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

#[tokio::test]
async fn receipt_target_drift_after_review_aborts_the_whole_build() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = TestRuntime::new(root.path());
    let reviewed = expect_prepared(
        runtime
            .executor()
            .review_build_request(
                reviewed_request("Receipt Drift"),
                &[],
                crate::propresenter::PresentationSize::FULL_HD,
            )
            .await
            .expect("prepare reviewed build"),
    );
    let receipt_path = runtime
        .locations()
        .playlist_output()
        .join("Receipt Drift.proplaylist.proflow-build.json");
    std::fs::write(&receipt_path, b"appeared after review").expect("create concurrent receipt");

    let error = runtime
        .executor()
        .build_prepared_request(reviewed)
        .await
        .expect_err("changed receipt target must invalidate the transaction");

    assert!(matches!(error, BuildServiceError::Io(_)));
    assert!(error.to_string().contains("changed concurrently"));
    assert_eq!(
        std::fs::read(&receipt_path).expect("concurrent receipt preserved"),
        b"appeared after review"
    );
    assert!(!runtime
        .locations()
        .playlist_output()
        .join("Receipt Drift.proplaylist")
        .exists());
}

#[tokio::test]
async fn final_presentation_reference_diagnostics_block_preparation() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = TestRuntime::new(root.path());
    let source = root.path().join("dangling.pro");
    let mut presentation = presentation_with_size("Dangling", 1920.0, 1080.0);
    presentation.cue_groups[0].cue_identifiers[0].string = "missing-cue".to_string();
    std::fs::write(&source, presentation.encode_to_vec()).expect("write invalid presentation");

    let error = runtime
        .executor()
        .review_build_request(
            reviewed_request("Invalid Structure"),
            &[use_existing_plan("pco:item:dangling", source)],
            crate::propresenter::PresentationSize::FULL_HD,
        )
        .await
        .expect_err("dangling presentation references must block preparation");

    assert!(matches!(
        error,
        BuildServiceError::PresentationStructureDiagnostics {
            output_key,
            diagnostics,
        } if output_key == "pco:item:dangling" && !diagnostics.is_empty()
    ));
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    std::fs::read(path).map(|bytes| digest_hex(&Sha256::digest(bytes).into()))
}

fn digest_hex(digest: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}
