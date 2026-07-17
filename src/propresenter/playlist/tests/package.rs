use super::*;
use crate::propresenter::playlist::package_validation::media_archive_path;

#[test]
fn writes_library_local_playlist_file() {
    let directory = tempfile::tempdir().expect("tempdir");
    let output = directory.path().join("test.proplaylist");
    let entries = vec![linked_entry(
        "Test Song",
        "/Users/Shared/ProPresenter/Libraries/Default/Test.pro",
    )];
    let document = build_playlist("Test Playlist", &entries, &test_metadata());
    write_playlist_document_for_fidelity(&document, &entries, &output).expect("write playlist");
    assert!(!std::fs::read(output).expect("read playlist").is_empty());
}

#[test]
fn embedded_archive_identity_comes_from_source_not_display_name() {
    let directory = tempfile::tempdir().expect("tempdir");
    let output = directory.path().join("alias.proplaylist");
    let entries = vec![PlaylistEntry::embedded(
        "Display Alias",
        "/Libraries/Default/Actual File.pro",
        presentation_bytes("Actual File"),
    )
    .expect("valid alias")];
    let document = build_playlist("Service", &entries, &test_metadata());
    write_playlist_document_for_fidelity(&document, &entries, &output).expect("write playlist");
    let package =
        crate::propresenter::package::read_playlist_package(&output).expect("read playlist");
    let items = presentation_items(&package.document);
    assert_eq!(items[0].name, "Display Alias");
    assert_eq!(
        items[0].local_relative_path.as_deref(),
        Some("Libraries/Default/Actual File.pro")
    );
    assert_eq!(package.embedded_files, vec!["Actual File.pro"]);
}

#[test]
fn repeated_source_entries_share_one_embedded_presentation() {
    let directory = tempfile::tempdir().expect("tempdir");
    let output = directory.path().join("repeated.proplaylist");
    let source_path = "/Libraries/Default/Shared Source.pro";
    let embedded_data = presentation_bytes("Shared Source");
    let entries = vec![
        PlaylistEntry::embedded("Opening", source_path, embedded_data.clone())
            .expect("valid opening"),
        PlaylistEntry::embedded("Closing", source_path, embedded_data).expect("valid closing"),
    ];
    let document = build_playlist("Service", &entries, &test_metadata());
    write_playlist_document_for_fidelity(&document, &entries, &output).expect("write playlist");
    let package =
        crate::propresenter::package::read_playlist_package(&output).expect("read playlist");
    let items = presentation_items(&package.document);
    assert_eq!(package.embedded_files, vec!["Shared Source.pro"]);
    assert_eq!(items.len(), 2);
    assert_eq!(items[1].local_relative_path, items[0].local_relative_path);
}

#[test]
fn repeated_source_entries_reject_conflicting_embedded_bytes() {
    let source_path = "/Libraries/Default/Shared Source.pro";
    let entries = vec![
        PlaylistEntry::embedded("First", source_path, presentation_bytes("First"))
            .expect("valid first"),
        PlaylistEntry::embedded("Second", source_path, presentation_bytes("Second"))
            .expect("valid second"),
    ];
    let document = build_playlist("Service", &entries, &test_metadata());
    let directory = tempfile::tempdir().expect("tempdir");
    let error = write_playlist_document_for_fidelity(
        &document,
        &entries,
        directory.path().join("conflict.proplaylist"),
    )
    .expect_err("same source cannot carry different bytes");
    assert!(matches!(
        error,
        PlaylistError::ConflictingEmbeddedSource {
            first_index: 0,
            conflicting_index: 1,
            ..
        }
    ));
}

#[test]
fn portable_write_embeds_media_assets() {
    let directory = tempfile::tempdir().expect("tempdir");
    let media_path = directory.path().join("default.jpg");
    std::fs::write(&media_path, [1, 2, 3]).expect("write media asset");
    let native_archive_path = media_path
        .canonicalize()
        .expect("canonical media path")
        .display()
        .to_string();
    let output = directory.path().join("portable.proplaylist");
    let entries = vec![linked_entry("Test Song", "/Libraries/Default/Test.pro")];
    let document = build_playlist("Test Playlist", &entries, &test_metadata());
    let intent = PlaylistExportIntent::portable_import(vec![PlaylistMediaAsset::new(media_path)]);
    write_playlist_document_file_with_intent(&document, &entries, &output, intent)
        .expect("write portable playlist");
    let package =
        crate::propresenter::package::read_playlist_package(&output).expect("read package");
    assert_eq!(
        crate::propresenter::package::infer_package_mode(&package),
        PlaylistPackageMode::ExportPortable
    );
    assert_eq!(package.embedded_files, vec![native_archive_path]);
    assert_eq!(package.embedded_file_details[0].basename, "default.jpg");
    assert!(!package.embedded_file_details[0].is_presentation);
}

#[test]
fn reviewed_portable_write_uses_captured_media_bytes() {
    let directory = tempfile::tempdir().expect("tempdir");
    let media_path = directory.path().join("reviewed.jpg");
    let reviewed_bytes = [1, 2, 3];
    std::fs::write(&media_path, reviewed_bytes).expect("write reviewed media");
    let asset = PlaylistMediaAsset::new(
        media_path
            .canonicalize()
            .expect("canonical reviewed media path"),
    );
    let archive_path = media_archive_path(&asset).expect("native archive path");
    let reviewed_asset = asset
        .bind_reviewed(&reviewed_bytes)
        .expect("bind reviewed bytes");
    std::fs::write(&media_path, [9, 9, 9]).expect("change live media bytes");
    let output = directory.path().join("reviewed.proplaylist");
    let playlist_set = PlaylistSet::new(vec![
        NamedPlaylist::new("Reviewed", Vec::new()).expect("named playlist")
    ])
    .expect("playlist set");
    write_playlist_set_file_with_reviewed_media(
        &playlist_set,
        &test_metadata(),
        &output,
        ReviewedPlaylistExportIntent::PortableImport(&[reviewed_asset]),
    )
    .expect("write reviewed portable playlist");
    let package =
        crate::propresenter::package::read_playlist_package(&output).expect("read package");
    assert_eq!(
        package
            .embedded_file_data
            .get(&archive_path)
            .expect("reviewed archive member"),
        &reviewed_bytes
    );
}

fn media_presentation(name: &str, media_path: &Path, root: &Path) -> Vec<u8> {
    rv_data::Presentation {
        name: name.to_string(),
        uuid: Some(rv_data::Uuid {
            string: Uuid::new_v4().to_string(),
        }),
        cues: vec![rv_data::Cue {
            actions: vec![
                crate::propresenter::background::make_background_media_action_for_test(
                    media_path,
                    (1, 1),
                    root,
                ),
            ],
            ..rv_data::Cue::default()
        }],
        ..rv_data::Presentation::default()
    }
    .encode_to_vec()
}

struct NativeDependencyFixture<'a> {
    presentation_chord: &'a Path,
    audio_file: &'a Path,
    external_presentation: &'a Path,
    web_content: &'a Path,
    image_file: &'a Path,
    file_feed: &'a Path,
    ticker_file: &'a Path,
    custom_attribute_media: &'a Path,
    video_file: &'a Path,
}

fn native_dependency_presentation(name: &str, fixture: &NativeDependencyFixture<'_>) -> Vec<u8> {
    use rv_data::action;
    use rv_data::media;

    rv_data::Presentation {
        name: name.to_string(),
        uuid: Some(rv_data::Uuid {
            string: Uuid::new_v4().to_string(),
        }),
        chord_chart: Some(test_file_url(fixture.presentation_chord)),
        cues: vec![rv_data::Cue {
            actions: vec![
                test_media_action(rv_data::Media {
                    type_properties: Some(media::TypeProperties::Audio(
                        media::AudioTypeProperties {
                            file: Some(test_file_properties(fixture.audio_file)),
                            ..media::AudioTypeProperties::default()
                        },
                    )),
                    ..rv_data::Media::default()
                }),
                rv_data::Action {
                    action_type_data: Some(action::ActionTypeData::ExternalPresentation(
                        action::ExternalPresentationType {
                            url: Some(test_file_url(fixture.external_presentation)),
                        },
                    )),
                    ..rv_data::Action::default()
                },
                test_media_action(rv_data::Media {
                    type_properties: Some(media::TypeProperties::WebContent(
                        media::WebContentTypeProperties {
                            url: Some(test_file_url(fixture.web_content)),
                            ..media::WebContentTypeProperties::default()
                        },
                    )),
                    ..rv_data::Media::default()
                }),
                rv_data::Action {
                    action_type_data: Some(action::ActionTypeData::Slide(action::SlideType {
                        slide: Some(action::slide_type::Slide::Presentation(
                            native_dependency_slide(fixture),
                        )),
                    })),
                    ..rv_data::Action::default()
                },
            ],
            ..rv_data::Cue::default()
        }],
        timeline: Some(rv_data::presentation::Timeline {
            audio_action: Some(test_media_action(rv_data::Media {
                type_properties: Some(media::TypeProperties::Video(media::VideoTypeProperties {
                    file: Some(test_file_properties(fixture.video_file)),
                    ..media::VideoTypeProperties::default()
                })),
                ..rv_data::Media::default()
            })),
            ..rv_data::presentation::Timeline::default()
        }),
        ..rv_data::Presentation::default()
    }
    .encode_to_vec()
}

fn native_dependency_slide(fixture: &NativeDependencyFixture<'_>) -> rv_data::PresentationSlide {
    use rv_data::graphics;
    use rv_data::media;
    use rv_data::slide;

    let image_media = rv_data::Media {
        type_properties: Some(media::TypeProperties::Image(media::ImageTypeProperties {
            file: Some(test_file_properties(fixture.image_file)),
            ..media::ImageTypeProperties::default()
        })),
        ..rv_data::Media::default()
    };
    rv_data::PresentationSlide {
        base_slide: Some(rv_data::Slide {
            elements: vec![
                slide::Element {
                    element: Some(graphics::Element {
                        fill: Some(graphics::Fill {
                            enable: true,
                            fill_type: Some(graphics::fill::FillType::Media(image_media)),
                        }),
                        ..graphics::Element::default()
                    }),
                    ..slide::Element::default()
                },
                slide::Element {
                    data_links: vec![slide::element::DataLink {
                        property_type: Some(slide::element::data_link::PropertyType::FileFeed(
                            slide::element::data_link::FileFeed {
                                url: Some(test_file_url(fixture.file_feed)),
                            },
                        )),
                    }],
                    ..slide::Element::default()
                },
                slide::Element {
                    data_links: vec![slide::element::DataLink {
                        property_type: Some(slide::element::data_link::PropertyType::Ticker(
                            slide::element::data_link::Ticker {
                                source_type: Some(
                                    slide::element::data_link::ticker::SourceType::FileType(
                                        slide::element::data_link::ticker::FileType {
                                            url: Some(test_file_url(fixture.ticker_file)),
                                        },
                                    ),
                                ),
                                ..slide::element::data_link::Ticker::default()
                            },
                        )),
                    }],
                    ..slide::Element::default()
                },
                slide::Element {
                    element: Some(graphics::Element {
                        text: Some(graphics::Text {
                            attributes: Some(graphics::text::Attributes {
                                custom_attributes: vec![
                                    graphics::text::attributes::CustomAttribute {
                                        attribute: Some(
                                            graphics::text::attributes::custom_attribute::Attribute::MediaFill(
                                                graphics::text::MediaFill {
                                                    media: Some(rv_data::Media {
                                                        url: Some(test_file_url(
                                                            fixture.custom_attribute_media,
                                                        )),
                                                        ..rv_data::Media::default()
                                                    }),
                                                },
                                            ),
                                        ),
                                        ..graphics::text::attributes::CustomAttribute::default()
                                    },
                                ],
                                ..graphics::text::Attributes::default()
                            }),
                            ..graphics::Text::default()
                        }),
                        ..graphics::Element::default()
                    }),
                    ..slide::Element::default()
                },
            ],
            ..rv_data::Slide::default()
        }),
        ..rv_data::PresentationSlide::default()
    }
}

fn test_media_action(element: rv_data::Media) -> rv_data::Action {
    rv_data::Action {
        action_type_data: Some(rv_data::action::ActionTypeData::Media(
            rv_data::action::MediaType {
                element: Some(element),
                ..rv_data::action::MediaType::default()
            },
        )),
        ..rv_data::Action::default()
    }
}

fn test_file_properties(path: &Path) -> rv_data::FileProperties {
    rv_data::FileProperties {
        local_url: Some(test_file_url(path)),
        ..rv_data::FileProperties::default()
    }
}

fn test_file_url(path: &Path) -> rv_data::Url {
    rv_data::Url {
        storage: Some(rv_data::url::Storage::AbsoluteString(format!(
            "file://{}",
            path.display()
        ))),
        ..rv_data::Url::default()
    }
}

#[test]
fn portable_write_embeds_discovered_media_assets() {
    let directory = tempfile::tempdir().expect("tempdir");
    let media_path = directory.path().join("default.jpg");
    std::fs::write(&media_path, [1, 2, 3]).expect("write media asset");
    let native_archive_path = media_path
        .canonicalize()
        .expect("canonical media path")
        .display()
        .to_string();
    let entries = vec![PlaylistEntry::embedded(
        "With Media",
        "/Libraries/Default/With Media.pro",
        media_presentation("With Media", &media_path, directory.path()),
    )
    .expect("valid media presentation")];
    let document = build_playlist("Test Playlist", &entries, &test_metadata());
    let output = directory.path().join("portable-discovered.proplaylist");
    write_playlist_document_file_with_intent(
        &document,
        &entries,
        &output,
        PlaylistExportIntent::portable_import(Vec::new()),
    )
    .expect("write portable playlist");
    let package =
        crate::propresenter::package::read_playlist_package(&output).expect("read package");
    assert!(package
        .embedded_files
        .iter()
        .any(|file| file == &native_archive_path));
}

#[test]
fn portable_write_embeds_native_local_file_carriers() {
    let directory = tempfile::tempdir().expect("tempdir");
    let [presentation_chord, audio_file, external_presentation, web_content, image_file, file_feed, ticker_file, custom_attribute_media, video_file] =
        [
            "presentation.prochord",
            "cue.mp3",
            "external.key",
            "local.html",
            "fill.png",
            "feed.txt",
            "ticker.txt",
            "custom-attribute.png",
            "timeline.mov",
        ]
        .map(|name| {
            let path = directory.path().join(name);
            std::fs::write(&path, name.as_bytes()).expect("write dependency fixture");
            path
        });
    let fixture = NativeDependencyFixture {
        presentation_chord: &presentation_chord,
        audio_file: &audio_file,
        external_presentation: &external_presentation,
        web_content: &web_content,
        image_file: &image_file,
        file_feed: &file_feed,
        ticker_file: &ticker_file,
        custom_attribute_media: &custom_attribute_media,
        video_file: &video_file,
    };
    let entries = vec![PlaylistEntry::embedded(
        "Native Dependencies",
        "/Libraries/Default/Native Dependencies.pro",
        native_dependency_presentation("Native Dependencies", &fixture),
    )
    .expect("valid dependency presentation")];
    let document = build_playlist("Test Playlist", &entries, &test_metadata());
    let output = directory.path().join("portable-native-fields.proplaylist");

    write_playlist_document_file_with_intent(
        &document,
        &entries,
        &output,
        PlaylistExportIntent::portable_import(Vec::new()),
    )
    .expect("write portable playlist");

    let package =
        crate::propresenter::package::read_playlist_package(&output).expect("read package");
    for path in [
        &presentation_chord,
        &audio_file,
        &external_presentation,
        &web_content,
        &image_file,
        &file_feed,
        &ticker_file,
        &custom_attribute_media,
        &video_file,
    ] {
        let expected = path
            .canonicalize()
            .expect("canonical dependency")
            .display()
            .to_string();
        assert!(
            package.embedded_files.contains(&expected),
            "missing native dependency {expected}"
        );
    }
}

#[test]
fn portable_write_surfaces_remote_native_file_references() {
    let remote_reference = "https://example.com/chart.prochord";
    let presentation = rv_data::Presentation {
        name: "Remote Dependency".to_string(),
        uuid: Some(rv_data::Uuid {
            string: Uuid::new_v4().to_string(),
        }),
        chord_chart: Some(rv_data::Url {
            storage: Some(rv_data::url::Storage::AbsoluteString(
                remote_reference.to_string(),
            )),
            ..rv_data::Url::default()
        }),
        ..rv_data::Presentation::default()
    };
    let entries = vec![PlaylistEntry::embedded(
        "Remote Dependency",
        "/Libraries/Default/Remote Dependency.pro",
        presentation.encode_to_vec(),
    )
    .expect("valid remote-dependency presentation")];
    let document = build_playlist("Service", &entries, &test_metadata());
    let directory = tempfile::tempdir().expect("tempdir");

    let error = write_playlist_document_file_with_intent(
        &document,
        &entries,
        directory.path().join("remote.proplaylist"),
        PlaylistExportIntent::portable_import(Vec::new()),
    )
    .expect_err("remote dependency requires operator resolution");

    assert!(matches!(
        error,
        PlaylistError::UnresolvedMediaDependency { reference, .. }
            if reference == remote_reference
    ));
}

#[test]
fn discovered_media_rejects_a_custom_archive_identity() {
    let directory = tempfile::tempdir().expect("tempdir");
    let media_path = directory.path().join("default.jpg");
    std::fs::write(&media_path, [1, 2, 3]).expect("write media asset");
    let entries = vec![PlaylistEntry::embedded(
        "With Media",
        "/Libraries/Default/With Media.pro",
        media_presentation("With Media", &media_path, directory.path()),
    )
    .expect("valid media presentation")];
    let document = build_playlist("Test Playlist", &entries, &test_metadata());
    let intent = PlaylistExportIntent::portable_import(vec![PlaylistMediaAsset {
        source_path: media_path.clone(),
        archive_path: Some("media/default.jpg".to_string()),
    }]);

    let error = write_playlist_document_file_with_intent(
        &document,
        &entries,
        directory.path().join("portable-discovered.proplaylist"),
        intent,
    )
    .expect_err("native dependency identity cannot be overridden");

    assert!(matches!(
        error,
        PlaylistError::MediaDependencyArchiveOverride { path, .. }
            if path == media_path.canonicalize().expect("canonical media")
    ));
}

#[test]
fn discovered_missing_media_is_a_typed_error() {
    let directory = tempfile::tempdir().expect("tempdir");
    let missing = directory.path().join("missing.jpg");
    let entries = vec![PlaylistEntry::embedded(
        "Missing Media",
        "/Libraries/Default/Missing Media.pro",
        media_presentation("Missing Media", &missing, directory.path()),
    )
    .expect("valid missing-media presentation")];
    let document = build_playlist("Service", &entries, &test_metadata());
    let result = write_playlist_document_file_with_intent(
        &document,
        &entries,
        directory.path().join("missing.proplaylist"),
        PlaylistExportIntent::portable_import(Vec::new()),
    );
    assert!(matches!(
        result,
        Err(PlaylistError::MissingMediaDependency { path, .. }) if path == missing
    ));
}

#[test]
fn malformed_embedded_presentation_is_a_typed_error() {
    let result = PlaylistEntry::embedded("Broken", "/Libraries/Default/Broken.pro", vec![1, 2, 3]);
    assert!(matches!(
        result,
        Err(PlaylistEntryError::InvalidEmbeddedPresentation { .. })
    ));
}

#[test]
fn identity_less_embedded_presentation_is_rejected() {
    let result = PlaylistEntry::embedded(
        "Identityless",
        "/Libraries/Default/Identityless.pro",
        rv_data::Presentation::default().encode_to_vec(),
    );
    assert!(matches!(
        result,
        Err(PlaylistEntryError::InvalidEmbeddedPresentation { .. })
    ));
}

#[test]
fn blank_entry_path_is_rejected_instead_of_guessing_a_library_path() {
    let result = PlaylistEntry::embedded("Generated", "", presentation_bytes("Generated"));
    assert!(matches!(
        result,
        Err(PlaylistEntryError::InvalidPresentationPath(path)) if path.is_empty()
    ));
}

#[test]
fn rejects_unsafe_and_reserved_archive_paths() {
    let directory = tempfile::tempdir().expect("tempdir");
    let media_path = directory.path().join("asset.jpg");
    std::fs::write(&media_path, b"asset").expect("write media");
    let document = build_playlist("Test", &[], &test_metadata());

    for archive_path in ["../asset.jpg", "/untrusted/asset.jpg"] {
        let intent = PlaylistExportIntent::portable_import(vec![PlaylistMediaAsset {
            source_path: media_path.clone(),
            archive_path: Some(archive_path.to_string()),
        }]);
        assert!(matches!(
            write_playlist_document_file_with_intent(
                &document,
                &[],
                directory.path().join("unsafe.proplaylist"),
                intent
            ),
            Err(PlaylistError::InvalidArchivePath(_))
        ));
    }

    let reserved = PlaylistExportIntent::portable_import(vec![PlaylistMediaAsset {
        source_path: media_path.clone(),
        archive_path: Some("data".to_string()),
    }]);
    assert!(matches!(
        write_playlist_document_file_with_intent(
            &document,
            &[],
            directory.path().join("reserved.proplaylist"),
            reserved
        ),
        Err(PlaylistError::DuplicateArchiveEntry(path)) if path == "data"
    ));

    let duplicate = PlaylistExportIntent::portable_import(vec![
        PlaylistMediaAsset {
            source_path: media_path.clone(),
            archive_path: Some("media/shared.jpg".to_string()),
        },
        PlaylistMediaAsset {
            source_path: media_path,
            archive_path: Some("media/shared.jpg".to_string()),
        },
    ]);
    assert!(matches!(
        write_playlist_document_file_with_intent(
            &document,
            &[],
            directory.path().join("duplicate.proplaylist"),
            duplicate
        ),
        Err(PlaylistError::DuplicateArchiveEntry(path)) if path == "media/shared.jpg"
    ));
}

#[test]
fn rejects_document_and_archive_entry_mismatch() {
    let directory = tempfile::tempdir().expect("tempdir");
    let data = presentation_bytes("Original");
    let original_entries =
        vec![
            PlaylistEntry::embedded("Original", "/Libraries/Default/Original.pro", data.clone())
                .expect("valid original"),
        ];
    let document = build_playlist("Test", &original_entries, &test_metadata());
    let different_entries =
        vec![
            PlaylistEntry::embedded("Different", "/Libraries/Default/Original.pro", data)
                .expect("valid different entry"),
        ];
    let result = write_playlist_document_for_fidelity(
        &document,
        &different_entries,
        directory.path().join("mismatch.proplaylist"),
    );
    assert!(matches!(
        result,
        Err(PlaylistError::PackageItemMismatch {
            field: PlaylistItemContractField::Name,
            ..
        })
    ));
}

fn assert_rejects_presentation_contract_mutation(
    mutate: impl FnOnce(&mut rv_data::playlist_item::Presentation),
    expected_field: PlaylistItemContractField,
) {
    let directory = tempfile::tempdir().expect("tempdir");
    let entries = vec![embedded_entry(
        "Original",
        "/Libraries/Default/Original.pro",
    )];
    let mut document = build_playlist("Test", &entries, &test_metadata());
    let root = document.root_node.as_mut().expect("root playlist");
    let Some(playlist::ChildrenType::Playlists(children)) = &mut root.children_type else {
        panic!("root playlist children");
    };
    let Some(playlist::ChildrenType::Items(items)) = &mut children.playlists[0].children_type
    else {
        panic!("presentation items");
    };
    let Some(rv_data::playlist_item::ItemType::Presentation(presentation)) =
        &mut items.items[0].item_type
    else {
        panic!("presentation item");
    };
    mutate(presentation);

    let result = write_playlist_document_for_fidelity(
        &document,
        &entries,
        directory.path().join("mismatch.proplaylist"),
    );
    assert!(matches!(
        result,
        Err(PlaylistError::PackageItemMismatch { field, .. }) if field == expected_field
    ));
}

#[test]
fn rejects_playlist_item_with_wrong_document_platform() {
    assert_rejects_presentation_contract_mutation(
        |presentation| {
            presentation
                .document_path
                .as_mut()
                .expect("document path")
                .platform = rv_data::url::Platform::Win32 as i32;
        },
        PlaylistItemContractField::DocumentPlatform,
    );
}

#[test]
fn rejects_playlist_item_with_wrong_absolute_file_url() {
    assert_rejects_presentation_contract_mutation(
        |presentation| {
            presentation
                .document_path
                .as_mut()
                .expect("document path")
                .storage = Some(rv_data::url::Storage::AbsoluteString(
                "file:///Libraries/Default/Different.pro".to_string(),
            ));
        },
        PlaylistItemContractField::AbsoluteFileUrl,
    );
}

#[test]
fn rejects_playlist_item_with_non_global_content_destination() {
    assert_rejects_presentation_contract_mutation(
        |presentation| {
            presentation.content_destination =
                rv_data::action::ContentDestination::Announcements as i32;
        },
        PlaylistItemContractField::ContentDestination,
    );
}

#[test]
fn atomic_write_preserves_existing_file_on_media_error() {
    let directory = tempfile::tempdir().expect("tempdir");
    let output = directory.path().join("existing.proplaylist");
    std::fs::write(&output, b"known-good").expect("write existing output");
    let document = build_playlist("Test", &[], &test_metadata());
    let intent = PlaylistExportIntent::portable_import(vec![PlaylistMediaAsset::new(
        directory.path().join("missing.jpg"),
    )]);
    assert!(write_playlist_document_file_with_intent(&document, &[], &output, intent).is_err());
    assert_eq!(
        std::fs::read(&output).expect("read preserved output"),
        b"known-good"
    );
}

#[test]
fn archive_members_follow_native_order() {
    let directory = tempfile::tempdir().expect("tempdir");
    let media_path = directory.path().join("asset.jpg");
    std::fs::write(&media_path, b"asset").expect("write media");
    let media_archive_path = media_path
        .canonicalize()
        .expect("canonical media")
        .display()
        .to_string();
    let entries = ["First", "Second"]
        .into_iter()
        .map(|name| {
            PlaylistEntry::embedded(
                name,
                format!("/Libraries/Default/{name}.pro"),
                presentation_bytes(name),
            )
            .expect("valid ordered entry")
        })
        .collect::<Vec<_>>();
    let document = build_playlist("Ordered", &entries, &test_metadata());
    let intent = PlaylistExportIntent::portable_import(vec![PlaylistMediaAsset::new(media_path)]);
    let output = directory.path().join("ordered.proplaylist");
    write_playlist_document_file_with_intent(&document, &entries, &output, intent)
        .expect("write ordered package");

    let file = std::fs::File::open(output).expect("open package");
    let mut archive = zip::ZipArchive::new(file).expect("read zip");
    let names = (0..archive.len())
        .map(|index| archive.by_index(index).expect("member").name().to_string())
        .collect::<Vec<_>>();
    let mut expected = vec![
        "First.pro".to_string(),
        "Second.pro".to_string(),
        media_archive_path,
        "data".to_string(),
    ];
    expected.sort();
    assert_eq!(names, expected);
}
