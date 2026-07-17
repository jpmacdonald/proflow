use super::*;

#[tokio::test]
async fn generated_target_appearing_after_preview_is_rejected() {
    let root = tempfile::tempdir().expect("temporary root");
    let mut runtime = TestRuntime::new(root.path());
    install_fixture_theme(&mut runtime);
    let executor = runtime.executor();
    let plan = generate_title_plan("pco:item:main", test_style(None));
    let target = executor
        .presentation_target(&plan)
        .expect("valid generated target name");
    let reviewed = executor
        .review_build_request(
            reviewed_request("Reviewed"),
            &[plan],
            crate::propresenter::PresentationSize::FULL_HD,
        )
        .await
        .expect("review absent target");
    let reviewed = expect_prepared(reviewed);
    std::fs::write(&target, b"external presentation").expect("create target after preview");

    let error = executor
        .build_prepared_request(reviewed)
        .await
        .expect_err("appeared target cannot be overwritten");

    assert!(matches!(error, BuildServiceError::Io(_)));
    assert!(error
        .to_string()
        .contains("build output target changed concurrently"));
    assert_eq!(
        std::fs::read(&target).expect("preserve external target"),
        b"external presentation"
    );
    assert!(
        runtime.file_index.lock().await.entry_at(&target).is_none(),
        "failed commits must not install prepared catalog metadata"
    );
}

#[tokio::test]
async fn generated_existing_target_is_rebuilt_at_full_hd_and_preserves_uuid() {
    let root = tempfile::tempdir().expect("temporary root");
    let mut runtime = TestRuntime::new(root.path());
    install_fixture_theme(&mut runtime);
    let executor = runtime.executor();
    let plan = generate_title_plan("pco:item:main", test_style(None));
    let target = executor
        .presentation_target(&plan)
        .expect("valid generated target name");
    let existing = presentation_with_size("Existing Target", 1280.0, 720.0);
    let existing_uuid = existing.uuid.clone();
    std::fs::write(&target, existing.encode_to_vec()).expect("write existing target");

    let reviewed = executor
        .review_build_request(
            reviewed_request("Reviewed"),
            &[plan],
            crate::propresenter::PresentationSize::FULL_HD,
        )
        .await
        .expect("wrong-sized generated target can be repaired");
    executor
        .build_prepared_request(expect_prepared(reviewed))
        .await
        .expect("commit rebuilt target");

    let rebuilt = rv_data::Presentation::decode(
        std::fs::read(&target)
            .expect("read rebuilt target")
            .as_slice(),
    )
    .expect("decode rebuilt target");
    assert_eq!(rebuilt.uuid, existing_uuid);
    assert!(
        crate::propresenter::resolution::inspect_presentation_size(&rebuilt)
            .matches(crate::propresenter::PresentationSize::FULL_HD)
    );
}

#[tokio::test]
async fn generated_absent_target_gets_one_fresh_stable_uuid() {
    let root = tempfile::tempdir().expect("temporary root");
    let mut runtime = TestRuntime::new(root.path());
    install_fixture_theme(&mut runtime);
    let executor = runtime.executor();
    let plan = generate_title_plan("pco:item:main", test_style(None));
    let target = executor
        .presentation_target(&plan)
        .expect("canonical generated target");
    assert!(!target.exists());

    for build in 0..2 {
        let reviewed = executor
            .review_build_request(
                reviewed_request(&format!("Reviewed {build}")),
                std::slice::from_ref(&plan),
                crate::propresenter::PresentationSize::FULL_HD,
            )
            .await
            .expect("prepare generated target");
        executor
            .build_prepared_request(expect_prepared(reviewed))
            .await
            .expect("commit generated target");
    }

    let generated = rv_data::Presentation::decode(
        std::fs::read(&target)
            .expect("read generated target")
            .as_slice(),
    )
    .expect("decode generated target");
    let uuid = generated.uuid.expect("fresh target identity").string;
    assert!(Uuid::parse_str(&uuid).is_ok());

    let reviewed = executor
        .review_build_request(
            reviewed_request("Reviewed stable"),
            &[plan],
            crate::propresenter::PresentationSize::FULL_HD,
        )
        .await
        .expect("prepare stable rebuild");
    executor
        .build_prepared_request(expect_prepared(reviewed))
        .await
        .expect("commit stable rebuild");
    let rebuilt = rv_data::Presentation::decode(
        std::fs::read(target)
            .expect("read stable rebuild")
            .as_slice(),
    )
    .expect("decode stable rebuild");
    assert_eq!(rebuilt.uuid.map(|value| value.string), Some(uuid));
}

#[tokio::test]
async fn edited_wrong_size_target_repairs_canvas_and_preserves_native_envelope() {
    let root = tempfile::tempdir().expect("temporary root");
    let mut runtime = TestRuntime::new(root.path());
    install_fixture_theme(&mut runtime);
    let target = runtime
        .locations()
        .presentation_library()
        .join("Editable.pro");
    let mut existing = presentation_with_size("Editable", 1280.0, 720.0);
    existing.category = "Liturgy".to_string();
    existing.notes = "Keep this operator note".to_string();
    existing.ccli = Some(rv_data::presentation::Ccli {
        author: "Preserved author".to_string(),
        ..rv_data::presentation::Ccli::default()
    });
    let existing_uuid = existing.uuid.clone();
    std::fs::write(&target, existing.encode_to_vec()).expect("write editable target");
    let mut plan = test_plan(
        "pco:item:edit",
        PlanDisposition::Ready(ReadyAction::EditDescription {
            file_path: target.clone(),
            parsed_content: parsed_content(),
            style: test_style(None),
        }),
    );
    plan.playlist_name = "Editable".to_string();

    let executor = runtime.executor();
    let reviewed = executor
        .review_build_request(
            reviewed_request("Edited"),
            &[plan],
            crate::propresenter::PresentationSize::FULL_HD,
        )
        .await
        .expect("wrong-sized edit source can be repaired");
    executor
        .build_prepared_request(expect_prepared(reviewed))
        .await
        .expect("commit edited target");

    let edited = rv_data::Presentation::decode(
        std::fs::read(target)
            .expect("read edited target")
            .as_slice(),
    )
    .expect("decode edited target");
    assert_eq!(edited.uuid, existing_uuid);
    assert_eq!(edited.category, "Liturgy");
    assert_eq!(edited.notes, "Keep this operator note");
    assert_eq!(
        edited.ccli.as_ref().map(|ccli| ccli.author.as_str()),
        Some("Preserved author")
    );
    assert!(
        crate::propresenter::resolution::inspect_presentation_size(&edited)
            .matches(crate::propresenter::PresentationSize::FULL_HD)
    );
}

#[tokio::test]
async fn playlist_changed_after_preview_is_rejected() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = TestRuntime::new(root.path());
    let executor = runtime.executor();
    let playlist_target = playlist_output_path(runtime.locations().playlist_output(), "Reviewed");
    std::fs::write(&playlist_target, b"reviewed playlist").expect("write playlist");
    let reviewed = executor
        .review_build_request(
            reviewed_request("Reviewed"),
            &[],
            crate::propresenter::PresentationSize::FULL_HD,
        )
        .await
        .expect("review playlist target");
    let reviewed = expect_prepared(reviewed);
    std::fs::write(&playlist_target, b"changed playlist").expect("change playlist");

    let error = executor
        .build_prepared_request(reviewed)
        .await
        .expect_err("changed playlist cannot be overwritten");
    assert!(matches!(error, BuildServiceError::Io(_)));
    assert!(error
        .to_string()
        .contains("build output target changed concurrently"));
    assert_eq!(
        std::fs::read(&playlist_target).expect("preserve external playlist change"),
        b"changed playlist"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn review_rejects_a_symlink_output_target() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = TestRuntime::new(root.path());
    let referent = root.path().join("existing-playlist");
    std::fs::write(&referent, b"existing playlist").expect("write referent");
    let playlist_target = playlist_output_path(runtime.locations().playlist_output(), "Reviewed");
    std::os::unix::fs::symlink(&referent, &playlist_target).expect("create output symlink");

    let error = runtime
        .executor()
        .review_build_request(
            reviewed_request("Reviewed"),
            &[],
            crate::propresenter::PresentationSize::FULL_HD,
        )
        .await
        .expect_err("lexical symlink and physical write targets must not diverge");

    assert!(matches!(
        error,
        BuildServiceError::OutputReview(OutputReviewError::SymlinkTarget { path })
            if path == playlist_target
    ));
    assert_eq!(
        std::fs::read(referent).expect("referent remains unchanged"),
        b"existing playlist"
    );
}

#[cfg(unix)]
#[test]
fn reviewed_background_keeps_canonical_identity_after_symlink_retarget() {
    let root = tempfile::tempdir().expect("temporary root");
    let backgrounds = root.path().join("backgrounds");
    std::fs::create_dir(&backgrounds).expect("create backgrounds");
    let first = backgrounds.join("first.png");
    let second = backgrounds.join("second.png");
    let first_bytes = minimal_png(1920, 1080);
    let second_bytes = minimal_png(1280, 720);
    std::fs::write(&first, &first_bytes).expect("write first");
    std::fs::write(&second, second_bytes).expect("write second");
    let selected = backgrounds.join("selected.png");
    std::os::unix::fs::symlink(&first, &selected).expect("link first background");
    let style = test_style(Some(ResolvedBackground::new(
        BackgroundId::new("selected").expect("background id"),
        BackgroundAssetPath::new("backgrounds/selected.png").expect("background path"),
    )));
    let reviewed = super::review::ReviewedBuildInputs::capture(
        BoundBuildRequest::try_from(reviewed_request("Reviewed")).expect("bound request"),
        vec![generate_title_plan("pco:item:main", style)],
        crate::propresenter::PresentationSize::FULL_HD,
        root.path(),
        root.path(),
        std::iter::empty(),
        std::iter::empty(),
    )
    .expect("capture background");

    std::fs::remove_file(&selected).expect("remove original link");
    std::os::unix::fs::symlink(&second, &selected).expect("retarget background");
    let canonical_first = first.canonicalize().expect("canonical first");

    assert_eq!(reviewed.backgrounds[0].path, canonical_first);
    assert_eq!(
        reviewed.reviewed.source_bytes(&canonical_first),
        Some(first_bytes.as_slice())
    );
}
