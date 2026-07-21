use chrono::{DateTime, Duration, Utc};
use futures::{stream, StreamExt};
use reqwest::{Client, Url};
use serde_json::Value;
use std::time::Duration as StdDuration;
use tokio::time::sleep;

use super::normalize::{parse_items, parse_plan, parse_service_types};
use super::snapshot::PlanSnapshot;
use crate::config::Config;
use crate::error::{Error, Result};
use crate::planning_center::types::{Item, Plan, Service};

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
    /// Create a new Planning Center client from config.
    ///
    /// # Errors
    ///
    /// Returns an error when credentials are missing or the HTTP client cannot
    /// be constructed with the required timeout policy.
    pub fn new(config: &Config) -> Result<Self> {
        Self::new_with_base_url(config, BASE_URL)
    }

    pub(crate) fn new_with_base_url(config: &Config, base_url: impl Into<String>) -> Result<Self> {
        let base_url = base_url.into();
        Self::with_base_url(config, normalize_base_url(&base_url))
    }

    fn with_base_url(config: &Config, base_url: String) -> Result<Self> {
        let app_id = config.pco_app_id.trim();
        let secret = config.pco_secret.trim();
        if app_id.is_empty() || secret.is_empty() {
            return Err(Error::config(
                "Planning Center credentials are missing or blank",
                "Set PCO_APP_ID and PCO_SECRET environment variables",
            ));
        }

        let client = Client::builder()
            .timeout(StdDuration::from_secs(30))
            .no_proxy()
            .build()
            .map_err(|error| {
                Error::Network(format!(
                    "failed to initialize Planning Center HTTP client: {error}"
                ))
            })?;
        Ok(Self {
            app_id: app_id.to_string(),
            secret: secret.to_string(),
            client,
            base_url,
        })
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

    async fn get_resource(&self, segments: &[&str]) -> Result<Value> {
        let url = resource_url(&self.base_url, segments)?;
        let json = self.get_url_with_retry(&url, &[]).await?;
        json.get("data").cloned().ok_or_else(|| {
            Error::parse(
                format!("Missing 'data' resource in response from {url}"),
                None,
            )
        })
    }

    /// Get upcoming services and plans using bounded concurrent API calls.
    ///
    /// Returns an error instead of a partial plan set when any service-type
    /// request fails.
    pub async fn get_upcoming_services(
        &self,
        days_ahead: i64,
    ) -> Result<(Vec<Service>, Vec<Plan>)> {
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
        Ok(parse_service_types(&response.data)?)
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

        let mut plans = Vec::new();
        for (index, plan_value) in entries.iter().enumerate() {
            let plan = parse_plan(plan_value, index, service_id, service_name)?;
            if (start_date..=end_date).contains(&plan.date) {
                plans.push(plan);
            }
        }
        Ok(plans)
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
        let path = format!("/plans/{plan_id}/items");
        let response = self
            .get_paginated_with_query(
                &path,
                &[("include", "song,arrangement"), ("per_page", "100")],
            )
            .await?;
        Ok(parse_items(&response.data, &response.included, plan_id)?)
    }

    /// Capture one exact plan by its stable identity and asserted service type.
    ///
    /// Unlike upcoming-plan discovery, this lookup is not bounded by the
    /// current date. Two consecutive direct reads must normalize identically
    /// before the snapshot is returned.
    pub async fn capture_plan_snapshot(
        &self,
        plan_id: &str,
        service_name: &str,
    ) -> Result<PlanSnapshot> {
        let services = self.fetch_service_types().await?;
        let matching = services
            .iter()
            .filter(|service| service.name == service_name)
            .collect::<Vec<_>>();
        let service = match matching.as_slice() {
            [service] => *service,
            [] => {
                return Err(Error::pco(format!(
                    "Planning Center has no service type named '{service_name}'"
                )))
            }
            _ => {
                return Err(Error::pco(format!(
                    "Planning Center has {} service types named '{service_name}'",
                    matching.len()
                )))
            }
        };
        self.fetch_stable_plan_snapshot(&service.id, plan_id).await
    }

    /// Directly refetch every normalized field represented by a reviewed plan.
    ///
    /// The captured service type makes this independent of the moving
    /// upcoming-plan date window: a plan can pass its scheduled time between
    /// preview and commit without becoming spuriously unavailable.
    pub(crate) async fn refresh_plan_snapshot(
        &self,
        reviewed: &PlanSnapshot,
    ) -> Result<PlanSnapshot> {
        self.fetch_stable_plan_snapshot(reviewed.service_id(), reviewed.plan_id())
            .await
    }

    async fn fetch_stable_plan_snapshot(
        &self,
        service_id: &str,
        plan_id: &str,
    ) -> Result<PlanSnapshot> {
        let first = self.fetch_plan_snapshot_once(service_id, plan_id).await?;
        let second = self.fetch_plan_snapshot_once(service_id, plan_id).await?;
        if first == second {
            return Ok(second);
        }
        let first_revision = first
            .revision()
            .map_err(|error| Error::pco(error.to_string()))?
            .to_string();
        let second_revision = second
            .revision()
            .map_err(|error| Error::pco(error.to_string()))?
            .to_string();
        Err(Error::PlanningCenterSnapshotUnstable {
            plan_id: plan_id.to_string(),
            first_revision,
            second_revision,
        })
    }

    async fn fetch_plan_snapshot_once(
        &self,
        service_id: &str,
        plan_id: &str,
    ) -> Result<PlanSnapshot> {
        let service_value = self.get_resource(&["service_types", service_id]).await?;
        let mut services = parse_service_types(std::slice::from_ref(&service_value))?;
        let service = services.pop().ok_or_else(|| {
            Error::pco(format!(
                "service type '{service_id}' returned no resource during freshness check"
            ))
        })?;

        let plan_value = self
            .get_resource(&["service_types", service_id, "plans", plan_id])
            .await?;
        let plan = parse_plan(&plan_value, 0, &service.id, &service.name)?;
        if plan.id != plan_id {
            return Err(Error::pco(format!(
                "Planning Center returned plan '{}' while refreshing '{}'",
                plan.id, plan_id
            )));
        }
        let identity = super::identity::resolve_plan_identity(
            std::slice::from_ref(&service),
            std::slice::from_ref(&plan),
            plan_id,
            0,
        )
        .map_err(|error| Error::pco(error.to_string()))?;
        let items = self.get_service_items(plan_id).await?;
        Ok(PlanSnapshot::from_resolved(identity, items))
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

fn resource_url(base_url: &str, segments: &[&str]) -> Result<String> {
    let mut url = Url::parse(base_url)
        .map_err(|error| Error::pco(format!("Invalid Planning Center base URL: {error}")))?;
    {
        let mut path = url.path_segments_mut().map_err(|()| {
            Error::pco("Planning Center base URL cannot contain hierarchical resource paths")
        })?;
        path.pop_if_empty();
        for segment in segments {
            path.push(segment);
        }
    }
    Ok(url.into())
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
#[path = "api/tests.rs"]
mod tests;
