//! Planning Center API integration.
//!
//! Provides functionality for integrating with Planning Center Online API,
//! including authentication, API request handling, and data caching.

/// API client for Planning Center Online requests
pub mod api;
pub(crate) mod identity;
mod lookahead;
mod normalize;
mod snapshot;
/// Data types representing Planning Center resources
pub mod types;

// Re-export key components
pub use api::PlanningCenterClient;
pub use lookahead::{PlanLookaheadDays, PlanLookaheadDaysError};
pub use snapshot::{PlanFreshnessError, PlanRevision, PlanRevisionError, PlanSnapshot};
// Re-export core types from the submodule
