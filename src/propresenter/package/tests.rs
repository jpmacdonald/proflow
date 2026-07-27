#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use super::*;
use crate::propresenter::generated::rv_data::{self, action, playlist, playlist_item};
use crate::propresenter::inspection::summarize_presentation_structure;
use crate::propresenter::media::presentation_media_dependencies;
use crate::propresenter::playlist::{
    build_playlist, write_playlist_document_file_with_intent, write_playlist_document_for_fidelity,
    write_playlist_set_file, PlaylistEntry, PlaylistExportIntent, PlaylistMediaAsset,
    PlaylistMetadata, PlaylistSet, SelectedArrangement,
};
use prost::Message;
use serde::Deserialize;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use tempfile::tempdir;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct RealFixtureManifest {
    playlists: Vec<RealPlaylistFixture>,
    presentations: Vec<RealPresentationFixture>,
}

#[derive(Debug, Deserialize)]
struct RealPlaylistFixture {
    path: String,
    provenance: String,
    #[serde(flatten)]
    evidence: RealFixtureEvidence,
    independent_native_export: bool,
    mode: PlaylistArchiveShape,
    item_count: usize,
    embedded_file_count: usize,
    media_file_count: usize,
    required_embedded_files: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RealPresentationFixture {
    path: String,
    provenance: String,
    #[serde(flatten)]
    evidence: RealFixtureEvidence,
    independent_native_export: bool,
    name: String,
    cue_count: usize,
    cue_group_count: usize,
    arrangement_count: usize,
    media_dependency_count: usize,
}

#[derive(Debug, Deserialize)]
struct RealFixtureEvidence {
    producer_version: String,
    operating_system: String,
    export_mode: String,
    covered_native_capabilities: Vec<String>,
}

fn assert_fixture_evidence(path: &str, evidence: &RealFixtureEvidence) {
    assert!(!evidence.producer_version.is_empty(), "{path}");
    assert!(!evidence.operating_system.is_empty(), "{path}");
    assert!(!evidence.export_mode.is_empty(), "{path}");
    assert!(!evidence.covered_native_capabilities.is_empty(), "{path}");
}

fn real_fixture_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/propresenter/native/corpus")
}

fn real_manifest() -> RealFixtureManifest {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/propresenter/native/corpus/manifest.json"
    ))
    .expect("real fixture manifest should parse")
}

fn presentation_bytes(name: &str) -> Vec<u8> {
    rv_data::Presentation {
        name: name.to_string(),
        uuid: Some(rv_data::Uuid {
            string: Uuid::new_v4().to_string(),
        }),
        ..rv_data::Presentation::default()
    }
    .encode_to_vec()
}

fn test_metadata() -> PlaylistMetadata {
    PlaylistMetadata::offline_test()
}

fn playlist_item(name: &str, local_relative_path: &str) -> PlaylistItemSummary {
    PlaylistItemSummary {
        item_uuid: None,
        name: name.to_string(),
        item_tags: Vec::new(),
        is_hidden: false,
        document_platform: None,
        absolute_string: None,
        storage_relative_path: None,
        local_relative_path: Some(local_relative_path.to_string()),
        local_root: Some(0),
        external_relative_path: None,
        arrangement_uuid: None,
        content_destination: 0,
        user_music_key: None,
        arrangement_name: String::new(),
    }
}

#[test]
fn absolute_path_normalization_ignores_windows_and_macos_machine_roots() {
    let windows = r"C:\Users\Operator\ProPresenter\Libraries\Default\Song.pro";
    let macos = "file:///Users/operator/ProPresenter/Libraries/Default/Song.pro";

    assert_eq!(
        normalize_absolute_path_value(windows),
        "Libraries/Default/Song.pro"
    );
    assert_eq!(
        normalize_absolute_path_value(macos),
        "Libraries/Default/Song.pro"
    );
}

#[test]
fn aligned_item_compare_does_not_cascade_after_missing_item() {
    let expected = vec![
        playlist_item("Prelude", "Libraries/Default/Prelude.pro"),
        playlist_item("Sermon", "Libraries/Default/5-10-26-SERMON.pro"),
        playlist_item("Prayer", "Libraries/Default/Prayer.pro"),
    ];
    let actual = vec![
        playlist_item("Prelude", "Libraries/Default/Prelude.pro"),
        playlist_item("Prayer", "Libraries/Default/Prayer.pro"),
    ];

    let diffs = compare_playlist_items_aligned(&expected, &actual);

    assert_eq!(diffs.len(), 1);
    assert_eq!(diffs[0].kind, "missing_item_aligned");
    assert_eq!(diffs[0].expected_name.as_deref(), Some("Sermon"));
}

#[test]
fn aligned_item_compare_reports_real_reorders() {
    let expected = vec![
        playlist_item("Prelude", "Libraries/Default/Prelude.pro"),
        playlist_item("Prayer", "Libraries/Default/Prayer.pro"),
    ];
    let actual = vec![
        playlist_item("Prayer", "Libraries/Default/Prayer.pro"),
        playlist_item("Prelude", "Libraries/Default/Prelude.pro"),
    ];

    let diffs = compare_playlist_items_aligned(&expected, &actual);

    assert!(diffs.iter().any(|diff| diff.kind == "moved_item_aligned"
        && diff.expected_name.as_deref() == Some("Prelude")));
    assert!(diffs
        .iter()
        .any(|diff| diff.kind == "moved_item_aligned"
            && diff.expected_name.as_deref() == Some("Prayer")));
}

#[test]
fn semantic_comparison_reports_scripture_label_and_group_binding_changes() {
    let expected = summarize_presentation_structure(&presentation_with_semantic_metadata());
    let mut changed_presentation = presentation_with_semantic_metadata();
    changed_presentation
        .bible_reference
        .as_mut()
        .expect("Bible reference")
        .verse_range
        .as_mut()
        .expect("verse range")
        .end = 18;
    changed_presentation.cues[0].actions[0]
        .label
        .as_mut()
        .expect("slide label")
        .text = "John 3:16-18".to_string();
    changed_presentation.cue_groups[0]
        .group
        .as_mut()
        .expect("cue group")
        .application_group_name = "Scripture".to_string();
    let actual = summarize_presentation_structure(&changed_presentation);
    let mut issues = Vec::new();

    compare_presentation_structure_summary("Scripture.pro", &expected, &actual, &mut issues);

    assert!(issues
        .iter()
        .any(|issue| { issue.kind == "embedded_presentation_bible_reference_mismatch" }));
    assert!(issues
        .iter()
        .any(|issue| issue.kind == "embedded_presentation_group_binding_mismatch"));
    assert!(issues
        .iter()
        .any(|issue| issue.kind == "embedded_presentation_operator_cue_mismatch"));
}

#[test]
fn semantic_comparison_reports_reference_diagnostics_instead_of_hiding_dangling_ids() {
    let presentation = presentation_with_semantic_metadata();
    let expected = summarize_presentation_structure(&presentation);
    let mut malformed = presentation;
    malformed.cue_groups[0].cue_identifiers.push(rv_data::Uuid {
        string: "missing-cue".to_string(),
    });
    let actual = summarize_presentation_structure(&malformed);
    let mut issues = Vec::new();

    compare_presentation_structure_summary("Scripture.pro", &expected, &actual, &mut issues);

    assert!(issues
        .iter()
        .any(|issue| { issue.kind == "embedded_presentation_reference_diagnostics_mismatch" }));
}

fn presentation_with_semantic_metadata() -> rv_data::Presentation {
    let cue_uuid = rv_data::Uuid {
        string: "CUE".to_string(),
    };
    rv_data::Presentation {
        name: "John 3:16-17".to_string(),
        bible_reference: Some(rv_data::presentation::BibleReference {
            book_index: 42,
            book_name: "John".to_string(),
            chapter_range: Some(rv_data::IntRange { start: 3, end: 3 }),
            verse_range: Some(rv_data::IntRange { start: 16, end: 17 }),
            translation_name: "New Revised Standard Version Updated Edition".to_string(),
            translation_display_abbreviation: "NRSVue".to_string(),
            translation_internal_abbreviation: "NRSVUE".to_string(),
            book_key: "JHN".to_string(),
        }),
        cues: vec![rv_data::Cue {
            uuid: Some(cue_uuid.clone()),
            actions: vec![rv_data::Action {
                label: Some(action::Label {
                    text: "John 3:16-17".to_string(),
                    color: Some(rv_data::Color {
                        red: 1.0,
                        green: 0.0,
                        blue: 0.0,
                        alpha: 1.0,
                    }),
                }),
                action_type_data: Some(action::ActionTypeData::Slide(action::SlideType::default())),
                ..rv_data::Action::default()
            }],
            ..rv_data::Cue::default()
        }],
        cue_groups: vec![rv_data::presentation::CueGroup {
            group: Some(rv_data::Group {
                uuid: Some(rv_data::Uuid {
                    string: "GROUP".to_string(),
                }),
                name: "Verse".to_string(),
                color: Some(rv_data::Color {
                    red: 0.25,
                    green: 0.5,
                    blue: 0.75,
                    alpha: 1.0,
                }),
                hot_key: Some(rv_data::HotKey {
                    code: rv_data::KeyCode::AnsiV as i32,
                    control_identifier: "verse".to_string(),
                }),
                application_group_identifier: Some(rv_data::Uuid {
                    string: "APPLICATION-GROUP".to_string(),
                }),
                application_group_name: "Verse".to_string(),
            }),
            cue_identifiers: vec![cue_uuid],
        }],
        ..rv_data::Presentation::default()
    }
}

#[test]
fn reads_generated_playlist_package() {
    let dir = tempdir().expect("tempdir");
    let output_path = dir.path().join("service.proplaylist");
    let entries = vec![PlaylistEntry::embedded(
        "Call to Worship",
        "/Users/jimmy/Documents/ProPresenter/Libraries/Default/Call to Worship.pro",
        presentation_bytes("Call to Worship"),
    )
    .expect("valid entry")];

    let document = build_playlist("Service", &entries, &test_metadata());
    write_playlist_document_for_fidelity(&document, &entries, &output_path)
        .expect("write playlist");

    let package = read_playlist_package(&output_path).expect("read package");
    assert_eq!(
        infer_archive_shape(&package),
        PlaylistArchiveShape::PresentationsOnly
    );
    assert_eq!(
        package.embedded_files().collect::<Vec<_>>(),
        ["Call to Worship.pro"]
    );
    let embedded_details = package.embedded_file_details().collect::<Vec<_>>();
    assert_eq!(embedded_details.len(), 1);
    assert_eq!(embedded_details[0].basename, "Call to Worship.pro");
    assert!(embedded_details[0].is_presentation);

    let items = presentation_items(package.document());
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, "Call to Worship");
    assert_eq!(
        items[0].local_relative_path.as_deref(),
        Some("Libraries/Default/Call to Worship.pro")
    );
}

#[test]
fn reads_native_unflagged_utf8_member_names_without_mojibake() {
    let directory = tempdir().expect("tempdir");
    let output_path = directory.path().join("unicode.proplaylist");
    let entries = vec![PlaylistEntry::embedded(
        "O Praise The Name (Anástasis)",
        "/Libraries/Default/O Praise The Name (Anástasis).pro",
        presentation_bytes("O Praise The Name (Anástasis)"),
    )
    .expect("valid unicode entry")];
    let document = build_playlist("Service", &entries, &test_metadata());
    write_playlist_document_for_fidelity(&document, &entries, &output_path)
        .expect("write playlist");

    let package = read_playlist_package(output_path).expect("read playlist");

    assert_eq!(
        package.embedded_files().collect::<Vec<_>>(),
        ["O Praise The Name (Anástasis).pro"]
    );
    assert!(package.has_embedded_file("O Praise The Name (Anástasis).pro"));
}

#[test]
fn rejects_package_with_malformed_embedded_presentation() {
    let directory = tempdir().expect("tempdir");
    let path = directory.path().join("malformed.proplaylist");
    let file = File::create(&path).expect("create package");
    let mut archive = zip::ZipWriter::new(file);
    let options = zip::write::FileOptions::default();
    archive
        .start_file("Broken.pro", options)
        .expect("start presentation");
    archive.write_all(&[1, 2, 3]).expect("write malformed");
    archive.start_file("data", options).expect("start data");
    archive
        .write_all(&build_playlist("Empty", &[], &test_metadata()).encode_to_vec())
        .expect("write data");
    archive.finish().expect("finish package");

    let result = read_playlist_package(path);

    assert!(matches!(
        result,
        Err(PackageError::InvalidEmbeddedPresentation { name, .. }) if name == "Broken.pro"
    ));
}

#[test]
fn rejects_package_with_identityless_embedded_presentation() {
    let directory = tempdir().expect("tempdir");
    let path = directory.path().join("identityless.proplaylist");
    let file = File::create(&path).expect("create package");
    let mut archive = zip::ZipWriter::new(file);
    let options = zip::write::FileOptions::default();
    archive
        .start_file("Identityless.pro", options)
        .expect("start presentation");
    archive.start_file("data", options).expect("start data");
    archive
        .write_all(&build_playlist("Empty", &[], &test_metadata()).encode_to_vec())
        .expect("write data");
    archive.finish().expect("finish package");

    let result = read_playlist_package(path);

    assert!(matches!(
        result,
        Err(PackageError::InvalidEmbeddedPresentation { name, .. })
            if name == "Identityless.pro"
    ));
}

#[test]
fn reads_checked_in_propresenter_fixture() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "tests/fixtures/propresenter/native/corpus/playlists/native-template-library.proplaylist",
    );

    let package = read_playlist_package(fixture).expect("read fixture package");
    assert!(package.document_round_trip_is_exact());
    assert_eq!(
        infer_archive_shape(&package),
        PlaylistArchiveShape::PresentationsOnly
    );
    assert_eq!(
        package.embedded_files().collect::<Vec<_>>(),
        vec![
            "__template_info__.pro",
            "__template_scripture__.pro",
            "__template_song__.pro"
        ]
    );
    assert_eq!(
        package
            .embedded_file_details()
            .map(|file| (file.basename.as_str(), file.size, file.crc32))
            .collect::<Vec<_>>(),
        vec![
            ("__template_info__.pro", 1731, 0x0232_052d),
            ("__template_scripture__.pro", 1354, 0xc8a6_509b),
            ("__template_song__.pro", 1705, 0x8040_52a0),
        ]
    );

    let items = presentation_items(package.document());
    assert_eq!(items.len(), 3);
    assert_eq!(
        items[0].local_relative_path.as_deref(),
        Some("Libraries/Default/__template_scripture__.pro")
    );
    assert!(items[0]
        .absolute_string
        .as_deref()
        .is_some_and(|path| path.starts_with("file:///Users/jimmy/")));
}

#[test]
fn real_fixture_manifest_matches_corpus() {
    let fixture_dir = real_fixture_dir();
    let manifest = real_manifest();

    for fixture in manifest.playlists {
        assert_fixture_evidence(&fixture.path, &fixture.evidence);
        if fixture.independent_native_export {
            assert_eq!(fixture.provenance, "independent_native_export");
        } else {
            assert_eq!(
                fixture.provenance,
                "proflow_reconstruction_from_live_library"
            );
        }
        let path = fixture_dir.join(&fixture.path);
        let package = read_playlist_package(&path).expect("read real playlist fixture");
        let items = presentation_items(package.document());

        assert!(
            package.document_round_trip_is_exact(),
            "{} playlist data should round-trip byte-for-byte",
            fixture.path
        );

        assert_eq!(
            infer_archive_shape(&package),
            fixture.mode,
            "{}",
            fixture.path
        );
        assert_eq!(items.len(), fixture.item_count, "{}", fixture.path);
        assert_eq!(
            package.embedded_file_count(),
            fixture.embedded_file_count,
            "{}",
            fixture.path
        );
        assert_eq!(
            package
                .embedded_file_details()
                .filter(|file| !file.is_presentation)
                .count(),
            fixture.media_file_count,
            "{}",
            fixture.path
        );
        for required in &fixture.required_embedded_files {
            assert!(
                package.embedded_files().any(|name| name == required),
                "{} should contain {required}",
                fixture.path
            );
        }
        assert_eq!(
            embedded_presentation_summaries(&package).len(),
            fixture.embedded_file_count - fixture.media_file_count,
            "{}",
            fixture.path
        );
    }

    for fixture in manifest.presentations {
        assert_eq!(fixture.provenance, "native_library_file");
        assert!(fixture.independent_native_export);
        assert_fixture_evidence(&fixture.path, &fixture.evidence);
        assert_eq!(fixture.evidence.export_mode, "library_document");
        let data = std::fs::read(fixture_dir.join(&fixture.path)).expect("read presentation");
        let presentation = rv_data::Presentation::decode(data.as_slice())
            .expect("decode real presentation fixture");
        let media_dependencies = presentation_media_dependencies(&presentation);

        assert_eq!(presentation.name, fixture.name, "{}", fixture.path);
        assert_eq!(
            presentation.cues.len(),
            fixture.cue_count,
            "{}",
            fixture.path
        );
        assert_eq!(
            presentation.cue_groups.len(),
            fixture.cue_group_count,
            "{}",
            fixture.path
        );
        assert_eq!(
            presentation.arrangements.len(),
            fixture.arrangement_count,
            "{}",
            fixture.path
        );
        assert_eq!(
            media_dependencies.len(),
            fixture.media_dependency_count,
            "{}",
            fixture.path
        );
    }
}

#[test]
fn compare_identical_package_is_compatible() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "tests/fixtures/propresenter/native/corpus/playlists/native-template-library.proplaylist",
    );

    let comparison =
        compare_playlist_packages(&fixture, &fixture).expect("compare identical fixture");

    assert!(comparison.compatible);
    assert!(comparison.issues.is_empty());
    assert_eq!(comparison.expected_item_count, 3);
    assert_eq!(comparison.actual_item_count, 3);
}

#[test]
fn native_package_reconstruction_matches_evidenced_shape() {
    let expected_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "tests/fixtures/propresenter/native/corpus/playlists/native-template-library.proplaylist",
    );
    let expected = read_playlist_package(&expected_path).expect("read native package");
    let entries = presentation_items(expected.document())
        .into_iter()
        .map(|item| {
            let relative_path = item
                .local_relative_path
                .as_deref()
                .expect("native item local path");
            let filename = Path::new(relative_path)
                .file_name()
                .and_then(|name| name.to_str())
                .expect("native item filename");
            let selected_arrangement = item.arrangement_uuid.as_deref().map(|uuid| {
                SelectedArrangement::new(
                    Uuid::parse_str(uuid).expect("valid arrangement UUID"),
                    item.arrangement_name.clone(),
                )
                .expect("complete arrangement metadata")
            });
            let user_music_key =
                item.user_music_key
                    .map(|(music_key, music_scale)| rv_data::MusicKeyScale {
                        music_key,
                        music_scale,
                    });
            let path = format!("/Users/test/ProPresenter/{relative_path}");
            let entry = expected
                .embedded_file(filename)
                .map_or_else(
                    || PlaylistEntry::linked(item.name.clone(), path.clone()),
                    |data| PlaylistEntry::embedded(item.name.clone(), path.clone(), data.to_vec()),
                )
                .expect("valid reconstructed entry");
            entry
                .with_selected_arrangement(selected_arrangement)
                .expect("embedded arrangement resolves")
                .with_user_music_key(user_music_key)
        })
        .collect::<Vec<_>>();
    let metadata =
        PlaylistMetadata::from_document(expected.document()).expect("native playlist metadata");
    let reconstructed = PlaylistSet::single("test", entries).expect("valid playlist set");
    let directory = tempdir().expect("tempdir");
    let actual_path = directory.path().join("reconstructed.proplaylist");
    write_playlist_set_file(
        &reconstructed,
        &metadata,
        &actual_path,
        PlaylistExportIntent::portable_import(Vec::new()),
    )
    .expect("write reconstruction through the production package boundary");

    let comparison =
        compare_playlist_packages(&expected_path, &actual_path).expect("compare packages");

    assert!(comparison.compatible, "{:#?}", comparison.issues);
    assert!(comparison.issues.is_empty());
}

#[test]
fn compare_detects_complete_presentation_item_metadata() {
    let directory = tempdir().expect("tempdir");
    let expected_path = directory.path().join("expected.proplaylist");
    let actual_path = directory.path().join("actual.proplaylist");
    let entries = vec![PlaylistEntry::embedded(
        "Song",
        "/Libraries/Default/Song.pro",
        presentation_bytes("Song"),
    )
    .expect("valid song")];
    let expected = build_playlist("Service", &entries, &test_metadata());
    let mut actual = expected.clone();
    let root = actual.root_node.as_mut().expect("root");
    let Some(playlist::ChildrenType::Playlists(playlists)) = &mut root.children_type else {
        panic!("playlist children");
    };
    let Some(playlist::ChildrenType::Items(items)) = &mut playlists.playlists[0].children_type
    else {
        panic!("playlist items");
    };
    let Some(playlist_item::ItemType::Presentation(presentation)) = &mut items.items[0].item_type
    else {
        panic!("presentation item");
    };
    let user_music_key = rv_data::MusicKeyScale {
        music_key: rv_data::music_key_scale::MusicKey::D as i32,
        music_scale: rv_data::music_key_scale::MusicScale::Minor as i32,
    };
    presentation.user_music_key = Some(user_music_key.clone());
    let actual_entries = vec![entries[0].clone().with_user_music_key(Some(user_music_key))];

    write_playlist_document_for_fidelity(&expected, &entries, &expected_path)
        .expect("write expected");
    write_playlist_document_for_fidelity(&actual, &actual_entries, &actual_path)
        .expect("write actual");
    let comparison =
        compare_playlist_packages(&expected_path, &actual_path).expect("compare packages");

    assert!(!comparison.compatible);
    assert!(comparison
        .issues
        .iter()
        .any(|issue| issue.kind == "playlist_root_mismatch"));
    assert!(comparison
        .issues
        .iter()
        .any(|issue| issue.kind == "item_music_key_mismatch"));
}

#[test]
fn native_playlist_reconstruction_is_not_mislabeled_compatible() {
    let fixture_dir = real_fixture_dir();
    for fixture in real_manifest()
        .playlists
        .into_iter()
        .filter(|fixture| !fixture.independent_native_export)
    {
        let expected_path = fixture_dir.join(&fixture.path);
        let expected = read_playlist_package(&expected_path).expect("read real fixture");
        let items = presentation_items(expected.document());
        let presentation_files: Vec<_> = expected
            .embedded_file_details()
            .filter(|file| file.is_presentation)
            .collect();
        assert_eq!(items.len(), presentation_files.len(), "{}", fixture.path);

        let entries: Vec<_> = items
            .iter()
            .zip(presentation_files.iter())
            .map(|(item, file)| {
                let presentation_path = item
                    .local_relative_path
                    .as_ref()
                    .map(|path| format!("/Users/jimmy/Documents/ProPresenter/{path}"))
                    .or_else(|| item.absolute_string.clone())
                    .expect("fixture presentation path");
                // These legacy reconstructed fixtures can contain the old
                // UUID-without-name bug. The typed entry deliberately
                // cannot restate that partial metadata, and this test
                // already requires the reconstruction to compare unequal.
                let selected_arrangement = item
                    .arrangement_uuid
                    .as_deref()
                    .filter(|_| !item.arrangement_name.trim().is_empty())
                    .map(|uuid| {
                        SelectedArrangement::new(
                            Uuid::parse_str(uuid).expect("valid arrangement UUID"),
                            item.arrangement_name.clone(),
                        )
                        .expect("complete arrangement metadata")
                    });
                let user_music_key =
                    item.user_music_key
                        .map(|(music_key, music_scale)| rv_data::MusicKeyScale {
                            music_key,
                            music_scale,
                        });
                let entry = expected
                    .embedded_file(&file.name)
                    .map_or_else(
                        || PlaylistEntry::linked(item.name.clone(), presentation_path.clone()),
                        |data| {
                            PlaylistEntry::embedded(
                                item.name.clone(),
                                presentation_path.clone(),
                                data.to_vec(),
                            )
                        },
                    )
                    .expect("valid fixture entry");
                entry
                    .with_selected_arrangement(selected_arrangement)
                    .expect("embedded arrangement resolves")
                    .with_user_music_key(user_music_key)
            })
            .collect();

        let metadata = PlaylistMetadata::from_document(expected.document())
            .expect("fixture playlist metadata");
        let playlist = build_playlist("Round Trip", &entries, &metadata);
        let dir = tempdir().expect("tempdir");
        let output_name = Path::new(&fixture.path)
            .file_name()
            .expect("fixture filename");
        let actual_path = dir.path().join(output_name);
        write_playlist_document_for_fidelity(&playlist, &entries, &actual_path)
            .expect("write round trip");

        let comparison =
            compare_playlist_packages(&expected_path, &actual_path).expect("compare packages");
        assert!(!comparison.compatible, "{}", fixture.path);
        assert!(comparison
            .issues
            .iter()
            .any(|issue| issue.kind == "playlist_root_mismatch"));
    }
}

#[test]
fn compare_reports_embedded_presentation_crc_mismatch() {
    let dir = tempdir().expect("tempdir");
    let expected_path = dir.path().join("expected.proplaylist");
    let actual_path = dir.path().join("actual.proplaylist");
    let path = "/Users/jimmy/Documents/ProPresenter/Libraries/Default/Call to Worship.pro";
    let entries =
        vec![
            PlaylistEntry::embedded("Call to Worship", path, presentation_bytes("Expected"))
                .expect("valid expected entry"),
        ];

    let document = build_playlist("Service", &entries, &test_metadata());
    write_playlist_document_for_fidelity(&document, &entries, &expected_path)
        .expect("write expected");
    let actual_entries =
        vec![
            PlaylistEntry::embedded("Call to Worship", path, presentation_bytes("Actual"))
                .expect("valid actual entry"),
        ];
    let document = build_playlist("Service", &actual_entries, &test_metadata());
    write_playlist_document_for_fidelity(&document, &actual_entries, &actual_path)
        .expect("write actual");

    let comparison =
        compare_playlist_packages(&expected_path, &actual_path).expect("compare packages");

    assert!(!comparison.compatible);
    assert!(comparison
        .issues
        .iter()
        .any(|issue| issue.kind == "embedded_presentation_crc_mismatch"));
}

#[test]
fn compare_reports_media_content_mismatch_at_same_archive_path() {
    let dir = tempdir().expect("tempdir");
    let expected_media_dir = dir.path().join("expected-media");
    let actual_media_dir = dir.path().join("actual-media");
    std::fs::create_dir_all(&expected_media_dir).expect("create expected media directory");
    std::fs::create_dir_all(&actual_media_dir).expect("create actual media directory");
    let expected_media = expected_media_dir.join("default.jpg");
    let actual_media = actual_media_dir.join("default.jpg");
    std::fs::write(&expected_media, [1, 2, 3]).expect("write expected media");
    std::fs::write(&actual_media, [1, 2, 4]).expect("write actual media");

    let document = build_playlist("Service", &[], &test_metadata());
    let expected_path = dir.path().join("expected.proplaylist");
    let actual_path = dir.path().join("actual.proplaylist");
    let intent_for = |source_path| {
        PlaylistExportIntent::portable_import(vec![PlaylistMediaAsset {
            source_path,
            archive_path: Some("media/default.jpg".to_string()),
        }])
    };
    write_playlist_document_file_with_intent(
        &document,
        &[],
        &expected_path,
        intent_for(expected_media),
    )
    .expect("write expected package");
    write_playlist_document_file_with_intent(
        &document,
        &[],
        &actual_path,
        intent_for(actual_media),
    )
    .expect("write actual package");

    let comparison =
        compare_playlist_packages(&expected_path, &actual_path).expect("compare packages");

    assert!(!comparison.compatible);
    assert!(comparison
        .issues
        .iter()
        .any(|issue| issue.kind == "media_asset_fingerprint_mismatch"));
}
