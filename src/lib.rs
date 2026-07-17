//! `ProFlow` - `Planning Center` to `ProPresenter` workflow tool.
//!
//! This crate provides integration between `Planning Center` Online and `ProPresenter`,
//! enabling streamlined worship service preparation.

// Re-export public modules for use in integration tests and as a library
pub mod bible;
pub mod config;
pub mod error;
pub mod mcp;
pub mod paths;
pub mod planning_center;
pub mod project_config;
pub mod propresenter;
pub(crate) mod setup;
pub mod workflow;
