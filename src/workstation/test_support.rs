//! A whole workstation over a temporary directory and a local stand-in for
//! Planning Center, for tests of the command-line and MCP handlers.

#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use httptest::{matchers::request, responders::json_encoded, Expectation, Server};
use serde_json::json;

use super::Workstation;
use crate::bible::BibleCorpusSnapshot;
use crate::config::Config;
use crate::paths::{BuildLocationInputs, BuildLocations};
use crate::planning_center::PlanningCenterClient;
use crate::project_config::{ProjectConfig, RawProjectConfig};
use crate::workflow::execute::RenderAssetSnapshot;
use proflow_propresenter::groups::GroupCatalog;
use proflow_propresenter::library::LibraryCatalog;
use proflow_propresenter::playlist::PlaylistMetadata;

/// The one plan the stand-in serves.
pub const PLAN_ID: &str = "plan-1";
/// Its service type.
pub const SERVICE: &str = "Sunday Morning";
/// Output key of its one item, which no rule of the empty config matches.
pub const ITEM: &str = "pco:item-1:main";

/// A loaded workstation and everything that must outlive it.
pub struct TestWorkstation {
    pub workstation: Workstation,
    root: tempfile::TempDir,
    planning_center: Server,
}

impl TestWorkstation {
    /// Serve [`PLAN_ID`], scheduled tomorrow, with one item.
    pub fn new() -> Self {
        let planning_center = Server::run();
        serve_plan(&planning_center);
        let root = tempfile::tempdir().expect("temporary root");
        let workstation = workstation(
            root.path(),
            planning_center
                .url_str("/")
                .trim_end_matches('/')
                .to_string(),
        );
        Self {
            workstation,
            root,
            planning_center,
        }
    }

    /// Hand the workstation to a server that owns it, keeping the directory
    /// and the Planning Center stand-in alive.
    pub fn into_parts(self) -> (Workstation, TestSite) {
        (
            self.workstation,
            TestSite {
                root: self.root,
                _planning_center: self.planning_center,
            },
        )
    }

    /// The directory and stand-in this workstation runs against.
    fn site_root(&self) -> &Path {
        self.root.path()
    }

    /// The plan's decisions file.
    pub fn decisions_file(&self) -> PathBuf {
        decisions_file(self.site_root())
    }

    /// Record the decision that leaves the plan's one item out.
    pub fn skip_the_item(&self) {
        skip_the_item(self.site_root());
    }

    /// How many playlists have been installed.
    pub fn installed_playlists(&self) -> usize {
        installed_playlists(self.site_root())
    }
}

/// What is left of a [`TestWorkstation`] once a server owns the workstation.
pub struct TestSite {
    root: tempfile::TempDir,
    _planning_center: Server,
}

impl TestSite {
    /// The plan's decisions file.
    pub fn decisions_file(&self) -> PathBuf {
        decisions_file(self.root.path())
    }

    /// Record the decision that leaves the plan's one item out.
    pub fn skip_the_item(&self) {
        skip_the_item(self.root.path());
    }

    /// How many playlists have been installed.
    pub fn installed_playlists(&self) -> usize {
        installed_playlists(self.root.path())
    }

    /// The data root's decision memory.
    pub fn memory_file(&self) -> PathBuf {
        self.root.path().join("data/decision-memory.toml")
    }
}

fn decisions_file(root: &Path) -> PathBuf {
    root.join("data/builds").join(PLAN_ID).join("review.toml")
}

fn skip_the_item(root: &Path) {
    let file = decisions_file(root);
    std::fs::create_dir_all(file.parent().expect("plan directory")).expect("plan directory");
    std::fs::write(
        file,
        format!("[[decision]]\nitem = \"{ITEM}\"\nskip = true\n"),
    )
    .expect("write decisions");
}

fn installed_playlists(root: &Path) -> usize {
    std::fs::read_dir(root.join("output"))
        .expect("list output")
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "proplaylist")
        })
        .count()
}

fn workstation(root: &Path, planning_center_url: String) -> Workstation {
    let data = root.join("data");
    let output = root.join("output");
    let propresenter = root.join("ProPresenter");
    for directory in [&data.join("bibles"), &output, &propresenter] {
        std::fs::create_dir_all(directory).expect("create workstation directory");
    }
    let locations = BuildLocations::from_inputs(BuildLocationInputs {
        project_data_root: data.clone(),
        presentation_library: output.clone(),
        playlist_output: output.clone(),
        propresenter_root: propresenter.clone(),
        themes: propresenter.join("Themes"),
        macros: propresenter.join("Configuration/Macros"),
    })
    .expect("checked locations");
    let group_catalog = GroupCatalog::load_optional(&locations.groups()).expect("no groups");
    Workstation {
        planning_center: PlanningCenterClient::new_with_base_url(
            &Config {
                pco_app_id: "test-app".to_string(),
                pco_secret: "test-secret".to_string(),
            },
            planning_center_url,
        )
        .expect("client"),
        bible_corpora: BibleCorpusSnapshot::capture(data.join("bibles")).expect("no corpora"),
        library: Mutex::new(LibraryCatalog::build(&output).expect("empty library")),
        group_catalog,
        playlist_metadata: PlaylistMetadata::offline_test(),
        render_assets: RenderAssetSnapshot::load(
            ProjectConfig::try_from(RawProjectConfig::default()).expect("empty config"),
            locations,
        )
        .expect("render assets"),
    }
}

fn serve_plan(server: &Server) {
    let date = (chrono::Utc::now() + chrono::Duration::days(1))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let service = json!({ "id": "service-1", "attributes": { "name": SERVICE } });
    let plan = json!({
        "id": PLAN_ID,
        "attributes": { "sort_date": date, "title": "Tomorrow" }
    });
    let respond = |path: &'static str, body: serde_json::Value| {
        server.expect(
            Expectation::matching(request::path(path))
                .times(0..)
                .respond_with(json_encoded(body)),
        );
    };
    respond(
        "/service_types",
        json!({ "data": [service], "links": { "next": null } }),
    );
    respond("/service_types/service-1", json!({ "data": service }));
    respond(
        "/service_types/service-1/plans",
        json!({ "data": [plan], "links": { "next": null } }),
    );
    respond(
        "/service_types/service-1/plans/plan-1",
        json!({ "data": plan }),
    );
    respond(
        "/plans/plan-1/items",
        json!({
            "data": [{
                "id": "item-1",
                "attributes": { "title": "Welcome", "sequence": 10 },
                "relationships": {}
            }],
            "included": [],
            "links": { "next": null }
        }),
    );
}
