use std::collections::BTreeSet;

use super::*;
use crate::propresenter::package::{read_playlist_package, PlaylistPackage};
use crate::propresenter::playlist::linked_presentation_filename;
use crate::propresenter::SlideType;

#[tokio::test]
async fn portable_restyle_embeds_exact_canonical_default_files_and_final_media() {
    let fixture = portable_fixture();
    let executor = fixture.runtime.executor();
    let mut request = reviewed_request("Portable Overwrite");
    request.playlist_export = PlaylistExportIntent::portable_import(Vec::new());
    let prepared = expect_prepared(
        executor
            .review_build_request(
                request,
                &fixture.plans,
                crate::propresenter::PresentationSize::FULL_HD,
            )
            .await
            .expect("review portable restyle"),
    );
    let result = executor
        .build_prepared_request(prepared)
        .await
        .expect("commit portable restyle");

    let package = read_playlist_package(&result.playlist_path).expect("read portable package");
    assert_portable_presentations(&package, &fixture);
    assert_exact_final_media(&package, &fixture);
    assert_complete_portable_receipt(&result, &fixture);
}

#[tokio::test]
async fn portable_receipt_records_each_missing_native_media_reference_and_warning() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = TestRuntime::new(root.path());
    let missing_media = root.path().join("missing-background.png");
    let (presentation, _) = portable_source_song("Missing Media", &missing_media, &runtime);
    let presentation_path = root.path().join("Missing Media.pro");
    std::fs::write(&presentation_path, presentation.encode_to_vec())
        .expect("write presentation with missing media");
    let mut plan = use_existing_plan("pco:missing-media", presentation_path);
    plan.playlist_name = "Missing Media".to_string();
    let mut request = reviewed_request("Portable Missing Media");
    request.playlist_export = PlaylistExportIntent::portable_import(Vec::new());

    let prepared = expect_prepared(
        runtime
            .executor()
            .review_build_request(
                request,
                &[plan],
                crate::propresenter::PresentationSize::FULL_HD,
            )
            .await
            .expect("review missing external media"),
    );
    let result = runtime
        .executor()
        .build_prepared_request(prepared)
        .await
        .expect("export presentation while retaining missing reference");

    let missing_path = missing_media.display().to_string();
    let expected_warning = format!(
        "Media was not embedded and retains its original external reference: {missing_path}"
    );
    assert!(result.warnings.contains(&expected_warning));
    let receipt: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&result.receipt_path).expect("read missing-media receipt"),
    )
    .expect("parse missing-media receipt");
    let export = &receipt["playlist_export"];
    assert_eq!(export["warnings"], serde_json::json!([expected_warning]));
    assert_eq!(
        export["media_manifest"]["references"],
        serde_json::json!([])
    );
    assert_eq!(export["media_manifest"]["members"], serde_json::json!([]));
    let unresolved = export["media_manifest"]["unresolved"]
        .as_array()
        .expect("unresolved media manifest");
    assert_eq!(unresolved.len(), 1);
    assert_eq!(unresolved[0]["presentation"], "Missing Media");
    assert_eq!(unresolved[0]["reason"], "missing_local_file");
    assert_eq!(unresolved[0]["candidate_path"], missing_path);
    assert!(unresolved[0]["native_locator"]
        .as_str()
        .is_some_and(|locator| locator.contains("missing-background.png")));
}

struct PortableFixture {
    _root: tempfile::TempDir,
    runtime: TestRuntime,
    plans: Vec<ResolvedItemPlan>,
    library: PathBuf,
    old_background: PathBuf,
    new_background: PathBuf,
    new_background_bytes: Vec<u8>,
    arrangement_uuids: Vec<Uuid>,
}

fn portable_fixture() -> PortableFixture {
    let root = tempfile::tempdir().expect("temporary root");
    let mut runtime = TestRuntime::new(root.path());
    let macro_path = runtime.locations().macros().to_path_buf();
    let macros = rv_data::MacrosDocument {
        macros: vec![rv_data::macros_document::Macro {
            uuid: Some(rv_data::Uuid {
                string: "00000000-0000-0000-0000-000000000001".to_string(),
            }),
            name: "Song".to_string(),
            ..rv_data::macros_document::Macro::default()
        }],
        ..rv_data::MacrosDocument::default()
    };
    std::fs::create_dir_all(macro_path.parent().expect("macro parent"))
        .expect("create macro parent");
    std::fs::write(&macro_path, macros.encode_to_vec()).expect("write macros");
    runtime.reload_render_assets();
    let library = runtime
        .locations()
        .propresenter_root()
        .join("Libraries/Default");
    let playlist_output = root.path().join("playlists");
    std::fs::create_dir_all(&library).expect("create Default library");
    std::fs::create_dir_all(&playlist_output).expect("create playlist output");

    let old_background = root.path().join("old-background.png");
    let new_background = runtime
        .locations()
        .project_data_root()
        .join("backgrounds/default.png");
    std::fs::create_dir_all(new_background.parent().expect("background parent"))
        .expect("create background directory");
    let new_background_bytes = minimal_png(2, 2);
    std::fs::write(&new_background, &new_background_bytes).expect("write new background");

    let mut plans = Vec::new();
    let mut arrangement_uuids = Vec::new();
    for name in ["First Song", "Second Song"] {
        let (presentation, arrangement_uuid) =
            portable_source_song(name, &old_background, &runtime);
        let path = library.join(format!("{name}.pro"));
        std::fs::write(&path, presentation.encode_to_vec()).expect("write source song");
        let mut plan = test_plan(
            &format!("pco:{name}:main"),
            PlanDisposition::Ready(ReadyAction::RestyleExisting {
                file_path: path,
                arrangement: Some("Default".to_string()),
                transform: test_transform(),
            }),
        );
        plan.set_slide_type(SlideType::Lyrics)
            .expect("song slide type is compatible with restyle");
        plan.playlist_name = name.to_string();
        plans.push(plan);
        arrangement_uuids.push(arrangement_uuid);
    }

    runtime.file_index = Arc::new(Mutex::new(
        LibraryCatalog::build(&library).expect("index Default library"),
    ));
    runtime.replace_locations(
        BuildLocations::from_inputs(BuildLocationInputs {
            project_data_root: runtime.locations().project_data_root().to_path_buf(),
            presentation_library: library.clone(),
            playlist_output,
            propresenter_root: runtime.locations().propresenter_root().to_path_buf(),
            themes: runtime.locations().themes().to_path_buf(),
            macros: runtime.locations().macros().to_path_buf(),
        })
        .expect("checked Default-library locations"),
    );

    PortableFixture {
        _root: root,
        runtime,
        plans,
        library,
        old_background,
        new_background,
        new_background_bytes,
        arrangement_uuids,
    }
}

fn assert_portable_presentations(package: &PlaylistPackage, fixture: &PortableFixture) {
    let items = crate::propresenter::package::presentation_items(package.document());
    assert_eq!(items.len(), 2);
    for ((item, name), arrangement_uuid) in items
        .iter()
        .zip(["First Song", "Second Song"])
        .zip(&fixture.arrangement_uuids)
    {
        let filename = format!("{name}.pro");
        assert_eq!(item.name, name);
        assert_eq!(
            item.local_relative_path.as_deref(),
            Some(format!("Libraries/Default/{filename}").as_str())
        );
        assert_eq!(
            linked_presentation_filename(item).as_deref(),
            Some(filename.as_str())
        );
        assert_eq!(item.arrangement_name, "Default");
        assert_eq!(
            item.arrangement_uuid.as_deref(),
            Some(arrangement_uuid.to_string().as_str())
        );
        assert_eq!(
            package
                .embedded_file(&filename)
                .expect("canonical embedded presentation"),
            std::fs::read(fixture.library.join(&filename))
                .expect("committed presentation")
                .as_slice(),
            "the package must carry the exact bytes committed to Default"
        );
    }
}

fn assert_exact_final_media(package: &PlaylistPackage, fixture: &PortableFixture) {
    let media = package
        .embedded_file_details()
        .filter(|file| !file.is_presentation)
        .collect::<Vec<_>>();
    assert_eq!(
        media.len(),
        1,
        "the shared final background is embedded once"
    );
    let final_media_path = fixture
        .new_background
        .canonicalize()
        .expect("canonical new background")
        .display()
        .to_string();
    assert_eq!(media[0].name, final_media_path);
    assert_eq!(
        package
            .embedded_file(&final_media_path)
            .expect("embedded final background"),
        fixture.new_background_bytes.as_slice()
    );
    let canonical_old_background = fixture
        .old_background
        .parent()
        .expect("old background parent")
        .canonicalize()
        .expect("canonical old background parent")
        .join(
            fixture
                .old_background
                .file_name()
                .expect("old background filename"),
        )
        .display()
        .to_string();
    assert!(!package.has_embedded_file(&canonical_old_background));

    let referenced_media = ["First Song.pro", "Second Song.pro"]
        .into_iter()
        .flat_map(|filename| {
            crate::propresenter::media::presentation_media_dependencies_from_bytes(
                package
                    .embedded_file(filename)
                    .expect("embedded presentation"),
            )
            .expect("inspect final presentation media")
        })
        .map(|dependency| {
            dependency
                .stored_absolute_path()
                .expect("final media is a local path")
                .display()
                .to_string()
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(referenced_media, BTreeSet::from([final_media_path]));
}

fn assert_complete_portable_receipt(
    result: &crate::workflow::report::BuildServiceResult,
    fixture: &PortableFixture,
) {
    let receipt: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&result.receipt_path).expect("read portable receipt"),
    )
    .expect("parse portable receipt");
    let manifest = &receipt["playlist_export"]["media_manifest"];
    assert_eq!(
        receipt["playlist_export"]["warnings"],
        serde_json::json!([])
    );
    assert_eq!(manifest["unresolved"], serde_json::json!([]));

    let canonical_background = fixture
        .new_background
        .canonicalize()
        .expect("canonical final background")
        .display()
        .to_string();
    assert_eq!(
        manifest["members"],
        serde_json::json!([{
            "source_path": canonical_background,
            "archive_member": canonical_background,
            "origin": "presentation_reference"
        }])
    );
    let references = manifest["references"]
        .as_array()
        .expect("portable reference manifest");
    assert_eq!(references.len(), 2);
    assert_eq!(
        references
            .iter()
            .map(|reference| reference["presentation"].as_str().expect("presentation"))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["First Song", "Second Song"])
    );
    assert!(references.iter().all(|reference| {
        reference["source_path"] == canonical_background
            && reference["archive_member"] == canonical_background
            && reference["native_locator"]
                .as_str()
                .is_some_and(|locator| locator.contains("default.png"))
    }));
}

fn portable_source_song(
    name: &str,
    old_background: &Path,
    runtime: &TestRuntime,
) -> (rv_data::Presentation, Uuid) {
    let arrangement_uuid = Uuid::new_v4();
    let mut source = presentation_with_size(name, 1920.0, 1080.0);
    let group_uuid = source.cue_groups[0]
        .group
        .as_ref()
        .and_then(|group| group.uuid.clone())
        .expect("fixture group identity");
    source.arrangements = vec![rv_data::presentation::Arrangement {
        uuid: Some(rv_data::Uuid {
            string: arrangement_uuid.to_string(),
        }),
        name: "Default".to_string(),
        group_identifiers: vec![group_uuid],
    }];
    crate::propresenter::background::add_reviewed_background_to_first_cue(
        &mut source,
        old_background,
        &minimal_png(1, 1),
        runtime.locations().propresenter_root(),
    )
    .expect("install old source background");
    (source, arrangement_uuid)
}
