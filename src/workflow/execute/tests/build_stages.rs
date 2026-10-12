use super::*;

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
