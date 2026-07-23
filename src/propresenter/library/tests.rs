#![allow(clippy::expect_used)]

use std::fs;

use prost::Message;
use tempfile::tempdir;

use super::*;
use crate::propresenter::generated::rv_data;

fn native_presentation(name: &str) -> Vec<u8> {
    rv_data::Presentation {
        uuid: Some(rv_data::Uuid {
            string: format!("{name}-id"),
        }),
        name: name.to_string(),
        ..Default::default()
    }
    .encode_to_vec()
}

fn native_presentation_with_arrangements(name: &str) -> Vec<u8> {
    rv_data::Presentation {
        uuid: Some(rv_data::Uuid {
            string: format!("{name}-id"),
        }),
        name: name.to_string(),
        arrangements: vec![
            rv_data::presentation::Arrangement {
                uuid: Some(rv_data::Uuid {
                    string: "550e8400-e29b-41d4-a716-446655440000".to_string(),
                }),
                name: "Christmas Eve".to_string(),
                group_identifiers: vec![rv_data::Uuid {
                    string: "group-1".to_string(),
                }],
            },
            rv_data::presentation::Arrangement {
                uuid: Some(rv_data::Uuid {
                    string: "not-a-uuid".to_string(),
                }),
                name: "Broken".to_string(),
                group_identifiers: vec![rv_data::Uuid {
                    string: "group-1".to_string(),
                }],
            },
        ],
        cues: vec![rv_data::Cue {
            uuid: Some(rv_data::Uuid {
                string: "cue-1".to_string(),
            }),
            actions: vec![rv_data::Action {
                action_type_data: Some(rv_data::action::ActionTypeData::Slide(
                    rv_data::action::SlideType {
                        slide: Some(rv_data::action::slide_type::Slide::Presentation(
                            rv_data::PresentationSlide {
                                base_slide: Some(rv_data::Slide {
                                    size: Some(rv_data::graphics::Size {
                                        width: 1920.0,
                                        height: 1080.0,
                                    }),
                                    ..rv_data::Slide::default()
                                }),
                                ..rv_data::PresentationSlide::default()
                            },
                        )),
                    },
                )),
                ..rv_data::Action::default()
            }],
            ..rv_data::Cue::default()
        }],
        cue_groups: vec![rv_data::presentation::CueGroup {
            group: Some(rv_data::Group {
                uuid: Some(rv_data::Uuid {
                    string: "group-1".to_string(),
                }),
                ..rv_data::Group::default()
            }),
            cue_identifiers: vec![rv_data::Uuid {
                string: "cue-1".to_string(),
            }],
        }],
        ..Default::default()
    }
    .encode_to_vec()
}

#[test]
fn catalog_excludes_non_presentation_files_with_pro_extension() {
    let directory = tempdir().expect("create library dir");
    fs::write(
        directory.path().join("Native.pro"),
        native_presentation("Native"),
    )
    .expect("write native presentation");
    fs::write(directory.path().join("Archive.pro"), b"PK\x03\x04archive")
        .expect("write ZIP fixture");
    fs::write(directory.path().join("Fixture.pro"), b"{\"slides\":[]}")
        .expect("write JSON fixture");
    fs::write(
        directory.path().join("Playlist.pro"),
        rv_data::Playlist {
            uuid: Some(rv_data::Uuid {
                string: "playlist-id".to_string(),
            }),
            name: "Playlist".to_string(),
            r#type: rv_data::playlist::Type::Playlist as i32,
            ..Default::default()
        }
        .encode_to_vec(),
    )
    .expect("write playlist fixture");
    fs::write(directory.path().join("Marker.pro"), b"MOCKPRESENTATION")
        .expect("write marker fixture");
    fs::write(directory.path().join("Unknown.pro"), [0xff, 0x00]).expect("write binary fixture");

    let catalog = LibraryCatalog::build(directory.path()).expect("build library catalog");

    assert_eq!(catalog.entries().len(), 1);
    assert_eq!(catalog.entries()[0].file_name(), "Native");
}

#[test]
fn catalog_records_complete_and_incomplete_native_arrangements() {
    let directory = tempdir().expect("create library dir");
    fs::write(
        directory.path().join("Song.pro"),
        native_presentation_with_arrangements("Song"),
    )
    .expect("write native presentation");

    let catalog = LibraryCatalog::build(directory.path()).expect("build library catalog");

    assert_eq!(
        catalog.entries()[0].arrangements(),
        &[
            LibraryArrangement::Complete {
                name: "Christmas Eve".to_string(),
            },
            LibraryArrangement::Incomplete {
                name: "Broken".to_string(),
            },
        ]
    );
    assert_eq!(
        catalog.entries()[0].presentation_size(),
        PresentationSizeStatus::Uniform {
            size: crate::propresenter::PresentationSize::new(1920, 1080)
                .expect("valid full HD size"),
        }
    );
    let capabilities = catalog.entries()[0].transform_capabilities();
    assert!(capabilities.exact_editable());
    assert!(!capabilities.background_entries_editable());
    assert_eq!(
        capabilities
            .traversal(Some("Christmas Eve"))
            .map(LibraryTraversalCapability::cue_count),
        Some(1)
    );
}

#[test]
fn catalog_keeps_opaque_documents_searchable_but_not_editable() {
    let directory = tempdir().expect("create library dir");
    let mut bytes = native_presentation("Opaque");
    bytes.extend_from_slice(&[0xf8, 0x7f, 0x01]);
    fs::write(directory.path().join("Opaque.pro"), bytes).expect("write opaque presentation");

    let catalog = LibraryCatalog::build(directory.path()).expect("build library catalog");

    assert_eq!(catalog.entries().len(), 1);
    assert!(!catalog.entries()[0]
        .transform_capabilities()
        .exact_editable());
}

#[test]
fn stale_selection_without_arrangements_uses_checked_group_traversal_for_transforms() {
    let directory = tempdir().expect("create library dir");
    let mut presentation = rv_data::Presentation::decode(native_presentation("Stale").as_slice())
        .expect("decode fixture");
    presentation.selected_arrangement = Some(rv_data::Uuid {
        string: "stale-arrangement-id".to_string(),
    });
    presentation.cues = vec![rv_data::Cue {
        uuid: Some(rv_data::Uuid {
            string: "entry-cue".to_string(),
        }),
        ..rv_data::Cue::default()
    }];
    presentation.cue_groups = vec![rv_data::presentation::CueGroup {
        group: Some(rv_data::Group {
            uuid: Some(rv_data::Uuid {
                string: "group-id".to_string(),
            }),
            name: "Group".to_string(),
            ..rv_data::Group::default()
        }),
        cue_identifiers: vec![rv_data::Uuid {
            string: "entry-cue".to_string(),
        }],
    }];
    fs::write(
        directory.path().join("Stale.pro"),
        presentation.encode_to_vec(),
    )
    .expect("write stale selection fixture");

    let catalog = LibraryCatalog::build(directory.path()).expect("build library catalog");
    let capabilities = catalog.entries()[0].transform_capabilities();

    assert!(capabilities.exact_editable());
    assert!(capabilities.background_entries_editable());
    assert_eq!(
        capabilities
            .traversal(None)
            .map(LibraryTraversalCapability::cue_count),
        Some(1)
    );
}

#[test]
fn catalog_does_not_offer_arrangements_with_an_ambiguous_uuid() {
    let directory = tempdir().expect("create library dir");
    let mut presentation =
        rv_data::Presentation::decode(native_presentation_with_arrangements("Song").as_slice())
            .expect("decode fixture");
    let mut duplicate = presentation.arrangements[0].clone();
    duplicate.name = "Duplicate Identity".to_string();
    presentation.arrangements.push(duplicate);
    fs::write(
        directory.path().join("Song.pro"),
        presentation.encode_to_vec(),
    )
    .expect("write native presentation");

    let catalog = LibraryCatalog::build(directory.path()).expect("build library catalog");
    let arrangements = catalog.entries()[0].arrangements();

    assert!(!arrangements[0].is_complete());
    assert!(!arrangements[2].is_complete());
}

#[test]
fn prepared_update_uses_exact_bytes_and_preserves_the_original_snapshot() {
    let directory = tempdir().expect("create library dir");
    let path = directory.path().join("Song.pro");
    fs::write(&path, native_presentation("Song")).expect("write initial presentation");
    let catalog = LibraryCatalog::build(directory.path()).expect("build initial catalog");

    let exact_bytes = native_presentation_with_arrangements("Song");
    let prepared = catalog
        .prepare_owned_update(&path, &exact_bytes)
        .expect("prepare exact catalog metadata")
        .expect("path belongs to catalog");
    fs::write(&path, native_presentation("External Edit"))
        .expect("simulate unrelated filesystem edit");
    let updated = catalog
        .with_prepared_updates(&[prepared])
        .expect("apply prepared metadata");

    assert!(catalog.entries()[0].arrangements().is_empty());
    assert_eq!(updated.entries().len(), 1);
    assert_eq!(
        updated.entries()[0].arrangements(),
        &[
            LibraryArrangement::Complete {
                name: "Christmas Eve".to_string(),
            },
            LibraryArrangement::Incomplete {
                name: "Broken".to_string(),
            },
        ]
    );
    assert_eq!(
        updated.entries()[0].presentation_size(),
        PresentationSizeStatus::Uniform {
            size: crate::propresenter::PresentationSize::new(1920, 1080)
                .expect("valid full HD size"),
        }
    );
}

#[test]
fn malformed_exact_bytes_cannot_be_prepared() {
    let directory = tempdir().expect("create library dir");
    let catalog = LibraryCatalog::build(directory.path()).expect("build empty catalog");
    let path = directory.path().join("Broken.pro");

    let error = catalog
        .prepare_owned_update(&path, b"not a native presentation")
        .expect_err("malformed bytes must fail before commit");

    assert!(error.to_string().contains("not a native presentation"));
    assert!(catalog.entries().is_empty());
}

#[test]
fn catalog_ignores_prepared_outputs_outside_its_library() {
    let directory = tempdir().expect("create library dir");
    let external = tempdir().expect("create external dir");
    let catalog = LibraryCatalog::build(directory.path()).expect("build empty catalog");
    let path = external.path().join("External.pro");

    let update = catalog
        .prepare_owned_update(&path, &native_presentation("External"))
        .expect("outside paths are valid build outputs");

    assert!(
        update.is_none(),
        "foreign output must not enter this catalog"
    );
    assert!(catalog.entries().is_empty());
}

#[test]
fn duplicate_prepared_paths_are_rejected() {
    let directory = tempdir().expect("create library dir");
    let catalog = LibraryCatalog::build(directory.path()).expect("build empty catalog");
    let path = directory.path().join("Song.pro");
    let bytes = native_presentation("Song");
    let first = catalog
        .prepare_owned_update(&path, &bytes)
        .expect("prepare first update")
        .expect("path belongs to catalog");
    let second = catalog
        .prepare_owned_update(&path, &bytes)
        .expect("prepare second update")
        .expect("path belongs to catalog");

    let error = catalog
        .with_prepared_updates(&[first, second])
        .expect_err("duplicate updates must not be order-dependent");

    assert!(error.to_string().contains("duplicate prepared"));
    assert!(catalog.entries().is_empty());
}

#[test]
fn parent_components_cannot_lexically_smuggle_an_external_update_into_the_catalog() {
    let root = tempdir().expect("create root");
    let library = root.path().join("library");
    fs::create_dir(&library).expect("create library");
    let catalog = LibraryCatalog::build(&library).expect("build empty catalog");
    let disguised_external = library.join("nested/../../External.pro");

    let update = catalog
        .prepare_owned_update(&disguised_external, &native_presentation("External"))
        .expect("resolve candidate identity");

    assert!(update.is_none());
}

#[cfg(unix)]
#[test]
fn symlinked_library_alias_uses_the_same_catalog_identity() {
    let root = tempdir().expect("create root");
    let library = root.path().join("library");
    let alias = root.path().join("library-alias");
    fs::create_dir(&library).expect("create library");
    std::os::unix::fs::symlink(&library, &alias).expect("create library alias");
    let catalog = LibraryCatalog::build(&alias).expect("build catalog through alias");
    let target_through_alias = alias.join("Song.pro");

    let prepared = catalog
        .prepare_owned_update(
            &target_through_alias,
            &native_presentation_with_arrangements("Song"),
        )
        .expect("prepare through alias")
        .expect("alias target belongs to catalog");
    let updated = catalog
        .with_prepared_updates(&[prepared])
        .expect("install prepared update");

    assert!(updated.entry_at(&library.join("Song.pro")).is_some());
    assert!(updated.entry_at(&target_through_alias).is_some());
    assert_eq!(
        updated.entries()[0].full_path(),
        library
            .canonicalize()
            .expect("canonical library")
            .join("Song.pro")
    );
}
