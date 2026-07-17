use super::*;

#[test]
fn override_updates_name_and_complete_render_action() {
    let original = test_plan(
        "pco:item:main",
        PlanDisposition::Ready(ReadyAction::GenerateDescription {
            parsed_content: parsed_content(),
            style: test_style(Some(test_background("default"))),
        }),
    );
    let override_entry = EntryOverride {
        output_key: original.output_key.to_string(),
        playlist_name: Some("Weekly Call to Worship".to_string()),
        slide_type: None,
        action: Some(OverrideAction::SetBackground {
            background: test_background("sermon"),
        }),
    };

    let effective = apply_override(&original, Some(&override_entry)).expect("valid override");

    assert_eq!(effective.playlist_name, "Weekly Call to Worship");
    assert_eq!(
        effective
            .render_style()
            .and_then(RenderStyle::background)
            .map(|background| background.id().to_string()),
        Some("sermon".to_string())
    );
    assert!(matches!(
        effective.ready_action(),
        Some(ReadyAction::GenerateDescription { .. })
    ));
}

#[test]
fn use_existing_override_cannot_retain_render_state() {
    let root = tempfile::tempdir().expect("temporary root");
    let source = root.path().join("Existing.pro");
    std::fs::write(
        &source,
        presentation_with_size("Existing", 1920.0, 1080.0).encode_to_vec(),
    )
    .expect("write native source");
    let original = generate_title_plan(
        "pco:item:main",
        test_style(Some(test_background("default"))),
    );
    let override_entry = EntryOverride {
        output_key: original.output_key.to_string(),
        playlist_name: None,
        slide_type: None,
        action: Some(OverrideAction::UseExisting {
            file_path: source.clone(),
            arrangement: Some("Default".to_string()),
        }),
    };

    let effective = apply_override(&original, Some(&override_entry)).expect("valid override");
    let canonical_source = source.canonicalize().expect("canonical source");

    assert!(effective.render_style().is_none());
    assert!(matches!(
        effective.ready_action(),
        Some(ReadyAction::UseExisting { file_path, arrangement })
            if file_path == canonical_source.as_path()
                && arrangement.as_deref() == Some("Default")
    ));
}

#[test]
fn use_existing_override_canonicalizes_its_native_source() {
    let root = tempfile::tempdir().expect("temporary root");
    let nested = root.path().join("nested");
    std::fs::create_dir(&nested).expect("create nested directory");
    let source = root.path().join("Existing.pro");
    std::fs::write(
        &source,
        presentation_with_size("Existing", 1920.0, 1080.0).encode_to_vec(),
    )
    .expect("write native source");
    let noncanonical = nested.join("..").join("Existing.pro");
    let original = generate_title_plan("pco:item:main", test_style(None));
    let override_entry = EntryOverride {
        output_key: original.output_key.to_string(),
        playlist_name: None,
        slide_type: None,
        action: Some(OverrideAction::UseExisting {
            file_path: noncanonical,
            arrangement: None,
        }),
    };

    let effective = apply_override(&original, Some(&override_entry)).expect("valid override");

    assert!(matches!(
        effective.ready_action(),
        Some(ReadyAction::UseExisting { file_path, .. })
            if file_path == &source.canonicalize().expect("canonical source")
    ));
}

#[tokio::test]
async fn review_rejects_override_with_wrong_size_before_approval() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = TestRuntime::new(root.path());
    let source = root.path().join("Legacy.pro");
    std::fs::write(
        &source,
        presentation_with_size("Legacy", 1280.0, 720.0).encode_to_vec(),
    )
    .expect("write legacy source");
    let plan = generate_title_plan("pco:item:main", test_style(None));
    let mut request = reviewed_request("Reviewed");
    request.overrides = vec![EntryOverride {
        output_key: plan.output_key.to_string(),
        playlist_name: None,
        slide_type: None,
        action: Some(OverrideAction::UseExisting {
            file_path: source,
            arrangement: None,
        }),
    }];

    assert!(matches!(
        runtime
            .executor()
            .review_build_request(
                request,
                &[plan],
                crate::propresenter::PresentationSize::FULL_HD,
            )
            .await,
        Err(BuildServiceError::PresentationSizeInvariant { .. })
    ));
}

#[tokio::test]
async fn review_rejects_override_with_missing_arrangement_before_approval() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = TestRuntime::new(root.path());
    let source = root.path().join("Existing.pro");
    std::fs::write(
        &source,
        presentation_with_size("Existing", 1920.0, 1080.0).encode_to_vec(),
    )
    .expect("write native source");
    let plan = generate_title_plan("pco:item:main", test_style(None));
    let mut request = reviewed_request("Reviewed");
    request.overrides = vec![EntryOverride {
        output_key: plan.output_key.to_string(),
        playlist_name: None,
        slide_type: None,
        action: Some(OverrideAction::UseExisting {
            file_path: source,
            arrangement: Some("Missing".to_string()),
        }),
    }];

    assert!(matches!(
        runtime
            .executor()
            .review_build_request(
                request,
                &[plan],
                crate::propresenter::PresentationSize::FULL_HD,
            )
            .await,
        Err(BuildServiceError::ArrangementUnavailable { .. })
    ));
}

#[test]
fn background_override_rejects_read_only_action() {
    let original = use_existing_plan("pco:item:main", PathBuf::from("/library/Existing.pro"));
    let override_entry = EntryOverride {
        output_key: original.output_key.to_string(),
        playlist_name: None,
        slide_type: None,
        action: Some(OverrideAction::SetBackground {
            background: test_background("default"),
        }),
    };

    let error = apply_override(&original, Some(&override_entry))
        .expect_err("read-only actions cannot accept backgrounds");

    assert!(error.to_string().contains("read-only presentation"));
}

#[tokio::test]
async fn unresolved_plan_cannot_cross_the_prepared_build_boundary() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = TestRuntime::new(root.path());
    let plan = ResolvedItemPlan {
        reason: "ambiguous classification".to_string(),
        ..test_plan(
            "pco:item:main",
            PlanDisposition::NeedsReview(ReviewContext::new(None)),
        )
    };
    let review = runtime
        .executor()
        .review_build_request(
            reviewed_request("Blocked"),
            &[plan],
            crate::propresenter::PresentationSize::FULL_HD,
        )
        .await
        .expect("capture unresolved review");

    assert!(matches!(&review, BuildReview::NeedsReview(_)));
    let error = review
        .into_prepared()
        .expect_err("unresolved plans cannot produce executable artifacts");

    assert!(error.to_string().contains("ambiguous classification"));
}

#[tokio::test]
async fn unresolved_proposal_does_not_capture_executable_sources() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = TestRuntime::new(root.path());
    let missing = root.path().join("missing.pro");
    let plan = ResolvedItemPlan {
        reason: "operator must choose a presentation".to_string(),
        ..test_plan(
            "pco:item:main",
            PlanDisposition::NeedsReview(ReviewContext::new(Some(ReadyAction::UseExisting {
                file_path: missing.clone(),
                arrangement: None,
            }))),
        )
    };

    let review = runtime
        .executor()
        .review_build_request(
            reviewed_request("Blocked"),
            &[plan],
            crate::propresenter::PresentationSize::FULL_HD,
        )
        .await
        .expect("unresolved proposals are diagnostic, not executable");

    assert!(matches!(&review, BuildReview::NeedsReview(_)));
    assert_eq!(review.plans()[0].file_path(), Some(missing.as_path()));
}

#[test]
fn duplicate_plan_identities_fail_resolution() {
    let plans = vec![
        test_plan("pco:item-1:main", PlanDisposition::Skip),
        test_plan("pco:item-1:main", PlanDisposition::Skip),
    ];

    let error =
        resolve_requested_plans(&plans, &[], &[]).expect_err("duplicate identities are ambiguous");

    assert_eq!(
        error.to_string(),
        "duplicate plan output_keys: pco:item-1:main"
    );
}

#[test]
fn same_output_key_cannot_be_skipped_and_overridden() {
    let overrides = [EntryOverride {
        output_key: "pco:item:main".to_string(),
        playlist_name: None,
        slide_type: None,
        action: None,
    }];

    let error = validate_unique_request_keys(&["pco:item:main".to_string()], &overrides)
        .expect_err("skip and override cannot compete");

    assert_eq!(
        error.to_string(),
        "output_key 'pco:item:main' cannot be both skipped and overridden"
    );
}

#[test]
fn slide_type_override_changes_only_playlist_semantics() {
    let original = test_plan("pco:item:main", PlanDisposition::Skip);
    let override_entry = EntryOverride {
        output_key: original.output_key.to_string(),
        playlist_name: None,
        slide_type: Some(OverrideSlideType::Scripture),
        action: None,
    };

    let effective =
        apply_override(&original, Some(&override_entry)).expect("valid metadata override");

    assert_eq!(effective.item_kind, ItemKind::Scripture);
    assert_eq!(effective.item_type.as_deref(), Some("scripture"));
    assert!(effective.render_style().is_none());
}
