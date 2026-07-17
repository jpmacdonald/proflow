use super::super::presentation_output::ReviewedRenderTarget;
use super::*;
use std::num::NonZeroUsize;

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
        crate::propresenter::PresentationSize::FULL_HD,
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
            crate::propresenter::PresentationSize::FULL_HD,
        ),
        Err(BuildServiceError::PresentationSizeInvariant { output_key, .. })
            if output_key == "pco:item:main"
    ));
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
    let bytes = presentation.encode_to_vec();

    let prepared = ServiceBuildExecutor::prepare_existing_presentation(
        "pco:item:main",
        &source,
        Some("default"),
        &bytes,
        crate::propresenter::PresentationSize::FULL_HD,
    )
    .expect("arrangement resolves case-insensitively");
    let selected = prepared.selected_arrangement.expect("selected arrangement");

    assert_eq!(selected.uuid(), &arrangement_uuid);
    assert_eq!(selected.name(), "Default");
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
        crate::propresenter::PresentationSize::FULL_HD,
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
            crate::propresenter::PresentationSize::FULL_HD,
        ),
        Err(BuildServiceError::ArrangementAmbiguous { matches: 2, .. })
    ));
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
            crate::propresenter::PresentationSize::FULL_HD,
        ),
        Err(BuildServiceError::Deserialize(
            crate::propresenter::deserialize::ProPresenterError::UnsupportedFormat { .. }
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

#[test]
fn styled_cue_orders_slide_macro_then_background() {
    let root = tempfile::tempdir().expect("temporary root");
    let mut runtime = TestRuntime::new(root.path());
    let macro_path = runtime.locations().macros().to_path_buf();
    let macros = rv_data::MacrosDocument {
        macros: vec![rv_data::macros_document::Macro {
            uuid: Some(rv_data::Uuid {
                string: "00000000-0000-0000-0000-000000000001".to_string(),
            }),
            name: "Styled Content".to_string(),
            ..rv_data::macros_document::Macro::default()
        }],
        ..rv_data::MacrosDocument::default()
    };
    std::fs::create_dir_all(macro_path.parent().expect("macro parent"))
        .expect("create macro parent");
    std::fs::write(&macro_path, macros.encode_to_vec()).expect("write macros");
    runtime.reload_render_assets();
    let executor = runtime.executor();
    let template = fixture_template_slide();
    let role_id =
        crate::propresenter::presentation_spec::CueRoleId::new("content").expect("valid role");
    let cue = crate::propresenter::presentation_spec::CueSpec::text(
        role_id.clone(),
        crate::propresenter::presentation_spec::TextBindings::single(
            crate::propresenter::presentation_spec::TextField::body(),
            vec![crate::propresenter::rtf::StyledSegment::unstyled(
                "Generated content",
            )],
        ),
    );
    let spec = crate::propresenter::presentation_spec::PresentationSpec::new(
        "Styled",
        crate::propresenter::presentation_spec::GroupSpec::anonymous(cue, Vec::new()),
        Vec::new(),
    )
    .expect("valid presentation specification");
    let role =
        crate::propresenter::render::ResolvedCueRole::body(role_id, &template).expect("body role");
    let assets =
        crate::propresenter::render::RenderAssets::new(role, Vec::new()).expect("render assets");
    let mut rendered =
        crate::propresenter::render::render_presentation(&spec, &assets).expect("render fixture");
    let macro_binding = CueMacro::new("Styled Content".to_string(), None).expect("valid cue macro");
    let style = RenderStyle::new(
        Some(test_background("styled")),
        test_role(Some(macro_binding)),
        None,
        None,
    )
    .expect("valid style");
    let background_path = root.path().join("styled.png");
    let background_bytes = minimal_png(1, 1);

    executor
        .apply_style(
            &mut rendered,
            &style,
            Some(ReviewedBackgroundAsset {
                path: &background_path,
                data: &background_bytes,
            }),
        )
        .expect("apply style");

    let cue = rendered.presentation.cues.first().expect("content cue");
    assert_eq!(
        cue.actions
            .iter()
            .map(|action| action.r#type)
            .collect::<Vec<_>>(),
        vec![
            rv_data::action::ActionType::PresentationSlide as i32,
            rv_data::action::ActionType::Macro as i32,
            rv_data::action::ActionType::Media as i32,
        ]
    );
}

#[test]
fn macro_only_graphics_transform_preserves_native_background_action_bytes() {
    let root = tempfile::tempdir().expect("temporary root");
    let mut runtime = TestRuntime::new(root.path());
    install_named_macros(&mut runtime, &["Graphics"]);
    let mut source = presentation_with_size("Native Graphic", 1920.0, 1080.0);
    crate::propresenter::background::add_reviewed_background_to_first_cue(
        &mut source,
        &root.path().join("native-graphic.png"),
        &minimal_png(3, 2),
        runtime.locations().propresenter_root(),
    )
    .expect("install native graphic");
    let native_backgrounds = native_background_action_bytes(&source);
    assert_eq!(
        native_backgrounds.len(),
        1,
        "fixture has one native background"
    );
    let transform = crate::workflow::plan::ExistingTransform::new(
        crate::workflow::plan::BackgroundTransform::Preserve,
        crate::workflow::plan::MacroTransform::Enforce(operator_macro_policy(&[(0, "Graphics")])),
        crate::workflow::plan::CueTransform::Preserve,
    )
    .expect("macro-only transform");

    let managed = run_native_transform(&runtime, root.path(), &source, None, &transform, None);

    assert_eq!(native_background_action_bytes(&managed), native_backgrounds);
    assert_eq!(macro_names(&managed.cues[0]), vec!["Graphics"]);
    assert_at_most_one_macro_per_cue(&managed);
}

#[test]
fn background_only_transform_preserves_baptism_macro_action_bytes() {
    let root = tempfile::tempdir().expect("temporary root");
    let mut runtime = TestRuntime::new(root.path());
    install_named_macros(
        &mut runtime,
        &[
            "NameTag",
            "Scripture/Prayer",
            "Scripture/Prayer (Highlighted)",
        ],
    );
    let mut source = presentation_with_cue_count("Baptism Him", 5);
    crate::propresenter::macros::apply_operator_macro_policy(
        &mut source,
        &operator_macro_policy(&[
            (0, "NameTag"),
            (1, "Scripture/Prayer"),
            (3, "Scripture/Prayer (Highlighted)"),
        ]),
        runtime.render_assets.macros(),
    )
    .expect("seed native baptism macro transitions");
    let native_macros = native_macro_action_bytes(&source);
    let replacement_path = root.path().join("replacement.png");
    let replacement_bytes = minimal_png(4, 3);
    std::fs::write(&replacement_path, &replacement_bytes).expect("write replacement background");
    let transform = crate::workflow::plan::ExistingTransform::new(
        crate::workflow::plan::BackgroundTransform::Replace(test_background("replacement")),
        crate::workflow::plan::MacroTransform::Preserve,
        crate::workflow::plan::CueTransform::Preserve,
    )
    .expect("background-only transform");

    let managed = run_native_transform(
        &runtime,
        root.path(),
        &source,
        None,
        &transform,
        Some(ReviewedBackgroundAsset {
            path: &replacement_path,
            data: &replacement_bytes,
        }),
    );

    assert_eq!(native_macro_action_bytes(&managed), native_macros);
    assert_eq!(macro_names(&managed.cues[0]), vec!["NameTag"]);
    assert_eq!(macro_names(&managed.cues[1]), vec!["Scripture/Prayer"]);
    assert!(macro_names(&managed.cues[2]).is_empty());
    assert_eq!(
        macro_names(&managed.cues[3]),
        vec!["Scripture/Prayer (Highlighted)"]
    );
    assert!(macro_names(&managed.cues[4]).is_empty());
    assert_at_most_one_macro_per_cue(&managed);
}

#[test]
fn combined_transform_retains_one_consistent_nametag_cue() {
    let root = tempfile::tempdir().expect("temporary root");
    let mut runtime = TestRuntime::new(root.path());
    install_named_macros(&mut runtime, &["NameTag"]);
    let (source, default_uuid) = arranged_presentation("Greeting", 3);
    let replacement_path = root.path().join("replacement.png");
    let replacement_bytes = minimal_png(4, 3);
    std::fs::write(&replacement_path, &replacement_bytes).expect("write replacement background");
    let transform = crate::workflow::plan::ExistingTransform::new(
        crate::workflow::plan::BackgroundTransform::Replace(test_background("replacement")),
        crate::workflow::plan::MacroTransform::Enforce(operator_macro_policy(&[(0, "NameTag")])),
        crate::workflow::plan::CueTransform::RetainOperatorPrefix(
            NonZeroUsize::new(1).expect("nonzero fixture count"),
        ),
    )
    .expect("combined transform");

    let managed = run_native_transform(
        &runtime,
        root.path(),
        &source,
        Some("Default"),
        &transform,
        Some(ReviewedBackgroundAsset {
            path: &replacement_path,
            data: &replacement_bytes,
        }),
    );

    assert_eq!(
        crate::propresenter::arrangement::operator_cue_indices(&managed),
        vec![0]
    );
    assert_eq!(managed.cues.len(), 1);
    assert_eq!(macro_names(&managed.cues[0]), vec!["NameTag"]);
    assert_eq!(
        managed
            .selected_arrangement
            .as_ref()
            .map(|identifier| identifier.string.as_str()),
        Some(default_uuid.to_string().as_str())
    );
    assert_no_dangling_group_or_arrangement_references(&managed);
    assert_at_most_one_macro_per_cue(&managed);
}

fn install_named_macros(runtime: &mut TestRuntime, names: &[&str]) {
    let document = rv_data::MacrosDocument {
        macros: names
            .iter()
            .enumerate()
            .map(|(index, name)| rv_data::macros_document::Macro {
                uuid: Some(rv_data::Uuid {
                    string: format!("00000000-0000-0000-0000-{:012}", index + 1),
                }),
                name: (*name).to_string(),
                ..rv_data::macros_document::Macro::default()
            })
            .collect(),
        ..rv_data::MacrosDocument::default()
    };
    let path = runtime.locations().macros();
    std::fs::create_dir_all(path.parent().expect("macro parent")).expect("create macro parent");
    std::fs::write(path, document.encode_to_vec()).expect("write macros");
    runtime.reload_render_assets();
}

fn operator_macro_policy(regions: &[(usize, &str)]) -> crate::workflow::plan::RestyleMacroPolicy {
    crate::workflow::plan::RestyleMacroPolicy::new(
        regions
            .iter()
            .map(|(index, name)| {
                crate::workflow::plan::RestyleMacroRegion::new(
                    crate::workflow::plan::RestyleMacroSelector::OperatorCue { index: *index },
                    (*name).to_string(),
                )
                .expect("valid macro region")
            })
            .collect(),
    )
    .expect("nonempty macro policy")
}

fn run_native_transform(
    runtime: &TestRuntime,
    root: &Path,
    source: &rv_data::Presentation,
    arrangement: Option<&str>,
    transform: &crate::workflow::plan::ExistingTransform,
    background: Option<ReviewedBackgroundAsset<'_>>,
) -> rv_data::Presentation {
    let source_path = root.join("source.pro");
    let output_path = root.join("managed.pro");
    let source_bytes = source.encode_to_vec();
    std::fs::write(&source_path, &source_bytes).expect("write source presentation");
    let plan = test_plan("pco:native:main", PlanDisposition::Skip);
    runtime
        .executor()
        .restyle_existing_presentation(
            &plan,
            &source_path,
            arrangement,
            transform,
            &source_bytes,
            ReviewedRenderTarget {
                write_path: &output_path,
                final_path: &output_path,
                existing_bytes: None,
                presentation_size: crate::propresenter::PresentationSize::FULL_HD,
                background,
            },
        )
        .expect("apply checked native transform");
    rv_data::Presentation::decode(
        std::fs::read(output_path)
            .expect("read managed presentation")
            .as_slice(),
    )
    .expect("decode managed presentation")
}

fn presentation_with_cue_count(name: &str, cue_count: usize) -> rv_data::Presentation {
    let mut presentation = presentation_with_size(name, 1920.0, 1080.0);
    let template = presentation.cues[0].clone();
    presentation.cues = (0..cue_count)
        .map(|index| {
            let mut cue = template.clone();
            cue.uuid = Some(rv_data::Uuid {
                string: Uuid::new_v4().to_string(),
            });
            cue.name = format!("Cue {}", index + 1);
            cue
        })
        .collect();
    presentation.cue_groups[0].cue_identifiers = presentation
        .cues
        .iter()
        .filter_map(|cue| cue.uuid.clone())
        .collect();
    presentation
}

fn arranged_presentation(name: &str, cue_count: usize) -> (rv_data::Presentation, Uuid) {
    let mut presentation = presentation_with_cue_count(name, cue_count);
    presentation.cue_groups = presentation
        .cues
        .iter()
        .enumerate()
        .map(|(index, cue)| rv_data::presentation::CueGroup {
            group: Some(rv_data::Group {
                uuid: Some(rv_data::Uuid {
                    string: Uuid::new_v4().to_string(),
                }),
                name: format!("Group {}", index + 1),
                ..rv_data::Group::default()
            }),
            cue_identifiers: vec![cue.uuid.clone().expect("fixture cue identity")],
        })
        .collect();
    let group_ids = presentation
        .cue_groups
        .iter()
        .filter_map(|group| group.group.as_ref()?.uuid.clone())
        .collect::<Vec<_>>();
    let default_uuid = Uuid::new_v4();
    presentation.arrangements = vec![
        rv_data::presentation::Arrangement {
            uuid: Some(rv_data::Uuid {
                string: default_uuid.to_string(),
            }),
            name: "Default".to_string(),
            group_identifiers: group_ids.clone(),
        },
        rv_data::presentation::Arrangement {
            uuid: Some(rv_data::Uuid {
                string: Uuid::new_v4().to_string(),
            }),
            name: "Alternate".to_string(),
            group_identifiers: group_ids.into_iter().skip(1).collect(),
        },
    ];
    presentation.selected_arrangement = Some(rv_data::Uuid {
        string: default_uuid.to_string(),
    });
    (presentation, default_uuid)
}

fn native_background_action_bytes(presentation: &rv_data::Presentation) -> Vec<Vec<u8>> {
    presentation
        .cues
        .iter()
        .flat_map(|cue| &cue.actions)
        .filter(|action| crate::propresenter::background::is_background_media_action(action))
        .map(Message::encode_to_vec)
        .collect()
}

fn native_macro_action_bytes(presentation: &rv_data::Presentation) -> Vec<Vec<Vec<u8>>> {
    presentation
        .cues
        .iter()
        .map(|cue| {
            cue.actions
                .iter()
                .filter(|action| crate::propresenter::macros::macro_action_name(action).is_some())
                .map(Message::encode_to_vec)
                .collect()
        })
        .collect()
}

fn macro_names(cue: &rv_data::Cue) -> Vec<&str> {
    cue.actions
        .iter()
        .filter_map(crate::propresenter::macros::macro_action_name)
        .collect()
}

fn assert_at_most_one_macro_per_cue(presentation: &rv_data::Presentation) {
    for cue in &presentation.cues {
        assert!(
            macro_names(cue).len() <= 1,
            "managed cue contains more than one macro: {:?}",
            macro_names(cue)
        );
    }
}

fn assert_no_dangling_group_or_arrangement_references(presentation: &rv_data::Presentation) {
    let cue_ids = presentation
        .cues
        .iter()
        .filter_map(|cue| cue.uuid.as_ref())
        .map(|identifier| identifier.string.as_str())
        .collect::<std::collections::HashSet<_>>();
    assert!(presentation.cue_groups.iter().all(|group| group
        .cue_identifiers
        .iter()
        .all(|identifier| cue_ids.contains(identifier.string.as_str()))));
    let group_ids = presentation
        .cue_groups
        .iter()
        .filter_map(|group| group.group.as_ref()?.uuid.as_ref())
        .map(|identifier| identifier.string.as_str())
        .collect::<std::collections::HashSet<_>>();
    assert!(presentation
        .arrangements
        .iter()
        .all(|arrangement| arrangement
            .group_identifiers
            .iter()
            .all(|identifier| group_ids.contains(identifier.string.as_str()))));
    if let Some(selected) = presentation.selected_arrangement.as_ref() {
        assert!(presentation.arrangements.iter().any(|arrangement| {
            arrangement
                .uuid
                .as_ref()
                .is_some_and(|identifier| identifier.string == selected.string)
        }));
    }
}

#[tokio::test]
async fn restyled_song_atomically_rewrites_its_canonical_library_file() {
    let fixture = restyle_fixture();
    let executor = fixture.runtime.executor();
    let managed_path = executor
        .presentation_target(&fixture.plan)
        .expect("managed target");
    assert_eq!(managed_path.parent(), fixture.source_path.parent());
    assert_eq!(
        managed_path.file_name().and_then(|name| name.to_str()),
        Some("Faithful Song.pro")
    );
    assert_eq!(managed_path, fixture.source_path);
    let prepared = expect_prepared(
        executor
            .review_build_request(
                fixture.request.clone(),
                std::slice::from_ref(&fixture.plan),
                crate::propresenter::PresentationSize::FULL_HD,
            )
            .await
            .expect("review restyled song"),
    );
    let first_result = executor
        .build_prepared_request(prepared)
        .await
        .expect("commit restyled song");
    let first_managed_bytes = std::fs::read(&managed_path).expect("read managed song");
    assert_restyled_copy(&fixture, &first_managed_bytes);
    assert_local_playlist_link(
        &first_result.playlist_path,
        &managed_path,
        fixture.default_uuid,
    );
    let second = expect_prepared(
        executor
            .review_build_request(
                fixture.request,
                &[fixture.plan],
                crate::propresenter::PresentationSize::FULL_HD,
            )
            .await
            .expect("review second restyle"),
    );
    executor
        .build_prepared_request(second)
        .await
        .expect("commit second restyle");
    assert_eq!(
        std::fs::read(&managed_path).expect("read second managed song"),
        first_managed_bytes,
        "restyling is deterministic once the managed document identity exists"
    );
}

struct RestyleFixture {
    _root: tempfile::TempDir,
    runtime: TestRuntime,
    plan: ResolvedItemPlan,
    request: BuildRequest,
    source_path: PathBuf,
    source: rv_data::Presentation,
    default_uuid: Uuid,
}

fn restyle_fixture() -> RestyleFixture {
    let root = tempfile::tempdir().expect("temporary root");
    let source_library = root.path().join("registered-library");
    let playlist_output = root.path().join("playlists");
    for directory in [&source_library, &playlist_output] {
        std::fs::create_dir_all(directory).expect("create test directory");
    }
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
    let background_path = runtime
        .locations()
        .project_data_root()
        .join("backgrounds/default.png");
    std::fs::create_dir_all(background_path.parent().expect("background parent"))
        .expect("create background directory");
    std::fs::write(&background_path, minimal_png(2, 2)).expect("write reviewed background");

    let (source, default_uuid) = source_song(&runtime, root.path());
    let source_path = source_library.join("Faithful Song.pro");
    let source_bytes = source.encode_to_vec();
    std::fs::write(&source_path, &source_bytes).expect("write source song");
    runtime.file_index = std::sync::Arc::new(Mutex::new(
        LibraryCatalog::build(&source_library).expect("index source library"),
    ));
    runtime.replace_locations(
        BuildLocations::from_inputs(BuildLocationInputs {
            project_data_root: runtime.locations().project_data_root().to_path_buf(),
            presentation_library: source_library,
            playlist_output,
            propresenter_root: runtime.locations().propresenter_root().to_path_buf(),
            themes: runtime.locations().themes().to_path_buf(),
            macros: runtime.locations().macros().to_path_buf(),
        })
        .expect("checked selected staging library"),
    );
    let mut plan = test_plan(
        "pco:song:main",
        PlanDisposition::Ready(ReadyAction::RestyleExisting {
            file_path: source_path.clone(),
            arrangement: Some("Default".to_string()),
            transform: test_transform(),
        }),
    );
    plan.item_kind = ItemKind::Song;
    plan.playlist_name = "Faithful Song".to_string();
    RestyleFixture {
        _root: root,
        runtime,
        plan,
        request: reviewed_request("Faithful Local Playlist"),
        source_path,
        source,
        default_uuid,
    }
}

fn source_song(runtime: &TestRuntime, root: &Path) -> (rv_data::Presentation, Uuid) {
    let default_uuid = Uuid::new_v4();
    let mut source = presentation_with_size("Faithful Song", 1920.0, 1080.0);
    let group_uuid = source.cue_groups[0]
        .group
        .as_ref()
        .and_then(|group| group.uuid.clone())
        .expect("fixture group identity");
    source.arrangements = [(&default_uuid, "Default"), (&Uuid::new_v4(), "Youth")]
        .into_iter()
        .map(|(uuid, name)| rv_data::presentation::Arrangement {
            uuid: Some(rv_data::Uuid {
                string: uuid.to_string(),
            }),
            name: name.to_string(),
            group_identifiers: vec![group_uuid.clone()],
        })
        .collect();
    source.selected_arrangement = Some(rv_data::Uuid {
        string: Uuid::new_v4().to_string(),
    });
    source.ccli = Some(rv_data::presentation::Ccli {
        song_title: "Faithful Song".to_string(),
        author: "Preserved Author".to_string(),
        ..rv_data::presentation::Ccli::default()
    });
    source.cues[0].actions.push(rv_data::Action {
        uuid: Some(rv_data::Uuid {
            string: Uuid::new_v4().to_string(),
        }),
        name: "Preserved non-background action".to_string(),
        r#type: rv_data::action::ActionType::Macro as i32,
        ..rv_data::Action::default()
    });
    crate::propresenter::background::add_reviewed_background_to_first_cue(
        &mut source,
        &root.join("old-background.png"),
        &minimal_png(1, 1),
        runtime.locations().propresenter_root(),
    )
    .expect("install old source background");
    (source, default_uuid)
}

fn assert_restyled_copy(fixture: &RestyleFixture, managed_bytes: &[u8]) {
    let managed = rv_data::Presentation::decode(managed_bytes).expect("decode managed song");
    let background_path = fixture
        .runtime
        .locations()
        .project_data_root()
        .join("backgrounds/default.png");
    let mut expected = fixture.source.clone();
    crate::propresenter::background::replace_arrangement_entry_backgrounds(
        &mut expected,
        &background_path,
        &std::fs::read(&background_path).expect("read expected background"),
        fixture.runtime.locations().propresenter_root(),
        &fixture.default_uuid,
        "Default",
    )
    .expect("apply expected background transform");
    crate::propresenter::macros::apply_operator_macro_policy(
        &mut expected,
        &test_macro_transitions(),
        fixture.runtime.render_assets.macros(),
    )
    .expect("apply expected macro transform");
    crate::propresenter::render::apply_application_info(
        &mut expected,
        Some(fixture.runtime.playlist_metadata.application_info()),
    );

    assert_eq!(managed, expected);
    assert_eq!(managed.uuid, fixture.source.uuid);
    assert_eq!(managed.name, fixture.source.name);
    assert_eq!(
        managed.application_info,
        Some(fixture.runtime.playlist_metadata.application_info().clone())
    );
    assert_eq!(managed.ccli, fixture.source.ccli);
    assert_eq!(managed.arrangements, fixture.source.arrangements);
    assert_eq!(managed.cue_groups, fixture.source.cue_groups);
    assert_eq!(managed.cues.len(), fixture.source.cues.len());
    assert_eq!(managed.cues[0].uuid, fixture.source.cues[0].uuid);
    assert_eq!(
        managed.cues[0].actions[0],
        fixture.source.cues[0].actions[0]
    );
    assert_eq!(
        managed
            .selected_arrangement
            .as_ref()
            .map(|uuid| &uuid.string),
        Some(&fixture.default_uuid.to_string())
    );
    assert_eq!(
        crate::propresenter::macros::macro_action_name(&managed.cues[0].actions[1]),
        Some("Song")
    );
    assert_eq!(
        managed.cues[0].actions[1].uuid, fixture.source.cues[0].actions[1].uuid,
        "replacing a stale macro preserves its native wrapper identity"
    );
    let dependencies =
        crate::propresenter::media::presentation_media_dependencies_from_bytes(managed_bytes)
            .expect("inspect managed media");
    let dependency_names = dependencies
        .iter()
        .filter_map(|dependency| dependency.path.as_deref().and_then(Path::file_name))
        .collect::<Vec<_>>();
    assert!(dependency_names.iter().any(|name| *name == "default.png"));
    assert!(!dependency_names
        .iter()
        .any(|name| *name == "old-background.png"));
}

fn assert_local_playlist_link(playlist_path: &str, managed_path: &Path, arrangement_uuid: Uuid) {
    let package = crate::propresenter::package::read_playlist_package(playlist_path)
        .expect("read local playlist");
    assert!(package.embedded_file_data.is_empty());
    let items = crate::propresenter::package::presentation_items(&package.document);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].arrangement_name, "Default");
    assert_eq!(
        items[0].arrangement_uuid.as_deref(),
        Some(arrangement_uuid.to_string().as_str())
    );
    assert_eq!(
        crate::propresenter::playlist::linked_presentation_filename(&items[0]).as_deref(),
        managed_path.file_name().and_then(|name| name.to_str())
    );
}
