//! MCP server for autonomous service preparation.
//!
//! Exposes the reviewed service workflow as MCP tools so an LLM can prep a
//! service without bypassing project configuration or preview approval.

use std::sync::Arc;

use rmcp::model::{Implementation, ServerCapabilities, ServerInfo};
use rmcp::{tool_handler, ServerHandler};

use crate::config::Config;
use crate::workflow::execute::BuildServiceError;
use crate::workstation::{WorkspaceDiscovery, Workstation, WorkstationLoadError};

mod config;
mod playlist;
mod review;
mod schema;
mod tools;

pub use schema::{
    BuildServiceArgs, CatalogAssetsArgs, EntryOverride, EntryOverrideAction, ExplainRuleMatchArgs,
    FetchPlanArgs, PlaylistMediaAssetArg, PreviewPlaylistArgs, ReadPlaylistArgs, SearchLibraryArgs,
    WriteProjectConfigArgs,
};

// ---------------------------------------------------------------------------
// Server struct
// ---------------------------------------------------------------------------

/// Shared MCP server state: one loaded workstation. Previews and decisions
/// live in the build directories, not in this process.
#[derive(Clone)]
pub struct ProFlowServer {
    workstation: Arc<Workstation>,
}

impl ProFlowServer {
    /// Build a new server from loaded configuration.
    ///
    /// Startup succeeds only after credentials, project config, and configured
    /// library state have been loaded successfully.
    pub fn new(config: &Config) -> Result<Self, WorkstationLoadError> {
        let workstation = Workstation::load(config, WorkspaceDiscovery::ActiveShow)?;
        Ok(Self {
            workstation: Arc::new(workstation),
        })
    }
}

pub(super) fn mcp_err(msg: impl Into<String>) -> rmcp::ErrorData {
    rmcp::ErrorData::internal_error(msg.into(), None)
}

/// Map a build failure onto the MCP error vocabulary: a malformed request is
/// the caller's problem; everything else is the server's.
pub(super) fn build_err(error: BuildServiceError) -> rmcp::ErrorData {
    match error {
        BuildServiceError::Request(request) => {
            rmcp::ErrorData::invalid_params(request.to_string(), None)
        }
        BuildServiceError::Decisions(decisions) => {
            rmcp::ErrorData::invalid_params(decisions.to_string(), None)
        }
        // The caller named a build that is not there to install: preview again.
        stale @ (BuildServiceError::NoVerifiedBuild { .. }
        | BuildServiceError::BuildAlreadyCommitted { .. }
        | BuildServiceError::PreparedBuildSuperseded) => {
            rmcp::ErrorData::invalid_params(stale.to_string(), None)
        }
        other => rmcp::ErrorData::internal_error(other.to_string(), None),
    }
}

// ---------------------------------------------------------------------------
// `ServerHandler` impl — wires up tool routing
// ---------------------------------------------------------------------------

#[tool_handler]
impl ServerHandler for ProFlowServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::from_build_env())
            .with_instructions(
                "ProFlow MCP server for worship service preparation. \
                 Use catalog_assets (optionally with an exact theme_name), fetch_plan, and \
                 show_effective_config when authoring configuration. \
                 The only production write workflow is: preview_playlist → user confirms → \
                 build_service. Put all skips, overrides, package, media, and naming choices in \
                 preview_playlist; build_service accepts no new output decisions. Preview metadata \
                 always comes from Planning Center. A fully resolved preview returns a revision \
                 naming its rendered build; an unresolved preview returns open_decisions and no \
                 revision and must be re-previewed with decisions. Use \
                 explain_rule_match and search_library for read-only inspection."
                    .to_string(),
            )
    }
}

#[cfg(test)]
mod tests;
