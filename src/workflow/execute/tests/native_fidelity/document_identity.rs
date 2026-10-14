use super::*;

#[test]
fn existing_preparation_uses_approved_bytes_and_checks_size() {
    let root = tempfile::tempdir().expect("temporary root");
    let source = root.path().join("existing.pro");
    let approved = presentation_with_size("Approved", 1920.0, 1080.0).encode_to_vec();
    std::fs::write(&source, b"changed on disk").expect("write changed source");

    let prepared = ServiceBuildExecutor::prepare_existing_presentation(
        "pco:item:main",
        &source,
        None,
        &approved,
        propresenter::PresentationSize::FULL_HD,
    )
    .expect("approved bytes prepare");
    assert_eq!(prepared.embedded_data, approved);

    let legacy = presentation_with_size("Legacy", 1280.0, 720.0).encode_to_vec();
    assert!(matches!(
        ServiceBuildExecutor::prepare_existing_presentation(
            "pco:item:main",
            &source,
            None,
            &legacy,
            propresenter::PresentationSize::FULL_HD,
        ),
        Err(BuildServiceError::PresentationSizeInvariant { output_key, .. })
            if output_key == "pco:item:main"
    ));
}

#[test]
fn opaque_existing_bytes_are_reused_but_mutation_requires_exact_wire_parity() {
    let root = tempfile::tempdir().expect("temporary root");
    let source = root.path().join("future-native.pro");
    let mut presentation = presentation_with_size("Future Native", 1920.0, 1080.0);
    presentation.arrangements = vec![rv_data::presentation::Arrangement {
        uuid: Some(rv_data::Uuid {
            string: Uuid::new_v4().to_string(),
        }),
        name: "Default".to_string(),
        group_identifiers: vec![presentation.cue_groups[0]
            .group
            .as_ref()
            .and_then(|group| group.uuid.clone())
            .expect("fixture group identity")],
    }];
    let mut bytes = presentation.encode_to_vec();
    bytes.extend_from_slice(&[0xf8, 0x7f, 0x01]);

    let unchanged = ServiceBuildExecutor::prepare_existing_presentation(
        "pco:item:main",
        &source,
        None,
        &bytes,
        propresenter::PresentationSize::FULL_HD,
    )
    .expect("unchanged opaque presentation remains reusable");
    assert_eq!(unchanged.embedded_data, bytes);

    assert!(matches!(
        ServiceBuildExecutor::prepare_existing_presentation(
            "pco:item:main",
            &source,
            Some("Default"),
            &bytes,
            propresenter::PresentationSize::FULL_HD,
        ),
        Err(BuildServiceError::NativeEdit(
            propresenter::native_document::NativeEditError::LossyDecode { .. }
        ))
    ));
}

#[test]
fn existing_without_arrangements_clears_stale_native_selection() {
    let root = tempfile::tempdir().expect("temporary root");
    let source = root.path().join("existing.pro");
    let mut presentation = presentation_with_size("Existing", 1920.0, 1080.0);
    presentation.selected_arrangement = Some(rv_data::Uuid {
        string: Uuid::new_v4().to_string(),
    });
    let source_bytes = presentation.encode_to_vec();

    let prepared = ServiceBuildExecutor::prepare_existing_presentation(
        "pco:item:main",
        &source,
        None,
        &source_bytes,
        propresenter::PresentationSize::FULL_HD,
    )
    .expect("stale selection without arrangements is mechanically repairable");
    let embedded = rv_data::Presentation::decode(prepared.embedded_data.as_slice())
        .expect("decode prepared presentation");

    assert!(prepared.selected_arrangement.is_none());
    assert!(embedded.selected_arrangement.is_none());
    assert!(presentation.selected_arrangement.is_some());
}

#[test]
fn existing_arrangement_resolves_native_identity() {
    let root = tempfile::tempdir().expect("temporary root");
    let source = root.path().join("existing.pro");
    let arrangement_uuid = Uuid::new_v4();
    let mut presentation = presentation_with_size("Existing", 1920.0, 1080.0);
    presentation.arrangements = vec![rv_data::presentation::Arrangement {
        uuid: Some(rv_data::Uuid {
            string: arrangement_uuid.to_string(),
        }),
        name: "Default".to_string(),
        group_identifiers: vec![presentation.cue_groups[0]
            .group
            .as_ref()
            .and_then(|group| group.uuid.clone())
            .expect("fixture group identity")],
    }];
    presentation.selected_arrangement = Some(rv_data::Uuid {
        string: Uuid::new_v4().to_string(),
    });
    let bytes = presentation.encode_to_vec();

    let prepared = ServiceBuildExecutor::prepare_existing_presentation(
        "pco:item:main",
        &source,
        Some("default"),
        &bytes,
        propresenter::PresentationSize::FULL_HD,
    )
    .expect("arrangement resolves case-insensitively");
    let embedded = rv_data::Presentation::decode(prepared.embedded_data.as_slice())
        .expect("decode prepared presentation");
    let selected = prepared.selected_arrangement.expect("selected arrangement");

    assert_eq!(selected.uuid(), &arrangement_uuid);
    assert_eq!(selected.name(), "Default");
    assert_eq!(
        embedded
            .selected_arrangement
            .expect("prepared native selection")
            .string,
        arrangement_uuid.to_string()
    );
}

#[test]
fn existing_arrangement_rejects_dangling_native_traversal() {
    let root = tempfile::tempdir().expect("temporary root");
    let source = root.path().join("existing.pro");
    let mut presentation = presentation_with_size("Existing", 1920.0, 1080.0);
    presentation.arrangements = vec![rv_data::presentation::Arrangement {
        uuid: Some(rv_data::Uuid {
            string: Uuid::new_v4().to_string(),
        }),
        name: "Default".to_string(),
        group_identifiers: vec![rv_data::Uuid {
            string: "missing-group".to_string(),
        }],
    }];

    let error = ServiceBuildExecutor::prepare_existing_presentation(
        "pco:item:main",
        &source,
        Some("Default"),
        &presentation.encode_to_vec(),
        propresenter::PresentationSize::FULL_HD,
    )
    .expect_err("dangling arrangement cannot be selected");

    assert!(error.to_string().contains("dangling group/cue references"));
}

#[test]
fn existing_arrangement_rejects_duplicate_case_insensitive_names() {
    let root = tempfile::tempdir().expect("temporary root");
    let source = root.path().join("existing.pro");
    let mut presentation = presentation_with_size("Existing", 1920.0, 1080.0);
    presentation.arrangements = ["Default", "default"]
        .into_iter()
        .map(|name| rv_data::presentation::Arrangement {
            uuid: Some(rv_data::Uuid {
                string: Uuid::new_v4().to_string(),
            }),
            name: name.to_string(),
            group_identifiers: Vec::new(),
        })
        .collect();

    assert!(matches!(
        ServiceBuildExecutor::prepare_existing_presentation(
            "pco:item:main",
            &source,
            Some("DEFAULT"),
            &presentation.encode_to_vec(),
            propresenter::PresentationSize::FULL_HD,
        ),
        Err(BuildServiceError::ArrangementAmbiguous { matches: 2, .. })
    ));
}

#[test]
fn arrangement_group_macro_policy_resolves_to_native_entry_cues() {
    let root = tempfile::tempdir().expect("temporary root");
    let mut runtime = TestRuntime::new(root.path());
    install_named_macros(&mut runtime, &["NameTag", "Song"]);
    let (mut presentation, _) = arranged_presentation("Hymn", 2);
    presentation.cue_groups[0]
        .group
        .as_mut()
        .expect("title group")
        .name = "Title".to_string();
    presentation.cue_groups[1]
        .group
        .as_mut()
        .expect("verse group")
        .name = "Verse 1".to_string();
    let policy = arrangement_group_macro_policy(&[
        (0, &["Title"], "NameTag"),
        (1, &["Verse", "Verse 1"], "Song"),
    ]);

    super::super::super::presentation_output::apply_restyle_macro_policy(
        &mut presentation,
        &policy,
        runtime.render_assets.macros(),
    )
    .expect("resolve checked group selectors");

    assert_eq!(macro_names(&presentation.cues[0]), vec!["NameTag"]);
    assert_eq!(macro_names(&presentation.cues[1]), vec!["Song"]);
}

#[test]
fn arrangement_group_macro_policy_rejects_unexpected_name_atomically() {
    let root = tempfile::tempdir().expect("temporary root");
    let mut runtime = TestRuntime::new(root.path());
    install_named_macros(&mut runtime, &["Song"]);
    let (presentation, _) = arranged_presentation("Song", 1);
    let original = presentation.clone();
    let policy = arrangement_group_macro_policy(&[(0, &["Background"], "Song")]);
    let mut presentation = presentation;

    let error = super::super::super::presentation_output::apply_restyle_macro_policy(
        &mut presentation,
        &policy,
        runtime.render_assets.macros(),
    )
    .expect_err("exact group evidence must match");

    assert!(matches!(
        error,
        propresenter::macros::MacroApplyError::UnexpectedGroup { .. }
    ));
    assert_eq!(presentation, original);
}

#[test]
fn existing_preparation_rejects_decodable_document_without_native_identity() {
    let root = tempfile::tempdir().expect("temporary root");
    let source = root.path().join("identity-less.pro");
    let identity_less = presentation_with_size("", 1920.0, 1080.0).encode_to_vec();

    assert!(matches!(
        ServiceBuildExecutor::prepare_existing_presentation(
            "pco:item:main",
            &source,
            None,
            &identity_less,
            propresenter::PresentationSize::FULL_HD,
        ),
        Err(BuildServiceError::Deserialize(
            propresenter::deserialize::ProPresenterError::UnsupportedFormat { .. }
        ))
    ));
}

#[test]
fn generated_target_preserves_owned_metadata_and_stamps_producer() {
    let root = tempfile::tempdir().expect("temporary root");
    let target = root.path().join("existing.pro");
    let existing = rv_data::Presentation {
        uuid: Some(rv_data::Uuid {
            string: "existing-id".to_string(),
        }),
        name: "Existing".to_string(),
        category: "Liturgy".to_string(),
        notes: "Operator note".to_string(),
        chord_chart: Some(rv_data::Url {
            platform: 9,
            ..rv_data::Url::default()
        }),
        ccli: Some(rv_data::presentation::Ccli {
            author: "Stale author".to_string(),
            ..rv_data::presentation::Ccli::default()
        }),
        bible_reference: Some(rv_data::presentation::BibleReference {
            book_name: "Stale book".to_string(),
            ..rv_data::presentation::BibleReference::default()
        }),
        multi_tracks_licensing: Some(rv_data::presentation::MultiTracksLicensing {
            song_identifier: 42,
            ..rv_data::presentation::MultiTracksLicensing::default()
        }),
        music_key: "Stale key".to_string(),
        music: Some(rv_data::presentation::Music {
            original_music_key: "Stale original key".to_string(),
            ..rv_data::presentation::Music::default()
        }),
        ..rv_data::Presentation::default()
    };
    let existing_bytes = existing.encode_to_vec();
    let mut regenerated = rv_data::Presentation::default();
    let producer = rv_data::ApplicationInfo {
        application: rv_data::application_info::Application::Propresenter as i32,
        ..rv_data::ApplicationInfo::default()
    };

    ServiceBuildExecutor::finalize_generated_document(
        &mut regenerated,
        &target,
        Some(&existing_bytes),
        &producer,
    )
    .expect("preserve existing envelope");

    assert_eq!(regenerated.uuid, existing.uuid);
    assert_eq!(regenerated.category, "Liturgy");
    assert_eq!(regenerated.notes, "Operator note");
    assert_eq!(regenerated.application_info.as_ref(), Some(&producer));
    assert!(regenerated.chord_chart.is_none());
    assert!(regenerated.ccli.is_none());
    assert!(regenerated.bible_reference.is_none());
    assert!(regenerated.multi_tracks_licensing.is_none());
    assert!(regenerated.music_key.is_empty());
    assert!(regenerated.music.is_none());
}
