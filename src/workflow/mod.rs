//! Shared headless workflow modules.
#![allow(clippy::redundant_pub_crate)]
//!
//! These modules hold runtime logic that should be reusable by MCP and future
//! internal tooling without depending on TUI-era application state.

mod approval;
#[allow(missing_docs)]
pub mod classify;
pub(crate) mod description_parser;
pub mod execute;
mod library_search;
pub(crate) mod plan;
pub use approval::{OutputReviewError, SourceReviewError};
pub use plan::{PlanAction, ResolvedBackground};
pub mod report;
mod scripture;
mod transaction;
