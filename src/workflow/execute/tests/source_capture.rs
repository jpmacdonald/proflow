use super::*;

#[test]
fn unsupported_bible_version_is_typed() {
    assert!(matches!(
        parse_bible_version("ESV"),
        Err(BuildServiceError::UnsupportedBibleVersion(version)) if version == "ESV"
    ));
}

#[tokio::test]
async fn changed_reviewed_source_is_rejected_before_commit() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = TestRuntime::new(root.path());
    let executor = runtime.executor();
    let source = root.path().join("source.pro");
    std::fs::write(
        &source,
        presentation_with_size("Reviewed", 1920.0, 1080.0).encode_to_vec(),
    )
    .expect("write reviewed source");
    let reviewed = executor
        .review_build_request(
            reviewed_request("Reviewed"),
            &[use_existing_plan("pco:item:main", source.clone())],
            crate::propresenter::PresentationSize::FULL_HD,
        )
        .await
        .expect("capture reviewed request");
    let reviewed = expect_prepared(reviewed);
    std::fs::write(&source, b"changed presentation").expect("change reviewed source");

    let error = executor
        .build_prepared_request(reviewed)
        .await
        .expect_err("changed source must invalidate review");

    assert!(matches!(
        error,
        BuildServiceError::SourceReview(SourceReviewError::Changed { .. })
    ));
    assert_eq!(
        std::fs::read_dir(runtime.locations().playlist_output())
            .expect("read output directory")
            .count(),
        0
    );
}

#[tokio::test]
async fn source_drift_while_waiting_for_catalog_lock_is_rejected() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = TestRuntime::new(root.path());
    let executor = runtime.executor();
    let source = root.path().join("source.pro");
    std::fs::write(
        &source,
        presentation_with_size("Reviewed", 1920.0, 1080.0).encode_to_vec(),
    )
    .expect("write reviewed source");
    let reviewed = executor
        .review_build_request(
            reviewed_request("Catalog Wait"),
            &[use_existing_plan("pco:item:main", source.clone())],
            crate::propresenter::PresentationSize::FULL_HD,
        )
        .await
        .expect("capture reviewed request");
    let reviewed = expect_prepared(reviewed);
    let playlist_target =
        playlist_output_path(runtime.locations().playlist_output(), "Catalog Wait");

    let catalog_guard = runtime.file_index.lock().await;
    let mut commit = Box::pin(executor.build_prepared_request(reviewed));
    std::future::poll_fn(|context| {
        let poll = std::future::Future::poll(commit.as_mut(), context);
        assert!(
            matches!(poll, std::task::Poll::Pending),
            "commit must wait while the catalog lock is held"
        );
        std::task::Poll::Ready(())
    })
    .await;

    std::fs::write(&source, b"changed while waiting").expect("change reviewed source");
    drop(catalog_guard);

    let error = commit
        .await
        .expect_err("source drift during catalog lock wait must invalidate review");
    assert!(matches!(
        error,
        BuildServiceError::SourceReview(SourceReviewError::Changed { .. })
    ));
    assert!(
        !playlist_target.exists(),
        "invalidated source review must not commit the staged playlist"
    );
}

#[tokio::test]
async fn reviewed_build_uses_bound_identity_without_refetch() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = TestRuntime::new(root.path());
    let executor = runtime.executor();
    let mut request = reviewed_request("May 24, 2026 - Sunday Morning");
    request.skip_output_keys = vec!["pco:item:main".to_string()];
    let reviewed = executor
        .review_build_request(
            request,
            &[test_plan(
                "pco:item:main",
                PlanDisposition::NeedsReview(ReviewContext::new(None)),
            )],
            crate::propresenter::PresentationSize::FULL_HD,
        )
        .await
        .expect("review bound request");
    let reviewed = expect_prepared(reviewed);

    assert!(reviewed.plans()[0].is_skipped());
    let result = executor
        .build_prepared_request(reviewed)
        .await
        .expect("bound request builds without network access");

    assert_eq!(result.skipped_count, 1);
    assert_eq!(
        result.playlist_path,
        runtime
            .locations()
            .playlist_output()
            .join("May 24, 2026 - Sunday Morning.proplaylist")
            .display()
            .to_string()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn unreferenced_portable_media_is_rejected_after_canonicalization() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = TestRuntime::new(root.path());
    let executor = runtime.executor();
    let media = root.path().join("background.png");
    let alias = root.path().join("media-alias.png");
    std::fs::write(&media, b"reviewed media").expect("write media");
    std::os::unix::fs::symlink(&media, &alias).expect("create media symlink");
    let mut request = reviewed_request("Portable");
    request.playlist_package_mode = PlaylistPackageMode::ExportPortable;
    request.media_assets = vec![PlaylistMediaAsset::new(&alias)];
    let error = executor
        .review_build_request(request, &[], crate::propresenter::PresentationSize::FULL_HD)
        .await
        .expect_err("unreferenced media has no evidenced native package role");
    assert!(matches!(
        error,
        BuildServiceError::Playlist(PlaylistError::UnreferencedPortableMediaAsset { path })
            if path == media.canonicalize().expect("canonical media path")
    ));
}

#[tokio::test]
async fn portable_review_captures_managed_background() {
    let root = tempfile::tempdir().expect("temporary root");
    let mut runtime = TestRuntime::new(root.path());
    let theme_dir = runtime.locations().themes().join("Background Theme");
    std::fs::create_dir_all(&theme_dir).expect("create theme directory");
    let theme = rv_data::template::Document {
        slides: vec![rv_data::template::Slide {
            base_slide: fixture_template_slide().base_slide,
            name: "Content".to_string(),
            actions: Vec::new(),
        }],
        ..rv_data::template::Document::default()
    };
    std::fs::write(theme_dir.join("Theme"), theme.encode_to_vec()).expect("write theme");
    runtime.select_theme("Background Theme");
    let background_dir = runtime.locations().project_data_root().join("backgrounds");
    std::fs::create_dir(&background_dir).expect("create backgrounds");
    let background = background_dir.join("default.png");
    let background_bytes = minimal_png(1920, 1080);
    std::fs::write(&background, &background_bytes).expect("write background");
    let mut request = reviewed_request("Portable Background");
    request.playlist_package_mode = PlaylistPackageMode::ExportPortable;
    let reviewed = runtime
        .executor()
        .review_build_request(
            request,
            &[generate_title_plan(
                "pco:item:main",
                test_style(Some(test_background("default"))),
            )],
            crate::propresenter::PresentationSize::FULL_HD,
        )
        .await
        .expect("capture portable background");
    let reviewed = expect_prepared(reviewed);
    let canonical = background.canonicalize().expect("canonical background");

    assert!(reviewed
        .media_assets()
        .iter()
        .any(|asset| asset.source_path == canonical));
    assert!(reviewed.has_reviewed_source(&canonical));
}

#[tokio::test]
async fn portable_review_rejects_custom_identity_for_managed_background() {
    let root = tempfile::tempdir().expect("temporary root");
    let mut runtime = TestRuntime::new(root.path());
    let theme_dir = runtime.locations().themes().join("Background Theme");
    std::fs::create_dir_all(&theme_dir).expect("create theme directory");
    let theme = rv_data::template::Document {
        slides: vec![rv_data::template::Slide {
            base_slide: fixture_template_slide().base_slide,
            name: "Content".to_string(),
            actions: Vec::new(),
        }],
        ..rv_data::template::Document::default()
    };
    std::fs::write(theme_dir.join("Theme"), theme.encode_to_vec()).expect("write theme");
    runtime.select_theme("Background Theme");
    let background_dir = runtime.locations().project_data_root().join("backgrounds");
    std::fs::create_dir(&background_dir).expect("create backgrounds");
    let background = background_dir.join("default.png");
    std::fs::write(&background, minimal_png(1920, 1080)).expect("write background");
    let mut request = reviewed_request("Portable Background");
    request.playlist_package_mode = PlaylistPackageMode::ExportPortable;
    request.media_assets = vec![PlaylistMediaAsset {
        source_path: background.clone(),
        archive_path: Some("media/default.png".to_string()),
    }];

    let error = runtime
        .executor()
        .review_build_request(
            request,
            &[generate_title_plan(
                "pco:item:main",
                test_style(Some(test_background("default"))),
            )],
            crate::propresenter::PresentationSize::FULL_HD,
        )
        .await
        .expect_err("managed media keeps its native archive identity");

    assert!(matches!(
        error,
        BuildServiceError::Playlist(PlaylistError::MediaDependencyArchiveOverride {
            path,
            ..
        }) if path == background.canonicalize().expect("canonical background")
    ));
}

#[tokio::test]
async fn portable_review_captures_media_inherited_from_theme_slide() {
    let root = tempfile::tempdir().expect("temporary root");
    let mut runtime = TestRuntime::new(root.path());
    let media_path = root.path().join("theme-fill.png");
    std::fs::write(&media_path, b"reviewed theme media").expect("write theme media");

    let mut base_slide = fixture_template_slide()
        .base_slide
        .expect("fixture base slide");
    base_slide.elements[0]
        .element
        .as_mut()
        .expect("fixture graphics element")
        .fill = Some(rv_data::graphics::Fill {
        enable: true,
        fill_type: Some(rv_data::graphics::fill::FillType::Media(rv_data::Media {
            url: Some(rv_data::Url {
                storage: Some(rv_data::url::Storage::AbsoluteString(format!(
                    "file://{}",
                    media_path.display()
                ))),
                ..rv_data::Url::default()
            }),
            ..rv_data::Media::default()
        })),
    });
    let theme_dir = runtime.locations().themes().join("Portable Theme");
    std::fs::create_dir_all(&theme_dir).expect("create theme directory");
    let theme = rv_data::template::Document {
        slides: vec![rv_data::template::Slide {
            base_slide: Some(base_slide),
            name: "Content".to_string(),
            actions: Vec::new(),
        }],
        ..rv_data::template::Document::default()
    };
    std::fs::write(theme_dir.join("Theme"), theme.encode_to_vec()).expect("write theme");
    runtime.select_theme("Portable Theme");

    let mut request = reviewed_request("Portable Theme Media");
    request.playlist_package_mode = PlaylistPackageMode::ExportPortable;
    let reviewed = runtime
        .executor()
        .review_build_request(
            request,
            &[generate_title_plan("pco:item:main", test_style(None))],
            crate::propresenter::PresentationSize::FULL_HD,
        )
        .await
        .expect("capture theme media");
    let reviewed = expect_prepared(reviewed);
    let canonical = media_path.canonicalize().expect("canonical media");

    assert!(reviewed
        .media_assets()
        .iter()
        .any(|asset| asset.source_path == canonical));
    assert!(reviewed.has_reviewed_source(&canonical));
}
