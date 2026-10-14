use super::*;

#[test]
#[ignore = "requires the live project data root; run `just live-data`"]
fn repo_rule_matrix_keeps_named_exceptions_ahead_of_song_fallbacks() {
    let config = crate::project_config::tests::live_project_config();

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
        let mut item = if title == "Prayer of Confession" {
            test_text_item(
                "rule-order-confession",
                1,
                title,
                Some("Leader: Let us pray."),
            )
        } else {
            song_item(None)
        };
        item.id = format!("rule-order-{title}");
        item.title = title.to_string();
        let plans = build_plan(&[item], &config, None, Some(service));
        let plan = plans
            .iter()
            .find(|plan| plan.pco_title == title)
            .expect("classified plan item");

        assert_eq!(
            plan.item_type().map(PresentationTypeKey::as_str),
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
        affirmation.item_type().map(PresentationTypeKey::as_str),
        Some("liturgical_audience_generated"),
        "text-category affirmations must use the liturgy rule"
    );
}

#[test]
#[ignore = "requires the live project data root; run `just live-data`"]
fn repo_expansion_rule_matrix_keeps_speaker_nametag_and_liturgy_order() {
    let config = crate::project_config::tests::live_project_config();

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
            .map(|plan| {
                plan.item_type()
                    .map(PresentationTypeKey::as_str)
                    .expect("expanded item type")
            })
            .collect::<Vec<_>>();
        assert_eq!(actual_types, expected_types, "wrong expansion for {title}");
    }
}

#[test]
#[ignore = "requires the live project data root; run `just live-data`"]
fn repo_native_liturgy_routes_select_the_reviewed_files() {
    let config = crate::project_config::tests::live_project_config();
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
        assert_eq!(
            plan.item_type().map(PresentationTypeKey::as_str),
            Some("preserved_liturgy_static")
        );
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
