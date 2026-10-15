//! HTTP, identity, and normalization failures at the API boundary.

use super::{PlanIdentityError, PlanRevisionError, PlanningCenterSourceError};

/// Failure to read from or make sense of the Planning Center API.
#[derive(Debug, thiserror::Error)]
pub enum PlanningCenterError {
    /// `PCO_APP_ID` or `PCO_SECRET` is unset or blank.
    #[error("Planning Center credentials are missing or blank; set PCO_APP_ID and PCO_SECRET")]
    MissingCredentials,
    /// The HTTP client could not be built with the required timeout policy.
    #[error("failed to initialize Planning Center HTTP client: {source}")]
    HttpClient {
        /// Client construction failure.
        #[source]
        source: reqwest::Error,
    },
    /// The API answered with a non-success status.
    #[error("request to {label} returned HTTP {status}")]
    Status {
        /// Request path relative to the API base URL.
        label: String,
        /// HTTP status code.
        status: u16,
    },
    /// The request could not be sent or completed.
    #[error("request to {label} failed: {source}")]
    Request {
        /// Request path relative to the API base URL.
        label: String,
        /// Transport failure.
        #[source]
        source: reqwest::Error,
    },
    /// Every retry of a transient failure was exhausted.
    #[error("request to {label} failed after {retries} retries")]
    RetriesExhausted {
        /// Request path relative to the API base URL.
        label: String,
        /// Retries attempted.
        retries: u32,
    },
    /// The response body was not valid JSON.
    #[error("invalid JSON from {label}: {source}")]
    InvalidJson {
        /// Request path relative to the API base URL.
        label: String,
        /// Decode failure.
        #[source]
        source: reqwest::Error,
    },
    /// A JSON:API envelope lacked a required member.
    #[error("missing '{field}' in response from {label}")]
    MissingResponseField {
        /// Request path or URL.
        label: String,
        /// Missing envelope member.
        field: &'static str,
    },
    /// One or more service types could not be read while discovering plans.
    #[error("failed to fetch plans for {} service type(s): {}", failures.len(), describe_failures(failures))]
    ServiceTypePlans {
        /// Each failed service type name with its error.
        failures: Vec<(String, Box<Self>)>,
    },
    /// No service type carries the asserted name.
    #[error("Planning Center has no service type named '{name}'")]
    ServiceTypeNotFound {
        /// Asserted service type name.
        name: String,
    },
    /// More than one service type carries the asserted name.
    #[error("Planning Center has {count} service types named '{name}'")]
    ServiceTypeAmbiguous {
        /// Asserted service type name.
        name: String,
        /// Number of matching service types.
        count: usize,
    },
    /// A service type resource read returned nothing.
    #[error("service type '{service_id}' returned no resource during freshness check")]
    ServiceTypeResourceMissing {
        /// Service type identity requested.
        service_id: String,
    },
    /// A plan refresh returned a different plan than requested.
    #[error("Planning Center returned plan '{returned}' while refreshing '{requested}'")]
    RefreshedPlanMismatch {
        /// Plan identity requested.
        requested: String,
        /// Plan identity returned.
        returned: String,
    },
    /// Refreshed plan metadata could not be bound to one identity.
    #[error("refreshed plan identity could not be resolved: {0}")]
    RefreshIdentity(#[source] PlanIdentityError),
    /// A captured snapshot has no computable revision.
    #[error(transparent)]
    Revision(#[from] PlanRevisionError),
    /// Two consecutive direct reads of one plan disagreed, so no coherent
    /// snapshot was available for review or commit.
    #[error(
        "Planning Center plan '{plan_id}' changed while it was being captured ({first_revision} then {second_revision}); retry after edits settle"
    )]
    SnapshotUnstable {
        /// Stable plan identity being captured.
        plan_id: String,
        /// Revision of the first direct normalized read.
        first_revision: String,
        /// Revision of the immediately following direct normalized read.
        second_revision: String,
    },
    /// The configured base URL is not a URL.
    #[error("invalid Planning Center base URL: {source}")]
    InvalidBaseUrl {
        /// Parse failure.
        #[source]
        source: url::ParseError,
    },
    /// The configured base URL cannot carry resource path segments.
    #[error("Planning Center base URL cannot contain hierarchical resource paths")]
    BaseUrlNotHierarchical,
    /// A page URL recorded while paginating is not a URL.
    #[error("invalid Planning Center page URL: {source}")]
    InvalidPageUrl {
        /// Parse failure.
        #[source]
        source: url::ParseError,
    },
    /// A `links.next` value could not be resolved against the current page.
    #[error("invalid Planning Center pagination URL: {source}")]
    InvalidPaginationUrl {
        /// Parse failure.
        #[source]
        source: url::ParseError,
    },
    /// A `links.next` value pointed at a different origin.
    #[error("rejected Planning Center pagination URL with a different origin")]
    PaginationOriginMismatch,
    /// A response could not be normalized into the plan model.
    #[error(transparent)]
    Source(#[from] PlanningCenterSourceError),
}

fn describe_failures(failures: &[(String, Box<PlanningCenterError>)]) -> String {
    failures
        .iter()
        .map(|(service_name, error)| format!("{service_name}: {error}"))
        .collect::<Vec<_>>()
        .join("; ")
}
