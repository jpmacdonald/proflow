#![allow(clippy::expect_used)]

use super::*;
use propresenter::unstable_native::rv_data;
use prost::Message;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

fn write_presentation(path: &Path) {
    let name = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .expect("fixture path has a UTF-8 stem");
    let presentation = rv_data::Presentation {
        uuid: Some(rv_data::Uuid {
            string: format!("{name}-id"),
        }),
        name: name.to_string(),
        ..Default::default()
    };
    fs::write(path, presentation.encode_to_vec()).expect("write presentation fixture");
}

fn build_index(files: &[&str]) -> (tempfile::TempDir, LibraryCatalog) {
    let dir = tempdir().expect("temp dir");
    for file in files {
        write_presentation(&dir.path().join(file));
    }
    let index = LibraryCatalog::build(dir.path()).expect("index fixture library");
    (dir, index)
}

#[test]
fn explicit_filename_resolution_only_normalizes_case_whitespace_and_extension() {
    let (_dir, index) = build_index(&["Call to Worship.pro", "Call to Worship Extended.pro"]);

    let resolved = resolve_exact_library_file(Some(&index), "  call to worship.PRO  ");
    assert!(
        matches!(resolved, ExactLibraryFileMatch::Unique(path) if path.ends_with("Call to Worship.pro"))
    );
    assert_eq!(
        resolve_exact_library_file(Some(&index), "Call to Wors"),
        ExactLibraryFileMatch::Missing
    );
}

#[test]
fn explicit_filename_resolution_rejects_duplicate_filenames() {
    let dir = tempdir().expect("temp dir");
    let nested = dir.path().join("nested");
    fs::create_dir(&nested).expect("create nested library folder");
    write_presentation(&dir.path().join("Announcements.pro"));
    write_presentation(&nested.join("announcements.pro"));
    let index = LibraryCatalog::build(dir.path()).expect("index fixture library");

    assert!(matches!(
        resolve_exact_library_file(Some(&index), "Announcements.pro"),
        ExactLibraryFileMatch::Ambiguous(paths) if paths.len() == 2
    ));
}

#[test]
fn strict_song_search_rejects_short_reverse_containment() {
    let (_dir, index) = build_index(&["By Faith.pro", "Praise.pro", "Our God.pro"]);

    assert!(search_index_strict(
        Some(&index),
        "#399 We Walk by Faith and Not by Sight (to AZMON-466)",
    )
    .is_none());
    assert!(search_index_strict(Some(&index), "O Praise The Name (Anastasis)").is_none());
    assert!(search_index_strict(Some(&index), "#210 Our God, Our Help in Ages Past").is_none());
}

#[test]
fn strict_song_search_allows_substantial_reverse_containment() {
    let (_dir, index) = build_index(&["Rock of Ages.pro", "Amazing Grace.pro"]);

    assert!(search_index_strict(Some(&index), "GTG #438 Rock of Ages").is_some());
    assert!(search_index_strict(Some(&index), "#649 Amazing Grace").is_some());
}

#[test]
fn strict_song_search_allows_reordered_hymn_catalog_tokens() {
    let (_dir, index) = build_index(&["[Hymn] Rock of Ages GTG #438.pro"]);

    assert!(search_index_strict(Some(&index), "GTG #438 Rock of Ages").is_some());
}

#[test]
fn song_resolution_prefers_direct_title_over_bracket_classifier() {
    let (_dir, index) = build_index(&[
        "#210 Our God, Our Help in Ages Past.pro",
        "[Hymn] Our God, Our Help in Ages Past.pro",
    ]);

    let resolved = resolve_song_library_match(
        Some(&index),
        "#210 Our God, Our Help in Ages Past",
        "#210 Our God, Our Help in Ages Past",
        "#210 Our God, Our Help in Ages Past",
        "Our God, Our Help in Ages Past",
    );

    assert!(matches!(
        resolved,
        SongLibraryMatch::Resolved(path)
            if path.contains("#210 Our God, Our Help in Ages Past.pro")
    ));
}

#[test]
fn song_resolution_prefers_exact_untagged_worship_title_over_tagged_partial() {
    let (_dir, index) = build_index(&[
        "This Is Amazing Grace.pro",
        "[Anthem] Amazing Grace.pro",
        "[Hymn] Amazing Grace.pro",
    ]);

    let resolved = resolve_song_library_match(
        Some(&index),
        "This Is Amazing Grace",
        "This Is Amazing Grace",
        "This Is Amazing Grace",
        "This Is Amazing Grace",
    );

    assert!(matches!(
        resolved,
        SongLibraryMatch::Resolved(path) if path.contains("This Is Amazing Grace.pro")
    ));
}

#[test]
fn song_resolution_requires_review_when_best_native_files_tie() {
    let (_dir, index) = build_index(&["[Anthem] Amazing Grace.pro", "[Hymn] Amazing Grace.pro"]);

    let resolved = resolve_song_library_match(
        Some(&index),
        "Amazing Grace",
        "Amazing Grace",
        "Amazing Grace",
        "Amazing Grace",
    );

    assert!(matches!(resolved, SongLibraryMatch::Candidate(_)));
}

#[test]
fn song_resolution_requires_review_for_a_unique_heuristic_match() {
    let (_dir, index) = build_index(&["This Is Amazing Grace.pro"]);

    let resolved = resolve_song_library_match(
        Some(&index),
        "Amazing Grace",
        "Amazing Grace",
        "Amazing Grace",
        "Amazing Grace",
    );

    assert!(matches!(resolved, SongLibraryMatch::Candidate(_)));
}

#[test]
fn song_resolution_requires_review_for_a_role_conflict() {
    let (_dir, index) = build_index(&["[Anthem] Amazing Grace.pro"]);

    let resolved = resolve_song_library_match(
        Some(&index),
        "Amazing Grace",
        "#649 Amazing Grace",
        "Amazing Grace",
        "Amazing Grace",
    );

    assert!(matches!(resolved, SongLibraryMatch::Candidate(_)));
}

#[test]
fn song_resolution_preserves_an_exact_parenthesized_title() {
    let (_dir, index) = build_index(&["O Praise The Name (Anastasis).pro"]);

    let resolved = resolve_song_library_match(
        Some(&index),
        "O Praise The Name (Anastasis)",
        "O Praise The Name (Anastasis)",
        "O Praise The Name (Anastasis)",
        "O Praise The Name (Anastasis)",
    );

    assert!(matches!(
        resolved,
        SongLibraryMatch::Resolved(path)
            if path.ends_with("O Praise The Name (Anastasis).pro")
    ));
}

#[test]
fn song_resolution_requires_review_between_title_variants() {
    let (_dir, index) = build_index(&[
        "O Praise The Name (Anastasis).pro",
        "O Praise The Name (Live).pro",
    ]);

    let resolved = resolve_song_library_match(
        Some(&index),
        "O Praise The Name",
        "O Praise The Name",
        "O Praise The Name",
        "O Praise The Name",
    );

    assert!(matches!(resolved, SongLibraryMatch::Candidate(_)));
}

#[test]
fn conflicting_exact_item_and_song_titles_require_review() {
    let (_dir, index) = build_index(&["Amazing Grace.pro", "[Hymn] Amazing Grace.pro"]);

    let resolved = resolve_song_library_match(
        Some(&index),
        "Amazing Grace",
        "[Hymn] Amazing Grace",
        "[Hymn] Amazing Grace",
        "Amazing Grace",
    );

    assert!(matches!(resolved, SongLibraryMatch::ConflictingTitles(paths) if paths.len() == 2));
}

#[test]
fn dropping_a_parenthesized_qualifier_is_only_a_candidate() {
    let (_dir, index) = build_index(&["O Praise The Name.pro"]);

    let resolved = resolve_song_library_match(
        Some(&index),
        "O Praise The Name (Anastasis)",
        "O Praise The Name (Anastasis)",
        "O Praise The Name (Anastasis)",
        "O Praise The Name (Anastasis)",
    );

    assert!(matches!(resolved, SongLibraryMatch::Candidate(_)));
}

#[test]
fn unbracketed_anthem_conflicts_with_a_numbered_hymn() {
    let (_dir, index) = build_index(&["Anthem: Amazing Grace.pro"]);

    let resolved = resolve_song_library_match(
        Some(&index),
        "Amazing Grace",
        "#649 Amazing Grace",
        "Amazing Grace",
        "Amazing Grace",
    );

    assert!(matches!(resolved, SongLibraryMatch::Candidate(_)));
}

#[test]
fn original_special_item_prefix_is_role_evidence() {
    let (_dir, index) = build_index(&["[Hymn] Amazing Grace.pro"]);

    let resolved = resolve_song_library_match(
        Some(&index),
        "Amazing Grace",
        "Offertory: Amazing Grace",
        "Amazing Grace",
        "Amazing Grace",
    );

    assert!(matches!(resolved, SongLibraryMatch::Candidate(_)));
}
#[test]
fn duplicate_exact_titles_are_ambiguous_regardless_of_parent_directory() {
    let dir = tempdir().expect("temp dir");
    let nested = dir.path().join("conflicted copy");
    fs::create_dir(&nested).expect("nested fixture directory");
    write_presentation(&dir.path().join("Amazing Grace.pro"));
    write_presentation(&nested.join("amazing grace.pro"));
    let index = LibraryCatalog::build(dir.path()).expect("fixture library");
    let resolved = resolve_song_library_match(
        Some(&index),
        "Amazing Grace",
        "Amazing Grace",
        "Amazing Grace",
        "Amazing Grace",
    );
    assert!(matches!(
        resolved,
        SongLibraryMatch::Ambiguous(paths)
            if paths.len() == 2 && paths.iter().all(|path| path.to_lowercase().ends_with("amazing grace.pro"))
    ));
}

#[test]
fn unrelated_candidates_cannot_change_full_title_identity() {
    let (dir, initial) = build_index(&["Amazing Grace (Live).pro"]);
    let before = resolve_song_library_match(
        Some(&initial),
        "Amazing Grace (Live)",
        "Amazing Grace (Live)",
        "Amazing Grace",
        "Amazing Grace",
    );
    for name in [
        "Amazing Grace.pro",
        "[Hymn] Amazing Grace.pro",
        "Amazing Grace (Anastasis).pro",
    ] {
        write_presentation(&dir.path().join(name));
    }
    let expanded = LibraryCatalog::build(dir.path()).expect("expanded fixture library");
    assert_eq!(
        resolve_song_library_match(
            Some(&expanded),
            "Amazing Grace (Live)",
            "Amazing Grace (Live)",
            "Amazing Grace",
            "Amazing Grace"
        ),
        before,
    );
}
