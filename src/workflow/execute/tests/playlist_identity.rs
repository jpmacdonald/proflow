use prost::Message;

use super::*;

#[tokio::test]
async fn generated_item_keeps_reviewed_display_name_separate_from_native_filename() {
    let root = tempfile::tempdir().expect("temporary root");
    let mut runtime = TestRuntime::new(root.path());
    install_fixture_theme(&mut runtime);

    let mut plan = generate_title_plan("pco:item:main", test_style(None));
    plan.playlist_name = "Call to Worship (Leader)".to_string();
    let executor = runtime.executor();
    let expected_target = executor
        .presentation_target(&plan)
        .expect("reviewed name has a safe canonical target");
    assert_eq!(
        expected_target.file_name().and_then(|name| name.to_str()),
        Some("Call to Worship.pro")
    );

    let reviewed = executor
        .review_build_request(
            reviewed_request("Reviewed Playlist"),
            &[plan],
            crate::propresenter::PresentationSize::FULL_HD,
        )
        .await
        .expect("review generated item");
    let reviewed = expect_prepared(reviewed);
    let playlist_target = playlist_output_path(
        runtime.locations().playlist_output(),
        reviewed.playlist_name(),
    );
    let prepared_presentation = reviewed
        .prepared_artifact_bytes(&expected_target)
        .expect("read prepared presentation")
        .expect("presentation was materialized during review");
    let prepared_playlist = reviewed
        .prepared_artifact_bytes(&playlist_target)
        .expect("read prepared playlist")
        .expect("playlist was materialized during review");
    let result = executor
        .build_prepared_request(reviewed)
        .await
        .expect("build generated item");

    assert_eq!(
        std::fs::read(&expected_target).expect("committed presentation"),
        prepared_presentation,
        "build must commit the exact presentation bytes prepared before approval"
    );
    assert_eq!(
        std::fs::read(&playlist_target).expect("committed playlist"),
        prepared_playlist,
        "build must commit the exact playlist bytes prepared before approval"
    );
    let catalog = runtime.file_index.lock().await;
    let indexed = catalog
        .entry_at(&expected_target)
        .expect("successful commit installs prepared catalog metadata");
    assert_eq!(indexed.file_name(), "Call to Worship");
    assert!(indexed
        .presentation_size()
        .matches(crate::propresenter::PresentationSize::FULL_HD));
    drop(catalog);

    let package = crate::propresenter::package::read_playlist_package(&result.playlist_path)
        .expect("read generated playlist");
    let items = crate::propresenter::package::presentation_items(&package.document);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, "Call to Worship (Leader)");
    assert_eq!(
        crate::propresenter::playlist::linked_presentation_filename(&items[0]).as_deref(),
        Some("Call to Worship.pro")
    );

    assert!(
        package.embedded_file_data.is_empty(),
        "library-local packages link to the presentation already installed by the transaction"
    );
    let presentation = rv_data::Presentation::decode(
        std::fs::read(&expected_target)
            .expect("read linked presentation")
            .as_slice(),
    )
    .expect("decode linked presentation");
    assert_eq!(presentation.name, "Call to Worship");
}
