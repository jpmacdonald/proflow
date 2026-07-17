use std::collections::BTreeSet;

use super::*;
use crate::propresenter::package::{read_playlist_package, PlaylistPackage, PlaylistPackageMode};
use crate::propresenter::playlist::linked_presentation_filename;

#[tokio::test]
async fn portable_restyle_embeds_exact_canonical_default_files_and_final_media() {
    let fixture = portable_fixture();
    let executor = fixture.runtime.executor();
    let mut request = reviewed_request("Portable Overwrite");
    request.playlist_package_mode = PlaylistPackageMode::ExportPortable;
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
        plan.item_kind = ItemKind::Song;
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
    let items = crate::propresenter::package::presentation_items(&package.document);
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
                .embedded_file_data
                .get(&filename)
                .expect("canonical embedded presentation"),
            &std::fs::read(fixture.library.join(&filename)).expect("committed presentation"),
            "the package must carry the exact bytes committed to Default"
        );
    }
}

fn assert_exact_final_media(package: &PlaylistPackage, fixture: &PortableFixture) {
    let media = package
        .embedded_file_details
        .iter()
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
            .embedded_file_data
            .get(&final_media_path)
            .expect("embedded final background"),
        &fixture.new_background_bytes
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
    assert!(!package
        .embedded_file_data
        .contains_key(&canonical_old_background));

    let referenced_media = ["First Song.pro", "Second Song.pro"]
        .into_iter()
        .flat_map(|filename| {
            crate::propresenter::media::presentation_media_dependencies_from_bytes(
                package
                    .embedded_file_data
                    .get(filename)
                    .expect("embedded presentation"),
            )
            .expect("inspect final presentation media")
        })
        .map(|dependency| {
            dependency
                .path
                .expect("final media is a local path")
                .display()
                .to_string()
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(referenced_media, BTreeSet::from([final_media_path]));
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
