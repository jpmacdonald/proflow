//! MCP server binary for `ProFlow`.
//!
//! Runs the `ProFlow` MCP server over stdio, enabling LLM-driven service preparation.

use proflow::config::Config;
use proflow::mcp::ProFlowServer;
use rmcp::ServiceExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load config from environment / .env
    let config = Config::load()?;
    let server = ProFlowServer::new(&config)?;

    let service = server.serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;

    Ok(())
}
