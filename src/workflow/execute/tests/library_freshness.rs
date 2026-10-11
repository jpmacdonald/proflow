use super::*;

use crate::planning_center::PlanSnapshot;
use crate::workflow::classify::tests::support::{
    song_config, song_item, write_song_with_arrangements,
};

fn song_snapshot() -> PlanSnapshot {
    PlanSnapshot::offline_with_items("plan-1", "Sunday Morning", vec![song_item(None)])
}

#[test]
fn review_sees_a_duplicate_title_added_after_the_catalog_was_built() {
    let root = tempfile::tempdir().expect("temporary root");
    let mut runtime = TestRuntime::new(root.path());
    let library = runtime.locations().presentation_library().to_path_buf();
    write_song_with_arrangements(&library.join("Amazing Grace.pro"), &[]);
    runtime.file_index = Mutex::new(LibraryCatalog::build(&library).expect("catalog one song"));
    runtime.replace_config(song_config(None, None));

    // The duplicate arrives outside the workflow, after the catalog was built.
    std::fs::create_dir_all(library.join("Archive")).expect("create archive directory");
    write_song_with_arrangements(&library.join("Archive/Amazing Grace.pro"), &[]);

    let review = runtime
        .executor()
        .review_snapshot(reviewed_request("Duplicate Title"), song_snapshot())
        .expect("review the song plan");

    assert!(
        matches!(review, BuildReview::NeedsReview(_)),
        "a duplicate title must be reviewed, not resolved against a stale catalog"
    );
    assert!(review
        .plans()
        .iter()
        .any(|plan| plan.needs_review() && plan.reason.contains("ambiguous")));
}

#[test]
fn prepared_build_refuses_commit_when_the_library_candidate_set_changed() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = TestRuntime::new(root.path());
    let library = runtime.locations().presentation_library().to_path_buf();
    write_song_with_arrangements(&library.join("Amazing Grace.pro"), &[]);
    let prepared = PreparedBuildRequest::offline_test_prepared_by(
        &runtime.executor(),
        "plan-1",
        "Sunday Morning",
        "Candidates",
    )
    .expect("prepare offline build");

    write_song_with_arrangements(&library.join("Amazing Grace Copy.pro"), &[]);
    let error = runtime
        .executor()
        .build_prepared_offline(prepared)
        .expect_err("a library candidate added after preparation must invalidate it");

    assert!(matches!(error, BuildServiceError::LibraryCandidatesChanged));
}

fn prepared_song_review(runtime: &TestRuntime) -> PreparedBuildRequest {
    let review = runtime
        .executor()
        .review_snapshot(reviewed_request("Prepared Song"), song_snapshot())
        .expect("review the song plan");
    assert!(
        matches!(review, BuildReview::Prepared(_)),
        "a unique library song must cross the prepared boundary"
    );
    expect_prepared(review)
}

#[test]
fn reviewed_song_build_commits_when_the_library_is_unchanged() {
    let root = tempfile::tempdir().expect("temporary root");
    let mut runtime = TestRuntime::new(root.path());
    let library = runtime.locations().presentation_library().to_path_buf();
    write_song_with_arrangements(&library.join("Amazing Grace.pro"), &[]);
    runtime.replace_config(song_config(None, None));
    let prepared = prepared_song_review(&runtime);

    runtime
        .executor()
        .build_prepared_offline(prepared)
        .expect("an unchanged library commits the build it reviewed");

    assert!(runtime
        .locations()
        .playlist_output()
        .join("Prepared Song.proplaylist")
        .is_file());
}

#[test]
fn reviewed_song_build_refuses_commit_when_a_duplicate_title_appears_after_review() {
    let root = tempfile::tempdir().expect("temporary root");
    let mut runtime = TestRuntime::new(root.path());
    let library = runtime.locations().presentation_library().to_path_buf();
    write_song_with_arrangements(&library.join("Amazing Grace.pro"), &[]);
    runtime.replace_config(song_config(None, None));
    let prepared = prepared_song_review(&runtime);

    std::fs::create_dir_all(library.join("Archive")).expect("create archive directory");
    write_song_with_arrangements(&library.join("Archive/Amazing Grace.pro"), &[]);
    let error = runtime
        .executor()
        .build_prepared_offline(prepared)
        .expect_err("a duplicate title added after review must invalidate the build");

    assert!(matches!(error, BuildServiceError::LibraryCandidatesChanged));
    assert!(!runtime
        .locations()
        .playlist_output()
        .join("Prepared Song.proplaylist")
        .exists());
}

fn review_against_candidates(
    runtime: &TestRuntime,
    candidates: proflow_propresenter::library::LibraryCandidates,
) -> Result<BuildReview, BuildServiceError> {
    let request = reviewed_request("Scanned Candidates");
    let planning_center = super::review::ReviewedPlanningCenterSource::offline(&request);
    runtime.executor().review_planned_request(
        request,
        &[generate_title_plan("pco:item:generated", test_style(None))],
        proflow_propresenter::PresentationSize::FULL_HD,
        planning_center,
        candidates,
    )
}

#[test]
fn preparation_refuses_a_library_that_changed_after_review_scanned_it() {
    let root = tempfile::tempdir().expect("temporary root");
    let mut runtime = TestRuntime::new(root.path());
    install_fixture_theme(&mut runtime);
    let library = runtime.locations().presentation_library().to_path_buf();
    write_song_with_arrangements(&library.join("Amazing Grace.pro"), &[]);
    let scanned = runtime
        .file_index
        .lock()
        .expect("catalog lock")
        .scan_candidates()
        .expect("scan candidates");

    // A duplicate lands after the classification the review was based on but
    // before the artifacts are prepared.
    write_song_with_arrangements(&library.join("Amazing Grace Copy.pro"), &[]);
    let error = review_against_candidates(&runtime, scanned)
        .expect_err("preparation must notice the changed library");

    assert!(matches!(error, BuildServiceError::LibraryCandidatesChanged));
}

#[test]
fn preparation_accepts_a_library_unchanged_since_review_scanned_it() {
    let root = tempfile::tempdir().expect("temporary root");
    let mut runtime = TestRuntime::new(root.path());
    install_fixture_theme(&mut runtime);
    let library = runtime.locations().presentation_library().to_path_buf();
    write_song_with_arrangements(&library.join("Amazing Grace.pro"), &[]);
    let scanned = runtime
        .file_index
        .lock()
        .expect("catalog lock")
        .scan_candidates()
        .expect("scan candidates");

    let review = review_against_candidates(&runtime, scanned).expect("prepare against same set");

    assert!(matches!(review, BuildReview::Prepared(_)));
}
