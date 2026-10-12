#![allow(clippy::expect_used)]

use super::*;
use proflow_propresenter::playlist::PlaylistExportMode;
use proflow_propresenter::text_fit::TextFitContractSummary;

fn parse(line: &str) -> Result<Invocation, UsageError> {
    Invocation::parse(line.split_whitespace().map(ToString::to_string))
}

fn result(warnings: &[&str]) -> Box<BuildServiceResult> {
    Box::new(BuildServiceResult {
        playlist_path: PathBuf::from("/show/Playlists/May 24.proplaylist"),
        receipt_path: PathBuf::from("/show/Playlists/May 24.proplaylist.proflow-build.json"),
        receipt_revision: String::new(),
        text_fit_contract: TextFitContractSummary::diagnostic(),
        package_mode: PlaylistExportMode::LibraryLinks,
        media_asset_count: 0,
        entries: Vec::new(),
        total_items: 12,
        generated_count: 3,
        library_count: 9,
        skipped_count: 1,
        warnings: warnings.iter().map(ToString::to_string).collect(),
    })
}

#[test]
fn a_command_line_names_a_step_a_plan_and_optional_assertions() {
    assert_eq!(
        parse("build --plan 12345").expect("build"),
        Invocation {
            command: Command::Build,
            plan: PlanSelector::Id("12345".to_string()),
            service: None,
            workspace: WorkspaceDiscovery::ActiveShow,
        }
    );
    assert_eq!(
        parse("commit --configured-workspace --service Traditional --plan next").expect("commit"),
        Invocation {
            command: Command::Commit,
            plan: PlanSelector::Next,
            service: Some("Traditional".to_string()),
            workspace: WorkspaceDiscovery::ConfiguredOnly,
        }
    );
}

#[test]
fn a_malformed_command_line_says_what_is_wrong() {
    for (line, problem) in [
        ("", "expected `build` or `commit`"),
        (
            "review --plan 1",
            "expected `build` or `commit`, found 'review'",
        ),
        ("build", "--plan <id|next> is required"),
        ("build --plan", "--plan requires a value"),
        ("build --plan 1 --service", "--service requires a value"),
        ("build --plan 1 --plan 2", "--plan may be given only once"),
        ("build --plan 1 extra", "unexpected argument 'extra'"),
        ("commit --plan 1 --force", "unexpected argument '--force'"),
    ] {
        assert_eq!(parse(line).expect_err(line).to_string(), problem, "{line}");
    }
}

#[test]
fn open_decisions_exit_two_and_say_where_and_how_to_answer() {
    let outcome = Outcome::NeedsDecisions {
        plan_id: "12345".to_string(),
        playlist_name: "May 24, 2026 - Sunday Morning".to_string(),
        decisions: vec![
            OpenDecision {
                item: "pco:101:main".to_string(),
                title: "Amazing Grace".to_string(),
                question: "2 library presentations are titled 'Amazing Grace'".to_string(),
            },
            OpenDecision {
                item: "pco:107:main".to_string(),
                title: "Announcements".to_string(),
                question: "no rule matches this item".to_string(),
            },
        ],
        decisions_file: PathBuf::from("/data/builds/12345/review.toml"),
    };

    assert_eq!(outcome.exit_code(), 2);
    assert_eq!(
        outcome.to_string(),
        "\
Plan 12345 \"May 24, 2026 - Sunday Morning\" needs 2 decision(s) before it can be built.

  pco:101:main  Amazing Grace
      2 library presentations are titled 'Amazing Grace'
  pco:107:main  Announcements
      no rule matches this item

Record each one in /data/builds/12345/review.toml and build again. To leave an item out:

  [[decision]]
  item = \"pco:101:main\"
  skip = true

Every other kind of decision is described in docs/review-decisions.md.
"
    );
}

#[test]
fn a_verified_build_exits_zero_and_says_nothing_is_installed() {
    let outcome = Outcome::Built {
        plan_id: "12345".to_string(),
        playlist_name: "May 24, 2026 - Sunday Morning".to_string(),
        result: result(&["'Offertory' has no background"]),
        build_directory: PathBuf::from("/data/builds/12345/ab12"),
    };

    assert_eq!(outcome.exit_code(), 0);
    assert_eq!(
        outcome.to_string(),
        "\
Plan 12345 \"May 24, 2026 - Sunday Morning\" is built and verified: 12 in the playlist (3 generated, 9 from the library), 1 skipped.
Nothing is installed yet. To install it:

  proflow commit --plan 12345

Build: /data/builds/12345/ab12

Warnings:
  - 'Offertory' has no background
"
    );
}

#[test]
fn a_committed_build_exits_zero_and_names_what_it_installed() {
    let outcome = Outcome::Committed {
        plan_id: "12345".to_string(),
        result: result(&[]),
    };

    assert_eq!(outcome.exit_code(), 0);
    assert_eq!(
        outcome.to_string(),
        "\
Plan 12345 is installed: 12 in the playlist (3 generated, 9 from the library), 1 skipped.
Playlist: /show/Playlists/May 24.proplaylist
Receipt:  /show/Playlists/May 24.proplaylist.proflow-build.json
"
    );
}

mod handlers {
    use super::*;
    use crate::workstation::test_support::{TestWorkstation, ITEM, PLAN_ID, SERVICE};

    async fn run_line(station: &TestWorkstation, line: &str) -> Result<Outcome, BuildServiceError> {
        run(&station.workstation, &parse(line).expect("command line")).await
    }

    #[tokio::test]
    async fn a_plan_goes_from_open_decision_to_built_to_installed_exactly_once() {
        let station = TestWorkstation::new();

        let blocked = run_line(&station, "build --plan plan-1")
            .await
            .expect("build");
        let Outcome::NeedsDecisions {
            plan_id,
            decisions,
            decisions_file,
            ..
        } = &blocked
        else {
            panic!("the unmatched item needs a decision: {blocked:?}");
        };
        assert_eq!(blocked.exit_code(), 2);
        assert_eq!(plan_id, PLAN_ID);
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].item, ITEM);
        assert_eq!(decisions_file, &station.decisions_file());

        let nothing_built = run_line(&station, "commit --plan plan-1").await;
        assert!(matches!(
            nothing_built,
            Err(BuildServiceError::NoVerifiedBuild { .. })
        ));

        station.skip_the_item();
        let built = run_line(&station, "build --plan plan-1")
            .await
            .expect("build");
        let Outcome::Built {
            build_directory,
            result,
            ..
        } = &built
        else {
            panic!("a fully decided plan builds: {built:?}");
        };
        assert_eq!(built.exit_code(), 0);
        assert_eq!(result.skipped_count, 1);
        assert!(build_directory.join("verified").is_dir());
        assert_eq!(station.installed_playlists(), 0, "build installs nothing");

        let committed = run_line(&station, "commit --plan plan-1")
            .await
            .expect("commit");
        let Outcome::Committed { result, .. } = &committed else {
            panic!("the verified build installs: {committed:?}");
        };
        assert!(result.playlist_path.is_file());
        assert_eq!(station.installed_playlists(), 1);

        assert!(matches!(
            run_line(&station, "commit --plan plan-1").await,
            Err(BuildServiceError::BuildAlreadyCommitted { .. })
        ));
    }

    #[tokio::test]
    async fn a_decision_edited_after_the_build_leaves_nothing_to_install() {
        let station = TestWorkstation::new();
        station.skip_the_item();
        run_line(&station, "build --plan plan-1")
            .await
            .expect("build");

        std::fs::write(
            station.decisions_file(),
            format!("playlist_name = \"Renamed\"\n[[decision]]\nitem = \"{ITEM}\"\nskip = true\n"),
        )
        .expect("edit decisions");

        assert!(matches!(
            run_line(&station, "commit --plan plan-1").await,
            Err(BuildServiceError::NoVerifiedBuild { .. })
        ));
        assert_eq!(station.installed_playlists(), 0);
    }

    #[tokio::test]
    async fn next_names_the_soonest_plan_and_a_wrong_service_type_is_refused() {
        let station = TestWorkstation::new();

        let next = run_line(&station, "build --plan next").await.expect("next");
        assert!(matches!(
            &next,
            Outcome::NeedsDecisions { plan_id, .. } if plan_id == PLAN_ID
        ));
        assert_eq!(
            station
                .workstation
                .next_plan_id(Some(SERVICE))
                .await
                .expect("next of the service type"),
            PLAN_ID
        );
        assert!(matches!(
            run_line(&station, "build --plan next --service Wednesday").await,
            Err(BuildServiceError::Request(_))
        ));
        assert!(matches!(
            run_line(&station, "build --plan plan-1 --service Wednesday").await,
            Err(BuildServiceError::PlanningCenter(_) | BuildServiceError::Request(_))
        ));
    }
}
