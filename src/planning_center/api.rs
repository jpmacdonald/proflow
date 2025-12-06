use chrono::{DateTime, Duration, Utc};
use reqwest::Client;
use serde_json::Value;
use std::time::Duration as StdDuration;
use futures::future;

use crate::config::Config;
use crate::error::{Error, Result};
use crate::planning_center::types::*;

const BASE_URL: &str = "https://api.planningcenteronline.com/services/v2";

/// Client for accessing Planning Center Online API
///
/// Uses concurrent requests when fetching plans for multiple service types,
/// which significantly improves performance when there are many service types.
#[derive(Clone)]
pub struct PlanningCenterClient {
    app_id: String,
    secret: String,
    client: Client,
}

impl PlanningCenterClient {
    /// Create a new Planning Center client from config
    pub fn new(config: &Config) -> Self {
        Self {
            app_id: config.pco_app_id.clone(),
            secret: config.pco_secret.clone(),
            client: Client::builder()
                .timeout(StdDuration::from_secs(30))
                .build()
                .unwrap_or_default(),
        }
    }

    /// Check if credentials are configured
    fn is_configured(&self) -> bool {
        !self.app_id.is_empty() && !self.secret.is_empty()
    }

    /// Make an authenticated GET request to the PCO API
    async fn get(&self, path: &str) -> Result<Value> {
        let url = format!("{}{}", BASE_URL, path);
        let resp = self.client
            .get(&url)
            .basic_auth(&self.app_id, Some(&self.secret))
            .header("Content-Type", "application/json")
            .send()
            .await
            .map_err(|e| Error::Network(format!("Request to {} failed: {}", path, e)))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(Error::pco_status(
                format!("Request to {} returned {}", path, status),
                status.as_u16(),
            ));
        }

        resp.json().await
            .map_err(|e| Error::parse(format!("Invalid JSON from {}: {}", path, e), None))
    }

    /// Make an authenticated GET request with query parameters
    async fn get_with_query(&self, path: &str, query: &[(&str, &str)]) -> Result<Value> {
        let url = format!("{}{}", BASE_URL, path);
        let resp = self.client
            .get(&url)
            .basic_auth(&self.app_id, Some(&self.secret))
            .header("Content-Type", "application/json")
            .query(query)
            .send()
            .await
            .map_err(|e| Error::Network(format!("Request to {} failed: {}", path, e)))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(Error::pco_status(
                format!("Request to {} returned {}", path, status),
                status.as_u16(),
            ));
        }

        resp.json().await
            .map_err(|e| Error::parse(format!("Invalid JSON from {}: {}", path, e), None))
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
                Err(e) => tracing::warn!("Failed to fetch plans for a service: {}", e),
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

        let data = json["data"].as_array()
            .ok_or_else(|| Error::parse("Missing 'data' array in service types response", None))?;

        Ok(data.iter().filter_map(|s| {
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
        let path = format!("/service_types/{}/plans", service_id);

        let json = self.get_with_query(&path, &[
            ("filter", "future"),
            ("per_page", "25"),
        ]).await?;

        let data = json["data"].as_array().map(|a| a.as_slice()).unwrap_or(&[]);

        Ok(data.iter().filter_map(|plan_data| {
            let id = plan_data["id"].as_str()?.to_string();
            let attrs = &plan_data["attributes"];

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
                _service_name: service_name.to_string(),
                date,
                title,
                _items: Vec::new(),
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

        let path = format!("/plans/{}/items", plan_id);
        let json = self.get_with_query(&path, &[
            ("include", "song,arrangement"),
            ("per_page", "100"),
        ]).await?;

        let data = json["data"].as_array()
            .ok_or_else(|| Error::parse("Missing 'data' array in items response", None))?;

        // Build lookup maps for included Song and Arrangement data
        let included = json["included"].as_array().map(|a| a.as_slice()).unwrap_or(&[]);
        let songs: std::collections::HashMap<_, _> = included.iter()
            .filter(|v| v["type"].as_str() == Some("Song"))
            .filter_map(|v| Some((v["id"].as_str()?, v)))
            .collect();
        let arrangements: std::collections::HashMap<_, _> = included.iter()
            .filter(|v| v["type"].as_str() == Some("Arrangement"))
            .filter_map(|v| Some((v["id"].as_str()?, v)))
            .collect();

        // Parse items
        let items: Vec<Item> = data.iter().enumerate().filter_map(|(idx, item_data)| {
            let id = item_data["id"].as_str()?.to_string();
            let attrs = &item_data["attributes"];
            let rels = &item_data["relationships"];

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
                    _reference: reference,
                    _text: description.clone(),
                    _translation: None,
                })
            } else {
                None
            };

            Some(Item {
                id,
                _position: idx + 1,
                title,
                _description: description,
                category,
                _note: note,
                song,
                _scripture: scripture,
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
    let song_data = songs.get(song_id)?;
    let attrs = &song_data["attributes"];

    let title = attrs["title"].as_str().unwrap_or("").to_string();
    let author = attrs["author"].as_str().map(String::from);
    let copyright = attrs["copyright"].as_str().map(String::from);
    let ccli = attrs["ccli_number"].as_str().map(String::from);

    // Get lyrics from arrangement
    let (lyrics, arrangement) = rels.get("arrangement")
        .and_then(|a| a.get("data")?.get("id")?.as_str())
        .and_then(|arr_id| arrangements.get(arr_id))
        .map(|arr| {
            let lyrics = arr["attributes"]["lyrics"].as_str().map(String::from);
            let name = arr["attributes"]["name"].as_str().map(String::from);
            (lyrics, name)
        })
        .unwrap_or((None, None));

    Some(Song {
        title,
        author,
        _copyright: copyright,
        _ccli: ccli,
        _themes: None,
        lyrics,
        _arrangement: arrangement,
    })
}

/// Classify an item based on its title and whether it has song data
fn classify_item(title: &str, has_song: bool) -> Category {
    if has_song {
        return Category::Song;
    }

    let upper = title.to_uppercase();
    match () {
        _ if title.contains("Scripture") || title.contains("Reading") => Category::Title,
        _ if title.contains("Sermon") || title.contains("Message") => Category::Title,
        _ if title.contains("Announcements") || title.contains("Welcome") => Category::Graphic,
        _ if ["PRE-SERVICE", "SERVICE", "POST-SERVICE", "PRAISE", "OFFERING",
              "GIVING", "PRAYER", "LORD'S PRAYER", "GREETING"]
            .iter().any(|h| upper.contains(h)) => Category::Other,
        _ => Category::Text,
    }
}
