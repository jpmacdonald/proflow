use super::*;

use crate::workflow::classify::tests::support::{
    song_config, song_item, write_song_with_arrangements,
};

/// A named arrangement and its UUID, as the song fixture takes them.
type Arrangement<'a> = (&'a str, Option<&'a str>);

fn song_runtime(root: &Path, files: &[(&str, &[Arrangement<'_>])]) -> TestRuntime {
    let mut runtime = TestRuntime::new(root);
    let library = runtime.locations().presentation_library().to_path_buf();
    for (file, arrangements) in files {
        let path = library.join(file);
        std::fs::create_dir_all(path.parent().expect("library directory")).expect("directory");
        write_song_with_arrangements(&path, arrangements);
    }
    runtime.replace_config(song_config(None, None));
    runtime
}

fn review_song(
    runtime: &TestRuntime,
    request: BuildRequest,
    arrangement: Option<&str>,
) -> BuildReview {
    runtime
        .executor()
        .review_snapshot(
            request,
            PlanSnapshot::offline_with_items(
                "plan-1",
                "Sunday Morning",
                vec![song_item(arrangement)],
            ),
        )
        .expect("review the song plan")
}

fn summaries(decision: &OpenDecision) -> Vec<&str> {
    decision
        .options
        .iter()
        .map(|option| option.summary.as_str())
        .collect()
}

/// The request that results from recording `choice` for the one open decision.
fn decided(
    runtime: &TestRuntime,
    decision: &OpenDecision,
    choice: &DecisionChoice,
) -> BuildRequest {
    let mut decisions = ReviewDecisions::default();
    decisions.decide(
        &decision.item,
        Some(decision.question.clone()),
        choice.clone(),
        DecidedBy::Operator,
        chrono::Utc::now(),
    );
    let mut request = decisions
        .into_request("plan-1", runtime.render_assets.config())
        .expect("an offered option is a valid decision");
    request.playlist_name = Some("Decided".to_string());
    request
}

#[test]
fn a_song_several_files_could_be_offers_each_file_and_any_of_them_resolves_it() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = song_runtime(
        root.path(),
        &[
            ("Amazing Grace.pro", &[]),
            ("Archive/Amazing Grace.pro", &[]),
        ],
    );

    let review = review_song(&runtime, reviewed_request("Decided"), None);
    let [decision] = review
        .open_decisions()
        .try_into()
        .expect("one open decision");
    let [first, second, skip] = decision.options.as_slice() else {
        panic!("two files and leaving it out: {:?}", summaries(&decision));
    };
    assert!(first.summary.starts_with("use ") && first.summary.ends_with("Amazing Grace.pro"));
    assert_ne!(first.choice, second.choice);
    assert_eq!(skip.choice, DecisionChoice::Skip);

    for option in &decision.options {
        let resolved = review_song(&runtime, decided(&runtime, &decision, &option.choice), None);
        assert!(
            resolved.open_decisions().is_empty(),
            "'{}' must resolve the item",
            option.summary
        );
    }
}

#[test]
fn an_unavailable_arrangement_offers_the_arrangements_the_file_defines() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = song_runtime(
        root.path(),
        &[(
            "Amazing Grace.pro",
            &[
                ("Short", Some("11111111-1111-4111-8111-111111111111")),
                ("Full", Some("22222222-2222-4222-8222-222222222222")),
            ],
        )],
    );

    let review = review_song(&runtime, reviewed_request("Decided"), Some("Evening"));
    let [decision] = review
        .open_decisions()
        .try_into()
        .expect("one open decision");

    assert_eq!(
        summaries(&decision),
        [
            "use arrangement 'Short'",
            "use arrangement 'Full'",
            "leave it out of the playlist"
        ]
    );
    let resolved = review_song(
        &runtime,
        decided(&runtime, &decision, &decision.options[1].choice),
        Some("Evening"),
    );
    assert!(resolved.open_decisions().is_empty());
    assert_eq!(resolved.plans()[0].arrangement(), Some("Full"));
}

#[test]
fn an_item_the_review_has_no_proposal_for_offers_only_leaving_it_out() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = song_runtime(root.path(), &[]);

    let review = review_song(&runtime, reviewed_request("Decided"), None);
    let [decision] = review
        .open_decisions()
        .try_into()
        .expect("one open decision");

    assert_eq!(summaries(&decision), ["leave it out of the playlist"]);
}

fn proposing(file: &Path) -> ResolvedItemPlan {
    let mut plan = use_existing_plan("pco:item:a", file.to_path_buf());
    plan.require_review("Configured to require review".to_string());
    plan
}

fn offered(runtime: &TestRuntime, plan: ResolvedItemPlan) -> OpenDecision {
    let review = runtime
        .executor()
        .review_build_request(
            reviewed_request("Decided"),
            &[plan],
            proflow_propresenter::PresentationSize::FULL_HD,
        )
        .expect("review");
    let [decision] = review
        .open_decisions()
        .try_into()
        .expect("one open decision");
    decision
}

#[test]
fn a_proposal_held_for_review_is_offered_as_it_stands_unless_the_plan_would_refuse_it() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = TestRuntime::new(root.path());
    let existing = runtime
        .locations()
        .presentation_library()
        .join("Doxology.pro");
    write_song_with_arrangements(&existing, &[]);

    let decision = offered(&runtime, proposing(&existing));
    assert_eq!(decision.kind, DecisionKind::ConfirmProposal);
    assert_eq!(
        summaries(&decision),
        [
            format!("use {}", existing.display()).as_str(),
            "leave it out of the playlist"
        ]
    );

    // The proposed file is gone, so taking the proposal would be refused.
    let missing = runtime.locations().presentation_library().join("Gone.pro");
    let decision = offered(&runtime, proposing(&missing));
    assert_eq!(summaries(&decision), ["leave it out of the playlist"]);
}

#[test]
fn each_kind_of_question_is_named() {
    let root = tempfile::tempdir().expect("temporary root");
    let ambiguous = song_runtime(
        root.path(),
        &[
            ("Amazing Grace.pro", &[]),
            ("Archive/Amazing Grace.pro", &[]),
        ],
    );
    let review = review_song(&ambiguous, reviewed_request("Decided"), None);
    assert_eq!(review.open_decisions()[0].kind, DecisionKind::WhichFile);

    let other_root = tempfile::tempdir().expect("temporary root");
    let arranged = song_runtime(
        other_root.path(),
        &[(
            "Amazing Grace.pro",
            &[("Short", Some("11111111-1111-4111-8111-111111111111"))],
        )],
    );
    let review = review_song(&arranged, reviewed_request("Decided"), Some("Evening"));
    assert_eq!(
        review.open_decisions()[0].kind,
        DecisionKind::WhichArrangement
    );

    let empty_root = tempfile::tempdir().expect("temporary root");
    let empty = song_runtime(empty_root.path(), &[]);
    let review = review_song(&empty, reviewed_request("Decided"), None);
    assert_eq!(review.open_decisions()[0].kind, DecisionKind::Unresolved);
}

#[test]
fn deciding_again_replaces_the_earlier_decision_and_keeps_who_when_and_why() {
    let at = chrono::DateTime::UNIX_EPOCH + chrono::Duration::days(7);
    let mut decisions = ReviewDecisions::default();
    decisions.decide("a", None, DecisionChoice::Skip, DecidedBy::Operator, at);
    decisions.decide("b", None, DecisionChoice::Skip, DecidedBy::Operator, at);
    decisions.decide(
        "a",
        Some("which file?".to_string()),
        DecisionChoice::Action(EntryOverrideAction::SelectArrangement {
            arrangement: "Short".to_string(),
        }),
        DecidedBy::Default,
        at,
    );

    let [b, a] = decisions.decisions.as_slice() else {
        panic!("one decision per item: {:?}", decisions.decisions);
    };
    assert_eq!((b.item.as_str(), b.skip), ("b", true));
    assert_eq!(a.item, "a");
    assert!(!a.skip && a.action.is_some());
    assert_eq!(a.question.as_deref(), Some("which file?"));
    assert_eq!(a.decided_by, DecidedBy::Default);
    assert_eq!(a.at, Some(at));

    decisions.forget("b");
    assert_eq!(decisions.decisions.len(), 1);
}

#[test]
fn a_linked_library_package_without_media_is_a_valid_intent() {
    use proflow_propresenter::playlist::PlaylistExportMode;

    let intent = playlist_export_intent(PlaylistExportMode::LibraryLinks, Vec::new())
        .expect("library links carry no media");
    assert_eq!(intent.mode(), PlaylistExportMode::LibraryLinks);
}

#[test]
fn a_possible_song_match_is_offered_as_the_file_to_use_even_when_the_plan_restyles_it() {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = TestRuntime::new(root.path());
    let candidate = runtime
        .locations()
        .presentation_library()
        .join("[Anthem] Amazing Grace.pro");
    write_song_with_arrangements(&candidate, &[]);
    let proposals = [
        ReadyAction::UseExisting {
            file_path: candidate.clone(),
            arrangement: None,
        },
        ReadyAction::RestyleExisting {
            file_path: candidate.clone(),
            arrangement: None,
            transform: test_transform(),
        },
    ];

    for proposal in proposals {
        let plan = test_plan(
            "pco:item:a",
            PlanDisposition::NeedsReview(ReviewContext::song_candidate(proposal)),
        );
        let decision = offered(&runtime, plan);

        assert_eq!(decision.kind, DecisionKind::WhichFile);
        assert_eq!(
            summaries(&decision),
            [
                format!("use {}", candidate.display()).as_str(),
                "leave it out of the playlist"
            ]
        );
    }
}
