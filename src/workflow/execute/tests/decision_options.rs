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
