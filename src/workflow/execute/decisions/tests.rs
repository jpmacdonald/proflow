#![allow(clippy::expect_used)]

use super::*;
use crate::project_config::{BackgroundAssetPath, RawProjectConfig};

const EXAMPLE: &str = r#"
playlist_name = "May 24, 2026 - Sunday Morning"
package_mode = "export_portable"

[[media]]
path = "/media/welcome.jpg"
archive_path = "Media/welcome.jpg"

[[decision]]
item = "pco:101:main"
question = "Two library presentations are titled 'Amazing Grace'"
options = ["/library/Amazing Grace.pro", "/library/Amazing Grace 2.pro"]
decided_by = "operator"
at = "2026-05-23T19:30:00Z"

[decision.action]
kind = "use_existing"
file_path = "/library/Amazing Grace.pro"
arrangement = "Default"

[[decision]]
item = "pco:102:main"
skip = true

[[decision]]
item = "pco:103:main"
playlist_name = "Sermon"
slide_type = "text"
decided_by = "rule"

[decision.action]
kind = "set_background"
background = "sermon"
"#;

fn config() -> ProjectConfig {
    let mut raw = RawProjectConfig::default();
    raw.backgrounds.insert(
        BackgroundId::new("sermon").expect("background id"),
        BackgroundAssetPath::new("backgrounds/sermon.png").expect("background path"),
    );
    ProjectConfig::try_from(raw).expect("valid config")
}

fn decisions(text: &str) -> Result<ReviewDecisions, DecisionError> {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("review.toml");
    std::fs::write(&path, text).expect("write decisions");
    ReviewDecisions::read(&path)
}

#[test]
fn recorded_decisions_become_the_request_a_review_consumes() {
    let request = decisions(EXAMPLE)
        .expect("parse")
        .into_request("plan-1", &config())
        .expect("request");

    assert_eq!(request.plan_id, "plan-1");
    assert_eq!(request.service_name, None);
    assert_eq!(
        request.playlist_name.as_deref(),
        Some("May 24, 2026 - Sunday Morning")
    );
    assert_eq!(request.skip_output_keys, ["pco:102:main"]);
    let media = request.playlist_export.media_assets();
    assert_eq!(media[0].source_path, Path::new("/media/welcome.jpg"));
    assert_eq!(media[0].archive_path.as_deref(), Some("Media/welcome.jpg"));

    let [existing, sermon] = request.overrides.as_slice() else {
        panic!("two overrides: {:?}", request.overrides);
    };
    assert_eq!(existing.output_key, "pco:101:main");
    assert!(matches!(
        &existing.action,
        Some(OverrideAction::UseExisting { file_path, arrangement })
            if file_path == Path::new("/library/Amazing Grace.pro")
                && arrangement.as_deref() == Some("Default")
    ));
    assert_eq!(sermon.playlist_name.as_deref(), Some("Sermon"));
    assert_eq!(sermon.slide_type, Some(OverrideSlideType::Text));
    assert!(matches!(
        &sermon.action,
        Some(OverrideAction::SetBackground { background })
            if background.id().as_str() == "sermon"
    ));
}

#[test]
fn a_plan_with_no_decisions_file_has_no_decisions() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let read = ReviewDecisions::read(&directory.path().join("review.toml")).expect("absent file");

    assert_eq!(read, ReviewDecisions::default());
    let request = read.into_request("plan-1", &config()).expect("request");
    assert!(request.skip_output_keys.is_empty() && request.overrides.is_empty());
}

#[test]
fn decisions_survive_being_written_and_read_again() {
    let parsed = decisions(EXAMPLE).expect("parse");
    let written = parsed.to_toml().expect("serialize");
    assert_eq!(decisions(&written).expect("parse again"), parsed);
}

#[test]
fn malformed_decisions_name_the_file_and_the_problem() {
    for (text, problem) in [
        ("playlist_nam = \"typo\"", "unknown field `playlist_nam`"),
        ("[[decision]]\nskip = true", "missing field `item`"),
        (
            "[[decision]]\nitem = \"a\"\n[decision.action]\nkind = \"use_existing\"",
            "missing field `file_path`",
        ),
        (
            "[[decision]]\nitem = \"a\"\n[decision.action]\nkind = \"delete\"",
            "unknown variant `delete`",
        ),
    ] {
        let Err(DecisionError::Parse { path, message }) = decisions(text) else {
            panic!("{text} must not parse");
        };
        assert!(path.ends_with("review.toml"));
        assert!(message.contains(problem), "{message}");
    }
}

#[test]
fn contradictory_decisions_are_rejected() {
    let request = |text: &str| {
        decisions(text)
            .expect("parse")
            .into_request("plan-1", &config())
    };

    assert!(matches!(
        request("[[decision]]\nitem = \"a\"\nskip = true\n[[decision]]\nitem = \"a\"\nskip = true"),
        Err(DecisionError::DuplicateItem { item }) if item == "a"
    ));
    assert!(matches!(
        request("[[decision]]\nitem = \"a\"\nskip = true\nplaylist_name = \"Renamed\""),
        Err(DecisionError::SkipWithChanges { item }) if item == "a"
    ));
    assert!(matches!(
        request("package_mode = \"library_local\"\n[[media]]\npath = \"/media/a.jpg\""),
        Err(DecisionError::MediaWithoutPortablePackage)
    ));
    assert!(matches!(
        request(
            "[[decision]]\nitem = \"a\"\n[decision.action]\nkind = \"set_background\"\nbackground = \"missing\""
        ),
        Err(DecisionError::UnknownBackground { id, available })
            if id.as_str() == "missing" && available == "sermon"
    ));
    assert!(matches!(
        request(
            "[[decision]]\nitem = \"a\"\n[decision.action]\nkind = \"select_arrangement\"\narrangement = \" padded\""
        ),
        Err(DecisionError::InvalidField { field: "arrangement" })
    ));
}

#[test]
fn a_decisions_file_that_cannot_be_read_is_an_error_not_an_empty_file() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("review.toml");
    std::fs::create_dir(&path).expect("a directory where the file should be");

    assert!(matches!(
        ReviewDecisions::read(&path),
        Err(DecisionError::Read { path: reported, .. }) if reported == path
    ));
}

#[test]
fn a_skipped_item_may_carry_no_change_of_any_kind() {
    for change in [
        "playlist_name = \"Renamed\"",
        "slide_type = \"text\"",
        "[decision.action]\nkind = \"select_arrangement\"\narrangement = \"Default\"",
    ] {
        let text = format!("[[decision]]\nitem = \"a\"\nskip = true\n{change}");
        assert!(
            matches!(
                decisions(&text)
                    .expect("parse")
                    .into_request("plan-1", &config()),
                Err(DecisionError::SkipWithChanges { .. })
            ),
            "{change}"
        );
    }
}
