use super::*;

#[test]
fn render_asset_snapshot_rejects_a_theme_from_outside_its_config() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = TestRuntime::new(root.path());
    let installed_dir = runtime.locations().themes().join("Installed Theme");
    std::fs::create_dir_all(&installed_dir).expect("create installed theme directory");
    let installed = rv_data::template::Document {
        slides: vec![rv_data::template::Slide {
            base_slide: fixture_template_slide().base_slide,
            name: "Content".to_string(),
            actions: Vec::new(),
        }],
        ..rv_data::template::Document::default()
    };
    std::fs::write(installed_dir.join("Theme"), installed.encode_to_vec())
        .expect("write installed theme");

    let mut raw = crate::project_config::RawProjectConfig::default();
    raw.defaults.theme = Some("Configured Theme".to_string());
    let config = ProjectConfig::try_from(raw).expect("valid configured project");
    let error = RenderAssetSnapshot::load(config, runtime.locations().clone())
        .err()
        .expect("an unrelated installed theme cannot satisfy the configured snapshot");

    assert!(matches!(
        error,
        RenderAssetSnapshotError::Theme(ThemeCacheLoadError::NotFound { name, .. })
            if name == "Configured Theme"
    ));
}

#[test]
fn render_asset_snapshot_owns_all_configured_asset_validation() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = TestRuntime::new(root.path());
    let background_dir = runtime.locations().project_data_root().join("backgrounds");
    std::fs::create_dir(&background_dir).expect("background directory");
    std::fs::write(background_dir.join("empty.png"), []).expect("empty background fixture");

    let mut raw = crate::project_config::RawProjectConfig::default();
    raw.backgrounds.insert(
        BackgroundId::new("default").expect("valid background id"),
        BackgroundAssetPath::new("backgrounds/empty.png").expect("valid relative path"),
    );
    raw.cue_roles.insert(
        "scripture".to_string(),
        CueRoleConfig {
            slide: "Scripture".to_string(),
            text_slots: BTreeMap::new(),
            enter_macro: Some("Scripture/Prayer".to_string()),
            leader_enter_macro: Some("Scripture/Prayer (Highlighted)".to_string()),
            speaker_colors: Some(crate::project_config::SpeakerColorConfig {
                leader: crate::project_config::RgbColor::new(254, 219, 79),
                audience: crate::project_config::RgbColor::new(255, 255, 255),
            }),
        },
    );

    let config = ProjectConfig::try_from(raw).expect("valid runtime config");
    let error = RenderAssetSnapshot::load(config, runtime.locations().clone())
        .err()
        .expect("unresolved bindings must fail snapshot construction");
    let RenderAssetSnapshotError::Unresolved(issues) = error else {
        panic!("expected aggregated installed-asset issues");
    };
    let messages = issues
        .issues()
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    assert!(messages
        .iter()
        .any(|issue| issue.contains("theme slide 'Scripture' was not found")));
    assert!(messages
        .iter()
        .any(|issue| issue.contains("enter_macro 'Scripture/Prayer'")));
    assert!(messages
        .iter()
        .any(|issue| { issue.contains("leader_enter_macro 'Scripture/Prayer (Highlighted)'") }));
    assert!(messages
        .iter()
        .any(|issue| issue.contains("background 'default'") && issue.contains("empty")));
}

#[test]
fn render_asset_snapshot_rejects_theme_slide_canvas_mismatch() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = TestRuntime::new(root.path());
    let installed_dir = runtime.locations().themes().join("Legacy Theme");
    std::fs::create_dir_all(&installed_dir).expect("create installed theme directory");
    let mut base_slide = fixture_template_slide()
        .base_slide
        .expect("fixture base slide");
    base_slide.size = Some(rv_data::graphics::Size {
        width: 1280.0,
        height: 720.0,
    });
    let installed = rv_data::template::Document {
        slides: vec![rv_data::template::Slide {
            base_slide: Some(base_slide),
            name: "Content".to_string(),
            actions: Vec::new(),
        }],
        ..rv_data::template::Document::default()
    };
    std::fs::write(installed_dir.join("Theme"), installed.encode_to_vec())
        .expect("write installed theme");

    let mut raw = crate::project_config::RawProjectConfig::default();
    raw.defaults.theme = Some("Legacy Theme".to_string());
    raw.cue_roles.insert(
        "content".to_string(),
        CueRoleConfig {
            slide: "Content".to_string(),
            text_slots: BTreeMap::new(),
            enter_macro: None,
            leader_enter_macro: None,
            speaker_colors: None,
        },
    );
    let config = ProjectConfig::try_from(raw).expect("valid configured project");

    let error = RenderAssetSnapshot::load(config, runtime.locations().clone())
        .err()
        .expect("legacy canvas must fail snapshot construction");
    let RenderAssetSnapshotError::Unresolved(issues) = error else {
        panic!("expected an installed-asset canvas issue");
    };
    assert!(issues.issues().iter().any(|issue| matches!(
        issue,
        RenderAssetIssue::ThemeSlideSize {
            expected,
            problem: ThemeSlideSizeProblem::Mismatch(actual),
            ..
        } if *expected == crate::propresenter::PresentationSize::FULL_HD
            && actual.width() == 1280
            && actual.height() == 720
    )));
}

#[test]
fn bound_request_rejects_padded_and_control_identities() {
    let padded = BuildRequest {
        plan_id: " plan-1".to_string(),
        service_name: Some("Sunday Morning".to_string()),
        playlist_name: Some("Weekly Service".to_string()),
        ..BuildRequest::default()
    };
    assert!(matches!(
        BoundBuildRequest::try_from(padded),
        Err(BuildServiceError::InvalidIdentity { field: "plan_id" })
    ));

    let control = BuildRequest {
        plan_id: "plan-1".to_string(),
        service_name: Some("Sunday\nMorning".to_string()),
        playlist_name: Some("Weekly Service".to_string()),
        ..BuildRequest::default()
    };
    assert!(matches!(
        BoundBuildRequest::try_from(control),
        Err(BuildServiceError::InvalidIdentity {
            field: "service_name"
        })
    ));
}

#[test]
fn request_edits_reject_malformed_exact_lookup_keys() {
    let skip_error = validate_unique_request_keys(&["pco:item:main ".to_string()], &[])
        .expect_err("padded skip keys must not be silently normalized");
    assert!(matches!(
        skip_error,
        BuildServiceError::InvalidIdentity {
            field: "skip output_key"
        }
    ));

    let override_error = validate_unique_request_keys(
        &[],
        &[EntryOverride {
            output_key: "pco:item:main".to_string(),
            playlist_name: Some(" Weekly title".to_string()),
            slide_type: None,
            action: None,
        }],
    )
    .expect_err("padded title identities must not be silently normalized");
    assert!(matches!(
        override_error,
        BuildServiceError::InvalidIdentity {
            field: "override playlist_name"
        }
    ));
}

#[test]
fn override_actions_reject_blank_paths_and_padded_arrangements() {
    let blank_path = EntryOverride {
        output_key: "pco:item:main".to_string(),
        playlist_name: None,
        slide_type: None,
        action: Some(OverrideAction::UseExisting {
            file_path: PathBuf::new(),
            arrangement: None,
        }),
    };
    assert!(matches!(
        validate_unique_request_keys(&[], &[blank_path]),
        Err(BuildServiceError::InvalidIdentity {
            field: "override file_path"
        })
    ));

    let padded_arrangement = EntryOverride {
        output_key: "pco:item:main".to_string(),
        playlist_name: None,
        slide_type: None,
        action: Some(OverrideAction::SelectArrangement {
            arrangement: "Default ".to_string(),
        }),
    };
    assert!(matches!(
        validate_unique_request_keys(&[], &[padded_arrangement]),
        Err(BuildServiceError::InvalidIdentity {
            field: "override arrangement"
        })
    ));
}

#[test]
fn classified_actions_cannot_bypass_path_identity_checks() {
    let plan = test_plan(
        "pco:item:main",
        PlanDisposition::Ready(ReadyAction::UseExisting {
            file_path: PathBuf::from("/library/Existing.pro"),
            arrangement: Some(" Default".to_string()),
        }),
    );
    assert!(matches!(
        resolve_requested_plans(&[plan], &[], &[]),
        Err(BuildServiceError::InvalidIdentity {
            field: "plan arrangement"
        })
    ));
}

#[test]
fn empty_override_is_rejected_before_review() {
    let empty = EntryOverride {
        output_key: "pco:item:main".to_string(),
        playlist_name: None,
        slide_type: None,
        action: None,
    };
    assert!(matches!(
        validate_unique_request_keys(&[], &[empty]),
        Err(BuildServiceError::EmptyOverride { output_key })
            if output_key == "pco:item:main"
    ));
}

#[test]
fn slide_type_parser_preserves_exact_boundary_semantics() {
    assert!(" song ".parse::<OverrideSlideType>().is_err());
    assert_eq!(
        "song".parse::<OverrideSlideType>(),
        Ok(OverrideSlideType::Lyrics)
    );
}

#[tokio::test]
async fn review_rejects_two_generated_overrides_with_one_physical_target() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = TestRuntime::new(root.path());
    let mut first = generate_title_plan("pco:item:first", test_style(None));
    first.playlist_name = "First title".to_string();
    let mut second = generate_title_plan("pco:item:second", test_style(None));
    second.playlist_name = "Second title".to_string();
    let mut request = reviewed_request("Reviewed");
    request.overrides = [&first, &second]
        .into_iter()
        .map(|plan| EntryOverride {
            output_key: plan.output_key.to_string(),
            playlist_name: Some("Shared title".to_string()),
            slide_type: None,
            action: None,
        })
        .collect();

    let error = runtime
        .executor()
        .review_build_request(
            request,
            &[first, second],
            crate::propresenter::PresentationSize::FULL_HD,
        )
        .await
        .expect_err("two generated entries cannot share one output target");

    assert!(matches!(
        error,
        BuildServiceError::OutputReview(OutputReviewError::DuplicateTarget {
            first,
            second,
            ..
        }) if first == "plan 'pco:item:first'" && second == "plan 'pco:item:second'"
    ));
}

#[tokio::test]
async fn review_rejects_edit_and_generate_for_one_physical_target() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = TestRuntime::new(root.path());
    let executor = runtime.executor();
    let mut generated = generate_title_plan("pco:item:generate", test_style(None));
    generated.playlist_name = "Shared title".to_string();
    let target = executor
        .presentation_target(&generated)
        .expect("valid generated target");
    std::fs::write(
        &target,
        presentation_with_size("Shared title", 1920.0, 1080.0).encode_to_vec(),
    )
    .expect("write edit target");
    let editable = test_plan(
        "pco:item:edit",
        PlanDisposition::Ready(ReadyAction::GenerateDescription {
            parsed_content: parsed_content(),
            style: test_style(None),
        }),
    );
    let mut request = reviewed_request("Reviewed");
    request.overrides = vec![EntryOverride {
        output_key: editable.output_key.to_string(),
        playlist_name: None,
        slide_type: None,
        action: Some(OverrideAction::EditDescription {
            file_path: target,
            background: None,
        }),
    }];

    let error = executor
        .review_build_request(
            request,
            &[generated, editable],
            crate::propresenter::PresentationSize::FULL_HD,
        )
        .await
        .expect_err("edit and generate cannot share one output target");

    assert!(matches!(
        error,
        BuildServiceError::OutputReview(OutputReviewError::DuplicateTarget {
            first,
            second,
            ..
        }) if first == "plan 'pco:item:generate'" && second == "plan 'pco:item:edit'"
    ));
}

#[tokio::test]
async fn review_rejects_restyle_and_edit_for_one_physical_target() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = TestRuntime::new(root.path());
    let target = runtime
        .locations()
        .presentation_library()
        .join("Shared Song.pro");
    std::fs::write(
        &target,
        presentation_with_size("Shared Song", 1920.0, 1080.0).encode_to_vec(),
    )
    .expect("write shared target");
    let restyled = test_plan(
        "pco:item:restyle",
        PlanDisposition::Ready(ReadyAction::RestyleExisting {
            file_path: target.clone(),
            arrangement: None,
            transform: test_transform(),
        }),
    );
    let edited = test_plan(
        "pco:item:edit",
        PlanDisposition::Ready(ReadyAction::EditDescription {
            file_path: target,
            parsed_content: parsed_content(),
            style: test_style(None),
        }),
    );

    let error = runtime
        .executor()
        .review_build_request(
            reviewed_request("Reviewed"),
            &[restyled, edited],
            crate::propresenter::PresentationSize::FULL_HD,
        )
        .await
        .expect_err("restyle and edit cannot share one target");

    assert!(matches!(
        error,
        BuildServiceError::OutputReview(OutputReviewError::DuplicateTarget {
            first,
            second,
            ..
        }) if first == "plan 'pco:item:restyle'" && second == "plan 'pco:item:edit'"
    ));
}

#[tokio::test]
async fn review_rejects_generated_target_selected_as_another_entries_source() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = TestRuntime::new(root.path());
    let executor = runtime.executor();
    let mut generated = generate_title_plan("pco:item:generate", test_style(None));
    generated.playlist_name = "Shared title".to_string();
    let target = executor
        .presentation_target(&generated)
        .expect("valid generated target");
    std::fs::write(
        &target,
        presentation_with_size("Shared title", 1920.0, 1080.0).encode_to_vec(),
    )
    .expect("write existing source");
    let selected = generate_title_plan("pco:item:existing", test_style(None));
    let mut request = reviewed_request("Reviewed");
    request.overrides = vec![EntryOverride {
        output_key: selected.output_key.to_string(),
        playlist_name: None,
        slide_type: None,
        action: Some(OverrideAction::UseExisting {
            file_path: target,
            arrangement: None,
        }),
    }];

    let error = executor
        .review_build_request(
            request,
            &[generated, selected],
            crate::propresenter::PresentationSize::FULL_HD,
        )
        .await
        .expect_err("a generated output cannot also be another entry's source");

    assert!(matches!(
        error,
        BuildServiceError::OutputReview(OutputReviewError::SourceOutputOverlap {
            input,
            output,
            ..
        }) if input == "plan 'pco:item:existing'" && output == "plan 'pco:item:generate'"
    ));
}
