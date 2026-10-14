#![allow(clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use propresenter::playlist_document::{
    playlist_document_schema, DocumentSource, ItemDetail, PlaylistDocumentError,
    PLAYLIST_DOCUMENT_SCHEMA_VERSION,
};
use propresenter::unstable_native::rv_data::{self, playlist, playlist_item};
use prost::Message;

use super::{read_playlist_document, read_playlist_err, PlaylistRequest, ReadPlaylistError};
use crate::mcp::ReadPlaylistArgs;
use crate::paths::{BuildLocationInputs, BuildLocations};

struct Roots {
    _dir: tempfile::TempDir,
    root: PathBuf,
    locations: BuildLocations,
}

fn roots() -> Roots {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().to_path_buf();
    let data = root.join("data");
    let propresenter = root.join("ProPresenter");
    let library = propresenter.join("Libraries/Default");
    let output = root.join("playlist-output");
    for directory in [&data, &library, &output, &propresenter.join("Playlists")] {
        std::fs::create_dir_all(directory).expect("directory");
    }
    let locations = BuildLocations::from_inputs(BuildLocationInputs {
        project_data_root: data,
        presentation_library: library,
        playlist_output: output,
        propresenter_root: propresenter.clone(),
        themes: propresenter.join("Themes"),
        macros: propresenter.join("Configuration/Macros"),
    })
    .expect("checked locations");
    Roots {
        _dir: dir,
        root,
        locations,
    }
}

fn fixture_package() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../propresenter/tests/fixtures/native/corpus/playlists")
        .join("native-template-library.proplaylist")
}

fn package(path: &str) -> PlaylistRequest {
    PlaylistRequest::Package(path.to_string())
}

fn code(error: &ReadPlaylistError) -> rmcp::model::ErrorCode {
    read_playlist_err(error).code
}

fn write_library(locations: &BuildLocations, playlist_name: &str) {
    let header = rv_data::PlaylistItem {
        name: "Welcome".to_string(),
        item_type: Some(playlist_item::ItemType::Header(
            playlist_item::Header::default(),
        )),
        ..rv_data::PlaylistItem::default()
    };
    let child = rv_data::Playlist {
        name: playlist_name.to_string(),
        children_type: Some(playlist::ChildrenType::Items(playlist::PlaylistItems {
            items: vec![header],
        })),
        ..rv_data::Playlist::default()
    };
    let root = rv_data::Playlist {
        name: "PLAYLIST".to_string(),
        children_type: Some(playlist::ChildrenType::Playlists(playlist::PlaylistArray {
            playlists: vec![child],
        })),
        ..rv_data::Playlist::default()
    };
    let document = rv_data::PlaylistDocument {
        root_node: Some(root),
        ..rv_data::PlaylistDocument::default()
    };
    std::fs::write(
        locations.propresenter_root().join("Playlists/Library"),
        document.encode_to_vec(),
    )
    .expect("write library");
}

#[test]
fn exactly_one_non_empty_source_is_required() {
    let parse = |value: serde_json::Value| {
        PlaylistRequest::try_from(
            serde_json::from_value::<ReadPlaylistArgs>(value).expect("args parse"),
        )
    };
    assert_eq!(
        parse(serde_json::json!({"name": "Sunday"})).expect("named"),
        PlaylistRequest::Named("Sunday".to_string())
    );
    assert_eq!(
        parse(serde_json::json!({"path": "a.proplaylist"})).expect("path"),
        package("a.proplaylist")
    );
    for invalid in [
        serde_json::json!({}),
        serde_json::json!({"name": "a", "path": "b"}),
        serde_json::json!({"name": "  "}),
        serde_json::json!({"path": ""}),
    ] {
        let error = parse(invalid.clone()).expect_err("must be rejected");
        assert_eq!(
            code(&error),
            rmcp::model::ErrorCode::INVALID_PARAMS,
            "{invalid}"
        );
    }
    assert!(
        serde_json::from_value::<ReadPlaylistArgs>(serde_json::json!({"nmae": "x"})).is_err(),
        "unknown fields are rejected"
    );
}

#[test]
fn package_path_is_read_relative_to_the_playlist_output_directory() {
    let roots = roots();
    std::fs::copy(
        fixture_package(),
        roots
            .locations
            .playlist_output()
            .join("service.proplaylist"),
    )
    .expect("copy fixture");

    let document =
        read_playlist_document(&roots.locations, &package("service.proplaylist")).expect("reads");
    assert_eq!(
        document.source,
        DocumentSource::Package {
            file_name: "service.proplaylist".to_string()
        }
    );
    assert_eq!(document.playlists[0].items.len(), 3);
    assert!(matches!(
        document.playlists[0].items[0].detail,
        ItemDetail::Presentation(_)
    ));
}

#[test]
fn package_path_may_be_absolute_inside_the_data_root() {
    let roots = roots();
    let inside = roots
        .locations
        .propresenter_root()
        .join("exported.proplaylist");
    std::fs::copy(fixture_package(), &inside).expect("copy fixture");

    let document = read_playlist_document(
        &roots.locations,
        &package(inside.to_str().expect("utf-8 path")),
    )
    .expect("reads");
    assert_eq!(document.playlists.len(), 1);
}

#[test]
fn package_paths_outside_the_roots_or_missing_are_indistinguishable_request_errors() {
    let roots = roots();
    let outside = roots.root.join("outside.proplaylist");
    std::fs::copy(fixture_package(), &outside).expect("copy fixture");
    std::fs::write(
        roots.locations.playlist_output().join("notes.txt"),
        b"not a playlist",
    )
    .expect("write");

    let mut attempts = vec![
        outside.to_str().expect("utf-8 path").to_string(),
        "../outside.proplaylist".to_string(),
        "absent.proplaylist".to_string(),
        "notes.txt".to_string(),
        "/etc/hostname".to_string(),
    ];
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            &outside,
            roots.locations.playlist_output().join("link.proplaylist"),
        )
        .expect("symlink");
        attempts.push("link.proplaylist".to_string());
    }
    for attempt in attempts {
        let error =
            read_playlist_document(&roots.locations, &package(&attempt)).expect_err(&attempt);
        assert!(
            matches!(error, ReadPlaylistError::PathNotAllowed),
            "{attempt}: {error}"
        );
        assert_eq!(code(&error), rmcp::model::ErrorCode::INVALID_PARAMS);
    }
}

#[test]
fn a_corrupt_package_inside_the_roots_is_a_server_error() {
    let roots = roots();
    std::fs::write(
        roots.locations.playlist_output().join("broken.proplaylist"),
        b"not a zip",
    )
    .expect("write");
    let error = read_playlist_document(&roots.locations, &package("broken.proplaylist"))
        .expect_err("corrupt package");
    assert!(matches!(
        error,
        ReadPlaylistError::Document(PlaylistDocumentError::Package { .. })
    ));
    assert_eq!(code(&error), rmcp::model::ErrorCode::INTERNAL_ERROR);
}

#[test]
fn named_playlist_is_read_from_the_live_library() {
    let roots = roots();
    write_library(&roots.locations, "Sunday");

    let document = read_playlist_document(
        &roots.locations,
        &PlaylistRequest::Named("Sunday".to_string()),
    )
    .expect("reads");
    assert_eq!(
        document.source,
        DocumentSource::LiveLibrary {
            selected: "Sunday".to_string()
        }
    );
    assert_eq!(document.playlists[0].items[0].detail, ItemDetail::Header);
}

#[test]
fn unknown_name_is_a_request_error_and_a_missing_library_a_server_error() {
    let roots = roots();
    let missing_library = read_playlist_document(
        &roots.locations,
        &PlaylistRequest::Named("Sunday".to_string()),
    )
    .expect_err("no library yet");
    assert!(matches!(
        missing_library,
        ReadPlaylistError::Document(PlaylistDocumentError::LibraryRead { .. })
    ));
    assert_eq!(
        code(&missing_library),
        rmcp::model::ErrorCode::INTERNAL_ERROR
    );

    write_library(&roots.locations, "Sunday");
    let unknown = read_playlist_document(
        &roots.locations,
        &PlaylistRequest::Named("Lent".to_string()),
    )
    .expect_err("unknown playlist");
    assert_eq!(code(&unknown), rmcp::model::ErrorCode::INVALID_PARAMS);
}

#[test]
fn schema_tool_payload_is_the_document_schema_at_the_current_version() {
    let schema = serde_json::to_value(playlist_document_schema()).expect("schema serializes");
    assert_eq!(
        schema["$defs"]["SchemaVersion"]["const"],
        PLAYLIST_DOCUMENT_SCHEMA_VERSION
    );
    assert_eq!(schema["title"], "ProPresenter playlist document");
}
