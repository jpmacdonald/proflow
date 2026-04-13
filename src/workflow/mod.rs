//! Shared headless workflow modules.
//!
//! These modules hold runtime logic that should be reusable by MCP and future
//! internal tooling without depending on TUI-era application state.

pub(crate) mod classify;
pub(crate) mod description_parser;
pub(crate) mod execute;
mod library_search;
pub(crate) mod plan;
pub(crate) mod report;
mod scripture;
