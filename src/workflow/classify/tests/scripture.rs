use super::support::*;
use crate::bible::BibleVersion;
use crate::planning_center::types::{Category, Item, Scripture};
use crate::project_config::{parse_project_config_str, ProjectConfig};
use crate::propresenter::library::LibraryCatalog;
use crate::workflow::classify::build_plan;
use crate::workflow::plan::{ItemKind, ScriptureRequest};
use tempfile::tempdir;

#[test]
fn scripture_plan_rejects_a_partially_valid_reference_list() {
    let config = load_config();
    let item = Item {
        id: "partial-scripture".to_string(),
        position: 1,
        title: "Scripture - Luke 8:26-39; not a reference NRSVue".to_string(),
        description: None,
        category: Category::Text,
        note: None,
        song: None,
        scripture: None,
    };

    let plans = build_plan(&[item], &config, None, None);

    assert_eq!(plans.len(), 1);
    assert!(plans[0].needs_review());
    assert_eq!(
        plans[0].reason,
        "Invalid scripture reference 'not a reference NRSVue'"
    );
}

#[test]
fn partial_verse_description_becomes_one_typed_reconciliation_proposal() {
    let mut raw = scripture_config().into_raw();
    raw.defaults.bible_version = Some(BibleVersion::NRSVue);
    let config = ProjectConfig::try_from(raw).expect("valid scripture config");
    let item = Item {
        id: "partial-verse".to_string(),
        position: 1,
        title: "Scripture (Robert) - Exodus 16:1-4a".to_string(),
        description: Some(
            "1 The whole congregation of the Israelites set out from Elim and came to the wilderness of Sin, which is between Elim and Sinai, on the fifteenth day of the second month after they had departed from the land of Egypt.\n\
             2 The whole congregation of the Israelites complained against Moses and Aaron in the wilderness.\n\
             3 The Israelites said to them, “If only we had died by the hand of the Lord in the land of Egypt, when we sat by the pots of meat and ate our fill of bread, for you have brought us out into this wilderness to kill this whole assembly with hunger.”\n\
             4 Then the Lord said to Moses, “I am going to rain bread from heaven for you, and each day the people shall go out and gather enough for that day."
                .to_string(),
        ),
        category: Category::Title,
        note: None,
        song: None,
        scripture: None,
    };

    let plans = build_plan(&[item], &config, None, None);

    assert_eq!(plans.len(), 1);
    assert!(plans[0].needs_review());
    assert_eq!(
        plans[0].reason,
        "Validate Planning Center partial-verse text against the local Bible corpus"
    );
    assert!(matches!(
        scripture_request(&plans[0]),
        ScriptureRequest::PrefixExcerpt {
            reference: "Exodus 16:1-4",
            display_reference: "Exodus 16:1-4a",
            bible_version: "NRSVue",
            ..
        }
    ));
}

#[test]
fn partial_verse_without_description_still_requires_human_review() {
    let mut raw = scripture_config().into_raw();
    raw.defaults.bible_version = Some(BibleVersion::NRSVue);
    let config = ProjectConfig::try_from(raw).expect("valid scripture config");
    let item = Item {
        id: "partial-verse-no-text".to_string(),
        position: 1,
        title: "Scripture (Robert) - Exodus 16:1-4a".to_string(),
        description: None,
        category: Category::Title,
        note: None,
        song: None,
        scripture: None,
    };

    let plans = build_plan(&[item], &config, None, None);

    assert!(plans[0].needs_review());
    assert!(plans[0].scripture_content().is_none());
    assert_eq!(
        plans[0].reason,
        "Partial-verse reference 'Exodus 16:1-4a' cannot be generated from whole-verse Bible data"
    );
}

#[test]
fn scripture_without_a_translation_uses_only_the_configured_default() {
    let config = scripture_config();
    let item = Item {
        id: "implicit-scripture-version".to_string(),
        position: 1,
        title: "Scripture - John 3:16".to_string(),
        description: None,
        category: Category::Text,
        note: None,
        song: None,
        scripture: None,
    };

    let unresolved = build_plan(std::slice::from_ref(&item), &config, None, None);
    assert!(unresolved[0].needs_review());
    assert_eq!(
        unresolved[0].reason,
        "No Bible version was supplied and no project default is configured"
    );

    let mut raw = config.into_raw();
    raw.defaults.bible_version = Some(BibleVersion::NIV);
    let config = ProjectConfig::try_from(raw).expect("valid scripture config with default");
    let resolved = build_plan(&[item], &config, None, None);
    assert!(is_generated(&resolved[0]));
    assert!(matches!(
        scripture_request(&resolved[0]),
        ScriptureRequest::Single {
            bible_version: "NIV",
            ..
        }
    ));
}

#[test]
fn scripture_plan_preserves_mixed_explicit_versions() {
    let config = load_config();
    let item = Item {
        id: "mixed-scripture".to_string(),
        position: 1,
        title: "Scripture - Psalm 23:1-6 NIV; John 3:16 NRSVue".to_string(),
        description: None,
        category: Category::Text,
        note: None,
        song: None,
        scripture: None,
    };

    let plans = build_plan(&[item], &config, None, None);

    assert_eq!(plans.len(), 1);
    assert!(is_generated(&plans[0]));
    let ScriptureRequest::Combined(references) = scripture_request(&plans[0]) else {
        panic!("expected combined scripture content");
    };
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].version(), "NIV");
    assert_eq!(references[1].version(), "NRSVue");
}

#[test]
fn scripture_plan_prefers_supported_structured_reference_and_translation() {
    let config = scripture_config();
    let item = Item {
        id: "structured-scripture".to_string(),
        position: 1,
        title: "Scripture: Malachi 1:1 NRSVue".to_string(),
        description: None,
        category: Category::Text,
        note: None,
        song: None,
        scripture: Some(Scripture {
            reference: "John 3:16-17".to_string(),
            text: None,
            translation: Some("niv".to_string()),
        }),
    };

    let plans = build_plan(&[item], &config, None, None);

    assert!(is_generated(&plans[0]));
    assert!(matches!(
        scripture_request(&plans[0]),
        ScriptureRequest::Single {
            reference: "John 3:16-17",
            bible_version: "NIV"
        }
    ));
}

#[test]
fn scripture_plan_preserves_discontinuous_same_chapter_ranges() {
    let config = scripture_config();
    let item = Item {
        id: "discontinuous-scripture".to_string(),
        position: 1,
        title: "Scripture: Joshua 3:1-5, 9-17 (Adrian)".to_string(),
        description: None,
        category: Category::Text,
        note: None,
        song: None,
        scripture: Some(Scripture {
            reference: "Joshua 3:1-5, 9-17".to_string(),
            text: None,
            translation: Some("NRSVue".to_string()),
        }),
    };

    let plans = build_plan(&[item], &config, None, None);

    assert!(is_generated(&plans[0]));
    assert!(matches!(
        scripture_request(&plans[0]),
        ScriptureRequest::Single {
            reference: "Joshua 3:1-5, 9-17",
            bible_version: "NRSVue"
        }
    ));
}

#[test]
fn scripture_plan_rejects_unsupported_structured_translation() {
    let config = scripture_config();
    let item = Item {
        id: "unsupported-translation".to_string(),
        position: 1,
        title: "Scripture: John 3:16 NRSVue".to_string(),
        description: None,
        category: Category::Text,
        note: None,
        song: None,
        scripture: Some(Scripture {
            reference: "John 3:16".to_string(),
            text: None,
            translation: Some("ESV".to_string()),
        }),
    };

    let plans = build_plan(&[item], &config, None, None);

    assert!(plans[0].needs_review());
    assert_eq!(plans[0].reason, "Unsupported Bible version 'ESV'");
}

#[test]
fn scripture_plan_falls_back_to_title_without_structured_translation() {
    let config = scripture_config();
    let item = Item {
        id: "title-fallback".to_string(),
        position: 1,
        title: "Scripture: Luke 2:1-3 NRSV".to_string(),
        description: None,
        category: Category::Text,
        note: None,
        song: None,
        scripture: Some(Scripture {
            reference: "John 3:16".to_string(),
            text: None,
            translation: None,
        }),
    };

    let plans = build_plan(&[item], &config, None, None);

    assert!(is_generated(&plans[0]));
    assert!(matches!(
        scripture_request(&plans[0]),
        ScriptureRequest::Single {
            reference: "Luke 2:1-3",
            bible_version: "NRSV"
        }
    ));
}

#[test]
fn static_scripture_type_reuses_an_existing_presentation() {
    let directory = tempdir().expect("fixture library directory");
    let path = directory.path().join("Jonah 4.pro");
    write_library_presentation(&path);
    let index = LibraryCatalog::build(directory.path()).expect("fixture library should index");
    let config = parse_project_config_str(
        r#"
            {
              "version": 4,
              "presentation_types": {
                "scripture_existing": {
                  "kind": "scripture",
                  "content_source": "static",
                  "output_strategy": "preserve_existing"
                }
              },
              "item_rules": [{
                "id": "jonah_4",
                "match": { "title_contains": ["jonah 4"] },
                "use_type": "scripture_existing",
                "target": { "library_file": "Jonah 4.pro" }
              }]
            }
            "#,
    )
    .expect("existing scripture config should parse");
    let item = Item {
        id: "jonah".to_string(),
        position: 1,
        title: "Scripture: Jonah 4".to_string(),
        description: None,
        category: Category::Title,
        note: None,
        song: None,
        scripture: None,
    };

    let plans = build_plan(&[item], &config, Some(&index), None);

    assert!(is_use_existing(&plans[0]));
    assert_eq!(plans[0].item_kind(), ItemKind::Scripture);
    assert_eq!(plans[0].playlist_name, "Jonah 4");
    assert_eq!(
        plans[0].file_path(),
        Some(
            path.canonicalize()
                .expect("canonical fixture path")
                .as_path()
        )
    );
}
