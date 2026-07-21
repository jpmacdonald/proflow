use super::support::*;
use crate::project_config::parse_project_config_str;
use crate::propresenter::library::LibraryCatalog;
use crate::workflow::classify::build_plan;
use crate::workflow::plan::ReadyAction;
use tempfile::tempdir;

#[test]
fn repo_rule_matrix_keeps_named_exceptions_ahead_of_song_fallbacks() {
    let config = parse_project_config_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/data/proflow.config.json"
    )))
    .expect("repo config should parse");

    for (title, service, expected_type) in [
        (
            "Doxology and Prayer of Dedication",
            "10:30am traditional",
            "doxology_with_prayer",
        ),
        (
            "Giving of Tithes and Offerings",
            "10:30am traditional",
            "static_graphic",
        ),
        ("Greeting", "10:30am traditional", "title_static"),
        (
            "The Apostles' Creed",
            "10:30am traditional",
            "audience_liturgy_static",
        ),
        (
            "Prayer of Confession",
            "10:30am traditional",
            "liturgical_edited",
        ),
        (
            "Unison Prayer",
            "10:30am traditional",
            "liturgical_audience_generated",
        ),
        (
            "Scripture - Exodus 16:1-4a",
            "10:30am traditional",
            "scripture",
        ),
        ("Gloria Patri", "10:30am traditional", "titled_song_static"),
        (
            "Offertory: O, The Depth of the Love of God",
            "10:30am traditional",
            "titled_song_static",
        ),
        ("Choir Anthem: Gloria", "10:30am traditional", "song"),
        ("Amazing Grace", "10:30am traditional", "hymn"),
        ("Amazing Grace", "9:00am contemporary", "song"),
    ] {
        let mut item = song_item(None);
        item.id = format!("rule-order-{title}");
        item.title = title.to_string();
        let plans = build_plan(&[item], &config, None, Some(service));
        let plan = plans
            .iter()
            .find(|plan| plan.pco_title == title)
            .expect("classified plan item");

        assert_eq!(
            plan.item_type(),
            Some(expected_type),
            "wrong rule selected for '{title}' in '{service}'"
        );
    }

    let affirmation = test_text_item(
        "rule-order-affirmation",
        1,
        "Affirmation of Faith - The Heidelberg Catechism",
        Some("Q. What is your only comfort?\nA. That I belong to Jesus Christ."),
    );
    let plans = build_plan(&[affirmation], &config, None, Some("10:30am traditional"));
    let affirmation = plans
        .iter()
        .find(|plan| plan.pco_title == "Affirmation of Faith - The Heidelberg Catechism")
        .expect("classified affirmation item");
    assert_eq!(
        affirmation.item_type(),
        Some("liturgical_audience_generated"),
        "text-category affirmations must use the liturgy rule"
    );
}

#[test]
fn repo_expansion_rule_matrix_keeps_speaker_nametag_and_liturgy_order() {
    let config = parse_project_config_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/data/proflow.config.json"
    )))
    .expect("repo config should parse");

    for (title, description, expected_types) in [
        (
            "Call to Worship (Hope)",
            "Leader: The Lord be with you.\nPeople: And also with you.",
            ["title_static", "liturgical_edited"],
        ),
        (
            "Prayer and The Lord's Prayer (Hope)",
            "Our Father, who art in heaven.",
            ["title_static", "leader_liturgy_static"],
        ),
    ] {
        let plans = build_plan(
            &[test_text_item(
                "production-expansion",
                1,
                title,
                Some(description),
            )],
            &config,
            None,
            Some("10:30am traditional"),
        );
        let actual_types = plans
            .iter()
            .filter(|plan| plan.pco_title == title)
            .map(|plan| plan.item_type().expect("expanded item type"))
            .collect::<Vec<_>>();
        assert_eq!(actual_types, expected_types, "wrong expansion for {title}");
    }
}

#[test]
fn repo_native_liturgy_routes_select_the_reviewed_files() {
    let config = parse_project_config_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/data/proflow.config.json"
    )))
    .expect("repo config should parse");
    let library = tempdir().expect("production-routing library");
    for file in [
        "Baptism Him.pro",
        "Baptism Her.pro",
        "Baptism Them.pro",
        "Heidleberg Chatechism - Question 1.pro",
        "New Member Recognition.pro",
    ] {
        write_library_presentation(&library.path().join(file));
    }
    let catalog = LibraryCatalog::build(library.path()).expect("fixture library should index");

    for (id, title, description, expected_file) in [
        (
            "baptism-him",
            "Baptism",
            Some("James, son of Jane and John"),
            "Baptism Him.pro",
        ),
        (
            "baptism-her",
            "Baptism",
            Some("Anna, daughter of Jane and John"),
            "Baptism Her.pro",
        ),
        (
            "baptism-them",
            "Baptism",
            Some("The children of Jane and John"),
            "Baptism Them.pro",
        ),
        (
            "heidelberg",
            "Heidelberg Confession, Q1",
            None,
            "Heidleberg Chatechism - Question 1.pro",
        ),
        (
            "new-members",
            "Welcome of New Members",
            None,
            "New Member Recognition.pro",
        ),
    ] {
        let plans = build_plan(
            &[test_text_item(id, 1, title, description)],
            &config,
            Some(&catalog),
            None,
        );
        assert_eq!(plans.len(), 1, "unexpected expansion for {title}");
        let plan = &plans[0];
        assert_eq!(plan.item_type(), Some("preserved_liturgy_static"));
        assert!(matches!(
            plan.ready_action(),
            Some(ReadyAction::RestyleExisting { .. })
        ));
        assert_eq!(
            plan.file_path()
                .and_then(|path| path.file_name())
                .and_then(|name| name.to_str()),
            Some(expected_file),
            "wrong native file for {title}"
        );
    }
}

#[test]
fn use_existing_song_uses_the_planning_center_arrangement() {
    let config = song_config(None, None);
    let (_directory, index) = song_index(&[(
        "PCO Verse Order",
        Some("550e8400-e29b-41d4-a716-446655440001"),
    )]);

    let plans = build_plan(
        &[song_item(Some("PCO Verse Order"))],
        &config,
        Some(&index),
        None,
    );

    assert!(is_use_existing(&plans[0]));
    assert_eq!(plans[0].arrangement(), Some("PCO Verse Order"));
}

#[test]
fn planning_center_default_arrangement_aliases_a_unique_native_default() {
    let config = song_config(None, None);
    let (_directory, index) =
        song_index(&[("Default", Some("550e8400-e29b-41d4-a716-446655440001"))]);

    let plans = build_plan(
        &[song_item(Some("Default Arrangement"))],
        &config,
        Some(&index),
        None,
    );

    assert!(is_use_existing(&plans[0]));
    assert_eq!(plans[0].arrangement(), Some("Default"));
    assert!(plans[0]
        .reason
        .contains("using native arrangement 'Default'"));
}

#[test]
fn unavailable_planning_center_arrangement_falls_back_to_one_complete_native_default() {
    let config = song_config(None, None);
    let (_directory, index) = song_index(&[
        ("Default", Some("550e8400-e29b-41d4-a716-446655440001")),
        ("Youth", Some("550e8400-e29b-41d4-a716-446655440002")),
    ]);

    let plans = build_plan(
        &[song_item(Some("Eight Arrangement"))],
        &config,
        Some(&index),
        None,
    );

    assert!(is_use_existing(&plans[0]));
    assert_eq!(plans[0].arrangement(), Some("Default"));
    assert!(plans[0]
        .reason
        .contains("requested Planning Center arrangement 'Eight Arrangement' is unavailable"));
}

#[test]
fn planning_center_default_arrangement_selects_none_when_native_has_no_arrangements() {
    let config = song_config(None, None);
    let (_directory, index) = song_index(&[]);

    let plans = build_plan(
        &[song_item(Some("Default Arrangement"))],
        &config,
        Some(&index),
        None,
    );

    assert!(is_use_existing(&plans[0]));
    assert_eq!(plans[0].arrangement(), None);
}

#[test]
fn exact_native_arrangement_precedes_the_default_alias() {
    let config = song_config(None, None);
    let (_directory, index) = song_index(&[
        (
            "Default Arrangement",
            Some("550e8400-e29b-41d4-a716-446655440001"),
        ),
        ("Default", Some("550e8400-e29b-41d4-a716-446655440002")),
    ]);

    let plans = build_plan(
        &[song_item(Some("Default Arrangement"))],
        &config,
        Some(&index),
        None,
    );

    assert!(is_use_existing(&plans[0]));
    assert_eq!(plans[0].arrangement(), Some("Default Arrangement"));
}

#[test]
fn default_arrangement_alias_requires_one_complete_native_default() {
    let config = song_config(None, None);
    let (_ambiguous_directory, ambiguous_index) = song_index(&[
        ("Default", Some("550e8400-e29b-41d4-a716-446655440001")),
        ("default", Some("550e8400-e29b-41d4-a716-446655440002")),
    ]);

    let ambiguous = build_plan(
        &[song_item(Some("Default Arrangement"))],
        &config,
        Some(&ambiguous_index),
        None,
    );

    assert!(ambiguous[0].needs_review());
    assert!(ambiguous[0].reason.contains("is ambiguous"));

    let (_incomplete_directory, incomplete_index) = song_index(&[("Default", None)]);
    let incomplete = build_plan(
        &[song_item(Some("Default Arrangement"))],
        &config,
        Some(&incomplete_index),
        None,
    );

    assert!(incomplete[0].needs_review());
    assert!(incomplete[0]
        .reason
        .contains("has a missing or invalid UUID"));

    let (_empty_record_directory, empty_record_index) = song_index(&[("", None)]);
    let empty_record = build_plan(
        &[song_item(Some("Default Arrangement"))],
        &config,
        Some(&empty_record_index),
        None,
    );

    assert!(empty_record[0].needs_review());
    assert!(empty_record[0].reason.contains("is unavailable"));
}

#[test]
fn planning_center_default_fallback_is_case_insensitive() {
    let config = song_config(None, None);
    let (_directory, index) =
        song_index(&[("Default", Some("550e8400-e29b-41d4-a716-446655440001"))]);

    let plans = build_plan(
        &[song_item(Some("default arrangement"))],
        &config,
        Some(&index),
        None,
    );

    assert!(is_use_existing(&plans[0]));
    assert_eq!(plans[0].arrangement(), Some("Default"));
}

#[test]
fn unavailable_configured_arrangement_does_not_fall_back_to_native_default() {
    let config = song_config(Some("Configured Order"), None);
    let (_directory, index) =
        song_index(&[("Default", Some("550e8400-e29b-41d4-a716-446655440001"))]);

    let plans = build_plan(&[song_item(Some("PCO Order"))], &config, Some(&index), None);

    assert!(plans[0].needs_review());
    assert!(plans[0]
        .reason
        .contains("'Configured Order' is unavailable"));

    let default_alias_config = song_config(Some("Default Arrangement"), None);
    let (_empty_directory, empty_index) = song_index(&[]);
    let empty = build_plan(
        &[song_item(Some("PCO Order"))],
        &default_alias_config,
        Some(&empty_index),
        None,
    );

    assert!(empty[0].needs_review());
    assert!(empty[0]
        .reason
        .contains("'Default Arrangement' is unavailable"));
}

#[test]
fn exact_planning_center_arrangement_precedes_native_default_fallback() {
    let config = song_config(None, None);
    let (_directory, index) = song_index(&[
        ("Default", Some("550e8400-e29b-41d4-a716-446655440001")),
        ("Youth", Some("550e8400-e29b-41d4-a716-446655440002")),
    ]);

    let plans = build_plan(&[song_item(Some("Youth"))], &config, Some(&index), None);

    assert!(is_use_existing(&plans[0]));
    assert_eq!(plans[0].arrangement(), Some("Youth"));
    assert_eq!(plans[0].reason, "Library match");
}

#[test]
fn configured_and_service_override_arrangements_precede_planning_center() {
    let config = song_config(Some("Configured Order"), Some("Christmas Order"));
    let (_directory, index) = song_index(&[
        (
            "Configured Order",
            Some("550e8400-e29b-41d4-a716-446655440001"),
        ),
        (
            "Christmas Order",
            Some("550e8400-e29b-41d4-a716-446655440002"),
        ),
        ("PCO Order", Some("550e8400-e29b-41d4-a716-446655440003")),
    ]);
    let item = song_item(Some("PCO Order"));

    let ordinary = build_plan(std::slice::from_ref(&item), &config, Some(&index), None);
    let christmas = build_plan(&[item], &config, Some(&index), Some("Christmas Eve"));

    assert!(is_use_existing(&ordinary[0]));
    assert_eq!(ordinary[0].arrangement(), Some("Configured Order"));
    assert!(is_use_existing(&christmas[0]));
    assert_eq!(christmas[0].arrangement(), Some("Christmas Order"));
}

#[test]
fn use_existing_song_without_an_arrangement_selects_none() {
    let config = song_config(None, None);
    let (_directory, index) = song_index(&[]);

    let plans = build_plan(&[song_item(None)], &config, Some(&index), None);

    assert!(is_use_existing(&plans[0]));
    assert_eq!(plans[0].arrangement(), None);
}

#[test]
fn song_without_a_requested_arrangement_uses_one_complete_native_default() {
    let config = song_config(None, None);
    let (_directory, index) = song_index(&[
        ("Default", Some("550e8400-e29b-41d4-a716-446655440001")),
        ("Youth", Some("550e8400-e29b-41d4-a716-446655440002")),
    ]);

    let plans = build_plan(&[song_item(None)], &config, Some(&index), None);

    assert!(is_use_existing(&plans[0]));
    assert_eq!(plans[0].arrangement(), Some("Default"));
    assert!(plans[0]
        .reason
        .contains("using native arrangement 'Default'"));
}

#[test]
fn restyled_song_does_not_inherit_the_generated_default_background() {
    let config = parse_project_config_str(
        r#"
            {
              "version": 4,
              "defaults": { "background": "default" },
              "backgrounds": { "default": "backgrounds/default.png" },
              "presentation_types": {
                "song": {
                  "kind": "song",
                  "content_source": "song",
                  "output_strategy": "restyle_existing",
                  "macro_transitions": {
                    "regions": [{
                      "selector": { "kind": "operator_cue", "index": 0 },
                      "enter_macro": "Song"
                    }]
                  }
                }
              },
              "item_rules": [{
                "id": "song",
                "match": { "category": "song" },
                "use_type": "song",
                "target": { "library_file": "Amazing Grace.pro" }
              }]
            }
            "#,
    )
    .expect("restyled song config should parse");
    let (_directory, index) = song_index(&[
        ("Default", Some("550e8400-e29b-41d4-a716-446655440001")),
        ("Youth", Some("550e8400-e29b-41d4-a716-446655440002")),
    ]);

    let plans = build_plan(&[song_item(None)], &config, Some(&index), None);

    let Some(ReadyAction::RestyleExisting {
        arrangement,
        transform,
        ..
    }) = plans[0].ready_action()
    else {
        panic!("expected a managed restyle action");
    };
    assert_eq!(arrangement.as_deref(), Some("Default"));
    assert!(transform.replacement_background().is_none());
    let crate::workflow::plan::MacroTransform::Enforce(macro_transitions) = transform.macros()
    else {
        panic!("restyle should enforce macros");
    };
    assert_eq!(macro_transitions.regions().len(), 1);
    assert_eq!(macro_transitions.regions()[0].enter_macro(), "Song");
}

#[test]
fn existing_song_uses_the_canonical_library_name() {
    let directory = tempdir().expect("fixture library directory");
    let path = directory
        .path()
        .join("[Hymn] Come, Thou Fount of Every Blessing.pro");
    write_song_with_arrangements(&path, &[]);
    let index = LibraryCatalog::build(directory.path()).expect("fixture library should index");
    let config = parse_project_config_str(
        r#"
            {
              "version": 4,
              "cue_roles": {
                "content": { "slide": "Content" }
              },
              "presentation_types": {
                "song": {
                  "kind": "song",
                  "content_source": "song",
                  "output_strategy": "preserve_existing"
                }
              },
              "item_rules": [{
                "id": "come_thou_fount",
                "match": { "category": "song" },
                "use_type": "song",
                "target": {
                  "library_file": "[Hymn] Come, Thou Fount of Every Blessing.pro"
                }
              }]
            }
            "#,
    )
    .expect("song config should parse");
    let mut item = song_item(Some("Default Arrangement"));
    item.title = "#356 Come, Thou Fount of Every Blessing".to_string();
    if let Some(song) = &mut item.song {
        song.title.clone_from(&item.title);
    }

    let plans = build_plan(&[item], &config, Some(&index), None);

    assert!(is_use_existing(&plans[0]));
    assert_eq!(
        plans[0].playlist_name,
        "[Hymn] Come, Thou Fount of Every Blessing"
    );
    assert_eq!(plans[0].arrangement(), None);
}

#[test]
fn unavailable_ambiguous_and_incomplete_arrangements_require_review() {
    let config = song_config(None, None);

    let (_missing_directory, missing_index) = song_index(&[
        ("Ordinary", Some("550e8400-e29b-41d4-a716-446655440001")),
        ("Seasonal", Some("550e8400-e29b-41d4-a716-446655440002")),
    ]);
    let missing = build_plan(
        &[song_item(Some("Missing"))],
        &config,
        Some(&missing_index),
        None,
    );
    assert!(missing[0].needs_review());
    assert!(missing[0].reason.contains("is unavailable"));
    assert!(missing[0].reason.contains("Ordinary, Seasonal"));

    let (_ambiguous_directory, ambiguous_index) = song_index(&[
        ("Default", Some("550e8400-e29b-41d4-a716-446655440001")),
        ("default", Some("550e8400-e29b-41d4-a716-446655440002")),
    ]);
    let ambiguous = build_plan(
        &[song_item(Some("DEFAULT"))],
        &config,
        Some(&ambiguous_index),
        None,
    );
    assert!(ambiguous[0].needs_review());
    assert!(ambiguous[0].reason.contains("is ambiguous"));
    assert!(ambiguous[0].reason.contains("Default, default"));

    let (_incomplete_directory, incomplete_index) = song_index(&[("Broken", None)]);
    let incomplete = build_plan(
        &[song_item(Some("Broken"))],
        &config,
        Some(&incomplete_index),
        None,
    );
    assert!(incomplete[0].needs_review());
    assert!(incomplete[0]
        .reason
        .contains("has a missing or invalid UUID"));
    assert!(incomplete[0]
        .reason
        .contains("available arrangements: Broken"));
}

#[test]
fn requested_arrangement_uses_the_canonical_native_casing() {
    let config = song_config(None, None);
    let (_directory, index) =
        song_index(&[("Verse Order", Some("550e8400-e29b-41d4-a716-446655440001"))]);

    let plans = build_plan(
        &[song_item(Some("verse order"))],
        &config,
        Some(&index),
        None,
    );

    assert!(is_use_existing(&plans[0]));
    assert_eq!(plans[0].arrangement(), Some("Verse Order"));
}

#[test]
fn explicit_generic_and_song_targets_never_use_fuzzy_matches() {
    let config = explicit_library_target_config();
    let library_dir = tempdir().expect("tempdir");
    write_library_presentation(&library_dir.path().join("Weekly Announcements.pro"));
    write_library_presentation(&library_dir.path().join("[Hymn] Amazing Grace.pro"));
    let index = LibraryCatalog::build(library_dir.path()).expect("fixture library should index");

    let plans = build_plan(
        &explicit_library_target_items(),
        &config,
        Some(&index),
        None,
    );

    assert!(plans[0].needs_review());
    assert_eq!(
        plans[0].reason,
        "Configured existing file not found: Announcements.pro"
    );
    assert_eq!(plans[0].file_path(), None);
    assert!(plans[1].needs_review());
    assert_eq!(
        plans[1].reason,
        "Configured existing song not found: Amazing Grace.pro"
    );
    assert_eq!(plans[1].file_path(), None);
}

#[test]
fn explicit_generic_and_song_targets_reject_duplicate_filenames() {
    let config = explicit_library_target_config();
    let library_dir = tempdir().expect("tempdir");
    let nested = library_dir.path().join("nested");
    std::fs::create_dir(&nested).expect("nested fixture directory");
    for (root_name, nested_name) in [
        ("Announcements.pro", "announcements.pro"),
        ("Amazing Grace.pro", "AMAZING GRACE.pro"),
    ] {
        write_library_presentation(&library_dir.path().join(root_name));
        write_library_presentation(&nested.join(nested_name));
    }
    let index = LibraryCatalog::build(library_dir.path()).expect("fixture library should index");

    let plans = build_plan(
        &explicit_library_target_items(),
        &config,
        Some(&index),
        None,
    );

    assert!(plans[0].needs_review());
    assert_eq!(
        plans[0].reason,
        "Configured existing file is ambiguous: Announcements.pro"
    );
    assert_eq!(plans[0].file_path(), None);
    assert!(plans[1].needs_review());
    assert_eq!(
        plans[1].reason,
        "Configured existing song target is ambiguous: Amazing Grace.pro"
    );
    assert_eq!(plans[1].file_path(), None);
}
