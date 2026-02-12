//! Integration tests for the Planning Center API client.

// Ensure this test only runs when integration tests are explicitly enabled
// or when running all tests, but provide feedback if skipped.
#![cfg(feature = "integration_test")]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use proflow_tui::config::Config;
use proflow_tui::planning_center::PlanningCenterClient;
use std::time::Instant;

// Helper function to set up the client for tests
async fn setup_client() -> Option<PlanningCenterClient> {
    match Config::load() {
        Ok(config) => {
            if config.has_planning_center_credentials() {
                Some(PlanningCenterClient::new(&config))
            } else {
                // Use raw string literal for the filename
                println!(r#"Skipping integration test: Planning Center credentials not found in environment/".env" file."#);
                None
            }
        }
        Err(e) => {
            println!("Skipping integration test: Failed to load config: {}", e);
            None // Indicate test should be skipped
        }
    }
}

// Test fetching services and plans
#[tokio::test]
async fn test_fetch_services_and_plans() {
    if let Some(client) = setup_client().await {
        println!("Testing get_upcoming_services...");
        let result = client.get_upcoming_services(30).await; // Fetch plans 30 days ahead

        match result {
            Ok((services, plans)) => {
                println!("Successfully fetched {} services and {} plans.", services.len(), plans.len());
                // Basic assertions
                assert!(!services.is_empty(), "Expected to find at least one service type.");
                assert!(!plans.is_empty(), "Expected to find at least one upcoming plan.");
            }
            Err(e) => {
                panic!("get_upcoming_services failed: {}", e);
            }
        }
    } 
    // If client is None, the test implicitly passes by being skipped.
}

// Test fetching items for a specific plan
#[tokio::test]
async fn test_fetch_items_for_plan() {
    if let Some(client) = setup_client().await {
        println!("Fetching plans to get a valid ID for item testing...");
        let plans_result = client.get_upcoming_services(60).await; // Look further ahead for plans

        let first_plan_id = match plans_result {
            Ok((_, plans)) if !plans.is_empty() => {
                 let id = plans[0].id.clone();
                 println!("Found plan ID for testing: {}", id);
                 Some(id)
            },
            Ok(_) => {
                println!("Skipping item fetch test: No upcoming plans found.");
                None
            },
            Err(e) => {
                println!("Skipping item fetch test: Failed to fetch plans initially: {}", e);
                None
            }
        };

        if let Some(plan_id) = first_plan_id {
            println!("Testing get_service_items for plan ID: {}...", plan_id);
            let items_result = client.get_service_items(&plan_id).await;

            match items_result {
                Ok(items) => {
                    println!("Successfully fetched {} items for plan {}.", items.len(), plan_id);
                    assert!(!items.is_empty(), "Expected plan to have at least one item.");
                }
                Err(e) => {
                     panic!("get_service_items failed for plan {}: {}", plan_id, e);
                }
            }
        }
    }
    // If client is None or no plan ID found, the test implicitly passes by being skipped.
}

// Performance test for the concurrent implementation
#[tokio::test]
async fn test_performance() {
    if let Some(client) = setup_client().await {
        println!("Running performance test for concurrent API implementation...");
        
        // Measure execution time
        let start = Instant::now();
        
        let result = client.get_upcoming_services(30).await;
        
        let duration = start.elapsed();
        
        match result {
            Ok((services, plans)) => {
                println!("Performance: Fetched {} services and {} plans in {:.2?}", 
                    services.len(), plans.len(), duration);
                    
                // Performance threshold - should be relatively fast
                assert!(duration.as_secs() < 5, "API call took longer than expected: {:.2?}", duration);
                assert!(!services.is_empty() && !plans.is_empty(), "Should have returned some data");
            }
            Err(e) => {
                panic!("Performance test failed: {}", e);
            }
        }
    }
}

// Removed trailing characters that were causing compile errors
 