//! Shared headless workflow modules.
#![allow(clippy::redundant_pub_crate)]
//!
//! These modules hold runtime logic that should be reusable by MCP and future
//! internal tooling without depending on TUI-era application state.

mod approval;
pub mod classify;
mod classify_matching;
mod classify_preview;
pub(crate) mod description_parser;
pub mod execute;
mod library_search;
pub(crate) mod plan;
mod presentation_render;
pub use approval::{OutputReviewError, SourceReviewError};
pub use execute::OverrideAction;
pub use plan::ResolvedBackground;
pub mod report;
mod scripture;
mod transaction;
