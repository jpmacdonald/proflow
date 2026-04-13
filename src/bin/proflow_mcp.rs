//! MCP server binary for `ProFlow`.
//!
//! Runs the `ProFlow` MCP server over stdio, enabling LLM-driven service preparation.

use proflow::config::Config;
use proflow::mcp::ProFlowServer;
use rmcp::ServiceExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load config from environment / .env
    let config = Config::load().unwrap_or_default();

    let server = ProFlowServer::new(config)
        .ok_or("Planning Center credentials not configured. Set PCO_APP_ID and PCO_SECRET.")?;

    let service = server.serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;

    Ok(())
}
