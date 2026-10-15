mod error;
mod transport;
pub use error::PlanningCenterError;

use chrono::{DateTime, Duration, Utc};
use futures::{stream, StreamExt};
use reqwest::{Client, Url};
use serde_json::Value;
use std::time::Duration as StdDuration;

use super::identity::{resolve_plan_identity, PlanCaptureError, PlanIdentityError, PlanLookup};
use super::normalize::PlanningCenterSourceError;
use super::PlanRevisionError;

type Result<T> = std::result::Result<T, PlanningCenterError>;

use super::normalize::{parse_items, parse_plan, parse_service_types};
use super::snapshot::PlanSnapshot;
use crate::config::Config;
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
            return Err(PlanningCenterError::MissingCredentials);
        }

        let client = Client::builder()
            .timeout(StdDuration::from_secs(30))
            .no_proxy()
            .build()
            .map_err(|source| PlanningCenterError::HttpClient { source })?;
        Ok(Self {
            app_id: app_id.to_string(),
            secret: secret.to_string(),
            client,
            base_url,
        })
    }

    /// Get upcoming services and plans using bounded concurrent API calls.
    ///
    /// Returns an error instead of a partial plan set when any service-type
    /// request fails.
    pub async fn get_upcoming_services(
        &self,
        days_ahead: crate::planning_center::PlanLookaheadDays,
    ) -> Result<(Vec<Service>, Vec<Plan>)> {
        // Fetch all service types
        let services = self.fetch_service_types().await?;

        let start_date = Utc::now();
        let end_date = start_date + Duration::days(days_ahead.get());
        let mut all_plans = self
            .fetch_plans_for_services(&services, start_date, end_date)
            .await?;

        // Sort services alphabetically, plans by date
        let mut sorted_services = services;
        sorted_services.sort_by(|a, b| a.name.cmp(&b.name));
        all_plans.sort_by_key(|a| a.date);

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
        all_plans.sort_by_key(|a| a.date);

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
                Err(error) => failures.push((service_name, Box::new(error))),
            }
        }

        if failures.is_empty() {
            Ok(plans)
        } else {
            Err(PlanningCenterError::ServiceTypePlans { failures })
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
                return Err(PlanningCenterError::ServiceTypeNotFound {
                    name: service_name.to_string(),
                })
            }
            _ => {
                return Err(PlanningCenterError::ServiceTypeAmbiguous {
                    name: service_name.to_string(),
                    count: matching.len(),
                })
            }
        };
        self.fetch_stable_plan_snapshot(&service.id, plan_id).await
    }

    /// Capture one stable plan snapshot for a reviewed build request.
    pub async fn capture_plan(
        &self,
        lookup: PlanLookup<'_>,
    ) -> std::result::Result<PlanSnapshot, PlanCaptureError> {
        match lookup {
            PlanLookup::Asserted {
                plan_id,
                service_name,
            } => Ok(self.capture_plan_snapshot(plan_id, service_name).await?),
            PlanLookup::Upcoming {
                plan_id,
                days_ahead,
            } => {
                let (services, plans) = self.get_upcoming_services(days_ahead).await?;
                let identity = resolve_plan_identity(&services, &plans, plan_id, days_ahead)
                    .map_err(PlanCaptureError::Identity)?;
                let items = self.get_service_items(plan_id).await?;
                let discovered = PlanSnapshot::from_resolved(identity, items);
                Ok(self.refresh_plan_snapshot(&discovered).await?)
            }
        }
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
        let first_revision = first.revision()?.to_string();
        let second_revision = second.revision()?.to_string();
        Err(PlanningCenterError::SnapshotUnstable {
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
        let service =
            services
                .pop()
                .ok_or_else(|| PlanningCenterError::ServiceTypeResourceMissing {
                    service_id: service_id.to_string(),
                })?;

        let plan_value = self
            .get_resource(&["service_types", service_id, "plans", plan_id])
            .await?;
        let plan = parse_plan(&plan_value, 0, &service.id, &service.name)?;
        if plan.id != plan_id {
            return Err(PlanningCenterError::RefreshedPlanMismatch {
                requested: plan_id.to_string(),
                returned: plan.id,
            });
        }
        let identity = super::identity::resolve_plan_identity(
            std::slice::from_ref(&service),
            std::slice::from_ref(&plan),
            plan_id,
            crate::planning_center::PlanLookaheadDays::DEFAULT,
        )
        .map_err(PlanningCenterError::RefreshIdentity)?;
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
    let mut url =
        Url::parse(base_url).map_err(|source| PlanningCenterError::InvalidBaseUrl { source })?;
    {
        let mut path = url
            .path_segments_mut()
            .map_err(|()| PlanningCenterError::BaseUrlNotHierarchical)?;
        path.pop_if_empty();
        for segment in segments {
            path.push(segment);
        }
    }
    Ok(url.into())
}

fn resolve_pagination_url(base_url: &str, current_url: &str, next: &str) -> Result<String> {
    let base =
        Url::parse(base_url).map_err(|source| PlanningCenterError::InvalidBaseUrl { source })?;
    let current =
        Url::parse(current_url).map_err(|source| PlanningCenterError::InvalidPageUrl { source })?;
    let candidate = current
        .join(next)
        .map_err(|source| PlanningCenterError::InvalidPaginationUrl { source })?;

    let same_origin = base.scheme() == candidate.scheme()
        && base.host_str() == candidate.host_str()
        && base.port_or_known_default() == candidate.port_or_known_default();
    if !same_origin {
        return Err(PlanningCenterError::PaginationOriginMismatch);
    }

    Ok(candidate.into())
}
#[cfg(test)]
#[path = "api/tests.rs"]
mod tests;
