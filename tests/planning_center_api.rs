//! Integration tests for the Planning Center API client.

// Compile the live smoke tests only when integration coverage is requested.
// Each test is also ignored so `--all-features` remains deterministic; use
// `just pco-smoke` to opt into real Planning Center requests.
#![cfg(feature = "integration_test")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::uninlined_format_args
)]

use proflow::config::Config;
use proflow::planning_center::PlanningCenterClient;
use std::time::Instant;

fn setup_client() -> PlanningCenterClient {
    let config = Config::load().expect("live Planning Center smoke test requires valid config");
    PlanningCenterClient::new(&config)
        .expect("live Planning Center smoke test requires a valid HTTP client")
}

// Test fetching services and plans
#[tokio::test]
#[ignore = "requires live Planning Center credentials and network"]
async fn test_fetch_services_and_plans() {
    let client = setup_client();
    println!("Testing get_upcoming_services...");
    let result = client.get_upcoming_services(30).await; // Fetch plans 30 days ahead

    match result {
        Ok((services, plans)) => {
            println!(
                "Successfully fetched {} services and {} plans.",
                services.len(),
                plans.len()
            );
            // Basic assertions
            assert!(
                !services.is_empty(),
                "Expected to find at least one service type."
            );
            assert!(
                !plans.is_empty(),
                "Expected to find at least one upcoming plan."
            );
        }
        Err(e) => {
            panic!("get_upcoming_services failed: {e}");
        }
    }
}

// Test fetching items for a specific plan
#[tokio::test]
#[ignore = "requires live Planning Center credentials and network"]
async fn test_fetch_items_for_plan() {
    let client = setup_client();
    println!("Fetching plans to get a valid ID for item testing...");
    let plans_result = client.get_upcoming_services(60).await; // Look further ahead for plans

    let first_plan_id = match plans_result {
        Ok((_, plans)) if !plans.is_empty() => {
            let id = plans[0].id.clone();
            println!("Found plan ID for testing: {id}");
            id
        }
        Ok(_) => panic!("Expected to find at least one upcoming plan for item testing"),
        Err(e) => panic!("Failed to fetch plans for item testing: {e}"),
    };

    println!("Testing get_service_items for plan ID: {first_plan_id}...");
    let items_result = client.get_service_items(&first_plan_id).await;

    match items_result {
        Ok(items) => {
            println!(
                "Successfully fetched {} items for plan {}.",
                items.len(),
                first_plan_id
            );
            assert!(
                !items.is_empty(),
                "Expected plan to have at least one item."
            );
        }
        Err(e) => {
            panic!("get_service_items failed for plan {first_plan_id}: {e}");
        }
    }
}

// Performance test for the concurrent implementation
#[tokio::test]
#[ignore = "requires live Planning Center credentials and network"]
async fn test_performance() {
    let client = setup_client();
    println!("Running performance test for concurrent API implementation...");

    // Measure execution time
    let start = Instant::now();

    let result = client.get_upcoming_services(30).await;

    let duration = start.elapsed();

    match result {
        Ok((services, plans)) => {
            println!(
                "Performance: Fetched {} services and {} plans in {:.2?}",
                services.len(),
                plans.len(),
                duration
            );

            // Performance threshold - should be relatively fast
            assert!(
                duration.as_secs() < 5,
                "API call took longer than expected: {duration:.2?}"
            );
            assert!(
                !services.is_empty() && !plans.is_empty(),
                "Should have returned some data"
            );
        }
        Err(e) => {
            panic!("Performance test failed: {e}");
        }
    }
}

// Removed trailing characters that were causing compile errors
