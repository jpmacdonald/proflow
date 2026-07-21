#![allow(clippy::expect_used)]

use super::*;

fn diagnostic_revision(playlist_export: &PlaylistExportEvidence) -> String {
    let snapshot = PlanSnapshot::offline("plan", "Sunday");
    let render_assets = RenderAssetFingerprint {
        schema: "proflow.render-assets.v2",
        config_sha256: "0".repeat(64),
        theme: None,
        macros: None,
        audience_workspace: None,
        audience_themes: Vec::new(),
        revision: "1".repeat(64),
    };
    let text_fit_contract = TextFitContractSummary::diagnostic();
    let body = ReceiptBody {
        playlist_name: "Sunday",
        package_mode: PlaylistExportMode::PortableImport,
        planning_center: PlanningCenterEvidence {
            revision: snapshot
                .revision()
                .expect("diagnostic snapshot revision")
                .to_string(),
            normalized_snapshot: &snapshot,
        },
        playlist_producer: PlaylistProducerEvidence {
            application_info_sha256: "2".repeat(64),
            application_info_protobuf_hex: String::new(),
        },
        playlist_export,
        render_assets: &render_assets,
        text_fit_contract: &text_fit_contract,
        reviewed_sources: Vec::new(),
        artifacts: Vec::new(),
        entries: &[],
    };
    hash_serialized(&RevisionMaterial {
        schema: RECEIPT_SCHEMA,
        body: &body,
    })
    .expect("diagnostic receipt revision")
}

#[test]
fn receipt_revision_seals_portable_warnings_and_media_manifest() {
    let empty = PlaylistExportEvidence::default();
    let missing = PlaylistExportEvidence::diagnostic_missing("/missing/background.png");
    let embedded = PlaylistExportEvidence::diagnostic_member("/media/background.png");

    let empty_revision = diagnostic_revision(&empty);
    assert_ne!(diagnostic_revision(&missing), empty_revision);
    assert_ne!(diagnostic_revision(&embedded), empty_revision);
    assert_ne!(
        diagnostic_revision(&missing),
        diagnostic_revision(&embedded)
    );
}

#[test]
fn receipt_path_appends_suffix_to_complete_playlist_filename() {
    assert_eq!(
        receipt_path_for_playlist(Path::new("/tmp/Sunday.proplaylist")).expect("receipt path"),
        Path::new("/tmp/Sunday.proplaylist.proflow-build.json")
    );
}

fn reviewed_transaction(paths: &[PathBuf]) -> crate::workflow::transaction::BuildFileTransaction {
    let paths = paths
        .iter()
        .map(|path| crate::workflow::approval::PhysicalPath::resolve_output(path))
        .collect::<Result<Vec<_>, _>>()
        .expect("resolve reviewed outputs");
    let outputs = crate::workflow::approval::OutputManifest::capture(paths)
        .expect("capture reviewed outputs");
    crate::workflow::transaction::BuildFileTransaction::from_reviewed(outputs)
}

#[test]
fn sealed_non_receipt_drift_is_rejected_against_receipt_evidence() {
    let dir = tempfile::tempdir().expect("tempdir");
    let receipt_target = dir.path().join("Sunday.proplaylist.proflow-build.json");
    let playlist_target = dir.path().join("Sunday.proplaylist");
    let mut transaction = reviewed_transaction(&[receipt_target.clone(), playlist_target.clone()]);
    let receipt_stage = transaction
        .stage_reviewed(&receipt_target)
        .expect("receipt stage");
    let playlist_stage = transaction
        .stage_reviewed(&playlist_target)
        .expect("playlist stage");
    std::fs::write(&playlist_stage, b"receipt-evidenced playlist")
        .expect("playlist evidence bytes");
    let evidence = transaction
        .staged_artifacts()
        .expect("snapshot receipt evidence");
    let receipt = PreparedBuildReceipt {
        revision: "revision".to_string(),
        bytes: b"exact receipt\n".to_vec(),
    };
    receipt.write_to(&receipt_stage).expect("write receipt");
    std::fs::write(&playlist_stage, b"drifted playlist").expect("mutate playlist stage");
    let sealed = transaction.seal().expect("seal changed transaction");

    let error = verify_sealed_build_artifacts(&receipt_target, &receipt, &evidence, &sealed)
        .expect_err("non-receipt drift must not cross the prepared boundary");

    assert!(matches!(
        error,
        BuildReceiptError::SealedArtifactDrift { ordinal: 1, .. }
    ));
}

#[test]
fn sealed_receipt_drift_is_rejected_against_prepared_receipt_bytes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let receipt_target = dir.path().join("Sunday.proplaylist.proflow-build.json");
    let playlist_target = dir.path().join("Sunday.proplaylist");
    let mut transaction = reviewed_transaction(&[receipt_target.clone(), playlist_target.clone()]);
    let receipt_stage = transaction
        .stage_reviewed(&receipt_target)
        .expect("receipt stage");
    let playlist_stage = transaction
        .stage_reviewed(&playlist_target)
        .expect("playlist stage");
    std::fs::write(&playlist_stage, b"playlist").expect("playlist bytes");
    let evidence = transaction
        .staged_artifacts()
        .expect("snapshot receipt evidence");
    let receipt = PreparedBuildReceipt {
        revision: "revision".to_string(),
        bytes: b"exact receipt\n".to_vec(),
    };
    receipt.write_to(&receipt_stage).expect("write receipt");
    std::fs::write(&receipt_stage, b"drifted receipt").expect("mutate receipt stage");
    let sealed = transaction.seal().expect("seal changed transaction");

    let error = verify_sealed_build_artifacts(&receipt_target, &receipt, &evidence, &sealed)
        .expect_err("receipt drift must not cross the prepared boundary");

    assert!(matches!(
        error,
        BuildReceiptError::SealedArtifactDrift { ordinal: 0, .. }
    ));
}
