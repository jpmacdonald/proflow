//! `ProFlow` - `Planning Center` to `ProPresenter` workflow tool.
//!
//! This crate provides integration between `Planning Center` Online and `ProPresenter`,
//! enabling streamlined worship service preparation.

// Re-export public modules for use in integration tests and as a library
pub mod app;
pub mod bible;
pub mod config;
pub mod constants;
pub mod error;
pub mod hymnal;
pub mod item_state;
pub mod lyrics;
pub mod planning_center;
pub mod propresenter;
pub mod services;
pub mod types;
pub mod ui;
pub mod utils;
