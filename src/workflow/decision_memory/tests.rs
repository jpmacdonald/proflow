#![allow(clippy::expect_used)]

use chrono::Duration;

use super::*;
use crate::workflow::execute::{DecisionOption, EntryOverrideAction};

fn at(days: i64) -> DateTime<Utc> {
    DateTime::<Utc>::UNIX_EPOCH + Duration::days(days)
}

fn use_file(file: &str) -> DecisionChoice {
    DecisionChoice::Action(EntryOverrideAction::UseExisting {
        file_path: file.to_string(),
        arrangement: None,
    })
}

fn question(title: &str, kind: DecisionKind, choices: &[DecisionChoice]) -> OpenDecision {
    OpenDecision {
        item: "pco:1:main".to_string(),
        title: title.to_string(),
        question: "which one?".to_string(),
        kind,
        options: choices.iter().cloned().map(DecisionOption::new).collect(),
    }
}

fn order(decision: &OpenDecision) -> Vec<String> {
    decision
        .options
        .iter()
        .map(|option| option.choice.to_string())
        .collect()
}

#[test]
fn remembered_choices_come_first_most_recent_first_and_carry_their_history() {
    let mut memory = DecisionMemory::default();
    memory.remember(
        "Amazing Grace",
        DecisionKind::WhichFile,
        &use_file("/b.pro"),
        at(1),
    );
    memory.remember(
        "Amazing Grace",
        DecisionKind::WhichFile,
        &use_file("/b.pro"),
        at(8),
    );
    memory.remember(
        "Amazing Grace",
        DecisionKind::WhichFile,
        &DecisionChoice::Skip,
        at(15),
    );
    let mut decision = question(
        "Amazing Grace",
        DecisionKind::WhichFile,
        &[
            use_file("/a.pro"),
            use_file("/b.pro"),
            use_file("/c.pro"),
            DecisionChoice::Skip,
        ],
    );

    memory.rank(&mut decision);

    assert_eq!(
        order(&decision),
        [
            "leave it out of the playlist",
            "use /b.pro",
            "use /a.pro",
            "use /c.pro"
        ]
    );
    assert_eq!(
        decision.options[1].remembered,
        Some(Remembered {
            times: 2,
            last: at(8)
        })
    );
    assert_eq!(decision.options[2].remembered, None);
}

#[test]
fn a_question_is_its_title_and_kind_whatever_the_case_or_spacing() {
    let mut memory = DecisionMemory::default();
    memory.remember(
        "Amazing  Grace",
        DecisionKind::WhichFile,
        &use_file("/b.pro"),
        at(1),
    );
    let choices = [use_file("/a.pro"), use_file("/b.pro")];

    let mut same = question(" amazing grace ", DecisionKind::WhichFile, &choices);
    memory.rank(&mut same);
    assert_eq!(order(&same), ["use /b.pro", "use /a.pro"]);

    for mut other in [
        question(
            "Amazing Grace (My Chains Are Gone)",
            DecisionKind::WhichFile,
            &choices,
        ),
        question("Amazing Grace", DecisionKind::WhichArrangement, &choices),
    ] {
        memory.rank(&mut other);
        assert_eq!(order(&other), ["use /a.pro", "use /b.pro"]);
        assert!(other
            .options
            .iter()
            .all(|option| option.remembered.is_none()));
    }
}

#[test]
fn a_default_is_set_and_cleared_per_question_and_is_not_implied_by_history() {
    let mut memory = DecisionMemory::default();
    let decision = question(
        "Amazing Grace",
        DecisionKind::WhichFile,
        &[use_file("/a.pro")],
    );
    memory.remember(
        "Amazing Grace",
        DecisionKind::WhichFile,
        &use_file("/a.pro"),
        at(1),
    );
    assert_eq!(memory.default_for(&decision), None);

    memory.set_default(
        "Amazing Grace",
        DecisionKind::WhichFile,
        Some(use_file("/a.pro")),
    );
    assert_eq!(memory.default_for(&decision), Some(&use_file("/a.pro")));
    assert_eq!(
        memory.default_for(&question(
            "Amazing Grace",
            DecisionKind::WhichArrangement,
            &[]
        )),
        None
    );

    memory.set_default("Amazing Grace", DecisionKind::WhichFile, None);
    assert_eq!(memory.default_for(&decision), None);
}

#[test]
fn memory_survives_being_written_and_read_and_an_absent_file_is_empty() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = DecisionMemory::path(directory.path());
    assert_eq!(path, directory.path().join("decision-memory.toml"));
    assert_eq!(
        DecisionMemory::read(&path).expect("absent"),
        DecisionMemory::default()
    );

    let mut memory = DecisionMemory::default();
    memory.remember(
        "Amazing Grace",
        DecisionKind::WhichFile,
        &use_file("/a.pro"),
        at(1),
    );
    memory.remember(
        "Welcome",
        DecisionKind::Unresolved,
        &DecisionChoice::Skip,
        at(2),
    );
    memory.set_default(
        "Welcome",
        DecisionKind::Unresolved,
        Some(DecisionChoice::Skip),
    );
    memory.write(&path).expect("write");

    assert_eq!(DecisionMemory::read(&path).expect("read"), memory);

    std::fs::write(&path, "[[question]]\ntitle = \"x\"\n").expect("corrupt");
    let error = DecisionMemory::read(&path).expect_err("a question needs a kind");
    assert!(error.to_string().contains("is invalid"), "{error}");
}
