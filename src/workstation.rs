//! The loaded local workstation: every client, immutable snapshot, and catalog
//! that a service build reads.
//!
//! This is the one composition root. The MCP server and the diagnostic
//! `proflow` binary both load a [`Workstation`] and borrow an executor
//! from it; neither assembles clients or snapshots on its own.

use std::path::PathBuf;

use std::sync::Mutex;
use thiserror::Error;

use proflow_propresenter::groups::GroupCatalog;
use proflow_propresenter::library::LibraryCatalog;
use proflow_propresenter::playlist::PlaylistMetadata;

use crate::bible::{BibleCorpusError, BibleCorpusSnapshot};
use crate::config::Config;
use crate::paths::{BuildLocations, BuildLocationsError, PROJECT_CONFIG_FILE};
use crate::planning_center::PlanningCenterClient;
use crate::project_config::load_project_config;
use crate::workflow::execute::{
    BuildRequest, BuildReview, BuildServiceError, RenderAssetSnapshot, RequestError,
    ReviewDecisions, ServiceBuildExecutor,
};
use crate::workflow::report::BuildServiceResult;

/// How the `ProPresenter` workspace root is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceDiscovery {
    /// Require the configured root to agree with the active macOS show. Normal
    /// builds use this so output cannot land in a stale or foreign workspace.
    ActiveShow,
    /// Trust the configured root without consulting the active show. Only for
    /// isolated QA and export workspaces.
    ConfiguredOnly,
}

/// Errors that prevent a coherent workstation from loading.
#[derive(Debug, Error)]
pub enum WorkstationLoadError {
    /// The Planning Center client could not be initialized into a usable state.
    #[error("failed to initialize Planning Center client: {0}")]
    PlanningCenterClient(#[from] crate::planning_center::PlanningCenterError),
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
    /// Bible corpora could not be captured into one immutable runtime snapshot.
    #[error("failed to capture Bible corpora: {0}")]
    BibleCorpora(#[from] BibleCorpusError),
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

/// One immutable workstation snapshot plus the single mutable library catalog.
///
/// Config activation writes a new project file for the next process; a loaded
/// workstation never mixes new policy with catalogs built from older policy.
pub struct Workstation {
    planning_center: PlanningCenterClient,
    bible_corpora: BibleCorpusSnapshot,
    library: Mutex<LibraryCatalog>,
    group_catalog: GroupCatalog,
    playlist_metadata: PlaylistMetadata,
    render_assets: RenderAssetSnapshot,
}

impl Workstation {
    /// Load credentials, project config, Bible corpora, the presentation
    /// library, playlist metadata, and display assets as one unit.
    ///
    /// Any failure aborts the load; there is no partially usable workstation.
    pub fn load(
        config: &Config,
        discovery: WorkspaceDiscovery,
    ) -> Result<Self, WorkstationLoadError> {
        let planning_center = PlanningCenterClient::new(config)?;

        let project_data_root = BuildLocations::discover_project_data_root()?;
        let config_path = project_data_root.join(PROJECT_CONFIG_FILE);
        let project_config = load_project_config(&config_path).map_err(|error| {
            WorkstationLoadError::ProjectConfig {
                path: config_path.clone(),
                message: error.to_string(),
            }
        })?;
        let library_name = &project_config.defaults().library;
        let locations = match discovery {
            WorkspaceDiscovery::ActiveShow => BuildLocations::discover(library_name)?,
            WorkspaceDiscovery::ConfiguredOnly => {
                BuildLocations::discover_configured_workspace(library_name)?
            }
        };

        let bible_corpora =
            BibleCorpusSnapshot::capture(locations.project_data_root().join("bibles"))?;

        let propresenter_root = locations.propresenter_root();
        let playlist_metadata = PlaylistMetadata::read_from_propresenter_root(propresenter_root)
            .map_err(|error| WorkstationLoadError::PlaylistMetadata {
                path: propresenter_root.to_path_buf(),
                message: error.to_string(),
            })?;

        let library_path = locations.presentation_library();
        let library =
            LibraryCatalog::build(library_path).map_err(|error| WorkstationLoadError::Library {
                path: library_path.to_path_buf(),
                message: error.to_string(),
            })?;

        let group_catalog = GroupCatalog::load_optional(&locations.groups()).map_err(|error| {
            WorkstationLoadError::DisplayAssets {
                message: error.to_string(),
            }
        })?;
        let render_assets =
            RenderAssetSnapshot::load(project_config, locations).map_err(|error| {
                WorkstationLoadError::DisplayAssets {
                    message: error.to_string(),
                }
            })?;

        Ok(Self {
            planning_center,
            bible_corpora,
            library: Mutex::new(library),
            group_catalog,
            playlist_metadata,
            render_assets,
        })
    }

    /// Borrow an executor over this workstation's snapshots and catalog.
    pub const fn executor(&self) -> ServiceBuildExecutor<'_> {
        ServiceBuildExecutor::new(
            &self.bible_corpora,
            &self.library,
            &self.render_assets,
            &self.playlist_metadata,
        )
    }

    /// Capture the Planning Center snapshot `request` needs, then review it.
    pub async fn review_service_request(
        &self,
        request: BuildRequest,
    ) -> Result<BuildReview, BuildServiceError> {
        review_service_request(&self.planning_center, &self.executor(), request).await
    }

    /// Review `plan_id` with the decisions recorded in its `review.toml`, and
    /// render and verify it when every decision is answered.
    ///
    /// The file is the whole request: answering a question there is what
    /// makes the next review resolve it. `service_name`, when given, must be
    /// the plan's service type.
    pub async fn build_recorded(
        &self,
        plan_id: &str,
        service_name: Option<&str>,
    ) -> Result<BuildReview, BuildServiceError> {
        let mut request = self.executor().recorded_request(plan_id)?;
        request.service_name = service_name.map(ToString::to_string);
        self.review_service_request(request).await
    }

    /// Fetch the plan again and commit the verified build of exactly what it,
    /// the configuration, the library and the recorded decisions are now.
    ///
    /// If any of them changed since the build, there is no such build and
    /// nothing is committed. `revision`, when given, must be the revision the
    /// build reported.
    pub async fn commit_recorded(
        &self,
        plan_id: &str,
        service_name: Option<&str>,
        revision: Option<&str>,
    ) -> Result<BuildServiceResult, BuildServiceError> {
        let executor = self.executor();
        let mut request = executor.recorded_request(plan_id)?;
        request.service_name = service_name.map(ToString::to_string);
        let current = self
            .planning_center
            .capture_plan(executor.plan_lookup(&request)?)
            .await?;
        executor.commit_recorded(request, &current, revision)
    }

    /// Replace the decisions recorded for `plan_id`.
    pub fn record_decisions(
        &self,
        plan_id: &str,
        decisions: &ReviewDecisions,
    ) -> Result<(), BuildServiceError> {
        Ok(decisions.write(&self.executor().review_decisions_path(plan_id)?)?)
    }

    /// The upcoming plan that is scheduled soonest, optionally among plans of
    /// one service type.
    pub async fn next_plan_id(
        &self,
        service_name: Option<&str>,
    ) -> Result<String, BuildServiceError> {
        let days_ahead = self.render_assets.config().plan_lookahead_days();
        let (_, plans) = self
            .planning_center
            .get_upcoming_services(days_ahead)
            .await?;
        Ok(soonest_plan(
            plans
                .iter()
                .map(|plan| (plan.id.as_str(), plan.service_name.as_str(), plan.date)),
            service_name,
            days_ahead.get(),
        )?
        .to_string())
    }

    /// Planning Center API client.
    pub const fn planning_center(&self) -> &PlanningCenterClient {
        &self.planning_center
    }

    /// Immutable Bible corpora captured at load.
    pub const fn bible_corpora(&self) -> &BibleCorpusSnapshot {
        &self.bible_corpora
    }

    /// The presentation library catalog. Builds update it under this lock.
    pub const fn library(&self) -> &Mutex<LibraryCatalog> {
        &self.library
    }

    /// Installed cue-group catalog.
    pub const fn group_catalog(&self) -> &GroupCatalog {
        &self.group_catalog
    }

    /// Producer profile read from the live playlist library.
    pub const fn playlist_metadata(&self) -> &PlaylistMetadata {
        &self.playlist_metadata
    }

    /// Project config, locations, themes, macros, and background bytes.
    pub const fn render_assets(&self) -> &RenderAssetSnapshot {
        &self.render_assets
    }
}

/// The Planning Center boundary around a synchronous review: compute the
/// lookup, perform it, hand the snapshot to the executor.
pub async fn review_service_request(
    planning_center: &PlanningCenterClient,
    executor: &ServiceBuildExecutor<'_>,
    request: BuildRequest,
) -> Result<BuildReview, BuildServiceError> {
    let snapshot = planning_center
        .capture_plan(executor.plan_lookup(&request)?)
        .await?;
    executor.review_snapshot(request, snapshot)
}

/// Choose the soonest plan from `(id, service type, date)` triples.
///
/// Two service types often meet at the same time. Rather than guess between
/// them, the choice is refused and names the candidates.
fn soonest_plan<'a>(
    plans: impl IntoIterator<Item = (&'a str, &'a str, chrono::DateTime<chrono::Utc>)>,
    service_name: Option<&str>,
    days_ahead: i64,
) -> Result<&'a str, RequestError> {
    let mut candidates = plans
        .into_iter()
        .filter(|(_, service, _)| service_name.is_none_or(|wanted| wanted == *service))
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(_, _, date)| *date);
    let Some((first, _, soonest)) = candidates.first().copied() else {
        return Err(RequestError::NoUpcomingPlan { days_ahead });
    };
    candidates.retain(|(_, _, date)| *date == soonest);
    if candidates.len() > 1 {
        return Err(RequestError::AmbiguousNextPlan {
            candidates: candidates
                .iter()
                .map(|(id, service, _)| format!("{id} ({service})"))
                .collect(),
        });
    }
    Ok(first)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use chrono::{DateTime, Duration, Utc};

    use super::*;

    fn at(hours: i64) -> DateTime<Utc> {
        DateTime::<Utc>::UNIX_EPOCH + Duration::hours(hours)
    }

    #[test]
    fn the_next_plan_is_the_soonest_of_the_wanted_service_type() {
        let plans = [
            ("late", "Sunday Morning", at(48)),
            ("soon", "Sunday Morning", at(24)),
            ("sooner", "Wednesday Night", at(12)),
        ];
        assert_eq!(soonest_plan(plans, None, 30).expect("soonest"), "sooner");
        assert_eq!(
            soonest_plan(plans, Some("Sunday Morning"), 30).expect("soonest of type"),
            "soon"
        );
    }

    #[test]
    fn no_plan_and_two_plans_at_the_same_time_are_refused() {
        assert!(matches!(
            soonest_plan([("a", "Sunday Morning", at(1))], Some("Other"), 30),
            Err(RequestError::NoUpcomingPlan { days_ahead: 30 })
        ));
        let simultaneous = [
            ("a", "Traditional", at(24)),
            ("b", "Contemporary", at(24)),
            ("c", "Traditional", at(48)),
        ];
        assert!(matches!(
            soonest_plan(simultaneous, None, 30),
            Err(RequestError::AmbiguousNextPlan { candidates })
                if candidates == ["a (Traditional)", "b (Contemporary)"]
        ));
        assert_eq!(
            soonest_plan(simultaneous, Some("Contemporary"), 30).expect("named type"),
            "b"
        );
    }
}
