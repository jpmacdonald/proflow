#![allow(clippy::expect_used, clippy::panic)]

use chrono::{DateTime, TimeZone, Utc};
use httptest::{
    matchers::request,
    matchers::{all_of, contains, url_decoded},
    responders::{json_encoded, status_code},
    Expectation, Server,
};
use proptest::prelude::*;
use reqwest::Url;
use serde_json::json;

use super::*;
use crate::planning_center::types::Category;

fn test_config() -> Config {
    Config {
        pco_app_id: "dummy-app".to_string(),
        pco_secret: "dummy-secret".to_string(),
    }
}

fn test_client(base_url: String) -> PlanningCenterClient {
    PlanningCenterClient::new_with_base_url(&test_config(), base_url)
        .expect("test credentials and HTTP client settings are valid")
}

#[test]
fn new_rejects_missing_or_blank_credentials() {
    for config in [
        Config::default(),
        Config {
            pco_app_id: "   ".to_string(),
            pco_secret: "secret".to_string(),
        },
        Config {
            pco_app_id: "app".to_string(),
            pco_secret: "\t".to_string(),
        },
    ] {
        let error = PlanningCenterClient::new(&config)
            .err()
            .expect("missing or blank credentials must prevent client construction");
        assert!(error
            .to_string()
            .contains("credentials are missing or blank"));
    }
}

#[test]
fn join_base_and_path_normalizes_slashes() {
    assert_eq!(
        join_base_and_path("https://example.test/", "/service_types"),
        "https://example.test/service_types"
    );
    assert_eq!(
        join_base_and_path("https://example.test", "plans/123/items"),
        "https://example.test/plans/123/items"
    );
}

#[test]
fn resource_url_treats_identifiers_as_single_encoded_path_segments() {
    assert_eq!(
        resource_url(
            "https://api.example.test/services/v2",
            &["service_types", "service/id", "plans", "plan id"],
        )
        .expect("resource URL"),
        "https://api.example.test/services/v2/service_types/service%2Fid/plans/plan%20id"
    );
}

#[test]
fn resolve_pagination_url_accepts_relative_same_origin_links() {
    let resolved = resolve_pagination_url(
        "https://api.example.test/services/v2",
        "https://api.example.test/services/v2/service_types?page=1",
        "?page=2",
    )
    .expect("same-origin pagination URL should resolve");

    assert_eq!(
        resolved,
        "https://api.example.test/services/v2/service_types?page=2"
    );
}

proptest! {
    #[test]
    fn property_relative_pagination_links_remain_same_origin(
        path in "/[a-z0-9][a-z0-9/_]{0,64}",
        page in 0_u16..10_000,
    ) {
        let base = "https://api.example.test/services/v2";
        let current = "https://api.example.test/services/v2/service_types?page=1";
        let next = format!("{path}?page={page}");
        let resolved = resolve_pagination_url(base, current, &next)
            .expect("generated relative URL should resolve");
        let resolved = Url::parse(&resolved).expect("resolved URL should parse");

        prop_assert_eq!(resolved.scheme(), "https");
        prop_assert_eq!(resolved.host_str(), Some("api.example.test"));
        prop_assert_eq!(resolved.port_or_known_default(), Some(443));
    }
}

#[tokio::test]
async fn get_paginated_with_query_accumulates_pages() {
    let server = Server::run();
    let base_url = server.url_str("/").trim_end_matches('/').to_string();

    server.expect(
        Expectation::matching(all_of![
            request::method("GET"),
            request::path("/service_types"),
            request::query(url_decoded(contains(("per_page", "25")))),
        ])
        .respond_with(json_encoded(json!({
            "data": [
                { "id": "1", "attributes": { "name": "Sunday" } }
            ],
            "links": {
                "next": format!("{base_url}/service_types?page=2")
            }
        }))),
    );

    server.expect(
        Expectation::matching(all_of![
            request::method("GET"),
            request::path("/service_types"),
            request::query(url_decoded(contains(("page", "2")))),
        ])
        .respond_with(json_encoded(json!({
            "data": [
                { "id": "2", "attributes": { "name": "Wednesday" } }
            ],
            "links": {
                "next": null
            }
        }))),
    );

    let client = test_client(base_url);
    let response = client
        .get_paginated_with_query("/service_types", &[("per_page", "25")])
        .await
        .expect("pagination should succeed");

    let ids: Vec<_> = response
        .data
        .iter()
        .filter_map(|item| item["id"].as_str())
        .collect();

    assert_eq!(ids, vec!["1", "2"]);
}

#[tokio::test]
async fn get_paginated_with_query_rejects_cross_origin_next_link() {
    let server = Server::run();
    let base_url = server.url_str("/").trim_end_matches('/').to_string();

    server.expect(
        Expectation::matching(all_of![
            request::method("GET"),
            request::path("/service_types"),
        ])
        .respond_with(json_encoded(json!({
            "data": [],
            "links": {
                "next": "https://attacker.invalid/collect"
            }
        }))),
    );

    let client = test_client(base_url);
    let error = client
        .get_paginated_with_query("/service_types", &[])
        .await
        .expect_err("cross-origin pagination URL should fail");

    assert!(error.to_string().contains("different origin"));
}

#[tokio::test]
async fn fetch_plans_for_service_uses_server_bounded_date_range() {
    let server = Server::run();
    let base_url = server.url_str("/").trim_end_matches('/').to_string();

    server.expect(
        Expectation::matching(all_of![
            request::method("GET"),
            request::path("/service_types/1/plans"),
            request::query(url_decoded(contains(("filter", "after,before")))),
            request::query(url_decoded(contains(("after", "2026-07-05")))),
            request::query(url_decoded(contains(("before", "2026-07-12")))),
            request::query(url_decoded(contains(("order", "sort_date")))),
            request::query(url_decoded(contains(("per_page", "25")))),
        ])
        .respond_with(json_encoded(json!({
            "data": [
                {
                    "id": "in-range",
                    "attributes": {
                        "sort_date": "2026-07-12T09:00:00Z",
                        "title": "July 12"
                    }
                },
                {
                    "id": "outside-exact-window",
                    "attributes": {
                        "sort_date": "2026-07-12T18:00:01Z",
                        "title": "Later July 12"
                    }
                }
            ],
            "links": { "next": null }
        }))),
    );

    let start = DateTime::parse_from_rfc3339("2026-07-05T06:00:00Z")
        .expect("valid start date")
        .with_timezone(&Utc);
    let end = DateTime::parse_from_rfc3339("2026-07-12T18:00:00Z")
        .expect("valid end date")
        .with_timezone(&Utc);
    let client = test_client(base_url);

    let plans = client
        .fetch_plans_for_service_in_range("1", "Sunday", start, end)
        .await
        .expect("bounded plan request should succeed");

    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].id, "in-range");
}

#[tokio::test]
async fn get_upcoming_services_fails_if_any_service_plan_fetch_fails() {
    let server = Server::run();
    let base_url = server.url_str("/").trim_end_matches('/').to_string();

    server.expect(
        Expectation::matching(all_of![
            request::method("GET"),
            request::path("/service_types"),
        ])
        .respond_with(json_encoded(json!({
            "data": [
                { "id": "1", "attributes": { "name": "Sunday" } },
                { "id": "2", "attributes": { "name": "Wednesday" } }
            ],
            "links": { "next": null }
        }))),
    );
    server.expect(
        Expectation::matching(all_of![
            request::method("GET"),
            request::path("/service_types/1/plans"),
        ])
        .respond_with(json_encoded(json!({
            "data": [],
            "links": { "next": null }
        }))),
    );
    server.expect(
        Expectation::matching(all_of![
            request::method("GET"),
            request::path("/service_types/2/plans"),
        ])
        .respond_with(status_code(400)),
    );

    let client = test_client(base_url);
    let error = client
        .get_upcoming_services(30)
        .await
        .expect_err("partial service plan results should fail");

    assert!(error.to_string().contains("Wednesday"));
}

#[tokio::test]
async fn get_service_items_merges_pages_and_orders_by_sequence() {
    let server = Server::run();
    let base_url = server.url_str("/").trim_end_matches('/').to_string();

    server.expect(
        Expectation::matching(all_of![
            request::method("GET"),
            request::path("/plans/plan-1/items"),
            request::query(url_decoded(contains(("include", "song,arrangement")))),
            request::query(url_decoded(contains(("per_page", "100")))),
        ])
        .respond_with(json_encoded(json!({
            "data": [
                {
                    "id": "item-3",
                    "attributes": {
                        "title": "Amazing Grace",
                        "description": "Opening song",
                        "notes": "Sing all verses",
                        "sequence": 30
                    },
                    "relationships": {
                        "song": { "data": { "id": "song-1" } },
                        "arrangement": { "data": { "id": "arr-1" } }
                    }
                }
            ],
            "included": [
                {
                    "type": "Song",
                    "id": "song-1",
                    "attributes": {
                        "title": "Amazing Grace",
                        "author": "John Newton",
                        "copyright": "Public Domain",
                        "ccli_number": "12345"
                    }
                }
            ],
            "links": { "next": format!("{base_url}/plans/plan-1/items?page=2") }
        }))),
    );

    server.expect(
        Expectation::matching(all_of![
            request::method("GET"),
            request::path("/plans/plan-1/items"),
            request::query(url_decoded(contains(("page", "2")))),
        ])
        .respond_with(json_encoded(json!({
            "data": [
                {
                    "id": "item-2",
                    "attributes": { "title": "Sermon", "sequence": 20 },
                    "relationships": {}
                },
                {
                    "id": "item-1",
                    "attributes": { "title": "Welcome", "sequence": 10 },
                    "relationships": {}
                }
            ],
            "included": [
                {
                    "type": "Arrangement",
                    "id": "arr-1",
                    "attributes": {
                        "name": "Default Arrangement",
                        "lyrics": "[Verse 1]\nAmazing grace"
                    }
                }
            ],
            "links": { "next": null }
        }))),
    );

    let client = test_client(base_url);
    let items = client
        .get_service_items("plan-1")
        .await
        .expect("service items should resolve merged includes");

    assert_eq!(
        items
            .iter()
            .map(|item| (item.id.as_str(), item.position))
            .collect::<Vec<_>>(),
        vec![("item-1", 10), ("item-2", 20), ("item-3", 30)]
    );

    let item = &items[2];
    assert_eq!(item.title, "Amazing Grace");
    assert_eq!(item.category, Category::Song);

    let song = item.song.as_ref().expect("song data should be linked");
    assert_eq!(song.title, "Amazing Grace");
    assert_eq!(song.author.as_deref(), Some("John Newton"));
    assert_eq!(song.arrangement.as_deref(), Some("Default Arrangement"));
    assert_eq!(song.lyrics.as_deref(), Some("[Verse 1]\nAmazing grace"));
}

#[tokio::test]
async fn get_service_items_rejects_an_entry_without_a_stable_id() {
    let server = Server::run();
    let base_url = server.url_str("/").trim_end_matches('/').to_string();
    server.expect(
        Expectation::matching(all_of![
            request::method("GET"),
            request::path("/plans/plan-1/items"),
        ])
        .respond_with(json_encoded(json!({
            "data": [{
                "attributes": { "title": "Unidentified item" },
                "relationships": {}
            }],
            "included": [],
            "links": { "next": null }
        }))),
    );

    let client = test_client(base_url);
    let error = client
        .get_service_items("plan-1")
        .await
        .expect_err("an item without an id must not disappear from the plan");

    assert!(error
        .to_string()
        .contains("response index 0 is missing required field 'id'"));
}

#[tokio::test]
async fn capture_plan_snapshot_by_service_name_supports_historical_plans() {
    let server = Server::run();
    let base_url = server.url_str("/").trim_end_matches('/').to_string();

    server.expect(
        Expectation::matching(all_of![
            request::method("GET"),
            request::path("/service_types"),
        ])
        .respond_with(json_encoded(json!({
            "data": [{
                "id": "service-1",
                "attributes": { "name": "9:00am contemporary" }
            }],
            "links": { "next": null }
        }))),
    );
    server.expect(
        Expectation::matching(all_of![
            request::method("GET"),
            request::path("/service_types/service-1"),
        ])
        .times(2)
        .respond_with(json_encoded(json!({
            "data": {
                "id": "service-1",
                "attributes": { "name": "9:00am contemporary" }
            }
        }))),
    );
    server.expect(
        Expectation::matching(all_of![
            request::method("GET"),
            request::path("/service_types/service-1/plans/plan-1"),
        ])
        .times(2)
        .respond_with(json_encoded(json!({
            "data": {
                "id": "plan-1",
                "attributes": {
                    "sort_date": "2026-07-05T13:00:00Z",
                    "title": "July 5"
                }
            }
        }))),
    );
    server.expect(
        Expectation::matching(all_of![
            request::method("GET"),
            request::path("/plans/plan-1/items"),
        ])
        .times(2)
        .respond_with(json_encoded(json!({
            "data": [{
                "id": "item-1",
                "attributes": { "title": "Welcome", "sequence": 10 },
                "relationships": {}
            }],
            "included": [],
            "links": { "next": null }
        }))),
    );

    let snapshot = test_client(base_url)
        .capture_plan_snapshot("plan-1", "9:00am contemporary")
        .await
        .expect("historical exact plan snapshot");

    assert_eq!(snapshot.plan_id(), "plan-1");
    assert_eq!(snapshot.service_id(), "service-1");
    assert_eq!(
        snapshot.default_playlist_name(),
        "July 5, 2026 - 9am Contemporary"
    );
    assert_eq!(snapshot.items().len(), 1);
}

#[tokio::test]
async fn refresh_plan_snapshot_refetches_stable_resources_without_an_upcoming_window() {
    let server = Server::run();
    let base_url = server.url_str("/").trim_end_matches('/').to_string();

    server.expect(
        Expectation::matching(all_of![
            request::method("GET"),
            request::path("/service_types/service-1"),
        ])
        .times(2)
        .respond_with(json_encoded(json!({
            "data": {
                "id": "service-1",
                "attributes": { "name": "9:00am contemporary" }
            }
        }))),
    );
    server.expect(
        Expectation::matching(all_of![
            request::method("GET"),
            request::path("/service_types/service-1/plans/plan-1"),
        ])
        .times(2)
        .respond_with(json_encoded(json!({
            "data": {
                "id": "plan-1",
                "attributes": {
                    "sort_date": "2026-07-26T13:00:00Z",
                    "title": "July 26"
                }
            }
        }))),
    );
    server.expect(
        Expectation::matching(all_of![
            request::method("GET"),
            request::path("/plans/plan-1/items"),
            request::query(url_decoded(contains(("include", "song,arrangement")))),
        ])
        .times(2)
        .respond_with(json_encoded(json!({
            "data": [{
                "id": "item-1",
                "attributes": {
                    "title": "Welcome",
                    "description": "Good morning",
                    "sequence": 10
                },
                "relationships": {}
            }],
            "included": [],
            "links": { "next": null }
        }))),
    );

    let reviewed = PlanSnapshot::from_resolved(
        crate::planning_center::identity::ResolvedPlanIdentity {
            plan_id: "plan-1".to_string(),
            service_id: "service-1".to_string(),
            service_name: "9:00am contemporary".to_string(),
            plan_title: "July 26".to_string(),
            date: Utc
                .with_ymd_and_hms(2026, 7, 26, 13, 0, 0)
                .single()
                .expect("valid date"),
            default_playlist_name: "July 26, 2026 - 9am Contemporary".to_string(),
        },
        Vec::new(),
    );

    let current = test_client(base_url)
        .refresh_plan_snapshot(&reviewed)
        .await
        .expect("direct freshness refetch");

    assert_eq!(current.plan_id(), "plan-1");
    assert_eq!(current.service_id(), "service-1");
    assert_eq!(
        current.default_playlist_name(),
        reviewed.default_playlist_name()
    );
    assert_eq!(current.items().len(), 1);
    assert_eq!(current.items()[0].id, "item-1");
    assert_eq!(current.items()[0].position, 10);
}

#[tokio::test]
async fn refresh_plan_snapshot_rejects_two_different_consecutive_reads() {
    let server = Server::run();
    let base_url = server.url_str("/").trim_end_matches('/').to_string();
    server.expect(
        Expectation::matching(all_of![
            request::method("GET"),
            request::path("/service_types/service-1"),
        ])
        .times(2)
        .respond_with(json_encoded(json!({
            "data": {
                "id": "service-1",
                "attributes": { "name": "Sunday Morning" }
            }
        }))),
    );
    server.expect(
        Expectation::matching(all_of![
            request::method("GET"),
            request::path("/service_types/service-1/plans/plan-1"),
        ])
        .times(2)
        .respond_with(json_encoded(json!({
            "data": {
                "id": "plan-1",
                "attributes": {
                    "sort_date": "2026-07-26T13:00:00Z",
                    "title": "July 26"
                }
            }
        }))),
    );
    server.expect(
        Expectation::matching(all_of![
            request::method("GET"),
            request::path("/plans/plan-1/items"),
        ])
        .times(2)
        .respond_with(httptest::cycle![
            json_encoded(json!({
                "data": [{
                    "id": "item-1",
                    "attributes": { "title": "Before", "sequence": 10 },
                    "relationships": {}
                }],
                "included": [],
                "links": { "next": null }
            })),
            json_encoded(json!({
                "data": [{
                    "id": "item-1",
                    "attributes": { "title": "After", "sequence": 10 },
                    "relationships": {}
                }],
                "included": [],
                "links": { "next": null }
            }))
        ]),
    );
    let reviewed = PlanSnapshot::from_resolved(
        crate::planning_center::identity::ResolvedPlanIdentity {
            plan_id: "plan-1".to_string(),
            service_id: "service-1".to_string(),
            service_name: "Sunday Morning".to_string(),
            plan_title: "July 26".to_string(),
            date: Utc
                .with_ymd_and_hms(2026, 7, 26, 13, 0, 0)
                .single()
                .expect("valid date"),
            default_playlist_name: "July 26, 2026 - Sunday Morning".to_string(),
        },
        Vec::new(),
    );

    let error = test_client(base_url)
        .refresh_plan_snapshot(&reviewed)
        .await
        .expect_err("unstable direct reads cannot become a reviewed snapshot");

    assert!(matches!(
        error,
        Error::PlanningCenterSnapshotUnstable { plan_id, .. } if plan_id == "plan-1"
    ));
}
