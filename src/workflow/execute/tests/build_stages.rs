use super::*;
use std::path::Path;

fn build_directories(runtime: &TestRuntime) -> Vec<PathBuf> {
    let plan_root = runtime
        .locations()
        .project_data_root()
        .join("builds/plan-1");
    let mut directories = std::fs::read_dir(plan_root)
        .expect("plan build root")
        .map(|entry| entry.expect("build directory entry").path())
        .collect::<Vec<_>>();
    directories.sort();
    directories
}

fn review(runtime: &TestRuntime, request: BuildRequest, plans: &[ResolvedItemPlan]) -> BuildReview {
    runtime
        .executor()
        .review_build_request(
            request,
            plans,
            proflow_propresenter::PresentationSize::FULL_HD,
        )
        .expect("review")
}

fn skipped(output_key: &str) -> ResolvedItemPlan {
    test_plan(output_key, PlanDisposition::Skip)
}

#[test]
fn review_writes_its_inputs_and_plan_and_an_equal_review_reopens_them() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = TestRuntime::new(root.path());
    let plans = [skipped("pco:item:a")];

    review(&runtime, reviewed_request("Reviewed"), &plans);
    let directories = build_directories(&runtime);
    let [build] = directories.as_slice() else {
        panic!("one review writes one build directory: {directories:?}");
    };
    for file in [
        "snapshot/plan.json",
        "snapshot/render-assets.json",
        "snapshot/bible-corpora.json",
        "snapshot/catalog.json",
        "snapshot/decisions.json",
        "snapshot/stage.json",
        "classified/plans.json",
        "classified/stage.json",
    ] {
        assert!(build.join(file).is_file(), "missing {file}");
    }
    let plans_json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(build.join("classified/plans.json")).expect("plans"))
            .expect("plans are JSON");
    assert_eq!(plans_json[0]["output_key"], "pco:item:a");

    review(&runtime, reviewed_request("Reviewed"), &plans);
    assert_eq!(build_directories(&runtime), directories);
}

#[test]
fn a_different_decision_selects_a_different_build_directory() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = TestRuntime::new(root.path());
    let plans = [skipped("pco:item:a"), skipped("pco:item:b")];

    review(&runtime, reviewed_request("Reviewed"), &plans);
    let mut skipping = reviewed_request("Reviewed");
    skipping.skip_output_keys = vec!["pco:item:b".to_string(), "pco:item:a".to_string()];
    review(&runtime, skipping.clone(), &plans);
    assert_eq!(build_directories(&runtime).len(), 2);

    // The same decisions in another order are the same decisions.
    skipping.skip_output_keys.reverse();
    review(&runtime, skipping, &plans);
    assert_eq!(build_directories(&runtime).len(), 2);

    review(&runtime, reviewed_request("Renamed"), &plans);
    assert_eq!(build_directories(&runtime).len(), 3);
}

#[test]
fn a_library_presentation_edited_in_place_selects_a_different_build_directory() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = TestRuntime::new(root.path());
    let song = runtime.locations().presentation_library().join("Song.pro");
    std::fs::write(
        &song,
        presentation_with_size("Song", 1920.0, 1080.0).encode_to_vec(),
    )
    .expect("write library presentation");
    let plans = [skipped("pco:item:a")];

    review(&runtime, reviewed_request("Reviewed"), &plans);
    std::fs::write(
        &song,
        presentation_with_size("Song", 1280.0, 720.0).encode_to_vec(),
    )
    .expect("edit library presentation in place");
    review(&runtime, reviewed_request("Reviewed"), &plans);

    assert_eq!(build_directories(&runtime).len(), 2);
}

#[test]
fn a_review_that_still_needs_decisions_records_what_it_classified() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = TestRuntime::new(root.path());
    let plans = [test_plan(
        "pco:item:a",
        PlanDisposition::NeedsReview(ReviewContext::new(None)),
    )];

    let reviewed = review(&runtime, reviewed_request("Reviewed"), &plans);

    assert!(matches!(reviewed, BuildReview::NeedsReview(_)));
    let directories = build_directories(&runtime);
    assert!(directories[0].join("classified/plans.json").is_file());
    assert!(!directories[0].join("rendered").exists());
}

fn generated_title(runtime: &TestRuntime) -> PreparedBuildRequest {
    expect_prepared(review(
        runtime,
        reviewed_request("Reviewed"),
        &[generate_title_plan("pco:item:generated", test_style(None))],
    ))
}

fn themed_runtime(root: &Path) -> TestRuntime {
    let mut runtime = TestRuntime::new(root);
    install_fixture_theme(&mut runtime);
    runtime
}

fn staged_copies(runtime: &TestRuntime) -> usize {
    std::fs::read_dir(runtime.locations().presentation_library())
        .expect("list library")
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .contains("proflow-stage")
        })
        .count()
}

#[test]
fn a_prepared_build_is_held_on_disk_and_commit_records_what_it_installed() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = themed_runtime(root.path());
    let prepared = generated_title(&runtime);
    let build = prepared.build_directory().to_path_buf();

    assert_eq!(
        staged_copies(&runtime),
        0,
        "nothing stays staged beside live targets"
    );
    for file in [
        "rendered/manifest.json",
        "rendered/result.json",
        "rendered/artifacts/000",
        "verified/checks.json",
    ] {
        assert!(build.join(file).is_file(), "missing {file}");
    }
    assert!(!build.join("committed").exists());

    let result = runtime
        .executor()
        .build_prepared_offline(prepared)
        .expect("commit from the build directory");

    let targets: Vec<PathBuf> = serde_json::from_slice(
        &std::fs::read(build.join("committed/targets.json")).expect("committed record"),
    )
    .expect("targets are JSON");
    assert_eq!(targets.last(), Some(&result.playlist_path));
    assert!(targets.iter().all(|target| target.is_file()));
    assert_eq!(staged_copies(&runtime), 0);
}

#[test]
fn rendering_the_same_inputs_again_supersedes_the_earlier_prepared_build() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = themed_runtime(root.path());
    let earlier = generated_title(&runtime);
    let later = generated_title(&runtime);
    assert_eq!(earlier.build_directory(), later.build_directory());
    assert!(!later.build_directory().join("retired").exists());

    let error = runtime
        .executor()
        .build_prepared_offline(earlier)
        .expect_err("the stage the earlier preview showed is gone");
    assert!(matches!(error, BuildServiceError::PreparedBuildSuperseded));

    runtime
        .executor()
        .build_prepared_offline(later)
        .expect("the current render commits");
}

#[test]
fn an_artifact_changed_in_the_build_directory_is_refused_at_commit() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = themed_runtime(root.path());
    let prepared = generated_title(&runtime);
    std::fs::write(
        prepared.build_directory().join("rendered/artifacts/000"),
        b"swapped",
    )
    .expect("swap artifact");

    let error = runtime
        .executor()
        .build_prepared_offline(prepared)
        .expect_err("a changed artifact must not be installed");

    assert!(matches!(
        error,
        BuildServiceError::BuildDir(crate::workflow::build_dir::BuildDirError::SealMismatch {
            stage: "rendered",
            ..
        })
    ));
    assert_eq!(staged_copies(&runtime), 0);
}

#[test]
fn a_changed_snapshot_is_refused_at_commit() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = TestRuntime::new(root.path());
    let prepared = prepare_empty(&runtime, "Reviewed");
    let plan = prepared.build_directory().join("snapshot/plan.json");
    let mut edited = std::fs::read(&plan).expect("plan");
    edited.push(b'\n');
    std::fs::write(&plan, edited).expect("edit plan snapshot");

    let error = runtime
        .executor()
        .build_prepared_offline(prepared)
        .expect_err("a snapshot edited after review must not commit");

    assert!(matches!(
        error,
        BuildServiceError::BuildDir(crate::workflow::build_dir::BuildDirError::SealMismatch {
            stage: "snapshot",
            ..
        })
    ));
}

#[test]
fn a_snapshot_stage_from_another_build_is_refused_at_commit() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = TestRuntime::new(root.path());
    let prepared = prepare_empty(&runtime, "Reviewed");
    let other = prepare_empty(&runtime, "Another Playlist");
    let snapshot = prepared.build_directory().join("snapshot");
    std::fs::remove_dir_all(&snapshot).expect("remove snapshot stage");
    std::fs::create_dir(&snapshot).expect("recreate snapshot stage");
    for entry in std::fs::read_dir(other.build_directory().join("snapshot")).expect("list") {
        let entry = entry.expect("snapshot entry");
        std::fs::copy(entry.path(), snapshot.join(entry.file_name())).expect("copy");
    }

    let error = runtime
        .executor()
        .build_prepared_offline(prepared)
        .expect_err("a correctly sealed snapshot of other inputs must not commit");

    assert!(matches!(
        error,
        BuildServiceError::BuildDir(
            crate::workflow::build_dir::BuildDirError::InputsMismatch { .. }
        )
    ));
}

#[test]
fn a_committed_build_stays_as_a_record_when_its_inputs_are_built_again() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = TestRuntime::new(root.path());
    let first = prepare_empty(&runtime, "Reviewed");
    let build = first.build_directory().to_path_buf();
    runtime
        .executor()
        .build_prepared_offline(first)
        .expect("first commit");

    // An empty plan writes no presentation, so the library, and with it the
    // build directory, is the same for the second review.
    let second = prepare_empty(&runtime, "Reviewed");
    assert_eq!(second.build_directory(), build);
    assert!(build.join("retired/0/committed/targets.json").is_file());
    assert!(build.join("retired/0/rendered/manifest.json").is_file());
    assert!(!build.join("committed").exists());

    runtime
        .executor()
        .build_prepared_offline(second)
        .expect("the playlist written by the first commit is replaced by the second");
    assert!(build.join("committed/targets.json").is_file());
}
