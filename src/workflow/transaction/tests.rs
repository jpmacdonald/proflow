#![allow(clippy::expect_used)]

use std::cell::Cell;

use super::*;

fn reviewed_transaction(paths: &[PathBuf]) -> BuildFileTransaction {
    let paths = paths
        .iter()
        .map(|path| PhysicalPath::resolve_output(path))
        .collect::<Result<Vec<_>, _>>()
        .expect("resolve reviewed outputs");
    let outputs = OutputManifest::capture(paths).expect("capture reviewed outputs");
    BuildFileTransaction::from_reviewed(outputs)
}

#[test]
fn file_fingerprint_hashes_empty_and_nonempty_files_to_completion() {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, bytes) in [
        ("empty", b"".as_slice()),
        ("content", b"proflow".as_slice()),
    ] {
        let path = dir.path().join(name);
        fs::write(&path, bytes).expect("write fingerprint fixture");

        assert_eq!(
            fingerprint_file(&path).expect("fingerprint fixture"),
            fingerprint_bytes(bytes).expect("fingerprint fixture bytes")
        );
    }
}

#[test]
fn duplicate_target_is_rejected_before_rendering() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("same.pro");
    let mut transaction = reviewed_transaction(std::slice::from_ref(&target));
    transaction.stage_reviewed(&target).expect("first target");
    let error = transaction
        .stage_reviewed(&target)
        .expect_err("duplicate target");
    assert!(matches!(
        error,
        OutputReviewError::AlreadyStagedTarget { path } if path == target
    ));
}

#[test]
fn unreviewed_target_cannot_be_staged() {
    let dir = tempfile::tempdir().expect("tempdir");
    let reviewed = dir.path().join("reviewed.pro");
    let unreviewed = dir.path().join("unreviewed.pro");
    let mut transaction = reviewed_transaction(&[reviewed]);

    let error = transaction
        .stage_reviewed(&unreviewed)
        .expect_err("unreviewed output must not obtain a staging path");

    assert!(matches!(
        error,
        OutputReviewError::UnreviewedTarget { path } if path == unreviewed
    ));
}

#[test]
fn changed_reviewed_target_cannot_be_staged() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("existing.pro");
    fs::write(&target, b"reviewed").expect("reviewed target");
    let mut transaction = reviewed_transaction(std::slice::from_ref(&target));
    fs::write(&target, b"changed").expect("changed target");

    let error = transaction
        .stage_reviewed(&target)
        .expect_err("changed reviewed bytes must not obtain a staging path");

    assert!(matches!(
        error,
        OutputReviewError::Changed { path, .. } if path == target
    ));
}

#[cfg(unix)]
#[test]
fn symlink_introduced_after_review_cannot_be_staged() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("reviewed.pro");
    let referent = dir.path().join("referent.pro");
    let mut transaction = reviewed_transaction(std::slice::from_ref(&target));
    fs::write(&referent, b"referent").expect("referent bytes");
    std::os::unix::fs::symlink(&referent, &target).expect("output symlink");

    let error = transaction
        .stage_reviewed(&target)
        .expect_err("a symlink cannot replace the reviewed output identity");

    assert!(matches!(
        error,
        OutputReviewError::SymlinkTarget { path } if path == target
    ));
    assert_eq!(fs::read(referent).expect("referent remains"), b"referent");
}

#[test]
fn unstaged_reviewed_target_cannot_be_sealed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("required.pro");
    let transaction = reviewed_transaction(std::slice::from_ref(&target));

    let error = transaction
        .seal()
        .expect_err("every reviewed target must be staged");

    assert!(matches!(
        error,
        OutputReviewError::UnstagedTarget { path } if path == target
    ));
}

#[test]
fn non_regular_stage_cannot_be_sealed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("service.pro");
    let mut transaction = reviewed_transaction(std::slice::from_ref(&target));
    let staged = transaction.stage_reviewed(&target).expect("stage target");
    fs::remove_file(&staged).expect("remove reserved regular stage");
    fs::create_dir(&staged).expect("replace stage with directory");

    let error = transaction
        .seal()
        .expect_err("non-regular stage must not cross the prepared boundary");

    assert!(matches!(
        error,
        OutputReviewError::Stage { path, source }
            if path == target && source.to_string().contains("not a regular file")
    ));
}

#[test]
fn failed_seal_removes_every_later_regular_stage() {
    let dir = tempfile::tempdir().expect("tempdir");
    let invalid_target = dir.path().join("invalid.pro");
    let later_target = dir.path().join("later.proplaylist");
    let mut transaction = reviewed_transaction(&[invalid_target.clone(), later_target.clone()]);
    let invalid_stage = transaction
        .stage_reviewed(&invalid_target)
        .expect("invalid target stage");
    let later_stage = transaction
        .stage_reviewed(&later_target)
        .expect("later target stage");
    fs::write(&later_stage, b"later artifact").expect("later artifact");
    fs::remove_file(&invalid_stage).expect("remove reserved regular stage");
    fs::create_dir(&invalid_stage).expect("replace stage with directory");

    transaction
        .seal()
        .expect_err("non-regular early stage must fail sealing");

    assert!(
        !later_stage.exists(),
        "a failed seal must not leak later staging files"
    );
}

#[test]
fn prepared_presentation_artifacts_return_only_exact_sealed_pro_bytes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let presentation = dir.path().join("title.PRO");
    let playlist = dir.path().join("service.proplaylist");
    let mut transaction = reviewed_transaction(&[playlist.clone(), presentation.clone()]);
    let presentation_stage = transaction
        .stage_reviewed(&presentation)
        .expect("presentation stage");
    let playlist_stage = transaction
        .stage_reviewed(&playlist)
        .expect("playlist stage");
    fs::write(presentation_stage, b"presentation bytes").expect("presentation bytes");
    fs::write(playlist_stage, b"playlist bytes").expect("playlist bytes");

    let artifacts = transaction
        .seal()
        .expect("seal transaction")
        .presentation_artifacts()
        .expect("read exact presentation artifacts");

    assert_eq!(
        artifacts,
        vec![(presentation, b"presentation bytes".to_vec())]
    );
}

#[test]
fn sealing_preserves_staging_order_not_manifest_order() {
    let dir = tempfile::tempdir().expect("tempdir");
    let presentation = dir.path().join("title.pro");
    let playlist = dir.path().join("service.proplaylist");
    let mut transaction = reviewed_transaction(&[playlist.clone(), presentation.clone()]);
    let presentation_stage = transaction
        .stage_reviewed(&presentation)
        .expect("presentation stage");
    let playlist_stage = transaction
        .stage_reviewed(&playlist)
        .expect("playlist stage");
    fs::write(presentation_stage, b"presentation").expect("presentation bytes");
    fs::write(playlist_stage, b"playlist").expect("playlist bytes");
    let prepared = transaction.seal().expect("seal transaction");

    let mut committed = Vec::new();
    prepared
        .commit_with_before_recheck(|_, target| {
            committed.push(target.to_path_buf());
            Ok(())
        })
        .expect("commit staged outputs");

    assert_eq!(committed, vec![presentation, playlist]);
}

#[test]
fn staged_artifact_evidence_uses_exact_bytes_in_reservation_order() {
    let dir = tempfile::tempdir().expect("tempdir");
    let receipt = dir.path().join("service.proplaylist.proflow-build.json");
    let playlist = dir.path().join("service.proplaylist");
    let mut transaction = reviewed_transaction(&[playlist.clone(), receipt.clone()]);
    let receipt_stage = transaction.stage_reviewed(&receipt).expect("receipt stage");
    let playlist_stage = transaction
        .stage_reviewed(&playlist)
        .expect("playlist stage");
    fs::write(receipt_stage, b"receipt bytes").expect("receipt bytes");
    fs::write(playlist_stage, b"playlist bytes").expect("playlist bytes");

    let evidence = transaction
        .staged_artifacts()
        .expect("exact staged evidence");

    assert_eq!(evidence.len(), 2);
    assert_eq!(evidence[0].target(), receipt);
    assert_eq!(evidence[0].length(), 13);
    let receipt_sha256: [u8; 32] = Sha256::digest(b"receipt bytes").into();
    assert_eq!(evidence[0].sha256(), receipt_sha256);
    assert_eq!(evidence[1].target(), playlist);
    assert_eq!(evidence[1].length(), 14);
    let playlist_sha256: [u8; 32] = Sha256::digest(b"playlist bytes").into();
    assert_eq!(evidence[1].sha256(), playlist_sha256);
}

#[test]
fn prepared_presentation_artifacts_reject_changed_staged_bytes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("title.pro");
    let mut transaction = reviewed_transaction(std::slice::from_ref(&target));
    let staged = transaction
        .stage_reviewed(&target)
        .expect("presentation stage");
    fs::write(&staged, b"reviewed bytes").expect("reviewed bytes");
    let prepared = transaction.seal().expect("seal transaction");
    fs::write(staged, b"changed bytes").expect("mutate staged bytes");

    let error = prepared
        .presentation_artifacts()
        .expect_err("changed staging file must be rejected");

    assert!(error.to_string().contains("changed after preview"));
}

#[test]
fn dropping_transaction_preserves_existing_target() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("service.pro");
    fs::write(&target, b"old").expect("old target");
    let staged;
    {
        let mut transaction = reviewed_transaction(std::slice::from_ref(&target));
        staged = transaction.stage_reviewed(&target).expect("stage target");
        fs::write(&staged, b"new").expect("stage bytes");
    }
    assert_eq!(fs::read(target).expect("target"), b"old");
    assert!(!staged.exists(), "drop must remove staged output");
}

#[test]
fn commit_replaces_target() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("service.pro");
    fs::write(&target, b"old").expect("old target");
    let mut transaction = reviewed_transaction(std::slice::from_ref(&target));
    let staged = transaction.stage_reviewed(&target).expect("stage target");
    fs::write(&staged, b"new").expect("stage bytes");
    transaction.seal().expect("seal").commit().expect("commit");
    assert_eq!(fs::read(target).expect("target"), b"new");
}

#[test]
fn sealed_artifact_tampering_aborts_before_any_rename() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first_target = dir.path().join("first.pro");
    let second_target = dir.path().join("second.proplaylist");
    fs::write(&first_target, b"old first").expect("old first target");
    fs::write(&second_target, b"old second").expect("old second target");
    let mut transaction = reviewed_transaction(&[first_target.clone(), second_target.clone()]);
    let first_stage = transaction
        .stage_reviewed(&first_target)
        .expect("first stage");
    let second_stage = transaction
        .stage_reviewed(&second_target)
        .expect("second stage");
    fs::write(&first_stage, b"prepared first").expect("first artifact");
    fs::write(&second_stage, b"prepared second").expect("second artifact");
    let prepared = transaction.seal().expect("seal exact artifacts");

    fs::write(&second_stage, b"tampered second").expect("tamper staged artifact");
    let mut rechecked_targets = Vec::new();
    let error = prepared
        .commit_with_before_recheck(|_, target| {
            rechecked_targets.push(target.to_path_buf());
            Ok(())
        })
        .expect_err("tampered artifact must not be committed");

    assert!(error.to_string().contains("changed after preview"));
    assert!(
        rechecked_targets.is_empty(),
        "every sealed stage must pass preflight before the first target recheck or rename"
    );
    assert_eq!(fs::read(first_target).expect("first target"), b"old first");
    assert_eq!(
        fs::read(second_target).expect("second target"),
        b"old second"
    );
}

#[test]
fn sealed_receipt_drift_aborts_before_any_output_is_replaced() {
    let dir = tempfile::tempdir().expect("tempdir");
    let receipt = dir.path().join("service.proplaylist.proflow-build.json");
    let playlist = dir.path().join("service.proplaylist");
    fs::write(&receipt, b"old receipt").expect("old receipt");
    fs::write(&playlist, b"old playlist").expect("old playlist");
    let mut transaction = reviewed_transaction(&[playlist.clone(), receipt.clone()]);
    let receipt_stage = transaction.stage_reviewed(&receipt).expect("receipt stage");
    let playlist_stage = transaction
        .stage_reviewed(&playlist)
        .expect("playlist stage");
    fs::write(&receipt_stage, b"prepared receipt").expect("prepared receipt");
    fs::write(playlist_stage, b"prepared playlist").expect("prepared playlist");
    let prepared = transaction.seal().expect("seal transaction");

    fs::write(receipt_stage, b"tampered receipt").expect("tamper receipt stage");
    let entered_commit_loop = Cell::new(false);
    let error = prepared
        .commit_with_before_recheck(|_, _| {
            entered_commit_loop.set(true);
            Ok(())
        })
        .expect_err("receipt drift must invalidate the transaction");

    assert!(error.to_string().contains("changed after preview"));
    assert!(!entered_commit_loop.get());
    assert_eq!(fs::read(receipt).expect("live receipt"), b"old receipt");
    assert_eq!(fs::read(playlist).expect("live playlist"), b"old playlist");
}

#[test]
fn playlist_is_last_and_its_failure_rolls_back_the_receipt() {
    let dir = tempfile::tempdir().expect("tempdir");
    let receipt = dir.path().join("service.proplaylist.proflow-build.json");
    let playlist = dir.path().join("service.proplaylist");
    fs::write(&receipt, b"old receipt").expect("old receipt");
    fs::write(&playlist, b"old playlist").expect("old playlist");
    let mut transaction = reviewed_transaction(&[playlist.clone(), receipt.clone()]);
    let receipt_stage = transaction.stage_reviewed(&receipt).expect("receipt stage");
    let playlist_stage = transaction
        .stage_reviewed(&playlist)
        .expect("playlist stage");
    fs::write(receipt_stage, b"prepared receipt").expect("prepared receipt");
    fs::write(playlist_stage, b"prepared playlist").expect("prepared playlist");
    let mut order = Vec::new();

    let error = transaction
        .seal()
        .expect("seal transaction")
        .commit_with_before_recheck(|index, target| {
            order.push(target.to_path_buf());
            if index == 1 {
                return Err(io::Error::other("simulated playlist commit failure"));
            }
            Ok(())
        })
        .expect_err("playlist failure must roll back earlier receipt commit");

    assert!(error.to_string().contains("playlist commit failure"));
    assert_eq!(order, vec![receipt.clone(), playlist.clone()]);
    assert_eq!(fs::read(receipt).expect("restored receipt"), b"old receipt");
    assert_eq!(
        fs::read(playlist).expect("unchanged playlist"),
        b"old playlist"
    );
}

#[cfg(unix)]
#[test]
fn sealed_stage_replaced_by_symlink_to_identical_bytes_is_rejected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("service.pro");
    let referent = dir.path().join("identical-bytes.pro");
    fs::write(&target, b"old target").expect("old target");
    fs::write(&referent, b"prepared bytes").expect("symlink referent");
    let mut transaction = reviewed_transaction(std::slice::from_ref(&target));
    let staged = transaction.stage_reviewed(&target).expect("stage target");
    fs::write(&staged, b"prepared bytes").expect("prepared bytes");
    let prepared = transaction.seal().expect("seal regular stage");

    fs::remove_file(&staged).expect("remove sealed stage");
    std::os::unix::fs::symlink(&referent, &staged).expect("replace stage with symlink");
    let error = prepared
        .commit()
        .expect_err("symlinked stage must not be committed");

    assert!(error.to_string().contains("artifact is a symlink"));
    assert_eq!(fs::read(target).expect("live target"), b"old target");
    assert_eq!(fs::read(referent).expect("referent"), b"prepared bytes");
}

#[test]
fn commit_aborts_before_any_rename_when_a_target_changes_after_staging() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first_target = dir.path().join("first.pro");
    let second_target = dir.path().join("second.pro");
    fs::write(&first_target, b"old first").expect("old first target");
    fs::write(&second_target, b"old second").expect("old second target");
    let mut transaction = reviewed_transaction(&[first_target.clone(), second_target.clone()]);
    let first_stage = transaction
        .stage_reviewed(&first_target)
        .expect("first stage");
    let second_stage = transaction
        .stage_reviewed(&second_target)
        .expect("second stage");
    fs::write(first_stage, b"new first").expect("new first bytes");
    fs::write(second_stage, b"new second").expect("new second bytes");

    let prepared = transaction.seal().expect("seal");
    fs::write(&second_target, b"concurrent second").expect("concurrent target change");
    let entered_commit_loop = Cell::new(false);
    let error = prepared
        .commit_with_before_recheck(|_, _| {
            entered_commit_loop.set(true);
            Ok(())
        })
        .expect_err("concurrent target change must abort commit");

    assert!(error.to_string().contains("changed concurrently"));
    assert!(
        !entered_commit_loop.get(),
        "initial validation must stop before the first rename-loop iteration"
    );
    assert_eq!(
        fs::read(first_target).expect("unchanged first target"),
        b"old first"
    );
    assert_eq!(
        fs::read(second_target).expect("concurrent second target"),
        b"concurrent second"
    );
}

#[test]
fn commit_preserves_a_concurrently_created_target() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("new.pro");
    let mut transaction = reviewed_transaction(std::slice::from_ref(&target));
    let staged = transaction
        .stage_reviewed(&target)
        .expect("stage absent target");
    fs::write(staged, b"generated").expect("generated bytes");

    let prepared = transaction.seal().expect("seal");
    fs::write(&target, b"concurrent").expect("concurrently create target");
    let error = prepared
        .commit()
        .expect_err("concurrently created target must abort commit");

    assert!(error.to_string().contains("changed concurrently"));
    assert_eq!(fs::read(target).expect("concurrent target"), b"concurrent");
}

#[test]
fn per_target_recheck_rolls_back_without_overwriting_post_install_change() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first_target = dir.path().join("first.pro");
    let second_target = dir.path().join("second.pro");
    let third_target = dir.path().join("third.pro");
    fs::write(&first_target, b"old first").expect("old first target");
    fs::write(&second_target, b"old second").expect("old second target");
    fs::write(&third_target, b"old third").expect("old third target");

    let mut transaction = reviewed_transaction(&[
        first_target.clone(),
        second_target.clone(),
        third_target.clone(),
    ]);
    let first_stage = transaction
        .stage_reviewed(&first_target)
        .expect("first stage");
    let second_stage = transaction
        .stage_reviewed(&second_target)
        .expect("second stage");
    let third_stage = transaction
        .stage_reviewed(&third_target)
        .expect("third stage");
    fs::write(first_stage, b"new first").expect("new first bytes");
    fs::write(second_stage, b"new second").expect("new second bytes");
    fs::write(third_stage, b"new third").expect("new third bytes");

    let error = transaction
        .seal()
        .expect("seal")
        .commit_with_before_recheck(|index, target| {
            if index == 2 {
                fs::write(&second_target, b"concurrent second")?;
                fs::write(target, b"concurrent third")?;
            }
            Ok(())
        })
        .expect_err("third target changed after preflight must abort commit");

    assert!(error.to_string().contains("changed concurrently"));
    assert_eq!(
        fs::read(first_target).expect("restored first target"),
        b"old first"
    );
    assert_eq!(
        fs::read(second_target).expect("concurrent second target"),
        b"concurrent second"
    );
    assert_eq!(
        fs::read(third_target).expect("concurrent third target"),
        b"concurrent third"
    );
}

#[test]
fn review_capture_propagates_non_not_found_read_errors() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("directory-target.pro");
    fs::create_dir(&target).expect("directory target");
    let physical = PhysicalPath::resolve_output(&target).expect("resolve output path");

    assert!(OutputManifest::capture([physical]).is_err());
}

#[test]
fn optional_target_read_propagates_non_not_found_errors() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("directory-target.pro");
    fs::create_dir(&target).expect("directory target");

    let error = read_optional(&target).expect_err("a directory is not an absent target");

    assert_ne!(error.kind(), io::ErrorKind::NotFound);
}

#[test]
fn later_commit_failure_restores_an_earlier_target() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first_target = dir.path().join("first.pro");
    let second_target = dir.path().join("second.pro");
    fs::write(&first_target, b"old").expect("old first target");
    let mut transaction = reviewed_transaction(&[first_target.clone(), second_target.clone()]);
    let first_stage = transaction
        .stage_reviewed(&first_target)
        .expect("first stage");
    fs::write(first_stage, b"new").expect("new first target");
    let second_stage = transaction
        .stage_reviewed(&second_target)
        .expect("second stage");
    fs::write(&second_stage, b"second").expect("second bytes");
    let prepared = transaction.seal().expect("seal");
    fs::remove_file(second_stage).expect("force second commit failure");

    assert!(prepared.commit().is_err());
    assert_eq!(
        fs::read(first_target).expect("restored first target"),
        b"old"
    );
    assert!(!second_target.exists());
}

#[test]
fn later_commit_failure_removes_an_earlier_new_target() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first_target = dir.path().join("new-first.pro");
    let second_target = dir.path().join("second.pro");
    let mut transaction = reviewed_transaction(&[first_target.clone(), second_target.clone()]);
    let first_stage = transaction
        .stage_reviewed(&first_target)
        .expect("first stage");
    fs::write(first_stage, b"new").expect("new first target");
    let second_stage = transaction
        .stage_reviewed(&second_target)
        .expect("second stage");
    fs::write(&second_stage, b"second").expect("second bytes");
    let prepared = transaction.seal().expect("seal");
    fs::remove_file(second_stage).expect("force second commit failure");

    assert!(prepared.commit().is_err());
    assert!(!first_target.exists());
    assert!(!second_target.exists());
}

#[test]
fn rollback_does_not_overwrite_a_concurrently_changed_committed_target() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("service.pro");
    fs::write(&target, b"old").expect("old target");
    let mut transaction = reviewed_transaction(std::slice::from_ref(&target));
    let staged = transaction.stage_reviewed(&target).expect("stage target");
    fs::write(&staged, b"installed").expect("installed bytes");

    let mut prepared = transaction.seal().expect("seal");
    let installed = prepared.files[0].prepared;
    fs::rename(&staged, &target).expect("simulate committed rename");
    prepared.files[0].installed = Some(installed);
    fs::write(&target, b"concurrent").expect("concurrent post-commit change");

    let error = prepared
        .rollback_prefix(1)
        .expect_err("rollback must reject changed installed bytes");

    assert!(error.to_string().contains("changed concurrently"));
    assert_eq!(fs::read(target).expect("concurrent target"), b"concurrent");
}
