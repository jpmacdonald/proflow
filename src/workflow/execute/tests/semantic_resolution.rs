//! Source selection remains distinct from arrangement and styling decisions.

use super::*;
use crate::planning_center::types::{Category, Item, Song};
use crate::project_config::parse_project_config_value;

fn candidate_song(root: &Path, strategy: &str) -> (ResolvedItemPlan, PathBuf) {
    let source = root.join("O Praise The Name.pro");
    std::fs::write(
        &source,
        presentation_with_size("O Praise The Name", 1920.0, 1080.0).encode_to_vec(),
    )
    .expect("write unqualified song fixture");
    let source = source.canonicalize().expect("canonical song fixture path");
    let catalog = LibraryCatalog::build(root).expect("index song fixture");
    let mut presentation = serde_json::json!({
        "kind": "song",
        "content_source": "song",
        "output_strategy": strategy
    });
    if strategy == "restyle_existing" {
        presentation["background"] = serde_json::json!("default");
    }
    let config = parse_project_config_value(serde_json::json!({
        "version": 4,
        "backgrounds": {"default": "backgrounds/default.png"},
        "presentation_types": {"song": presentation},
        "item_rules": [{
            "id": "song",
            "match": {"category": "song"},
            "use_type": "song"
        }]
    }))
    .expect("song contract fixture config");
    let title = "O Praise The Name (Anastasis)";
    let item = Item {
        id: "qualified-song".to_string(),
        position: 0,
        title: title.to_string(),
        description: None,
        category: Category::Song,
        note: None,
        scripture: None,
        song: Some(Song {
            title: title.to_string(),
            author: None,
            copyright: None,
            ccli: None,
            themes: None,
            lyrics: None,
            arrangement: None,
        }),
    };
    let plan = classify::build_plan(&[item], &config, Some(&catalog), None)
        .pop()
        .expect("one song plan");
    assert!(plan.needs_review());
    assert!(plan.requires_song_selection());
    assert_eq!(plan.file_path(), Some(source.as_path()));
    (plan, source)
}

#[test]
fn partial_song_overrides_cannot_approve_an_unresolved_identity() {
    for strategy in ["preserve_existing", "restyle_existing"] {
        let root = tempfile::tempdir().expect("song fixture directory");
        let (plan, _) = candidate_song(root.path(), strategy);
        for action in [
            OverrideAction::SelectArrangement {
                arrangement: "Default".to_string(),
            },
            OverrideAction::SetBackground {
                background: test_background("default"),
            },
        ] {
            let decision = EntryOverride {
                output_key: plan.output_key.to_string(),
                playlist_name: None,
                slide_type: None,
                action: Some(action),
            };
            assert!(matches!(
                apply_override(&plan, Some(&decision)),
                Err(RequestError::UnsupportedOverride { .. })
            ));
        }
    }
}

#[test]
fn explicit_song_source_selection_resolves_identity_and_retains_its_transform() {
    for strategy in ["preserve_existing", "restyle_existing"] {
        let root = tempfile::tempdir().expect("song fixture directory");
        let (plan, source) = candidate_song(root.path(), strategy);
        let decision = EntryOverride {
            output_key: plan.output_key.to_string(),
            playlist_name: None,
            slide_type: None,
            action: Some(OverrideAction::UseExisting {
                file_path: source.clone(),
                arrangement: Some("Reviewed Order".to_string()),
            }),
        };
        let selected = apply_override(&plan, Some(&decision)).expect("explicit source decision");
        assert!(!selected.requires_song_selection());
        assert_eq!(selected.arrangement(), Some("Reviewed Order"));
        let canonical = source.canonicalize().expect("canonical fixture source");
        assert_eq!(selected.file_path(), Some(canonical.as_path()));
        if strategy == "restyle_existing" {
            assert!(matches!(
                selected.ready_action(),
                Some(ReadyAction::RestyleExisting { transform, .. })
                    if transform.replacement_background() == plan.background()
            ));
        } else {
            assert!(matches!(
                selected.ready_action(),
                Some(ReadyAction::UseExisting { .. })
            ));
        }
    }
}

#[test]
fn later_review_checks_cannot_erase_an_unresolved_song_identity() {
    let root = tempfile::tempdir().expect("song fixture directory");
    let (mut plan, _) = candidate_song(root.path(), "restyle_existing");
    plan.require_review("Another validation requires review".to_string());
    assert!(plan.requires_song_selection());
    assert!(!plan
        .approve_proposed_action()
        .expect("checked review transition"));
    assert!(plan.needs_review());
    assert!(plan.preview_action().is_some());
}

#[test]
fn unresolved_arrangements_require_an_arrangement_decision() {
    let source = PathBuf::from("/library/Resolved Song.pro");
    for action in [
        ReadyAction::UseExisting {
            file_path: source.clone(),
            arrangement: None,
        },
        ReadyAction::RestyleExisting {
            file_path: source.clone(),
            arrangement: None,
            transform: test_transform(),
        },
    ] {
        let mut plan = test_plan(
            "unresolved-arrangement",
            PlanDisposition::NeedsReview(ReviewContext::song_arrangement(action)),
        );
        plan.require_review("Another validation requires review".to_string());
        assert!(plan.requires_song_arrangement());
        assert!(!plan
            .approve_proposed_action()
            .expect("checked review transition"));
        let mut decision = EntryOverride {
            output_key: plan.output_key.to_string(),
            playlist_name: None,
            slide_type: None,
            action: Some(OverrideAction::SetBackground {
                background: test_background("default"),
            }),
        };
        assert!(matches!(
            apply_override(&plan, Some(&decision)),
            Err(RequestError::UnsupportedOverride { .. })
        ));
        decision.action = Some(OverrideAction::SelectArrangement {
            arrangement: "Youth".to_string(),
        });
        let selected =
            apply_override(&plan, Some(&decision)).expect("explicit arrangement decision");
        assert!(!selected.needs_review());
        assert!(!selected.requires_song_arrangement());
        assert_eq!(selected.file_path(), Some(source.as_path()));
        assert_eq!(selected.arrangement(), Some("Youth"));
        assert_eq!(selected.background(), plan.background());
    }
}
