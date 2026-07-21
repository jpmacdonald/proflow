use super::*;

use chrono::{TimeZone, Utc};
use httptest::{
    matchers::{all_of, request},
    responders::json_encoded,
    Expectation, Server,
};
use serde_json::json;

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

#[tokio::test]
async fn planning_center_drift_invalidates_prepared_artifacts_before_commit() {
    let root = tempfile::tempdir().expect("temporary root");
    let server = Server::run();
    let base_url = server.url_str("/").trim_end_matches('/').to_string();
    expect_changed_plan_refresh(&server);

    let runtime = TestRuntime::new_with_pco_base_url(root.path(), base_url);
    let executor = runtime.executor();
    let reviewed = executor
        .review_planned_request(
            reviewed_request("Freshness Review"),
            &[test_plan("pco:item:main", PlanDisposition::Skip)],
            crate::propresenter::PresentationSize::FULL_HD,
            super::review::ReviewedPlanningCenterSource::captured(freshness_snapshot()),
        )
        .await
        .expect("capture production-style review");
    let reviewed = expect_prepared(reviewed);
    let playlist_target =
        playlist_output_path(runtime.locations().playlist_output(), "Freshness Review");

    let error = executor
        .build_prepared_request(reviewed)
        .await
        .expect_err("Planning Center drift must invalidate prepared artifacts");

    assert!(matches!(
        error,
        BuildServiceError::PlanningCenterFreshness(PlanFreshnessError::Changed {
            plan_id,
            ..
        }) if plan_id == "plan-1"
    ));
    assert!(
        !playlist_target.exists(),
        "invalidated Planning Center review must not commit the staged playlist"
    );
}

fn expect_changed_plan_refresh(server: &Server) {
    server.expect(
        Expectation::matching(all_of![
            request::method("GET"),
            request::path("/service_types/service-1"),
        ])
        .times(2)
        .respond_with(json_encoded(json!({
            "data": {
                "id": "service-1",
                "attributes": { "name": "Sunday Morning" }
            }
        }))),
    );
    server.expect(
        Expectation::matching(all_of![
            request::method("GET"),
            request::path("/service_types/service-1/plans/plan-1"),
        ])
        .times(2)
        .respond_with(json_encoded(json!({
            "data": {
                "id": "plan-1",
                "attributes": {
                    "sort_date": "2026-07-26T13:00:00Z",
                    "title": "July 26"
                }
            }
        }))),
    );
    server.expect(
        Expectation::matching(all_of![
            request::method("GET"),
            request::path("/plans/plan-1/items"),
        ])
        .times(2)
        .respond_with(json_encoded(json!({
            "data": [{
                "id": "item-1",
                "attributes": {
                    "title": "Welcome",
                    "description": "Changed after preview",
                    "sequence": 10
                },
                "relationships": {}
            }],
            "included": [],
            "links": { "next": null }
        }))),
    );
}

fn freshness_snapshot() -> PlanSnapshot {
    PlanSnapshot::from_resolved(
        crate::planning_center::identity::ResolvedPlanIdentity {
            plan_id: "plan-1".to_string(),
            service_id: "service-1".to_string(),
            service_name: "Sunday Morning".to_string(),
            plan_title: "July 26".to_string(),
            date: Utc
                .with_ymd_and_hms(2026, 7, 26, 13, 0, 0)
                .single()
                .expect("valid date"),
            default_playlist_name: "July 26, 2026 - Sunday Morning".to_string(),
        },
        vec![crate::planning_center::types::Item {
            id: "item-1".to_string(),
            position: 10,
            title: "Welcome".to_string(),
            description: Some("Good morning".to_string()),
            category: crate::planning_center::types::Category::Graphic,
            note: None,
            song: None,
            scripture: None,
        }],
    )
}

#[cfg(unix)]
#[tokio::test]
async fn additional_portable_media_is_canonicalized_and_embedded() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = TestRuntime::new(root.path());
    let executor = runtime.executor();
    let media = root.path().join("background.png");
    let alias = root.path().join("media-alias.png");
    std::fs::write(&media, b"reviewed media").expect("write media");
    std::os::unix::fs::symlink(&media, &alias).expect("create media symlink");
    let mut request = reviewed_request("Portable");
    request.playlist_export = PlaylistExportIntent::portable_import(vec![PlaylistMediaAsset {
        source_path: alias,
        archive_path: Some("extras/background.png".to_string()),
    }]);
    let reviewed = executor
        .review_build_request(request, &[], crate::propresenter::PresentationSize::FULL_HD)
        .await
        .expect("additional media is a supported portable package member");
    let reviewed = expect_prepared(reviewed);
    let canonical = media.canonicalize().expect("canonical media path");
    assert_eq!(reviewed.additional_media_assets()[0].source_path, canonical);
    assert_eq!(
        reviewed.additional_media_assets()[0]
            .archive_path
            .as_deref(),
        Some("extras/background.png")
    );

    let result = executor
        .build_prepared_request(reviewed)
        .await
        .expect("commit portable package");
    let package = crate::propresenter::package::read_playlist_package(&result.playlist_path)
        .expect("read portable package");
    assert_eq!(result.media_asset_count, 1);
    assert_eq!(
        package
            .embedded_file("extras/background.png")
            .expect("explicit additional media member"),
        b"reviewed media"
    );
    let receipt: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&result.receipt_path).expect("read additional-media receipt"),
    )
    .expect("parse additional-media receipt");
    assert_eq!(
        receipt["playlist_export"]["media_manifest"],
        serde_json::json!({
            "references": [],
            "members": [{
                "source_path": canonical.display().to_string(),
                "archive_member": "extras/background.png",
                "origin": "additional_request"
            }],
            "unresolved": []
        })
    );
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
    request.playlist_export = PlaylistExportIntent::portable_import(Vec::new());
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

    assert!(reviewed.additional_media_assets().is_empty());
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
    request.playlist_export = PlaylistExportIntent::portable_import(vec![PlaylistMediaAsset {
        source_path: background.clone(),
        archive_path: Some("media/default.png".to_string()),
    }]);

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
async fn portable_review_prefers_show_relative_theme_media_over_stored_absolute_path() {
    let root = tempfile::tempdir().expect("temporary root");
    let mut runtime = TestRuntime::new(root.path());
    let show_relative_path = Path::new("Media/Assets/theme-fill.png");
    let media_path = runtime
        .locations()
        .propresenter_root()
        .join(show_relative_path);
    std::fs::create_dir_all(media_path.parent().expect("theme media parent"))
        .expect("create theme media parent");
    std::fs::write(&media_path, b"reviewed theme media").expect("write theme media");
    let stale_media = root.path().join("stale-theme-fill.png");
    std::fs::write(&stale_media, b"stale theme media").expect("write stale theme media");

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
                    stale_media.display()
                ))),
                relative_file_path: Some(rv_data::url::RelativeFilePath::Local(
                    rv_data::url::LocalRelativePath {
                        root: rv_data::url::local_relative_path::Root::Show as i32,
                        path: show_relative_path.display().to_string(),
                    },
                )),
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
    request.playlist_export = PlaylistExportIntent::portable_import(Vec::new());
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

    assert!(reviewed.additional_media_assets().is_empty());
    assert!(reviewed.has_reviewed_source(&canonical));
    assert!(!reviewed.has_reviewed_source(
        &stale_media
            .canonicalize()
            .expect("canonical stale theme media")
    ));

    let result = runtime
        .executor()
        .build_prepared_request(reviewed)
        .await
        .expect("export show-relative theme media");
    assert_show_relative_export(&result, &runtime, &canonical, &stale_media);
}

fn assert_show_relative_export(
    result: &crate::workflow::report::BuildServiceResult,
    runtime: &TestRuntime,
    canonical: &Path,
    stale_media: &Path,
) {
    let package = crate::propresenter::package::read_playlist_package(&result.playlist_path)
        .expect("read portable theme package");
    let canonical_string = canonical.display().to_string();
    assert_eq!(
        package
            .embedded_file(&canonical_string)
            .expect("resolved ROOT_SHOW media member"),
        b"reviewed theme media"
    );
    assert!(!package.has_embedded_file(
        &stale_media
            .canonicalize()
            .expect("canonical stale media")
            .display()
            .to_string()
    ));
    let presentation_member = package
        .embedded_file_details()
        .find(|member| member.is_presentation)
        .expect("embedded generated presentation");
    let dependencies = crate::propresenter::media::presentation_media_dependencies_from_bytes(
        package
            .embedded_file(&presentation_member.name)
            .expect("embedded generated presentation bytes"),
    )
    .expect("inspect generated presentation media");
    assert_eq!(dependencies.len(), 1);
    assert_eq!(
        dependencies[0].source(),
        format!("file://{}", stale_media.display())
    );
    let crate::propresenter::media::MediaDependencyResolution::Available(resolved) =
        dependencies[0].resolve(Some(runtime.locations().propresenter_root()))
    else {
        panic!("ROOT_SHOW locator did not resolve to available media");
    };
    assert_eq!(
        resolved.canonicalize().expect("canonical resolved media"),
        canonical
    );
    let receipt: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&result.receipt_path).expect("read portable theme receipt"),
    )
    .expect("parse portable theme receipt");
    let manifest = &receipt["playlist_export"]["media_manifest"];
    assert_eq!(
        receipt["playlist_export"]["warnings"],
        serde_json::json!([])
    );
    assert_eq!(manifest["unresolved"], serde_json::json!([]));
    assert_eq!(
        manifest["members"],
        serde_json::json!([{
            "source_path": canonical_string,
            "archive_member": canonical_string,
            "origin": "presentation_reference"
        }])
    );
    assert_eq!(
        manifest["references"],
        serde_json::json!([{
            "presentation": "Test item",
            "native_locator": format!("file://{}", stale_media.display()),
            "source_path": canonical_string,
            "archive_member": canonical_string
        }])
    );
}
