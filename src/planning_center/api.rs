use chrono::{DateTime, Duration, Utc};
use futures::{stream, StreamExt};
use reqwest::{Client, Url};
use serde_json::Value;
use std::time::Duration as StdDuration;
use tokio::time::sleep;

use crate::config::Config;
use crate::error::{Error, Result};
use crate::planning_center::types::{Category, Item, Plan, Scripture, Service, Song};

const BASE_URL: &str = "https://api.planningcenteronline.com/services/v2";

/// Retry configuration for API requests
const MAX_RETRIES: u32 = 3;
/// Initial backoff delay in milliseconds before the first retry
const INITIAL_BACKOFF_MS: u64 = 500;
/// Maximum backoff delay cap in milliseconds
const MAX_BACKOFF_MS: u64 = 10_000;
/// Maximum number of Planning Center requests issued concurrently during fan-out.
const MAX_CONCURRENT_REQUESTS: usize = 4;

/// Client for accessing Planning Center Online API
///
/// Uses concurrent requests when fetching plans for multiple service types,
/// which significantly improves performance when there are many service types.
#[derive(Clone)]
pub struct PlanningCenterClient {
    /// Application ID for API authentication
    app_id: String,
    /// Secret key for API authentication
    secret: String,
    /// HTTP client with timeout configuration
    client: Client,
    /// Base URL used for API requests (allows overriding in tests)
    base_url: String,
}

#[derive(Debug, Default)]
struct PaginatedResponse {
    data: Vec<Value>,
    included: Vec<Value>,
}

impl PlanningCenterClient {
    /// Create a new Planning Center client from config
    pub fn new(config: &Config) -> Self {
        Self::new_with_base_url(config, BASE_URL)
    }

    fn new_with_base_url(config: &Config, base_url: impl Into<String>) -> Self {
        let base_url = base_url.into();
        Self::with_base_url(config, normalize_base_url(&base_url))
    }

    fn with_base_url(config: &Config, base_url: String) -> Self {
        // Client::build() should never fail with default settings, but if it does,
        // we create a client without timeout rather than silently failing
        let client = Client::builder()
            .timeout(StdDuration::from_secs(30))
            .no_proxy()
            .build()
            .unwrap_or_else(|e| {
                tracing::warn!(
                    "Failed to create HTTP client with timeout, using default client: {e}"
                );
                Client::default()
            });
        Self {
            app_id: config.pco_app_id.clone(),
            secret: config.pco_secret.clone(),
            client,
            base_url,
        }
    }

    /// Check if credentials are configured
    const fn is_configured(&self) -> bool {
        // String::is_empty is not const, so we check len directly
        !self.app_id.is_empty() && !self.secret.is_empty()
    }

    /// Internal method that performs the actual request with retry logic
    async fn get_url_with_retry(&self, url: &str, query: &[(&str, &str)]) -> Result<Value> {
        let label = url.strip_prefix(&self.base_url).unwrap_or(url);
        let mut last_error: Option<Error> = None;
        let mut backoff_ms = INITIAL_BACKOFF_MS;

        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                tracing::info!(
                    "Retrying request to {label} (attempt {}/{})",
                    attempt + 1,
                    MAX_RETRIES + 1
                );
                sleep(StdDuration::from_millis(backoff_ms)).await;
                backoff_ms = (backoff_ms * 2).min(MAX_BACKOFF_MS);
            }

            let request = self
                .client
                .get(url)
                .basic_auth(&self.app_id, Some(&self.secret))
                .header("Content-Type", "application/json");

            let request = if query.is_empty() {
                request
            } else {
                request.query(query)
            };

            match request.send().await {
                Ok(resp) => {
                    let status = resp.status();

                    // Don't retry client errors (4xx) except 429 (rate limit)
                    if status.is_client_error() && status.as_u16() != 429 {
                        return Err(Error::pco_status(
                            format!("Request to {label} returned {status}"),
                            status.as_u16(),
                        ));
                    }

                    // Retry on server errors (5xx) or rate limiting (429)
                    if status.is_server_error() || status.as_u16() == 429 {
                        last_error = Some(Error::pco_status(
                            format!("Request to {label} returned {status}"),
                            status.as_u16(),
                        ));
                        continue;
                    }

                    if !status.is_success() {
                        return Err(Error::pco_status(
                            format!("Request to {label} returned {status}"),
                            status.as_u16(),
                        ));
                    }

                    return resp.json().await.map_err(|e| {
                        Error::parse(format!("Invalid JSON from {label}: {e}"), None)
                    });
                }
                Err(e) => {
                    // Network errors are retryable
                    if e.is_timeout() || e.is_connect() {
                        last_error =
                            Some(Error::Network(format!("Request to {label} failed: {e}")));
                        continue;
                    }
                    // Other errors are not retryable
                    return Err(Error::Network(format!("Request to {label} failed: {e}")));
                }
            }
        }

        // All retries exhausted
        Err(last_error.unwrap_or_else(|| {
            Error::Network(format!(
                "Request to {label} failed after {MAX_RETRIES} retries"
            ))
        }))
    }

    async fn get_paginated_with_query(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<PaginatedResponse> {
        let mut response = PaginatedResponse::default();
        let mut next_url = join_base_and_path(&self.base_url, path);
        let mut is_first_page = true;

        loop {
            let json = if is_first_page {
                self.get_url_with_retry(&next_url, query).await?
            } else {
                self.get_url_with_retry(&next_url, &[]).await?
            };
            is_first_page = false;

            let data = json["data"].as_array().ok_or_else(|| {
                Error::parse(
                    format!("Missing 'data' array in response from {path}"),
                    None,
                )
            })?;
            response.data.extend(data.iter().cloned());

            if let Some(included) = json["included"].as_array() {
                response.included.extend(included.iter().cloned());
            }

            let Some(next) = json["links"]["next"]
                .as_str()
                .filter(|next| !next.is_empty())
            else {
                break;
            };
            next_url = resolve_pagination_url(&self.base_url, &next_url, next)?;
        }

        Ok(response)
    }

    /// Get upcoming services and plans using bounded concurrent API calls.
    ///
    /// Returns an error instead of a partial plan set when any service-type
    /// request fails.
    pub async fn get_upcoming_services(
        &self,
        days_ahead: i64,
    ) -> Result<(Vec<Service>, Vec<Plan>)> {
        if !self.is_configured() {
            return Err(Error::config(
                "Planning Center client not configured",
                "Set PCO_APP_ID and PCO_SECRET environment variables",
            ));
        }

        // Fetch all service types
        let services = self.fetch_service_types().await?;

        let start_date = Utc::now();
        let end_date = start_date + Duration::days(days_ahead);
        let mut all_plans = self
            .fetch_plans_for_services(&services, start_date, end_date)
            .await?;

        // Sort services alphabetically, plans by date
        let mut sorted_services = services;
        sorted_services.sort_by(|a, b| a.name.cmp(&b.name));
        all_plans.sort_by(|a, b| a.date.cmp(&b.date));

        Ok((sorted_services, all_plans))
    }

    /// Get recent past services and plans using bounded concurrent API calls.
    ///
    /// Returns an error instead of a partial plan set when any service-type
    /// request fails.
    pub async fn get_recent_services(&self, days_back: i64) -> Result<(Vec<Service>, Vec<Plan>)> {
        if !self.is_configured() {
            return Err(Error::config(
                "Planning Center client not configured",
                "Set PCO_APP_ID and PCO_SECRET environment variables",
            ));
        }

        let services = self.fetch_service_types().await?;
        let now = Utc::now();
        let start_date = now - Duration::days(days_back);

        let mut all_plans = self
            .fetch_plans_for_services(&services, start_date, now)
            .await?;

        let mut sorted_services = services;
        sorted_services.sort_by(|a, b| a.name.cmp(&b.name));
        all_plans.sort_by(|a, b| a.date.cmp(&b.date));

        Ok((sorted_services, all_plans))
    }

    /// Fetch all service types
    async fn fetch_service_types(&self) -> Result<Vec<Service>> {
        let response = self.get_paginated_with_query("/service_types", &[]).await?;
        let entries = &response.data;

        Ok(entries
            .iter()
            .filter_map(|s| {
                let id = s["id"].as_str()?.to_string();
                let name = s["attributes"]["name"]
                    .as_str()
                    .unwrap_or("Unknown")
                    .to_string();
                Some(Service { id, name })
            })
            .collect())
    }

    async fn fetch_plans_for_service_in_range(
        &self,
        service_id: &str,
        service_name: &str,
        start_date: DateTime<Utc>,
        end_date: DateTime<Utc>,
    ) -> Result<Vec<Plan>> {
        let path = format!("/service_types/{service_id}/plans");
        // Planning Center accepts calendar dates for these filters. They bound
        // pagination at the server; the exact timestamps are enforced below.
        let after = start_date.format("%Y-%m-%d").to_string();
        let before = end_date.format("%Y-%m-%d").to_string();

        let response = self
            .get_paginated_with_query(
                &path,
                &[
                    ("filter", "after,before"),
                    ("after", after.as_str()),
                    ("before", before.as_str()),
                    ("order", "sort_date"),
                    ("per_page", "25"),
                ],
            )
            .await?;
        let entries = response.data.as_slice();

        Ok(entries
            .iter()
            .filter_map(|plan_value| {
                let id = plan_value["id"].as_str()?.to_string();
                let attrs = &plan_value["attributes"];

                #[allow(clippy::similar_names)]
                let date = DateTime::parse_from_rfc3339(attrs["sort_date"].as_str()?)
                    .ok()?
                    .with_timezone(&Utc);

                if date < start_date {
                    return None;
                }
                if date > end_date {
                    return None;
                }

                let title = attrs["title"]
                    .as_str()
                    .or_else(|| attrs["dates"].as_str())
                    .unwrap_or("Untitled Plan")
                    .to_string();

                Some(Plan {
                    id,
                    service_id: service_id.to_string(),
                    service_name: service_name.to_string(),
                    date,
                    title,
                    items: Vec::new(),
                })
            })
            .collect())
    }

    async fn fetch_plans_for_services(
        &self,
        services: &[Service],
        start_date: DateTime<Utc>,
        end_date: DateTime<Utc>,
    ) -> Result<Vec<Plan>> {
        let results = stream::iter(services.iter().cloned())
            .map(|service| async move {
                let result = self
                    .fetch_plans_for_service_in_range(
                        &service.id,
                        &service.name,
                        start_date,
                        end_date,
                    )
                    .await;
                (service.name, result)
            })
            .buffer_unordered(MAX_CONCURRENT_REQUESTS)
            .collect::<Vec<_>>()
            .await;

        let mut plans = Vec::new();
        let mut failures = Vec::new();
        for (service_name, result) in results {
            match result {
                Ok(service_plans) => plans.extend(service_plans),
                Err(error) => failures.push(format!("{service_name}: {error}")),
            }
        }

        if failures.is_empty() {
            Ok(plans)
        } else {
            Err(Error::pco(format!(
                "Failed to fetch plans for {} service type(s): {}",
                failures.len(),
                failures.join("; ")
            )))
        }
    }

    /// Get service items for a specific plan
    pub async fn get_service_items(&self, plan_id: &str) -> Result<Vec<Item>> {
        if !self.is_configured() {
            return Err(Error::config(
                "Planning Center client not configured",
                "Set PCO_APP_ID and PCO_SECRET environment variables",
            ));
        }

        let path = format!("/plans/{plan_id}/items");
        let response = self
            .get_paginated_with_query(
                &path,
                &[("include", "song,arrangement"), ("per_page", "100")],
            )
            .await?;
        let entries = response.data.as_slice();

        // Build lookup maps for included Song and Arrangement data
        let songs: std::collections::HashMap<_, _> = response
            .included
            .iter()
            .filter(|v| v["type"].as_str() == Some("Song"))
            .filter_map(|v| Some((v["id"].as_str()?, v)))
            .collect();
        let arrangements: std::collections::HashMap<_, _> = response
            .included
            .iter()
            .filter(|v| v["type"].as_str() == Some("Arrangement"))
            .filter_map(|v| Some((v["id"].as_str()?, v)))
            .collect();

        // Parse items
        let items = entries
            .iter()
            .enumerate()
            .map(|(idx, item_value)| {
                let id = item_value["id"].as_str().ok_or_else(|| {
                    Error::pco(format!(
                        "plan '{plan_id}' item at response index {idx} has no id"
                    ))
                })?;
                let attrs = &item_value["attributes"];
                let rels = &item_value["relationships"];

                let title = attrs["title"].as_str().unwrap_or("Untitled").to_string();
                let description = attrs["description"].as_str().map(String::from);
                let note = attrs["notes"].as_str().map(String::from);

                // Parse linked song if present
                let song = parse_song(rels, &songs, &arrangements);

                // Classify item
                let category = classify_item(&title, song.is_some());

                // Parse scripture reference using the bible module's parser,
                // which correctly handles verse ranges with dashes.
                let scripture =
                    if category == Category::Title && title.to_lowercase().contains("scripture") {
                        crate::bible::parse_scripture_ref(&title).map(|r| {
                            let reference = if let Some(end) = r.end_verse {
                                format!("{} {}:{}-{}", r.book, r.chapter, r.start_verse, end)
                            } else {
                                format!("{} {}:{}", r.book, r.chapter, r.start_verse)
                            };
                            Scripture {
                                reference,
                                text: description.clone(),
                                translation: None,
                            }
                        })
                    } else {
                        None
                    };

                Ok(Item {
                    id: id.to_string(),
                    position: idx + 1,
                    title,
                    description,
                    category,
                    note,
                    song,
                    scripture,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(items)
    }
}

/// Parse song data from relationships and included maps
fn parse_song(
    rels: &Value,
    songs: &std::collections::HashMap<&str, &Value>,
    arrangements: &std::collections::HashMap<&str, &Value>,
) -> Option<Song> {
    let song_id = rels.get("song")?.get("data")?.get("id")?.as_str()?;
    let song_value = songs.get(song_id)?;
    let attrs = &song_value["attributes"];

    let title = attrs["title"].as_str().unwrap_or("").to_string();
    let author = attrs["author"].as_str().map(String::from);
    let copyright = attrs["copyright"].as_str().map(String::from);
    let ccli = attrs["ccli_number"].as_str().map(String::from);

    // Get lyrics from arrangement
    let (lyrics, arrangement) = rels
        .get("arrangement")
        .and_then(|a| a.get("data")?.get("id")?.as_str())
        .and_then(|arr_id| arrangements.get(arr_id))
        .map_or((None, None), |arr| {
            let lyrics = arr["attributes"]["lyrics"].as_str().map(String::from);
            let name = arr["attributes"]["name"].as_str().map(String::from);
            (lyrics, name)
        });

    Some(Song {
        title,
        author,
        copyright,
        ccli,
        themes: None,
        lyrics,
        arrangement,
    })
}

/// Classify an item based on its title and whether it has song data
fn classify_item(title: &str, has_song: bool) -> Category {
    if has_song {
        return Category::Song;
    }

    let lowercase_title = title.to_lowercase();
    if ["scripture", "reading", "sermon", "message"]
        .iter()
        .any(|category| lowercase_title.contains(category))
    {
        Category::Title
    } else if ["announcements", "welcome"]
        .iter()
        .any(|category| lowercase_title.contains(category))
    {
        Category::Graphic
    } else if [
        "PRE-SERVICE",
        "SERVICE",
        "POST-SERVICE",
        "PRAISE",
        "OFFERING",
        "GIVING",
        "PRAYER",
        "LORD'S PRAYER",
        "GREETING",
    ]
    .iter()
    .any(|heading| title.to_uppercase().contains(heading))
    {
        Category::Other
    } else {
        Category::Text
    }
}

fn normalize_base_url(base_url: &str) -> String {
    base_url.trim_end_matches('/').to_string()
}

fn join_base_and_path(base_url: &str, path: &str) -> String {
    let normalized_base = base_url.trim_end_matches('/');
    let normalized_path = path.strip_prefix('/').unwrap_or(path);
    format!("{normalized_base}/{normalized_path}")
}

fn resolve_pagination_url(base_url: &str, current_url: &str, next: &str) -> Result<String> {
    let base = Url::parse(base_url)
        .map_err(|error| Error::pco(format!("Invalid Planning Center base URL: {error}")))?;
    let current = Url::parse(current_url)
        .map_err(|error| Error::pco(format!("Invalid Planning Center page URL: {error}")))?;
    let candidate = current
        .join(next)
        .map_err(|error| Error::pco(format!("Invalid Planning Center pagination URL: {error}")))?;

    let same_origin = base.scheme() == candidate.scheme()
        && base.host_str() == candidate.host_str()
        && base.port_or_known_default() == candidate.port_or_known_default();
    if !same_origin {
        return Err(Error::pco(
            "Rejected Planning Center pagination URL with a different origin",
        ));
    }

    Ok(candidate.into())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic)]

    use super::*;
    use httptest::{
        matchers::request,
        matchers::{all_of, contains, url_decoded},
        responders::{json_encoded, status_code},
        Expectation, Server,
    };
    use proptest::prelude::*;
    use serde_json::json;

    fn test_config() -> Config {
        Config {
            pco_app_id: "dummy-app".to_string(),
            pco_secret: "dummy-secret".to_string(),
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

        let client = PlanningCenterClient::new_with_base_url(&test_config(), base_url);
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

        let client = PlanningCenterClient::new_with_base_url(&test_config(), base_url);
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
        let client = PlanningCenterClient::new_with_base_url(&test_config(), base_url);

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

        let client = PlanningCenterClient::new_with_base_url(&test_config(), base_url);
        let error = client
            .get_upcoming_services(30)
            .await
            .expect_err("partial service plan results should fail");

        assert!(error.to_string().contains("Wednesday"));
    }

    #[tokio::test]
    async fn get_service_items_merges_paginated_included_payloads() {
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
                        "id": "item-1",
                        "attributes": {
                            "title": "Amazing Grace",
                            "description": "Opening song",
                            "notes": "Sing all verses"
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
                "links": {
                    "next": format!("{base_url}/plans/plan-1/items?page=2")
                }
            }))),
        );

        server.expect(
            Expectation::matching(all_of![
                request::method("GET"),
                request::path("/plans/plan-1/items"),
                request::query(url_decoded(contains(("page", "2")))),
            ])
            .respond_with(json_encoded(json!({
                "data": [],
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
                "links": {
                    "next": null
                }
            }))),
        );

        let client = PlanningCenterClient::new_with_base_url(&test_config(), base_url);
        let items = client
            .get_service_items("plan-1")
            .await
            .expect("service items should resolve merged includes");

        assert_eq!(items.len(), 1);

        let item = &items[0];
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

        let client = PlanningCenterClient::new_with_base_url(&test_config(), base_url);
        let error = client
            .get_service_items("plan-1")
            .await
            .expect_err("an item without an id must not disappear from the plan");

        assert!(error.to_string().contains("response index 0 has no id"));
    }

    #[test]
    fn item_classification_is_case_insensitive() {
        assert_eq!(classify_item("scripture - John 1", false), Category::Title);
        assert_eq!(classify_item("WELCOME", false), Category::Graphic);
        assert_eq!(classify_item("Sunday SERMON", false), Category::Title);
    }
}
