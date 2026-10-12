use super::*;

/// A themed runtime with two images in the data root; only `default` is the
/// image the test plans are styled with.
fn runtime_with_images(root: &Path) -> (TestRuntime, PathBuf, PathBuf) {
    let mut runtime = TestRuntime::new(root);
    install_fixture_theme(&mut runtime);
    let directory = runtime.locations().project_data_root().join("backgrounds");
    std::fs::create_dir(&directory).expect("create backgrounds");
    let configured = directory.join("default.png");
    let this_week = directory.join("this-week.png");
    for image in [&configured, &this_week] {
        std::fs::write(image, minimal_png(1920, 1080)).expect("write background");
    }
    (
        runtime,
        configured.canonicalize().expect("canonical"),
        this_week.canonicalize().expect("canonical"),
    )
}

fn this_week(id: &str) -> BTreeMap<BackgroundId, BackgroundAssetPath> {
    BTreeMap::from([(
        BackgroundId::new(id).expect("background id"),
        BackgroundAssetPath::new("backgrounds/this-week.png").expect("background path"),
    )])
}

fn styled_title() -> ResolvedItemPlan {
    generate_title_plan(
        "pco:item:main",
        test_style(Some(test_background("default"))),
    )
}

fn prepare(
    runtime: &TestRuntime,
    request: BuildRequest,
    plan: ResolvedItemPlan,
) -> PreparedBuildRequest {
    expect_prepared(
        runtime
            .executor()
            .review_build_request(
                request,
                &[plan],
                proflow_propresenter::PresentationSize::FULL_HD,
            )
            .expect("review"),
    )
}

#[test]
fn a_plan_can_use_another_image_for_a_configured_background() {
    let root = tempfile::tempdir().expect("temporary root");
    let (runtime, configured, weekly) = runtime_with_images(root.path());

    let usual = prepare(&runtime, reviewed_request("Usual"), styled_title());
    assert!(usual.has_reviewed_source(&configured));
    assert!(!usual.has_reviewed_source(&weekly));

    let mut request = reviewed_request("This Week");
    request.background_files = this_week("default");
    let changed = prepare(&runtime, request, styled_title());

    assert!(changed.has_reviewed_source(&weekly));
    assert!(!changed.has_reviewed_source(&configured));
    assert_eq!(
        changed.plans()[0]
            .background()
            .map(|background| background.id().as_str()),
        Some("default"),
        "the background keeps its identity; only its file differs"
    );
    assert_ne!(usual.build_directory(), changed.build_directory());
}

#[test]
fn an_image_for_another_background_changes_nothing() {
    let root = tempfile::tempdir().expect("temporary root");
    let (runtime, configured, weekly) = runtime_with_images(root.path());
    let mut request = reviewed_request("Other Background");
    request.background_files = this_week("communion");

    let prepared = prepare(&runtime, request, styled_title());

    assert!(prepared.has_reviewed_source(&configured));
    assert!(!prepared.has_reviewed_source(&weekly));
}

#[test]
fn the_image_follows_a_proposal_that_is_still_under_review() {
    let mut plan = styled_title();
    plan.require_review("Configured to require review".to_string());

    plan.use_background_files(&this_week("default"));

    assert!(plan.needs_review());
    assert_eq!(
        plan.background()
            .map(|background| background.file().as_path()),
        Some(Path::new("backgrounds/this-week.png"))
    );
}
