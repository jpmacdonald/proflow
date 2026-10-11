#![allow(clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use prost::Message;

use super::*;
use crate::generated::rv_data::{self, playlist, playlist_item, url};
use crate::inspection::PresentationReferenceDiagnostic;

const GOLDEN_SCHEMA: &str = "tests/fixtures/playlist-document.schema.json";
const BLESS_VARIABLE: &str = "PROFLOW_BLESS_PLAYLIST_DOCUMENT_SCHEMA";

fn crate_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn corpus(relative: &str) -> PathBuf {
    crate_path("tests/fixtures/native/corpus").join(relative)
}

fn playlist_corpus() -> Vec<PathBuf> {
    let mut paths: Vec<_> = std::fs::read_dir(corpus("playlists"))
        .expect("playlist corpus")
        .map(|entry| entry.expect("corpus entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "proplaylist"))
        .collect();
    paths.sort();
    paths
}

fn as_json(document: &PlaylistDocument) -> serde_json::Value {
    serde_json::to_value(document).expect("document serializes")
}

fn schema_json() -> serde_json::Value {
    serde_json::to_value(playlist_document_schema()).expect("schema serializes")
}

fn first_presentation(document: &PlaylistDocument) -> &PresentationItem {
    let ItemDetail::Presentation(item) = &document.playlists[0].items[0].detail else {
        panic!("first item is not a presentation");
    };
    item
}

// ---------------------------------------------------------------------------
// Schema
// ---------------------------------------------------------------------------

#[test]
fn schema_matches_the_checked_in_golden() {
    let current = format!(
        "{}\n",
        serde_json::to_string_pretty(&schema_json()).expect("pretty schema")
    );
    let golden_path = crate_path(GOLDEN_SCHEMA);
    if std::env::var_os(BLESS_VARIABLE).is_some() {
        std::fs::write(&golden_path, &current).expect("bless golden schema");
        return;
    }
    let golden = std::fs::read_to_string(&golden_path).expect("golden schema");
    assert!(
        golden == current,
        "the playlist document schema changed. If intentional, bump \
         PLAYLIST_DOCUMENT_SCHEMA_VERSION when readers could misread the new shape, \
         then regenerate with {BLESS_VARIABLE}=1 and review the diff"
    );
}

#[test]
fn schema_pins_the_version_constant() {
    let schema = schema_json();
    assert_eq!(
        schema["properties"]["schema_version"]["$ref"],
        "#/$defs/SchemaVersion"
    );
    assert_eq!(
        schema["$defs"]["SchemaVersion"]["const"],
        PLAYLIST_DOCUMENT_SCHEMA_VERSION
    );
    assert_eq!(
        SchemaVersion::current().get(),
        PLAYLIST_DOCUMENT_SCHEMA_VERSION
    );
}

#[test]
fn every_fixture_playlist_produces_a_document_that_validates_against_the_schema() {
    let validator = jsonschema::validator_for(&schema_json()).expect("schema compiles");
    let playlists = playlist_corpus();
    assert_eq!(playlists.len(), 4, "the corpus changed; review this test");
    for path in playlists {
        let document = read_package_playlist_document(&path).expect("fixture reads");
        let instance = as_json(&document);
        let errors: Vec<_> = validator
            .iter_errors(&instance)
            .map(|error| error.to_string())
            .collect();
        assert!(errors.is_empty(), "{}: {errors:?}", path.display());
        assert_eq!(instance["schema_version"], PLAYLIST_DOCUMENT_SCHEMA_VERSION);
    }
}

#[test]
fn the_schema_rejects_a_document_with_a_wrong_version_or_unknown_kind() {
    let validator = jsonschema::validator_for(&schema_json()).expect("schema compiles");
    let document =
        read_package_playlist_document(&corpus("playlists/native-template-library.proplaylist"))
            .expect("fixture reads");
    let mut wrong_version = as_json(&document);
    wrong_version["schema_version"] = serde_json::json!(2);
    assert!(!validator.is_valid(&wrong_version));
    let mut unknown_kind = as_json(&document);
    unknown_kind["playlists"][0]["items"][0]["kind"] = serde_json::json!("slideshow");
    assert!(!validator.is_valid(&unknown_kind));
}

// ---------------------------------------------------------------------------
// Packages
// ---------------------------------------------------------------------------

#[test]
fn every_fixture_item_is_present_in_order_and_readable() {
    for path in playlist_corpus() {
        let package = crate::package::read_playlist_package(&path).expect("package reads");
        let expected: Vec<_> = crate::package::presentation_items(package.document())
            .into_iter()
            .map(|item| item.name)
            .collect();
        let document = playlist_document_from_package("fixture.proplaylist", &package)
            .expect("document builds");
        let described: Vec<_> = document
            .playlists
            .iter()
            .flat_map(|playlist| &playlist.items)
            .map(|item| item.name.clone())
            .collect();
        assert_eq!(described, expected, "{}", path.display());
        for item in document
            .playlists
            .iter()
            .flat_map(|playlist| &playlist.items)
        {
            let ItemDetail::Presentation(presentation) = &item.detail else {
                panic!("fixtures hold only presentation items");
            };
            assert!(
                matches!(presentation.content, PresentationContent::Available(_)),
                "{}: {} is unavailable: {:?}",
                path.display(),
                item.name,
                presentation.content
            );
        }
    }
}

#[test]
fn template_library_fixture_reports_identity_locator_and_content() {
    let document =
        read_package_playlist_document(&corpus("playlists/native-template-library.proplaylist"))
            .expect("fixture reads");

    assert_eq!(
        document.source,
        DocumentSource::Package {
            file_name: "native-template-library.proplaylist".to_string()
        }
    );
    assert_eq!(document.playlists.len(), 1);
    let playlist = &document.playlists[0];
    assert_eq!(playlist.name, "test");
    assert!(playlist.folders.is_empty());
    let names: Vec<_> = playlist
        .items
        .iter()
        .map(|item| item.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "__template_scripture__",
            "__template_info__",
            "__template_song__"
        ]
    );
    assert_eq!(
        playlist
            .items
            .iter()
            .map(|item| item.index)
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );

    let first = first_presentation(&document);
    assert_eq!(
        first.source,
        PresentationLocator::ShowRelative {
            path: "Libraries/Default/__template_scripture__.pro".to_string()
        }
    );
    assert_eq!(first.arrangement, ArrangementChoice::PresentationDefault);
    let PresentationContent::Available(summary) = &first.content else {
        panic!("template presentation should be readable");
    };
    assert_eq!(summary.name, "__template_scripture__");
    assert_eq!(summary.slide_count, 1);
    assert!(first.problems.is_empty());
}

#[test]
fn easter_fixture_reports_non_show_roots_arrangements_and_structure_problems() {
    let document = read_package_playlist_document(&corpus(
        "playlists/native-easter-portable-media.proplaylist",
    ))
    .expect("fixture reads");
    let presentations: Vec<_> = document
        .playlists
        .iter()
        .flat_map(|playlist| &playlist.items)
        .filter_map(|item| match &item.detail {
            ItemDetail::Presentation(presentation) => Some(presentation),
            _ => None,
        })
        .collect();
    assert_eq!(presentations.len(), 17);
    assert!(presentations
        .iter()
        .all(|item| matches!(item.source, PresentationLocator::OtherRelative { .. })));
    let summaries: Vec<_> = presentations
        .iter()
        .filter_map(|item| match &item.content {
            PresentationContent::Available(summary) => Some(summary),
            PresentationContent::Unavailable { .. } => None,
        })
        .collect();
    let dangling = summaries
        .iter()
        .flat_map(|summary| &summary.structure_problems)
        .filter(|problem| {
            matches!(
                problem,
                PresentationReferenceDiagnostic::DanglingSelectedArrangement { .. }
            )
        })
        .count();
    assert_eq!(dangling, 1);
    assert!(presentations
        .iter()
        .any(|item| matches!(item.arrangement, ArrangementChoice::Selected { .. })));
}

/// Copy a fixture package, dropping one archive member.
fn package_without_member(source: &Path, dropped: &str, destination: &Path) {
    let mut archive =
        zip::ZipArchive::new(std::fs::File::open(source).expect("open fixture")).expect("zip");
    let mut writer = zip::ZipWriter::new(std::fs::File::create(destination).expect("create"));
    for index in 0..archive.len() {
        let mut member = archive.by_index(index).expect("member");
        if member.name() == dropped {
            continue;
        }
        let name = member.name().to_string();
        writer
            .start_file(name, zip::write::FileOptions::default())
            .expect("start member");
        std::io::copy(&mut member, &mut writer).expect("copy member");
    }
    writer.finish().expect("finish zip");
}

#[test]
fn a_package_missing_an_embedded_presentation_reports_it_on_the_item() {
    let directory = tempfile::tempdir().expect("tempdir");
    let damaged = directory.path().join("damaged.proplaylist");
    package_without_member(
        &corpus("playlists/native-template-library.proplaylist"),
        "__template_info__.pro",
        &damaged,
    );

    let document = read_package_playlist_document(&damaged).expect("document still builds");
    let items = &document.playlists[0].items;
    assert_eq!(
        items.len(),
        3,
        "the item with a missing file is not dropped"
    );
    let ItemDetail::Presentation(missing) = &items[1].detail else {
        panic!("presentation item");
    };
    assert_eq!(
        missing.content,
        PresentationContent::Unavailable {
            reason: UnavailableReason::EmbeddedFileMissing {
                file_name: "__template_info__.pro".to_string()
            }
        }
    );
    let ItemDetail::Presentation(intact) = &items[0].detail else {
        panic!("presentation item");
    };
    assert!(matches!(intact.content, PresentationContent::Available(_)));
}

#[test]
fn an_unreadable_package_is_a_typed_error() {
    let directory = tempfile::tempdir().expect("tempdir");
    let bogus = directory.path().join("bogus.proplaylist");
    std::fs::write(&bogus, b"not a zip").expect("write");
    assert!(matches!(
        read_package_playlist_document(&bogus),
        Err(PlaylistDocumentError::Package { .. })
    ));
    assert!(matches!(
        read_package_playlist_document(&directory.path().join("absent.proplaylist")),
        Err(PlaylistDocumentError::Package { .. })
    ));
}

// ---------------------------------------------------------------------------
// Live library
// ---------------------------------------------------------------------------

fn uuid(value: &str) -> rv_data::Uuid {
    rv_data::Uuid {
        string: value.to_string(),
    }
}

fn show_url(path: &str) -> rv_data::Url {
    rv_data::Url {
        relative_file_path: Some(url::RelativeFilePath::Local(url::LocalRelativePath {
            root: url::local_relative_path::Root::Show as i32,
            path: path.to_string(),
        })),
        ..rv_data::Url::default()
    }
}

fn absolute_url(path: &Path) -> rv_data::Url {
    rv_data::Url {
        storage: Some(url::Storage::AbsoluteString(format!(
            "file://{}",
            path.display()
        ))),
        ..rv_data::Url::default()
    }
}

fn presentation_item(
    name: &str,
    document_path: rv_data::Url,
    arrangement: Option<&str>,
) -> rv_data::PlaylistItem {
    rv_data::PlaylistItem {
        uuid: Some(uuid(&format!("ITEM-{name}"))),
        name: name.to_string(),
        item_type: Some(playlist_item::ItemType::Presentation(
            playlist_item::Presentation {
                document_path: Some(document_path),
                arrangement: arrangement.map(uuid),
                arrangement_name: arrangement
                    .map(|_| "Chosen".to_string())
                    .unwrap_or_default(),
                ..playlist_item::Presentation::default()
            },
        )),
        ..rv_data::PlaylistItem::default()
    }
}

fn items_node(name: &str, items: Vec<rv_data::PlaylistItem>) -> rv_data::Playlist {
    rv_data::Playlist {
        uuid: Some(uuid(&format!("PLAYLIST-{name}"))),
        name: name.to_string(),
        children_type: Some(playlist::ChildrenType::Items(playlist::PlaylistItems {
            items,
        })),
        ..rv_data::Playlist::default()
    }
}

fn folder_node(name: &str, children: Vec<rv_data::Playlist>) -> rv_data::Playlist {
    rv_data::Playlist {
        name: name.to_string(),
        children_type: Some(playlist::ChildrenType::Playlists(playlist::PlaylistArray {
            playlists: children,
        })),
        ..rv_data::Playlist::default()
    }
}

/// A `ProPresenter` data root holding the given playlists under the root node.
fn data_root(playlists: Vec<rv_data::Playlist>) -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(root.path().join("Playlists")).expect("playlists dir");
    std::fs::create_dir_all(root.path().join("Libraries/Default")).expect("library dir");
    let document = rv_data::PlaylistDocument {
        root_node: Some(folder_node("PLAYLIST", playlists)),
        ..rv_data::PlaylistDocument::default()
    };
    std::fs::write(
        root.path().join("Playlists/Library"),
        document.encode_to_vec(),
    )
    .expect("write library");
    root
}

fn install(root: &Path, fixture: &str, as_name: &str) {
    std::fs::copy(
        crate_path("tests/fixtures/native").join(fixture),
        root.join("Libraries/Default").join(as_name),
    )
    .expect("copy fixture presentation");
}

fn only_item(document: &PlaylistDocument) -> &PresentationItem {
    assert_eq!(document.playlists.len(), 1);
    assert_eq!(document.playlists[0].items.len(), 1);
    first_presentation(document)
}

#[test]
fn library_read_summarizes_scripture_media_and_cue_groups() {
    let root = data_root(vec![items_node(
        "Sunday",
        vec![
            presentation_item("Reading", show_url("Libraries/Default/titus.pro"), None),
            presentation_item("Hymn", show_url("Libraries/Default/amazing.pro"), None),
        ],
    )]);
    // The checked-in scripture example carries no scripture metadata, so
    // attach a reference the way the renderer does.
    let mut titus = rv_data::Presentation::decode(
        std::fs::read(crate_path(
            "tests/fixtures/native/examples/scripture-titus-2v11-13-nrsvue.pro",
        ))
        .expect("read example")
        .as_slice(),
    )
    .expect("decode example");
    titus.bible_reference = Some(rv_data::presentation::BibleReference {
        book_index: 55,
        book_name: "Titus".to_string(),
        chapter_range: Some(rv_data::IntRange { start: 2, end: 2 }),
        verse_range: Some(rv_data::IntRange { start: 11, end: 13 }),
        translation_display_abbreviation: "NRSVUE".to_string(),
        ..rv_data::presentation::BibleReference::default()
    });
    std::fs::write(
        root.path().join("Libraries/Default/titus.pro"),
        titus.encode_to_vec(),
    )
    .expect("write scripture presentation");
    install(
        root.path(),
        "examples/hymn-amazing-grace.pro",
        "amazing.pro",
    );

    let document = read_library_playlist_document(root.path(), "Sunday").expect("reads");
    assert_eq!(
        document.source,
        DocumentSource::LiveLibrary {
            selected: "Sunday".to_string()
        }
    );
    let reading = first_presentation(&document);
    let PresentationContent::Available(summary) = &reading.content else {
        panic!("scripture presentation readable");
    };
    let scripture = summary.scripture.as_ref().expect("scripture metadata");
    assert_eq!(scripture.book, "Titus");
    assert_eq!(scripture.chapters, Some(NumberRange { start: 2, end: 2 }));
    assert_eq!(scripture.verses, Some(NumberRange { start: 11, end: 13 }));
    assert_eq!(scripture.translation, "NRSVUE");

    let ItemDetail::Presentation(hymn) = &document.playlists[0].items[1].detail else {
        panic!("presentation item");
    };
    let PresentationContent::Available(hymn) = &hymn.content else {
        panic!("hymn readable");
    };
    assert!(hymn.slide_count > 1);
    assert!(!hymn.cue_groups.is_empty());
    let grouped: usize = hymn.cue_groups.iter().map(|group| group.slide_count).sum();
    assert!(grouped <= hymn.slide_count);
}

#[test]
fn library_read_reports_missing_unparseable_and_traversing_links() {
    let root = data_root(vec![items_node(
        "Sunday",
        vec![
            presentation_item("Gone", show_url("Libraries/Default/gone.pro"), None),
            presentation_item("Garbage", show_url("Libraries/Default/garbage.pro"), None),
            presentation_item("Traversal", show_url("../../etc/passwd"), None),
            rv_data::PlaylistItem {
                name: "No link".to_string(),
                item_type: Some(playlist_item::ItemType::Presentation(
                    playlist_item::Presentation::default(),
                )),
                ..rv_data::PlaylistItem::default()
            },
        ],
    )]);
    std::fs::write(root.path().join("Libraries/Default/garbage.pro"), b"hello").expect("write");

    let document = read_library_playlist_document(root.path(), "Sunday").expect("reads");
    let reasons: Vec<_> = document.playlists[0]
        .items
        .iter()
        .map(|item| match &item.detail {
            ItemDetail::Presentation(PresentationItem {
                content: PresentationContent::Unavailable { reason },
                ..
            }) => reason.clone(),
            other => panic!("expected an unavailable presentation, got {other:?}"),
        })
        .collect();
    assert_eq!(reasons.len(), 4);
    assert_eq!(
        reasons[0],
        UnavailableReason::FileMissing {
            path: "Libraries/Default/gone.pro".to_string()
        }
    );
    assert!(matches!(
        reasons[1],
        UnavailableReason::NotAPresentation { .. }
    ));
    assert_eq!(reasons[2], UnavailableReason::NoLocator);
    assert_eq!(reasons[3], UnavailableReason::NoLocator);
}

#[test]
fn library_read_refuses_links_that_resolve_outside_the_root() {
    let outside = tempfile::tempdir().expect("outside dir");
    let secret = outside.path().join("outside.pro");
    std::fs::copy(
        crate_path("tests/fixtures/native/examples/title-nametag.pro"),
        &secret,
    )
    .expect("copy");
    let root = data_root(vec![items_node(
        "Sunday",
        vec![
            presentation_item("Absolute", absolute_url(&secret), None),
            presentation_item("Symlink", show_url("Libraries/Default/link.pro"), None),
        ],
    )]);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&secret, root.path().join("Libraries/Default/link.pro"))
        .expect("symlink");

    let document = read_library_playlist_document(root.path(), "Sunday").expect("reads");
    let items = &document.playlists[0].items;
    for item in items {
        let ItemDetail::Presentation(presentation) = &item.detail else {
            panic!("presentation item");
        };
        assert!(
            matches!(
                presentation.content,
                PresentationContent::Unavailable {
                    reason: UnavailableReason::FileOutsideRoot { .. }
                }
            ),
            "{}: {:?}",
            item.name,
            presentation.content
        );
    }
}

#[test]
fn selected_arrangement_missing_from_the_presentation_is_an_item_problem() {
    let root = data_root(vec![items_node(
        "Sunday",
        vec![presentation_item(
            "Hymn",
            show_url("Libraries/Default/amazing.pro"),
            Some("NO-SUCH-ARRANGEMENT"),
        )],
    )]);
    install(
        root.path(),
        "examples/hymn-amazing-grace.pro",
        "amazing.pro",
    );

    let document = read_library_playlist_document(root.path(), "Sunday").expect("reads");
    let item = only_item(&document);
    assert_eq!(
        item.arrangement,
        ArrangementChoice::Selected {
            id: NativeId("NO-SUCH-ARRANGEMENT".to_string()),
            name: "Chosen".to_string()
        }
    );
    assert_eq!(
        item.problems,
        [ItemProblem::ArrangementNotInPresentation {
            id: NativeId("NO-SUCH-ARRANGEMENT".to_string())
        }]
    );
}

#[test]
fn non_presentation_items_have_their_own_kinds() {
    let header = rv_data::PlaylistItem {
        name: "Gathering".to_string(),
        item_type: Some(playlist_item::ItemType::Header(
            playlist_item::Header::default(),
        )),
        ..rv_data::PlaylistItem::default()
    };
    let placeholder = rv_data::PlaylistItem {
        name: "TBD".to_string(),
        is_hidden: true,
        item_type: Some(playlist_item::ItemType::Placeholder(Box::default())),
        ..rv_data::PlaylistItem::default()
    };
    let untyped = rv_data::PlaylistItem {
        name: "Odd".to_string(),
        ..rv_data::PlaylistItem::default()
    };
    let root = data_root(vec![items_node(
        "Sunday",
        vec![header, placeholder, untyped],
    )]);

    let document = read_library_playlist_document(root.path(), "Sunday").expect("reads");
    let items = &document.playlists[0].items;
    assert_eq!(items[0].detail, ItemDetail::Header);
    assert_eq!(items[1].detail, ItemDetail::Placeholder);
    assert!(items[1].hidden);
    assert_eq!(items[2].detail, ItemDetail::Untyped);
    let json = as_json(&document);
    assert_eq!(json["playlists"][0]["items"][0]["kind"], "header");
}

#[test]
fn playlist_selection_by_name_is_exact_then_case_insensitive_and_never_ambiguous() {
    let root = data_root(vec![
        folder_node(
            "Services",
            vec![
                items_node("Easter", Vec::new()),
                items_node("Christmas", Vec::new()),
            ],
        ),
        items_node("Easter", Vec::new()),
        items_node("Advent", Vec::new()),
    ]);

    let ambiguous = read_library_playlist_document(root.path(), "Easter");
    assert!(matches!(
        ambiguous,
        Err(PlaylistDocumentError::PlaylistNameAmbiguous { count: 2, .. })
    ));

    let advent = read_library_playlist_document(root.path(), " advent ").expect("loose match");
    assert_eq!(advent.playlists[0].name, "Advent");

    let folder = read_library_playlist_document(root.path(), "Services").expect("folder");
    let names: Vec<_> = folder.playlists.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Easter", "Christmas"]);
    assert_eq!(folder.playlists[0].folders, ["Services"]);

    match read_library_playlist_document(root.path(), "Lent") {
        Err(PlaylistDocumentError::PlaylistNotFound { available, .. }) => {
            assert_eq!(
                available,
                ["Services", "Easter", "Christmas", "Easter", "Advent"]
            );
        }
        other => panic!("expected PlaylistNotFound, got {other:?}"),
    }
}

#[test]
fn missing_or_corrupt_library_file_is_a_typed_error() {
    let empty = tempfile::tempdir().expect("tempdir");
    assert!(matches!(
        read_library_playlist_document(empty.path(), "Sunday"),
        Err(PlaylistDocumentError::LibraryRead { .. })
    ));

    std::fs::create_dir_all(empty.path().join("Playlists")).expect("dir");
    std::fs::write(empty.path().join("Playlists/Library"), [0xff, 0xff, 0xff]).expect("write");
    assert!(matches!(
        read_library_playlist_document(empty.path(), "Sunday"),
        Err(PlaylistDocumentError::LibraryDecode { .. })
    ));
}

#[test]
fn contemporary_fixture_reports_background_media_without_duplicates() {
    let document = read_package_playlist_document(&corpus(
        "playlists/2026-04-19-0900-contemporary.proplaylist",
    ))
    .expect("fixture reads");
    let media: Vec<&String> = document
        .playlists
        .iter()
        .flat_map(|playlist| &playlist.items)
        .filter_map(|item| match &item.detail {
            ItemDetail::Presentation(PresentationItem {
                content: PresentationContent::Available(summary),
                ..
            }) => Some(summary),
            _ => None,
        })
        .flat_map(|summary| &summary.media)
        .collect();
    assert!(media.iter().any(|name| name.as_str() == "Welcome!.png"));
    for item in document
        .playlists
        .iter()
        .flat_map(|playlist| &playlist.items)
    {
        if let ItemDetail::Presentation(PresentationItem {
            content: PresentationContent::Available(summary),
            ..
        }) = &item.detail
        {
            let mut unique = summary.media.clone();
            unique.sort();
            unique.dedup();
            assert_eq!(unique.len(), summary.media.len(), "{}", item.name);
        }
    }
}
