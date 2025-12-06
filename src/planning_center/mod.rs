// Planning Center API integration

// TODO: This module will handle API calls to Planning Center Online for fetching
// service plans, items, and other data needed by the application.

// 1. Authentication
// 2. API request handling
// 3. Data caching
// 4. Error handling

// Planning Center integration module
//
// This module provides functionality for integrating with Planning Center Online API.

pub mod api;
pub mod types; // Declare the types submodule

// Re-export key components
// We likely only need to export the client itself, 
// as the types (Plan, Item, etc.) should be accessed via crate::types
pub use api::PlanningCenterClient;
// Re-export core types from the submodule
