#![allow(clippy::expect_used)]

use serde_json::json;

use super::*;

#[test]
fn service_type_parser_rejects_missing_identity_and_name() {
    let missing_id = parse_service_types(&[json!({
        "attributes": { "name": "Sunday" }
    })])
    .expect_err("a service type without an id must not disappear");
    assert!(matches!(
        missing_id,
        PlanningCenterSourceError::MissingField { field: "id", .. }
    ));

    let missing_name = parse_service_types(&[json!({
        "id": "service-1",
        "attributes": {}
    })])
    .expect_err("a missing service name must not become Unknown");
    assert!(matches!(
        missing_name,
        PlanningCenterSourceError::MissingField { field: "name", .. }
    ));
}

#[test]
fn plan_parser_rejects_missing_identity_date_and_title() {
    let missing_id = parse_plan(
        &json!({
            "attributes": {
                "sort_date": "2026-07-19T09:00:00Z",
                "title": "July 19"
            }
        }),
        0,
        "service-1",
        "Sunday",
    )
    .expect_err("a plan without an id must not disappear");
    assert!(matches!(
        missing_id,
        PlanningCenterSourceError::MissingField { field: "id", .. }
    ));

    let missing_date = parse_plan(
        &json!({
            "id": "plan-1",
            "attributes": { "title": "July 19" }
        }),
        0,
        "service-1",
        "Sunday",
    )
    .expect_err("a plan without a date must not disappear");
    assert!(matches!(
        missing_date,
        PlanningCenterSourceError::MissingField {
            field: "sort_date",
            ..
        }
    ));

    let missing_title = parse_plan(
        &json!({
            "id": "plan-1",
            "attributes": { "sort_date": "2026-07-19T09:00:00Z" }
        }),
        0,
        "service-1",
        "Sunday",
    )
    .expect_err("a missing plan title must not become Untitled Plan");
    assert!(matches!(
        missing_title,
        PlanningCenterSourceError::MissingPlanTitle { .. }
    ));
}

#[test]
fn item_parser_rejects_missing_title() {
    let error = parse_items(
        &[json!({
            "id": "item-1",
            "attributes": {},
            "relationships": {}
        })],
        &[],
        "plan-1",
    )
    .expect_err("a missing item title must not become Untitled");

    assert!(matches!(
        error,
        PlanningCenterSourceError::MissingField { field: "title", .. }
    ));
}

#[test]
fn item_parser_requires_an_integer_sequence() {
    let missing = parse_items(
        &[json!({
            "id": "item-1",
            "attributes": { "title": "Welcome" },
            "relationships": {}
        })],
        &[],
        "plan-1",
    )
    .expect_err("a missing sequence must not inherit response order");
    assert!(matches!(
        missing,
        PlanningCenterSourceError::MissingField {
            field: "sequence",
            ..
        }
    ));

    for invalid in [
        json!(null),
        json!("1"),
        json!(-1),
        json!(1.0),
        json!(1.5),
        json!(true),
    ] {
        let error = parse_items(
            &[json!({
                "id": "item-1",
                "attributes": { "title": "Welcome", "sequence": invalid },
                "relationships": {}
            })],
            &[],
            "plan-1",
        )
        .expect_err("a non-integer sequence must not become a position");
        assert!(matches!(
            error,
            PlanningCenterSourceError::InvalidFieldType {
                field: "sequence",
                ..
            }
        ));
    }
}

#[test]
fn item_parser_rejects_duplicate_sequences() {
    let error = parse_items(
        &[
            json!({
                "id": "item-a",
                "attributes": { "title": "Welcome", "sequence": 20 },
                "relationships": {}
            }),
            json!({
                "id": "item-b",
                "attributes": { "title": "Sermon", "sequence": 20 },
                "relationships": {}
            }),
        ],
        &[],
        "plan-1",
    )
    .expect_err("duplicate sequences make the service order ambiguous");

    assert!(matches!(
        error,
        PlanningCenterSourceError::DuplicateItemSequence {
            sequence: 20,
            first_item_id,
            duplicate_item_id,
            ..
        } if first_item_id == "item-a" && duplicate_item_id == "item-b"
    ));
}

#[test]
fn declared_song_relationship_requires_included_song() {
    let error = parse_items(
        &[json!({
            "id": "item-1",
            "attributes": { "title": "Amazing Grace", "sequence": 10 },
            "relationships": {
                "song": { "data": { "id": "song-1" } }
            }
        })],
        &[],
        "plan-1",
    )
    .expect_err("a declared song cannot be reclassified when its include is missing");

    assert!(matches!(
        error,
        PlanningCenterSourceError::MissingIncludedRelationship {
            relationship: "song",
            target_type: "Song",
            target_id,
            ..
        } if target_id == "song-1"
    ));
}

#[test]
fn declared_arrangement_relationship_requires_included_arrangement() {
    let included = [json!({
        "type": "Song",
        "id": "song-1",
        "attributes": { "title": "Amazing Grace" }
    })];
    let error = parse_items(
        &[json!({
            "id": "item-1",
            "attributes": { "title": "Amazing Grace", "sequence": 10 },
            "relationships": {
                "song": { "data": { "id": "song-1" } },
                "arrangement": { "data": { "id": "arrangement-1" } }
            }
        })],
        &included,
        "plan-1",
    )
    .expect_err("a declared arrangement cannot silently disappear");

    assert!(matches!(
        error,
        PlanningCenterSourceError::MissingIncludedRelationship {
            relationship: "arrangement",
            target_type: "Arrangement",
            target_id,
            ..
        } if target_id == "arrangement-1"
    ));
}

#[test]
fn included_song_without_identity_is_rejected() {
    let error = parse_items(
        &[],
        &[json!({
            "type": "Song",
            "attributes": { "title": "Amazing Grace" }
        })],
        "plan-1",
    )
    .expect_err("an included song without an id must not disappear from the catalog");

    assert!(matches!(
        error,
        PlanningCenterSourceError::MissingField { field: "id", .. }
    ));
}

#[test]
fn item_classification_is_case_insensitive() {
    assert_eq!(classify_item("scripture - John 1", false), Category::Title);
    assert_eq!(classify_item("WELCOME", false), Category::Graphic);
    assert_eq!(classify_item("Sunday SERMON", false), Category::Title);
}
