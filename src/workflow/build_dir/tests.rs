#![allow(clippy::expect_used)]

use super::*;

const INPUTS: BuildInputs<'static> = BuildInputs {
    plan_snapshot: b"plan",
    config: b"config",
    bible_corpora: b"corpora",
    review_decisions: b"decisions",
};

fn plan() -> BuildPlanId {
    BuildPlanId::new("12345").expect("plan id")
}

fn opened(root: &Path) -> BuildDir<Opened> {
    BuildDir::open(root, &plan(), &INPUTS).expect("open build directory")
}

fn snapshotted(root: &Path) -> BuildDir<Snapshotted> {
    opened(root)
        .write(|stage| {
            fs::write(stage.join("plan.json"), b"{}")?;
            fs::create_dir(stage.join("nested"))?;
            fs::write(stage.join("nested/catalog.json"), b"[]")
        })
        .expect("write snapshot stage")
}

fn resume_snapshot(root: &Path) -> Result<Resumed<Opened, Snapshotted>, BuildDirError> {
    opened(root).resume()
}

#[test]
fn equal_inputs_hash_equally_and_every_input_byte_changes_the_hash() {
    let base = InputHash::of(&INPUTS);
    assert_eq!(base, InputHash::of(&INPUTS));
    assert_eq!(base.to_string().len(), 64);

    let changed = [
        BuildInputs {
            plan_snapshot: b"plaN",
            ..INPUTS
        },
        BuildInputs {
            config: b"confiG",
            ..INPUTS
        },
        BuildInputs {
            bible_corpora: b"corporA",
            ..INPUTS
        },
        BuildInputs {
            review_decisions: b"decisionS",
            ..INPUTS
        },
    ];
    for inputs in changed {
        assert_ne!(base, InputHash::of(&inputs));
    }
}

#[test]
fn a_byte_moved_between_two_inputs_changes_the_hash() {
    let moved = BuildInputs {
        plan_snapshot: b"planc",
        config: b"onfig",
        ..INPUTS
    };
    assert_ne!(InputHash::of(&INPUTS), InputHash::of(&moved));
}

#[test]
fn the_directory_is_named_by_plan_and_inputs_and_decisions_live_beside_it() {
    let root = tempfile::tempdir().expect("temporary root");
    let first = opened(root.path());
    assert_eq!(first.path(), opened(root.path()).path());
    assert_eq!(
        first.path(),
        root.path()
            .join("12345")
            .join(InputHash::of(&INPUTS).to_string())
    );

    let answered = BuildInputs {
        review_decisions: b"answered",
        ..INPUTS
    };
    let other = BuildDir::open(root.path(), &plan(), &answered).expect("open");
    assert_ne!(first.path(), other.path());

    let decisions = review_decisions_path(root.path(), &plan());
    assert_eq!(decisions, root.path().join("12345").join("review.toml"));
    assert!(!decisions.starts_with(first.path()));
}

#[test]
fn a_written_stage_is_resumed_with_its_files() {
    let root = tempfile::tempdir().expect("temporary root");
    let written = snapshotted(root.path());
    assert_eq!(
        fs::read(written.stage_path().join("plan.json")).expect("plan"),
        b"{}"
    );

    let Resumed::Written(resumed) = resume_snapshot(root.path()).expect("resume") else {
        panic!("snapshot stage must be resumable");
    };
    assert_eq!(resumed.stage_path(), written.stage_path());
    assert!(matches!(
        resumed.resume::<Classified>().expect("resume next"),
        Resumed::NotYet(_)
    ));
}

#[test]
fn a_stage_is_written_once() {
    let root = tempfile::tempdir().expect("temporary root");
    snapshotted(root.path());
    let again = opened(root.path()).write::<Snapshotted>(|_| Ok(()));
    assert!(matches!(
        again,
        Err(BuildDirError::AlreadyWritten { stage: "snapshot" })
    ));
}

#[test]
fn a_failed_write_leaves_no_stage_and_no_temporary_directory() {
    let root = tempfile::tempdir().expect("temporary root");
    let failed = opened(root.path()).write::<Snapshotted>(|stage| {
        fs::write(stage.join("plan.json"), b"partial")?;
        Err(io::Error::other("interrupted"))
    });
    assert!(matches!(failed, Err(BuildDirError::Io { .. })));

    let build = opened(root.path());
    assert!(!build.path().join("snapshot").exists());
    assert!(!build.path().join("snapshot.tmp").exists());
    assert!(matches!(
        build.resume::<Snapshotted>().expect("resume"),
        Resumed::NotYet(_)
    ));
}

#[test]
fn a_temporary_directory_left_by_a_crash_is_not_a_stage_and_is_discarded() {
    let root = tempfile::tempdir().expect("temporary root");
    let crashed = opened(root.path()).path().join("snapshot.tmp");
    fs::create_dir(&crashed).expect("crashed stage");
    fs::write(crashed.join("half-written.json"), b"{").expect("partial file");

    assert!(matches!(
        resume_snapshot(root.path()).expect("resume"),
        Resumed::NotYet(_)
    ));
    let written = snapshotted(root.path());
    assert!(!written.stage_path().join("half-written.json").exists());
    assert!(!crashed.exists());
}

#[test]
fn a_stage_changed_after_sealing_is_refused() {
    type Tamper = fn(&Path);
    let tampers: [(Tamper, &str); 4] = [
        (
            |stage| fs::write(stage.join("plan.json"), b"{ }").expect("edit"),
            "file 'plan.json'",
        ),
        (
            |stage| fs::write(stage.join("extra.json"), b"{}").expect("add"),
            "file 'extra.json'",
        ),
        (
            |stage| fs::remove_file(stage.join("nested/catalog.json")).expect("remove"),
            "file 'nested/catalog.json'",
        ),
        (
            |stage| fs::write(stage.join("stage.json"), b"{}").expect("replace seal"),
            "missing field",
        ),
    ];
    for (tamper, named) in tampers {
        let root = tempfile::tempdir().expect("temporary root");
        tamper(&snapshotted(root.path()).stage_path());
        let Err(BuildDirError::SealMismatch {
            stage: "snapshot",
            detail,
        }) = resume_snapshot(root.path())
        else {
            panic!("a changed stage must not resume");
        };
        assert!(detail.contains(named), "{detail}");
    }
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_inside_a_stage_is_refused() {
    let root = tempfile::tempdir().expect("temporary root");
    let outside = root.path().join("outside.json");
    fs::write(&outside, b"{}").expect("outside file");
    let linked = opened(root.path()).write::<Snapshotted>(|stage| {
        std::os::unix::fs::symlink(&outside, stage.join("plan.json"))
    });
    assert!(matches!(
        linked,
        Err(BuildDirError::UnsupportedEntry {
            stage: "snapshot",
            ..
        })
    ));
}

#[test]
fn plan_ids_that_could_leave_the_builds_root_are_rejected() {
    for value in ["", "..", "a/b", "a\\b", "plan id", &"9".repeat(65)] {
        assert!(matches!(
            BuildPlanId::new(value),
            Err(BuildDirError::InvalidPlanId { .. })
        ));
    }
    assert_eq!(
        BuildPlanId::new("plan_7-a").expect("valid").as_str(),
        "plan_7-a"
    );
}
