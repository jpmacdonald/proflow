#![allow(clippy::expect_used)]

use super::*;

#[test]
fn explicit_locations_are_one_checked_snapshot() {
    let root = tempfile::tempdir().expect("temporary root");
    let data = root.path().join("data");
    let library = root.path().join("ProPresenter/Libraries/Default");
    let propresenter = root.path().join("ProPresenter");
    std::fs::create_dir_all(&data).expect("data directory");
    std::fs::create_dir_all(&library).expect("library directory");

    let locations = BuildLocations::from_inputs(BuildLocationInputs {
        project_data_root: data.clone(),
        presentation_library: library.clone(),
        playlist_output: root.path().join("playlists"),
        propresenter_root: propresenter.clone(),
        themes: propresenter.join("Themes"),
        macros: propresenter.join("Configuration/Macros"),
    })
    .expect("checked locations");

    assert_eq!(locations.project_data_root(), data);
    assert_eq!(locations.project_config(), data.join(PROJECT_CONFIG_FILE));
    assert_eq!(locations.presentation_library(), library);
    assert_eq!(locations.propresenter_root(), propresenter);
    assert_eq!(
        locations.workspace(),
        propresenter.join("Configuration/Workspace")
    );
}

#[test]
fn output_path_cannot_be_an_existing_file() {
    let root = tempfile::tempdir().expect("temporary root");
    let data = root.path().join("data");
    let library = root.path().join("ProPresenter/Libraries/Default");
    let propresenter = root.path().join("ProPresenter");
    let output_file = root.path().join("playlist-output");
    std::fs::create_dir_all(&data).expect("data directory");
    std::fs::create_dir_all(&library).expect("library directory");
    std::fs::write(&output_file, b"not a directory").expect("output fixture");

    let error = BuildLocations::from_inputs(BuildLocationInputs {
        project_data_root: data,
        presentation_library: library,
        playlist_output: output_file.clone(),
        propresenter_root: propresenter.clone(),
        themes: propresenter.join("Themes"),
        macros: propresenter.join("Configuration/Macros"),
    })
    .expect_err("file output must fail");

    assert!(matches!(
        error,
        BuildLocationsError::OutputIsFile { path, .. } if path == output_file
    ));
}

#[test]
fn playlists_default_to_a_dedicated_directory_outside_the_presentation_library() {
    let root = Path::new("/show/ProPresenter");

    assert_eq!(
        default_playlist_output_dir(root),
        PathBuf::from("/show/ProPresenter/Playlists/ProFlow")
    );
}

#[test]
fn active_show_is_authoritative_and_rejects_a_different_configured_root() {
    let root = tempfile::tempdir().expect("temporary root");
    let configured = root.path().join("configured");
    let active = root.path().join("active");
    std::fs::create_dir(&configured).expect("configured show");
    std::fs::create_dir(&active).expect("active show");

    let error = select_propresenter_root(
        Some(configured.clone()),
        Some(active.clone()),
        root.path().join("fallback"),
    )
    .expect_err("a stale configured clone must not override the active show");

    assert!(matches!(
        error,
        BuildLocationsError::ConflictingActiveProPresenterRoot {
            configured: error_configured,
            active: error_active,
        } if error_configured == configured && error_active == active
    ));
    assert_eq!(
        select_propresenter_root(None, Some(active.clone()), root.path().join("fallback"))
            .expect("active show is sufficient"),
        active
    );
}

#[test]
fn active_show_preference_accepts_one_valid_path() {
    let active = interpret_active_show_preference(
        true,
        "exit status: 0".to_owned(),
        b"/Users/example/Documents/ProPresenter\n",
        b"",
    )
    .expect("valid preference output");

    assert_eq!(
        active,
        Some(PathBuf::from("/Users/example/Documents/ProPresenter"))
    );
}

#[test]
fn absent_active_show_preference_is_the_only_failed_read_that_allows_fallback() {
    let missing = format!("defaults diagnostic prefix\n{MISSING_ACTIVE_SHOW_PREFERENCE}\n");
    let active = interpret_active_show_preference(
        false,
        "exit status: 1".to_owned(),
        b"",
        missing.as_bytes(),
    )
    .expect("an absent preference is not a command failure");

    assert_eq!(active, None);

    let error = interpret_active_show_preference(
        false,
        "exit status: 1".to_owned(),
        b"unexpected output",
        missing.as_bytes(),
    )
    .expect_err("unexpected output must not be mistaken for an absent preference");
    assert!(matches!(
        error,
        BuildLocationsError::ActiveProPresenterShowReadFailed { .. }
    ));
}

#[test]
fn active_show_preference_read_failure_is_typed_and_fails_closed() {
    let error = interpret_active_show_preference(
        false,
        "exit status: 70".to_owned(),
        b"",
        b"preferences service unavailable\n",
    )
    .expect_err("an unknown preference failure must stop discovery");

    assert!(matches!(
        error,
        BuildLocationsError::ActiveProPresenterShowReadFailed {
            status,
            diagnostic,
        } if status == "exit status: 70" && diagnostic == "preferences service unavailable"
    ));
}

#[test]
fn malformed_active_show_preference_is_typed_and_fails_closed() {
    for (value, expected_reason) in [
        (b"".as_slice(), "preference value is empty"),
        (
            b"/Users/example/Documents/ProPresenter\0unexpected".as_slice(),
            "preference value contains control characters",
        ),
        (b"\xff".as_slice(), "preference value is not UTF-8"),
    ] {
        let error = interpret_active_show_preference(true, "exit status: 0".to_owned(), value, b"")
            .expect_err("malformed preference output must stop discovery");

        assert!(matches!(
            error,
            BuildLocationsError::MalformedActiveProPresenterShow { reason }
                if reason == expected_reason
        ));
    }
}
