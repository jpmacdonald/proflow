use super::*;

fn arranged_presentation_bytes(
    name: &str,
    arrangement_uuid: Uuid,
    duplicate_arrangement: bool,
) -> Vec<u8> {
    let cue_uuid = Uuid::new_v4().to_string();
    let group_uuid = Uuid::new_v4().to_string();
    let arrangement = rv_data::presentation::Arrangement {
        uuid: Some(rv_data::Uuid {
            string: arrangement_uuid.to_string(),
        }),
        name: "Default".to_string(),
        group_identifiers: vec![rv_data::Uuid {
            string: group_uuid.clone(),
        }],
    };
    let arrangements = if duplicate_arrangement {
        vec![arrangement.clone(), arrangement]
    } else {
        vec![arrangement]
    };
    rv_data::Presentation {
        name: name.to_string(),
        uuid: Some(rv_data::Uuid {
            string: Uuid::new_v4().to_string(),
        }),
        cues: vec![rv_data::Cue {
            uuid: Some(rv_data::Uuid {
                string: cue_uuid.clone(),
            }),
            ..rv_data::Cue::default()
        }],
        cue_groups: vec![rv_data::presentation::CueGroup {
            group: Some(rv_data::Group {
                uuid: Some(rv_data::Uuid { string: group_uuid }),
                ..rv_data::Group::default()
            }),
            cue_identifiers: vec![rv_data::Uuid { string: cue_uuid }],
        }],
        arrangements,
        ..rv_data::Presentation::default()
    }
    .encode_to_vec()
}

#[test]
fn builds_empty_playlist_with_native_two_level_shape() {
    let document = build_playlist("Test Playlist", &[], &test_metadata());
    let root = document.root_node.expect("root playlist");
    assert_eq!(root.name, "PLAYLIST");
    let Some(playlist::ChildrenType::Playlists(children)) = root.children_type else {
        panic!("expected playlists in root");
    };
    assert_eq!(children.playlists.len(), 1);
    assert_eq!(children.playlists[0].name, "Test Playlist");
}

#[test]
fn playlist_set_rejects_missing_structure() {
    assert_eq!(
        NamedPlaylist::new("  ", Vec::new()).expect_err("empty name"),
        PlaylistSetError::EmptyName
    );
    assert_eq!(
        PlaylistSet::new(Vec::new()).expect_err("empty set"),
        PlaylistSetError::Empty
    );
    assert_eq!(
        NamedPlaylist::new(" padded ", Vec::new()).expect_err("padded name"),
        PlaylistSetError::InvalidName
    );
}

#[test]
fn playlist_set_owns_child_and_package_order() {
    let shared_path = "/Libraries/Default/Shared Song.pro";
    let shared_bytes = presentation_bytes("Shared Song");
    let shared_entry = |name: &str| {
        PlaylistEntry::embedded(name, shared_path, shared_bytes.clone())
            .expect("valid shared entry")
    };
    let set = PlaylistSet::new(vec![
        NamedPlaylist::new("Sunday Morning", vec![shared_entry("Shared Song")])
            .expect("named playlist"),
        NamedPlaylist::new(
            "Sunday Evening",
            vec![shared_entry("Shared Song (Reprise)")],
        )
        .expect("named playlist"),
    ])
    .expect("playlist set");

    assert_eq!(set.presentation_count(), 2);
    assert_eq!(
        set.children()
            .map(|(name, entries)| (name.to_string(), entries.len()))
            .collect::<Vec<_>>(),
        vec![
            ("Sunday Morning".to_string(), 1),
            ("Sunday Evening".to_string(), 1),
        ]
    );

    let document = build_playlist_set(&set, &test_metadata());
    let root = document.root_node.as_ref().expect("root");
    let Some(playlist::ChildrenType::Playlists(children)) = &root.children_type else {
        panic!("root playlists");
    };
    assert_eq!(
        children
            .playlists
            .iter()
            .map(|child| child.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Sunday Morning", "Sunday Evening"]
    );

    let directory = tempfile::tempdir().expect("tempdir");
    let output = directory.path().join("Playlists.proplaylist");
    write_playlist_set_file(
        &set,
        &test_metadata(),
        &output,
        PlaylistExportIntent::portable_import(Vec::new()),
    )
    .expect("write playlist set");
    let package =
        crate::propresenter::package::read_playlist_package(&output).expect("read playlist set");
    assert_eq!(presentation_items(package.document()).len(), 2);
    assert_eq!(
        package.embedded_files().collect::<Vec<_>>(),
        ["Shared Song.pro"]
    );
}

#[test]
fn builder_uses_native_fixture_metadata_and_current_node_defaults() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "tests/fixtures/propresenter/native/corpus/playlists/native-template-library.proplaylist",
    );
    let native =
        crate::propresenter::package::read_playlist_package(fixture).expect("read native fixture");
    let metadata = PlaylistMetadata::from_document(native.document()).expect("native metadata");

    let built = build_playlist("Native Defaults", &[], &metadata);
    assert_eq!(
        built.application_info.as_ref(),
        native.document().application_info.as_ref()
    );
    let root = built.root_node.expect("root playlist");
    assert_eq!(root.r#type, playlist::Type::Unknown as i32);
    assert!(!root.expanded);
    let Some(playlist::ChildrenType::Playlists(children)) = root.children_type else {
        panic!("playlist children");
    };
    assert_eq!(children.playlists.len(), 1);
    assert_eq!(children.playlists[0].r#type, playlist::Type::Unknown as i32);
    assert!(!children.playlists[0].expanded);
}

#[test]
fn live_metadata_snapshot_survives_source_removal() {
    let directory = tempfile::tempdir().expect("tempdir");
    let root = directory.path();
    let library = root.join("Libraries/Default");
    std::fs::create_dir_all(&library).expect("create library");
    std::fs::create_dir_all(root.join("Playlists")).expect("create playlists");
    let document = build_playlist("Snapshot", &[], &test_metadata());
    let source = root.join("Playlists/Library");
    std::fs::write(&source, document.encode_to_vec()).expect("write live library");

    let metadata =
        PlaylistMetadata::read_from_propresenter_root(root).expect("capture metadata exactly once");
    std::fs::remove_file(source).expect("remove live library");
    assert_eq!(
        metadata.application_info(),
        test_metadata().application_info()
    );
}

#[test]
fn playlist_metadata_requires_propresenter_on_macos() {
    let document_with = |platform, application| rv_data::PlaylistDocument {
        application_info: Some(rv_data::ApplicationInfo {
            platform,
            application,
            ..rv_data::ApplicationInfo::default()
        }),
        ..rv_data::PlaylistDocument::default()
    };

    assert!(matches!(
        PlaylistMetadata::from_document(&rv_data::PlaylistDocument {
            application_info: Some(rv_data::ApplicationInfo::default()),
            ..rv_data::PlaylistDocument::default()
        }),
        Err(PlaylistMetadataError::UnsupportedPlatform { platform: 0 })
    ));
    assert!(matches!(
        PlaylistMetadata::from_document(&document_with(
            rv_data::application_info::Platform::Windows as i32,
            rv_data::application_info::Application::Propresenter as i32,
        )),
        Err(PlaylistMetadataError::UnsupportedPlatform { platform })
            if platform == rv_data::application_info::Platform::Windows as i32
    ));
    assert!(matches!(
        PlaylistMetadata::from_document(&document_with(
            rv_data::application_info::Platform::Macos as i32,
            rv_data::application_info::Application::Pvp as i32,
        )),
        Err(PlaylistMetadataError::UnsupportedApplication { application })
            if application == rv_data::application_info::Application::Pvp as i32
    ));
    assert!(PlaylistMetadata::from_document(&document_with(
        rv_data::application_info::Platform::Macos as i32,
        rv_data::application_info::Application::Propresenter as i32,
    ))
    .is_ok());
}

#[test]
fn builds_playlist_items_in_entry_order() {
    let entries = vec![
        linked_entry("Amazing Grace", "/path/to/amazing_grace.pro"),
        linked_entry("How Great Thou Art", "/path/to/how_great.pro")
            .with_selected_arrangement(Some(
                SelectedArrangement::new(Uuid::new_v4(), "Default").expect("valid arrangement"),
            ))
            .expect("linked arrangements cannot be verified without source bytes"),
    ];

    let document = build_playlist("Sunday Service", &entries, &test_metadata());
    let root = document.root_node.expect("root");
    let Some(playlist::ChildrenType::Playlists(children)) = root.children_type else {
        panic!("expected playlists");
    };
    let Some(playlist::ChildrenType::Items(items)) = &children.playlists[0].children_type else {
        panic!("expected items");
    };
    assert_eq!(items.items.len(), 2);
    assert_eq!(items.items[0].name, "Amazing Grace");
    assert_eq!(items.items[1].name, "How Great Thou Art");
}

#[test]
fn embedded_entry_keeps_known_source_document_path() {
    let entries = vec![embedded_entry(
        "Call to Worship",
        "/Users/jimmy/Documents/ProPresenter/Libraries/Default/Call to Worship.pro",
    )];
    let document = build_playlist("Service", &entries, &test_metadata());
    let items = presentation_items(&document);
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].absolute_string.as_deref(),
        Some(
            "file:///Users/jimmy/Documents/ProPresenter/Libraries/Default/Call%20to%20Worship.pro"
        )
    );
    assert_eq!(
        items[0].local_relative_path.as_deref(),
        Some("Libraries/Default/Call to Worship.pro")
    );
}

#[test]
fn embedded_entry_converts_to_linked_without_changing_playlist_item_contract() {
    let arrangement_uuid = Uuid::new_v4();
    let selected_arrangement =
        SelectedArrangement::new(arrangement_uuid, "Default").expect("valid arrangement");
    let music_key = rv_data::MusicKeyScale {
        music_key: rv_data::music_key_scale::MusicKey::D as i32,
        music_scale: rv_data::music_key_scale::MusicScale::Major as i32,
    };
    let embedded = PlaylistEntry::embedded(
        "Song Display Name",
        "/Users/jimmy/Documents/ProPresenter/Libraries/Default/Source Song.pro",
        arranged_presentation_bytes("Native Source Name", arrangement_uuid, false),
    )
    .expect("valid arranged presentation")
    .with_selected_arrangement(Some(selected_arrangement.clone()))
    .expect("arrangement resolves")
    .with_user_music_key(Some(music_key.clone()));
    let linked = embedded.clone().into_linked();

    assert!(embedded.embedded_data().is_some());
    assert!(linked.embedded_data().is_none());
    assert_eq!(linked.name(), embedded.name());
    assert_eq!(linked.presentation_path(), embedded.presentation_path());
    assert_eq!(linked.selected_arrangement(), Some(&selected_arrangement));
    assert_eq!(linked.user_music_key(), Some(&music_key));

    let embedded_document = build_playlist("Service", &[embedded], &test_metadata());
    let linked_document = build_playlist("Service", &[linked], &test_metadata());
    assert_eq!(
        embedded_document.application_info,
        linked_document.application_info
    );
    let mut embedded_item = presentation_items(&embedded_document)
        .into_iter()
        .next()
        .expect("embedded playlist item");
    let mut linked_item = presentation_items(&linked_document)
        .into_iter()
        .next()
        .expect("linked playlist item");
    // Item UUIDs are intentionally generated for each document build. Every
    // stable native presentation-item field must otherwise remain identical.
    embedded_item.item_uuid = None;
    linked_item.item_uuid = None;
    assert_eq!(linked_item, embedded_item);
}

#[test]
fn selected_arrangement_round_trips_uuid_and_exact_name() {
    let directory = tempfile::tempdir().expect("tempdir");
    let output = directory.path().join("arrangement.proplaylist");
    let arrangement_uuid = Uuid::new_v4();
    let arrangement_uuid_text = arrangement_uuid.to_string();
    let entries = vec![PlaylistEntry::embedded(
        "Song",
        "/Libraries/Default/Song.pro",
        arranged_presentation_bytes("Song", arrangement_uuid, false),
    )
    .expect("valid arranged presentation")
    .with_selected_arrangement(Some(
        SelectedArrangement::new(arrangement_uuid, "Default").expect("valid arrangement"),
    ))
    .expect("arrangement resolves")];
    let document = build_playlist("Service", &entries, &test_metadata());
    write_playlist_document_for_fidelity(&document, &entries, &output).expect("write playlist");
    let package =
        crate::propresenter::package::read_playlist_package(&output).expect("read playlist");
    let items = presentation_items(package.document());
    assert_eq!(
        items[0].arrangement_uuid.as_deref(),
        Some(arrangement_uuid_text.as_str())
    );
    assert_eq!(items[0].arrangement_name, "Default");
}

#[test]
fn embedded_selected_arrangement_must_resolve_uniquely() {
    let arrangement_uuid = Uuid::new_v4();
    let entry = PlaylistEntry::embedded(
        "Song",
        "/Libraries/Default/Song.pro",
        arranged_presentation_bytes("Song", arrangement_uuid, false),
    )
    .expect("valid arranged presentation");
    assert!(matches!(
        entry.clone().with_selected_arrangement(Some(
            SelectedArrangement::new(arrangement_uuid, "Alternate").expect("valid selection")
        )),
        Err(PlaylistEntryError::EmbeddedArrangementUnavailable {
            arrangement_name,
            ..
        }) if arrangement_name == "Alternate"
    ));
    let missing_uuid = Uuid::new_v4();
    assert!(matches!(
        entry.with_selected_arrangement(Some(
            SelectedArrangement::new(missing_uuid, "Default").expect("valid selection")
        )),
        Err(PlaylistEntryError::EmbeddedArrangementUnavailable {
            arrangement_uuid: rejected,
            ..
        }) if rejected == missing_uuid
    ));

    let ambiguous = PlaylistEntry::embedded(
        "Song",
        "/Libraries/Default/Song.pro",
        arranged_presentation_bytes("Song", arrangement_uuid, true),
    )
    .expect("identity-valid presentation");
    assert!(matches!(
        ambiguous.with_selected_arrangement(Some(
            SelectedArrangement::new(arrangement_uuid, "Default").expect("valid selection")
        )),
        Err(PlaylistEntryError::EmbeddedArrangementUnavailable { .. })
    ));
}

#[test]
fn selected_arrangement_rejects_an_empty_name() {
    assert_eq!(
        SelectedArrangement::new(Uuid::new_v4(), "  "),
        Err(SelectedArrangementError::EmptyName)
    );
    assert_eq!(
        SelectedArrangement::new(Uuid::new_v4(), " Default "),
        Err(SelectedArrangementError::InvalidName)
    );
}
