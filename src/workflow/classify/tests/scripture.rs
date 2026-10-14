use super::support::*;
use crate::bible::BibleVersion;
use crate::planning_center::types::{Category, Item, Scripture};
use crate::project_config::{parse_project_config_str, PersonConfig, ProjectConfig};
use crate::workflow::classify::build_plan;
use crate::workflow::plan::{ItemKind, ScriptureRequest};
use propresenter::library::LibraryCatalog;
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
    raw.people
        .insert("Robert".to_string(), PersonConfig::default());
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
            bible_version: BibleVersion::NRSVue,
            ..
        }
    ));
}

#[test]
fn partial_verse_without_description_still_requires_human_review() {
    let mut raw = scripture_config().into_raw();
    raw.defaults.bible_version = Some(BibleVersion::NRSVue);
    raw.people
        .insert("Robert".to_string(), PersonConfig::default());
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
fn selected_verses_use_the_planning_center_description_as_a_typed_proposal() {
    let mut raw = scripture_config().into_raw();
    raw.defaults.bible_version = Some(BibleVersion::NRSVue);
    raw.people
        .insert("Robert".to_string(), PersonConfig::default());
    let config = ProjectConfig::try_from(raw).expect("valid scripture config");
    let item = Item {
        id: "selected-verses".to_string(),
        position: 1,
        title: "Scripture: 1 Samuel 17 (selected vv)".to_string(),
        description: Some(
            "1 Now the Philistines gathered their armies for battle… \
             2 Saul and the Israelites gathered and encamped in the valley of Elah."
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
        "Validate Planning Center selected-verse text against the local Bible corpus"
    );
    assert!(matches!(
        scripture_request(&plans[0]),
        ScriptureRequest::SelectedExcerpt {
            reference: "1 Samuel 17",
            bible_version: BibleVersion::NRSVue,
            excerpt_text,
        } if excerpt_text.starts_with("1 Now the Philistines")
    ));
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
            bible_version: BibleVersion::NIV,
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
    assert_eq!(references[0].version(), BibleVersion::NIV);
    assert_eq!(references[1].version(), BibleVersion::NRSVue);
}

#[test]
fn scripture_plan_prefers_supported_structured_reference_and_translation() {
    let config = scripture_config();
    let item = Item {
        id: "structured-scripture".to_string(),
        position: 1,
        title: "Scripture: Malachi 1:1 NIV".to_string(),
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
            bible_version: BibleVersion::NIV
        }
    ));
}

#[test]
fn scripture_plan_preserves_discontinuous_same_chapter_ranges() {
    let mut raw = scripture_config().into_raw();
    raw.people
        .insert("Adrian".to_string(), PersonConfig::default());
    let config = ProjectConfig::try_from(raw).expect("valid scripture config with known speakers");
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
            bible_version: BibleVersion::NRSVue
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
            bible_version: BibleVersion::NRSV
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

fn translation_item(title: &str) -> Item {
    Item {
        id: "translation-contract".to_string(),
        position: 1,
        title: title.to_string(),
        description: Some("1 Authoritative excerpt text".to_string()),
        category: Category::Text,
        note: None,
        song: None,
        scripture: None,
    }
}

#[test]
fn unsupported_title_translation_never_becomes_a_defaulted_or_excerpt_proposal() {
    let mut raw = scripture_config().into_raw();
    raw.defaults.bible_version = Some(BibleVersion::NRSVue);
    raw.people
        .insert("Robert".to_string(), PersonConfig::default());
    let config = ProjectConfig::try_from(raw).expect("translation contract config");
    for title in [
        "Scripture - John 3:16 ESV",
        "Scripture - John 3:16 (ESV)",
        "Scripture - John 3:16 (Unknown Edition)",
        "Scripture - John 3:16 ESV (Robert)",
        "Scripture - John 3:16 (ESV) (Robert)",
        "Scripture - Exodus 16:1-4a ESV",
        "Scripture - Exodus 16:1-4a (ESV) (Robert)",
        "Scripture - 1 Samuel 17 (selected vv) ESV",
        "Scripture - 1 Samuel 17 (ESV) (selected verses)",
    ] {
        let item = translation_item(title);
        for structured in [
            None,
            Some(Scripture {
                reference: "Exodus 16:1-4a".to_string(),
                text: Some("1 Authoritative excerpt text".to_string()),
                translation: Some("NRSVue".to_string()),
            }),
        ] {
            let mut item = item.clone();
            item.scripture = structured;
            let plans = build_plan(&[item], &config, None, None);
            assert!(plans[0].needs_review(), "{title}");
            assert!(plans[0].preview_action().is_none(), "{title}");
            assert!(
                plans[0].reason.contains("Unsupported Bible version"),
                "{title}: {}",
                plans[0].reason
            );
        }
    }
}

#[test]
fn conflicting_declarations_cannot_cross_the_whole_or_excerpt_boundary() {
    let mut raw = scripture_config().into_raw();
    raw.defaults.bible_version = Some(BibleVersion::KJV);
    let config = ProjectConfig::try_from(raw).expect("translation contract config");
    for reference in ["John 3:16", "Exodus 16:1-4a", "1 Samuel 17 (selected vv)"] {
        for title in [
            format!("Scripture - {reference} NIV (NRSVue)"),
            format!("Scripture - {reference} NIV"),
        ] {
            let mut item = translation_item(&title);
            item.scripture = Some(Scripture {
                reference: reference.to_string(),
                text: item.description.clone(),
                translation: Some("NRSVue".to_string()),
            });
            let plans = build_plan(&[item], &config, None, None);
            assert!(plans[0].needs_review(), "{title}");
            assert!(plans[0].preview_action().is_none(), "{title}");
            assert!(
                plans[0]
                    .reason
                    .contains("Conflicting explicit Bible versions"),
                "{title}: {}",
                plans[0].reason
            );
        }
    }
    let item = translation_item("Scripture - John 3:16 NIV NRSVue");
    let plans = build_plan(&[item], &config, None, None);
    assert!(plans[0].needs_review());
    assert!(plans[0].preview_action().is_none());
}

#[test]
fn explicit_translation_survives_known_speaker_and_selected_annotations() {
    let mut raw = scripture_config().into_raw();
    raw.defaults.bible_version = Some(BibleVersion::NRSVue);
    raw.people
        .insert("Robert".to_string(), PersonConfig::default());
    let config = ProjectConfig::try_from(raw).expect("translation contract config");
    for title in [
        "Scripture - John 3:16 NIV (Robert)",
        "Scripture - John 3:16 (NIV) (Robert)",
        "Scripture (Robert) - John 3:16 NIV",
        "Scripture (Robert) - John 3:16 (NIV)",
        "Scripture (Robert) - 1 Samuel 17 NIV (selected vv)",
        "Scripture - 1 Samuel 17 (selected vv) NIV",
        "Scripture - 1 Samuel 17 NIV (selected verses)",
    ] {
        let plans = build_plan(&[translation_item(title)], &config, None, None);
        assert!(
            matches!(
                scripture_request(&plans[0]),
                ScriptureRequest::Single {
                    bible_version: BibleVersion::NIV,
                    ..
                } | ScriptureRequest::SelectedExcerpt {
                    bible_version: BibleVersion::NIV,
                    ..
                }
            ),
            "{title}"
        );
    }
}

#[test]
fn unknown_speaker_annotation_requires_resolution() {
    let mut raw = scripture_config().into_raw();
    raw.defaults.bible_version = Some(BibleVersion::NRSVue);
    let config = ProjectConfig::try_from(raw).expect("translation contract config");
    for title in [
        "Scripture - John 3:16 (Someone)",
        "Scripture (ESV) - John 3:16",
        "Scripture - John 3:16ESV",
        "Scripture - John 3:16NIV",
        "Scripture - John 3:16 [ESV]",
    ] {
        let plans = build_plan(&[translation_item(title)], &config, None, None);
        assert!(plans[0].needs_review(), "{title}");
        assert!(plans[0].preview_action().is_none(), "{title}");
    }
}

#[test]
fn source_inferred_reference_does_not_erase_title_passages_or_translation() {
    let config = scripture_config();
    let mut item = translation_item("Scripture - Psalm 23:1; John 3:16 NIV");
    item.scripture = Some(Scripture {
        reference: "Psalm 23:1".to_string(),
        text: None,
        translation: None,
    });
    let plans = build_plan(&[item], &config, None, None);
    let ScriptureRequest::Combined(references) = scripture_request(&plans[0]) else {
        panic!("expected both title passages");
    };
    assert_eq!(references.len(), 2);
    assert!(references
        .iter()
        .all(|reference| reference.version() == BibleVersion::NIV));
}

#[test]
fn shorthand_selection_preserves_explicit_translation_and_rejects_conflict() {
    let mut raw = scripture_config().into_raw();
    raw.defaults.bible_version = Some(BibleVersion::NRSVue);
    let config = ProjectConfig::try_from(raw).expect("translation contract config");
    for title in [
        "Scripture - John 3:16 NIV; 17",
        "Scripture - John 3:16 (NIV); 17",
        "Scripture - John 3:16; 17 NIV",
    ] {
        let plans = build_plan(&[translation_item(title)], &config, None, None);
        assert!(
            matches!(
                scripture_request(&plans[0]),
                ScriptureRequest::Single {
                    reference: "John 3:16, 17",
                    bible_version: BibleVersion::NIV,
                }
            ),
            "{title}"
        );
    }
    let plans = build_plan(
        &[translation_item("Scripture - John 3:16 NIV; 17 NRSVue")],
        &config,
        None,
        None,
    );
    assert!(plans[0].needs_review());
    assert!(plans[0].preview_action().is_none());
}

#[test]
fn a_corpus_installed_beside_the_config_makes_its_translation_usable() {
    let root = tempfile::tempdir().expect("temporary data root");
    let config_file = root.path().join("proflow.config.json");
    std::fs::write(
        &config_file,
        serde_json::to_vec(&scripture_config().into_raw()).expect("serialize config"),
    )
    .expect("write config");
    let item = || Item {
        id: "esv".to_string(),
        position: 1,
        title: "Scripture: John 3:16 ESV".to_string(),
        description: None,
        category: Category::Text,
        note: None,
        song: None,
        scripture: None,
    };

    let without = crate::project_config::load_project_config(&config_file).expect("load");
    let plans = build_plan(&[item()], &without, None, None);
    assert_eq!(plans[0].reason, "Unsupported Bible version 'ESV'");

    std::fs::create_dir(root.path().join("bibles")).expect("bibles directory");
    std::fs::write(root.path().join("bibles/ESV.json"), b"{}").expect("install corpus");
    let with = crate::project_config::load_project_config(&config_file).expect("load");
    let plans = build_plan(&[item()], &with, None, None);

    assert!(
        !plans[0].reason.contains("Unsupported Bible version"),
        "{}",
        plans[0].reason
    );
    let esv = BibleVersion::new("ESV").expect("code");
    assert!(matches!(
        plans[0].scripture_content().map(|content| content.request()),
        Some(ScriptureRequest::Single { bible_version, .. }) if bible_version == esv
    ));
}
