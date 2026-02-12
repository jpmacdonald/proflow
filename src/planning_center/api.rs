use chrono::{DateTime, Duration, Utc};
use futures::future;
use reqwest::Client;
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
}

impl PlanningCenterClient {
    /// Create a new Planning Center client from config
    pub fn new(config: &Config) -> Self {
        // Client::build() should never fail with default settings, but if it does,
        // we create a client without timeout rather than silently failing
        let client = Client::builder()
            .timeout(StdDuration::from_secs(30))
            .build()
            .unwrap_or_else(|e| {
                tracing::warn!("Failed to create HTTP client with timeout, using default client: {e}");
                Client::default()
            });
        Self {
            app_id: config.pco_app_id.clone(),
            secret: config.pco_secret.clone(),
            client,
        }
    }

    /// Check if credentials are configured
    const fn is_configured(&self) -> bool {
        // String::is_empty is not const, so we check len directly
        !self.app_id.is_empty() && !self.secret.is_empty()
    }

    /// Make an authenticated GET request to the PCO API with retry/backoff
    async fn get(&self, path: &str) -> Result<Value> {
        self.get_with_retry(path, &[]).await
    }

    /// Make an authenticated GET request with query parameters and retry/backoff
    async fn get_with_query(&self, path: &str, query: &[(&str, &str)]) -> Result<Value> {
        self.get_with_retry(path, query).await
    }

    /// Internal method that performs the actual request with retry logic
    async fn get_with_retry(&self, path: &str, query: &[(&str, &str)]) -> Result<Value> {
        let url = format!("{BASE_URL}{path}");
        let mut last_error: Option<Error> = None;
        let mut backoff_ms = INITIAL_BACKOFF_MS;

        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                tracing::info!("Retrying request to {path} (attempt {}/{})", attempt + 1, MAX_RETRIES + 1);
                sleep(StdDuration::from_millis(backoff_ms)).await;
                backoff_ms = (backoff_ms * 2).min(MAX_BACKOFF_MS);
            }

            let request = self.client
                .get(&url)
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
                            format!("Request to {path} returned {status}"),
                            status.as_u16(),
                        ));
                    }

                    // Retry on server errors (5xx) or rate limiting (429)
                    if status.is_server_error() || status.as_u16() == 429 {
                        last_error = Some(Error::pco_status(
                            format!("Request to {path} returned {status}"),
                            status.as_u16(),
                        ));
                        continue;
                    }

                    if !status.is_success() {
                        return Err(Error::pco_status(
                            format!("Request to {path} returned {status}"),
                            status.as_u16(),
                        ));
                    }

                    return resp.json().await
                        .map_err(|e| Error::parse(format!("Invalid JSON from {path}: {e}"), None));
                }
                Err(e) => {
                    // Network errors are retryable
                    if e.is_timeout() || e.is_connect() {
                        last_error = Some(Error::Network(format!("Request to {path} failed: {e}")));
                        continue;
                    }
                    // Other errors are not retryable
                    return Err(Error::Network(format!("Request to {path} failed: {e}")));
                }
            }
        }

        // All retries exhausted
        Err(last_error.unwrap_or_else(|| Error::Network(format!("Request to {path} failed after {MAX_RETRIES} retries"))))
    }

    /// Get upcoming services and plans using concurrent API calls
    pub async fn get_upcoming_services(&self, days_ahead: i64) -> Result<(Vec<Service>, Vec<Plan>)> {
        if !self.is_configured() {
            return Err(Error::config(
                "Planning Center client not configured",
                "Set PCO_APP_ID and PCO_SECRET environment variables",
            ));
        }

        // Fetch all service types
        let services = self.fetch_service_types().await?;

        // Concurrently fetch plans for all service types
        let plan_futures = services.iter()
            .map(|s| self.fetch_plans_for_service(&s.id, &s.name, days_ahead));
        let plan_results = future::join_all(plan_futures).await;

        // Collect plans, logging failures but continuing
        let mut all_plans = Vec::new();
        for result in plan_results {
            match result {
                Ok(plans) => all_plans.extend(plans),
                Err(e) => tracing::warn!("Failed to fetch plans for a service: {e}"),
            }
        }

        // Sort services alphabetically, plans by date
        let mut sorted_services = services;
        sorted_services.sort_by(|a, b| a.name.cmp(&b.name));
        all_plans.sort_by(|a, b| a.date.cmp(&b.date));

        Ok((sorted_services, all_plans))
    }

    /// Fetch all service types
    async fn fetch_service_types(&self) -> Result<Vec<Service>> {
        let json = self.get("/service_types").await?;

        let entries = json["data"].as_array()
            .ok_or_else(|| Error::parse("Missing 'data' array in service types response", None))?;

        Ok(entries.iter().filter_map(|s| {
            let id = s["id"].as_str()?.to_string();
            let name = s["attributes"]["name"].as_str().unwrap_or("Unknown").to_string();
            Some(Service { id, name })
        }).collect())
    }

    /// Fetch plans for a specific service type
    async fn fetch_plans_for_service(
        &self,
        service_id: &str,
        service_name: &str,
        days_ahead: i64,
    ) -> Result<Vec<Plan>> {
        let end_date = Utc::now() + Duration::days(days_ahead);
        let path = format!("/service_types/{service_id}/plans");

        let json = self.get_with_query(&path, &[
            ("filter", "future"),
            ("per_page", "25"),
        ]).await?;

        let entries = json["data"].as_array().map_or(&[] as &[Value], Vec::as_slice);

        Ok(entries.iter().filter_map(|plan_value| {
            let id = plan_value["id"].as_str()?.to_string();
            let attrs = &plan_value["attributes"];

            #[allow(clippy::similar_names)]
            let date = DateTime::parse_from_rfc3339(attrs["sort_date"].as_str()?)
                .ok()?
                .with_timezone(&Utc);

            // Skip plans beyond date range
            if date > end_date { return None; }

            let title = attrs["title"].as_str()
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
        }).collect())
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
        let json = self.get_with_query(&path, &[
            ("include", "song,arrangement"),
            ("per_page", "100"),
        ]).await?;

        let entries = json["data"].as_array()
            .ok_or_else(|| Error::parse("Missing 'data' array in items response", None))?;

        // Build lookup maps for included Song and Arrangement data
        let included = json["included"].as_array().map_or(&[] as &[Value], Vec::as_slice);
        let songs: std::collections::HashMap<_, _> = included.iter()
            .filter(|v| v["type"].as_str() == Some("Song"))
            .filter_map(|v| Some((v["id"].as_str()?, v)))
            .collect();
        let arrangements: std::collections::HashMap<_, _> = included.iter()
            .filter(|v| v["type"].as_str() == Some("Arrangement"))
            .filter_map(|v| Some((v["id"].as_str()?, v)))
            .collect();

        // Parse items
        let items: Vec<Item> = entries.iter().enumerate().filter_map(|(idx, item_value)| {
            let id = item_value["id"].as_str()?.to_string();
            let attrs = &item_value["attributes"];
            let rels = &item_value["relationships"];

            let title = attrs["title"].as_str().unwrap_or("Untitled").to_string();
            let description = attrs["description"].as_str().map(String::from);
            let note = attrs["notes"].as_str().map(String::from);

            // Parse linked song if present
            let song = parse_song(rels, &songs, &arrangements);

            // Classify item
            let category = classify_item(&title, song.is_some());

            // Parse scripture if applicable
            let scripture = if category == Category::Title && title.contains("Scripture") {
                let reference = title.split('-')
                    .map(str::trim)
                    .find(|s| !s.is_empty() && !s.to_lowercase().contains("scripture"))
                    .unwrap_or(&title)
                    .to_string();
                Some(Scripture {
                    reference,
                    text: description.clone(),
                    translation: None,
                })
            } else {
                None
            };

            Some(Item {
                id,
                position: idx + 1,
                title,
                description,
                category,
                note,
                song,
                scripture,
            })
        }).collect();

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
    let (lyrics, arrangement) = rels.get("arrangement")
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

    let upper = title.to_uppercase();
    match () {
        () if title.contains("Scripture") || title.contains("Reading") => Category::Title,
        () if title.contains("Sermon") || title.contains("Message") => Category::Title,
        () if title.contains("Announcements") || title.contains("Welcome") => Category::Graphic,
        () if ["PRE-SERVICE", "SERVICE", "POST-SERVICE", "PRAISE", "OFFERING",
              "GIVING", "PRAYER", "LORD'S PRAYER", "GREETING"]
            .iter().any(|h| upper.contains(h)) => Category::Other,
        () => Category::Text,
    }
}
