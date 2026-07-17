//! MCP server for autonomous service preparation.
//!
//! Exposes the reviewed service workflow as MCP tools so an LLM can prep a
//! service without bypassing project configuration or preview approval.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use rmcp::model::{Implementation, ServerCapabilities, ServerInfo};
use rmcp::{tool_handler, ServerHandler};
use tokio::sync::Mutex;

use crate::bible::BibleService;
use crate::config::Config;
use crate::paths::{BuildLocations, BuildLocationsError, PROJECT_CONFIG_FILE};
use crate::planning_center::PlanningCenterClient;
use crate::project_config::load_project_config;
use crate::propresenter::groups::GroupCatalog;
use crate::propresenter::library::LibraryCatalog;
use crate::propresenter::playlist::PlaylistMetadata;
use crate::workflow::execute::{RenderAssetSnapshot, ServiceBuildExecutor};

mod config;
mod review;
mod schema;
mod tools;

use review::PreparedPlanSnapshot;
pub use schema::{
    BuildServiceArgs, CatalogAssetsArgs, EntryOverride, EntryOverrideAction, ExplainRuleMatchArgs,
    FetchPlanArgs, PlaylistMediaAssetArg, PreviewPlaylistArgs, SearchLibraryArgs,
    WriteProjectConfigArgs,
};

// ---------------------------------------------------------------------------
// Server struct
// ---------------------------------------------------------------------------

/// Shared MCP server state holding all service clients and caches.
#[derive(Clone)]
pub struct ProFlowServer {
    render_assets: Arc<RenderAssetSnapshot>,
    pco_client: Arc<PlanningCenterClient>,
    bible_service: Arc<Mutex<BibleService>>,
    file_index: Arc<Mutex<LibraryCatalog>>,
    group_catalog: Arc<GroupCatalog>,
    playlist_metadata: Arc<PlaylistMetadata>,
    reviewed_plans: Arc<Mutex<HashMap<String, PreparedPlanSnapshot>>>,
}

/// Errors that prevent the MCP server from starting in a coherent state.
#[derive(Debug, thiserror::Error)]
pub enum ProFlowServerInitError {
    /// The Planning Center client could not be initialized into a usable state.
    #[error("failed to initialize Planning Center client: {0}")]
    PlanningCenterClient(#[source] crate::error::Error),
    /// Workstation paths could not be resolved into one coherent snapshot.
    #[error(transparent)]
    Locations(#[from] BuildLocationsError),
    /// The configured project file could not be loaded.
    #[error("failed to load project config at {path}: {message}")]
    ProjectConfig {
        /// Config path that could not be loaded.
        path: PathBuf,
        /// Underlying load failure.
        message: String,
    },
    /// The configured `ProPresenter` library could not be indexed.
    #[error("failed to index ProPresenter library at {path}: {message}")]
    Library {
        /// Library root that could not be indexed.
        path: PathBuf,
        /// Underlying index failure.
        message: String,
    },
    /// The live playlist library did not provide a valid producer profile.
    #[error("failed to read playlist metadata for ProPresenter library at {path}: {message}")]
    PlaylistMetadata {
        /// Configured library used to find `Playlists/Library`.
        path: PathBuf,
        /// Underlying metadata read or decode failure.
        message: String,
    },
    /// A configured theme or macro document could not be loaded.
    #[error("failed to load configured display assets: {message}")]
    DisplayAssets {
        /// Underlying theme or macro load failure.
        message: String,
    },
}

impl ProFlowServer {
    /// Build a new server from loaded configuration.
    ///
    /// Startup succeeds only after credentials, project config, and configured
    /// library state have been loaded successfully.
    pub fn new(config: &Config) -> Result<Self, ProFlowServerInitError> {
        let pco_client = PlanningCenterClient::new(config)
            .map_err(ProFlowServerInitError::PlanningCenterClient)?;

        let project_data_root = BuildLocations::discover_project_data_root()?;
        let mappings_path = project_data_root.join(PROJECT_CONFIG_FILE);
        let mappings = load_project_config(&mappings_path).map_err(|error| {
            ProFlowServerInitError::ProjectConfig {
                path: mappings_path.clone(),
                message: error.to_string(),
            }
        })?;
        let locations = BuildLocations::discover(&mappings.defaults().library)?;

        let bible_path = locations.project_data_root().join("bibles");
        let bible_service = BibleService::new(bible_path);

        let library_path = locations.presentation_library();
        let propresenter_root = locations.propresenter_root();
        let playlist_metadata = PlaylistMetadata::read_from_propresenter_root(propresenter_root)
            .map_err(|error| ProFlowServerInitError::PlaylistMetadata {
                path: propresenter_root.to_path_buf(),
                message: error.to_string(),
            })?;

        let file_index = LibraryCatalog::build(library_path).map_err(|error| {
            ProFlowServerInitError::Library {
                path: library_path.to_path_buf(),
                message: error.to_string(),
            }
        })?;

        // Load and validate one immutable runtime snapshot. Config activation
        // writes a new snapshot for the next server process; it never mixes new
        // mappings with caches created from an older config.
        let group_catalog = GroupCatalog::load_optional(&locations.groups()).map_err(|error| {
            ProFlowServerInitError::DisplayAssets {
                message: error.to_string(),
            }
        })?;
        let render_assets = RenderAssetSnapshot::load(mappings, locations).map_err(|error| {
            ProFlowServerInitError::DisplayAssets {
                message: error.to_string(),
            }
        })?;

        Ok(Self {
            render_assets: Arc::new(render_assets),
            pco_client: Arc::new(pco_client),
            bible_service: Arc::new(Mutex::new(bible_service)),
            file_index: Arc::new(Mutex::new(file_index)),
            group_catalog: Arc::new(group_catalog),
            playlist_metadata: Arc::new(playlist_metadata),
            reviewed_plans: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    fn service_build_executor(&self) -> ServiceBuildExecutor<'_> {
        ServiceBuildExecutor::new(
            self.pco_client.as_ref(),
            &self.bible_service,
            &self.file_index,
            self.render_assets.as_ref(),
            self.playlist_metadata.as_ref(),
        )
    }
}

pub(super) fn mcp_err(msg: impl Into<String>) -> rmcp::ErrorData {
    rmcp::ErrorData::internal_error(msg.into(), None)
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
                 consumed by one matching build attempt; an unresolved preview returns no \
                 revision and must be re-previewed with decisions. Use \
                 explain_rule_match and search_library for read-only inspection."
                    .to_string(),
            )
    }
}

#[cfg(test)]
mod tests;
