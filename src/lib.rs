//! `ProFlow` - `Planning Center` to `ProPresenter` workflow tool.
//!
//! This crate provides integration between `Planning Center` Online and `ProPresenter`,
//! enabling streamlined worship service preparation.


// Re-export public modules for use in integration tests and as a library
pub mod app;
pub mod constants;
pub mod input;
pub mod item_state;
pub mod services;
pub mod types;
pub mod bible;
pub mod config;
pub mod hymnal;
pub mod error;
pub mod lyrics;
pub mod planning_center;
pub mod propresenter;
pub mod ui;
pub mod utils; 