//! HTTP retry, pagination, and resource retrieval.

use super::{
    join_base_and_path, resolve_pagination_url, resource_url, PaginatedResponse,
    PlanningCenterClient, PlanningCenterError, Result, INITIAL_BACKOFF_MS, MAX_BACKOFF_MS,
    MAX_RETRIES,
};
use serde_json::Value;
use std::time::Duration;
use tokio::time::sleep;

impl PlanningCenterClient {
    /// Internal method that performs the actual request with retry logic
    pub(super) async fn get_url_with_retry(
        &self,
        url: &str,
        query: &[(&str, &str)],
    ) -> Result<Value> {
        let label = url.strip_prefix(&self.base_url).unwrap_or(url);
        let mut last_error: Option<PlanningCenterError> = None;
        let mut backoff_ms = INITIAL_BACKOFF_MS;

        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                tracing::info!(
                    "Retrying request to {label} (attempt {}/{})",
                    attempt + 1,
                    MAX_RETRIES + 1
                );
                sleep(Duration::from_millis(backoff_ms)).await;
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
                        return Err(PlanningCenterError::Status {
                            label: label.to_string(),
                            status: status.as_u16(),
                        });
                    }

                    // Retry on server errors (5xx) or rate limiting (429)
                    if status.is_server_error() || status.as_u16() == 429 {
                        last_error = Some(PlanningCenterError::Status {
                            label: label.to_string(),
                            status: status.as_u16(),
                        });
                        continue;
                    }

                    if !status.is_success() {
                        return Err(PlanningCenterError::Status {
                            label: label.to_string(),
                            status: status.as_u16(),
                        });
                    }

                    return resp
                        .json()
                        .await
                        .map_err(|source| PlanningCenterError::InvalidJson {
                            label: label.to_string(),
                            source,
                        });
                }
                Err(source) => {
                    // Network errors are retryable
                    if source.is_timeout() || source.is_connect() {
                        last_error = Some(PlanningCenterError::Request {
                            label: label.to_string(),
                            source,
                        });
                        continue;
                    }
                    // Other errors are not retryable
                    return Err(PlanningCenterError::Request {
                        label: label.to_string(),
                        source,
                    });
                }
            }
        }

        // All retries exhausted
        Err(
            last_error.unwrap_or_else(|| PlanningCenterError::RetriesExhausted {
                label: label.to_string(),
                retries: MAX_RETRIES,
            }),
        )
    }

    pub(super) async fn get_paginated_with_query(
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
                PlanningCenterError::MissingResponseField {
                    label: path.to_string(),
                    field: "data",
                }
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

    pub(super) async fn get_resource(&self, segments: &[&str]) -> Result<Value> {
        let url = resource_url(&self.base_url, segments)?;
        let json = self.get_url_with_retry(&url, &[]).await?;
        json.get("data")
            .cloned()
            .ok_or_else(|| PlanningCenterError::MissingResponseField {
                label: url,
                field: "data",
            })
    }
}
