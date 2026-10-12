#![allow(clippy::expect_used)]

use super::*;

const INPUTS: BuildInputs<'static> = BuildInputs {
    plan_snapshot: b"plan",
    render_assets: b"assets",
    bible_corpora: b"corpora",
    library: b"library",
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
            render_assets: b"assetS",
            ..INPUTS
        },
        BuildInputs {
            library: b"librarY",
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
        plan_snapshot: b"plana",
        render_assets: b"ssets",
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

fn through<N>(build: BuildDir<N::Previous>) -> BuildDir<N>
where
    N: WrittenStage,
    N::Previous: Stage,
{
    build
        .write(|stage| fs::write(stage.join("file"), N::DIRECTORY))
        .expect("write stage")
}

fn rendered(root: &Path) -> BuildDir<Rendered> {
    through(through::<Classified>(snapshotted(root)))
}

#[test]
fn a_seal_digest_identifies_the_stage_content() {
    let root = tempfile::tempdir().expect("temporary root");
    let other_root = tempfile::tempdir().expect("other temporary root");
    let build = snapshotted(root.path());
    let digest = build.verify_stage::<Snapshotted>().expect("digest");

    assert_eq!(
        digest,
        snapshotted(other_root.path())
            .verify_stage::<Snapshotted>()
            .expect("equal stage")
    );
    let classified: BuildDir<Classified> = through(build);
    assert_ne!(
        digest,
        classified
            .verify_stage::<Classified>()
            .expect("other stage")
    );

    fs::write(classified.stage_path().join("file"), b"edited").expect("edit");
    assert!(matches!(
        classified.verify_stage::<Classified>(),
        Err(BuildDirError::SealMismatch {
            stage: "classified",
            ..
        })
    ));
}

#[test]
fn a_directory_is_named_only_by_its_own_inputs() {
    let root = tempfile::tempdir().expect("temporary root");
    let build = opened(root.path());
    assert!(build.is_named_by(&INPUTS));
    assert!(!build.is_named_by(&BuildInputs {
        library: b"another library",
        ..INPUTS
    }));
}

#[test]
fn clearing_deletes_a_render_that_was_never_committed() {
    let root = tempfile::tempdir().expect("temporary root");
    let build: BuildDir<Verified> = through(rendered(root.path()));
    let path = build.path().to_path_buf();
    let classified: BuildDir<Classified> = match opened(root.path())
        .resume::<Snapshotted>()
        .expect("resume snapshot")
    {
        Resumed::Written(build) => match build.resume().expect("resume classified") {
            Resumed::Written(build) => build,
            Resumed::NotYet(_) => panic!("classified stage must exist"),
        },
        Resumed::NotYet(_) => panic!("snapshot stage must exist"),
    };

    let cleared = classified.clear_later_stages().expect("clear");

    assert!(path.join("classified").is_dir());
    assert!(!path.join("rendered").exists());
    assert!(!path.join("verified").exists());
    assert!(!path.join("retired").exists());
    // Nothing is left to clear, and the build renders again.
    let cleared = cleared.clear_later_stages().expect("clear again");
    through::<Rendered>(cleared);
}

#[test]
fn clearing_keeps_a_committed_build_as_a_numbered_record() {
    let root = tempfile::tempdir().expect("temporary root");
    for record in ["0", "1"] {
        let classified = match snapshotted_or_resumed(root.path()) {
            Resumed::Written(build) => build,
            Resumed::NotYet(build) => through(build),
        };
        let committed: BuildDir<Committed> =
            through(through::<Verified>(through::<Rendered>(classified.clone())));
        let path = committed.path().to_path_buf();

        classified.clear_later_stages().expect("clear");

        for stage in ["rendered", "verified", "committed"] {
            assert!(!path.join(stage).exists());
            assert!(path.join("retired").join(record).join(stage).is_dir());
        }
    }
}

fn snapshotted_or_resumed(root: &Path) -> Resumed<Snapshotted, Classified> {
    let snapshot = match opened(root)
        .resume::<Snapshotted>()
        .expect("resume snapshot")
    {
        Resumed::Written(build) => build,
        Resumed::NotYet(build) => through(build),
    };
    snapshot.resume().expect("resume classified")
}
