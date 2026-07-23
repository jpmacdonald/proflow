use std::path::Path;

use crate::planning_center::types::{Category, Item, Scripture, Song};
use crate::project_config::{parse_project_config_str, ProjectConfig};
use crate::propresenter::generated::rv_data;
use crate::propresenter::library::LibraryCatalog;
use crate::workflow::plan::{
    CueMacro, ItemKind, OutputKey, PlanDisposition, ReadyAction, RenderRole, RenderStyle,
    ResolvedItemPlan, ScriptureRequest, SpeakerPalette,
};
use prost::Message;
use serde::Deserialize;
use tempfile::tempdir;

pub(super) fn scripture_request(plan: &ResolvedItemPlan) -> ScriptureRequest<'_> {
    plan.scripture_content()
        .expect("expected scripture content")
        .request()
}

pub(super) fn is_use_existing(plan: &ResolvedItemPlan) -> bool {
    matches!(plan.ready_action(), Some(ReadyAction::UseExisting { .. }))
}

pub(super) fn is_edit_description(plan: &ResolvedItemPlan) -> bool {
    matches!(
        plan.ready_action(),
        Some(ReadyAction::EditDescription { .. })
    )
}

pub(super) fn is_generated(plan: &ResolvedItemPlan) -> bool {
    matches!(
        plan.ready_action(),
        Some(
            ReadyAction::GenerateDescription { .. }
                | ReadyAction::GenerateScripture { .. }
                | ReadyAction::GenerateTitle { .. }
        )
    )
}

pub(super) fn test_plan(disposition: PlanDisposition) -> ResolvedItemPlan {
    ResolvedItemPlan::new(
        OutputKey::new("test:main".to_string()).expect("valid test output key"),
        0,
        "Test".to_string(),
        "Test".to_string(),
        "Test fixture".to_string(),
        ItemKind::Other,
        None,
        disposition,
    )
}

pub(super) fn test_render_role(id: &str, macro_binding: Option<CueMacro>) -> RenderRole {
    let speaker_palette = macro_binding
        .as_ref()
        .and_then(CueMacro::leader_enter)
        .map(|_| SpeakerPalette::new((254, 219, 79), (255, 255, 255)));
    RenderRole::new(
        id.to_string(),
        id.to_string(),
        std::collections::BTreeMap::new(),
        macro_binding,
        speaker_palette,
    )
    .expect("test render role should be valid")
}

pub(super) fn test_render_style(content: RenderRole, title: Option<RenderRole>) -> RenderStyle {
    RenderStyle::new(None, content, title, None).expect("test render style should be valid")
}

pub(super) fn write_library_presentation(path: &Path) {
    write_library_presentation_with_size(path, 1920.0, 1080.0);
}

fn presentation_cue_with_size(width: f64, height: f64) -> rv_data::Cue {
    rv_data::Cue {
        uuid: Some(rv_data::Uuid {
            string: "550e8400-e29b-41d4-a716-446655440010".to_string(),
        }),
        actions: vec![rv_data::Action {
            uuid: Some(rv_data::Uuid {
                string: "550e8400-e29b-41d4-a716-446655440011".to_string(),
            }),
            action_type_data: Some(rv_data::action::ActionTypeData::Slide(
                rv_data::action::SlideType {
                    slide: Some(rv_data::action::slide_type::Slide::Presentation(
                        rv_data::PresentationSlide {
                            base_slide: Some(rv_data::Slide {
                                size: Some(rv_data::graphics::Size { width, height }),
                                ..rv_data::Slide::default()
                            }),
                            ..rv_data::PresentationSlide::default()
                        },
                    )),
                },
            )),
            ..rv_data::Action::default()
        }],
        ..rv_data::Cue::default()
    }
}

pub(super) fn write_library_presentation_with_size(path: &Path, width: f64, height: f64) {
    let name = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .expect("fixture path has a UTF-8 stem");
    let presentation = rv_data::Presentation {
        uuid: Some(rv_data::Uuid {
            string: format!("{name}-id"),
        }),
        name: name.to_string(),
        cues: vec![presentation_cue_with_size(width, height)],
        ..rv_data::Presentation::default()
    };
    std::fs::write(path, presentation.encode_to_vec()).expect("write sized presentation fixture");
}

pub(super) fn write_song_with_arrangements(path: &Path, arrangements: &[(&str, Option<&str>)]) {
    let name = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .expect("fixture path has a UTF-8 stem");
    let cue_id = "fixture-cue";
    let group_id = "fixture-group";
    let mut cue = presentation_cue_with_size(1920.0, 1080.0);
    cue.uuid = Some(rv_data::Uuid {
        string: cue_id.to_string(),
    });
    let presentation = rv_data::Presentation {
        uuid: Some(rv_data::Uuid {
            string: "550e8400-e29b-41d4-a716-446655440000".to_string(),
        }),
        name: name.to_string(),
        arrangements: arrangements
            .iter()
            .map(|(name, uuid)| rv_data::presentation::Arrangement {
                uuid: uuid.map(|uuid| rv_data::Uuid {
                    string: uuid.to_string(),
                }),
                name: (*name).to_string(),
                group_identifiers: vec![rv_data::Uuid {
                    string: group_id.to_string(),
                }],
            })
            .collect(),
        cues: vec![cue],
        cue_groups: vec![rv_data::presentation::CueGroup {
            group: Some(rv_data::Group {
                uuid: Some(rv_data::Uuid {
                    string: group_id.to_string(),
                }),
                ..rv_data::Group::default()
            }),
            cue_identifiers: vec![rv_data::Uuid {
                string: cue_id.to_string(),
            }],
        }],
        ..Default::default()
    };
    std::fs::write(path, presentation.encode_to_vec()).expect("write song fixture");
}

pub(super) fn song_config(
    configured_arrangement: Option<&str>,
    override_arrangement: Option<&str>,
) -> ProjectConfig {
    let mut config = serde_json::json!({
        "version": 4,
        "presentation_types": {
            "song": {
                "kind": "song",
                "content_source": "song",
                "output_strategy": "preserve_existing"
            }
        },
        "item_rules": [{
            "id": "song",
            "match": { "category": "song" },
            "use_type": "song",
            "target": { "library_file": "Amazing Grace.pro" }
        }]
    });
    if let Some(arrangement) = configured_arrangement {
        config["presentation_types"]["song"]["arrangement"] =
            serde_json::Value::String(arrangement.to_string());
    }
    if let Some(arrangement) = override_arrangement {
        config["overrides"] = serde_json::json!([{
            "when": { "service_type": "Christmas Eve" },
            "arrangement": arrangement
        }]);
    }
    parse_project_config_str(&config.to_string()).expect("song config should parse")
}

pub(super) fn song_item(arrangement: Option<&str>) -> Item {
    Item {
        id: "song".to_string(),
        position: 1,
        title: "Amazing Grace".to_string(),
        description: None,
        category: Category::Song,
        note: None,
        song: Some(Song {
            title: "Amazing Grace".to_string(),
            author: None,
            copyright: None,
            ccli: None,
            themes: None,
            lyrics: None,
            arrangement: arrangement.map(str::to_string),
        }),
        scripture: None,
    }
}

pub(super) fn song_index(
    arrangements: &[(&str, Option<&str>)],
) -> (tempfile::TempDir, LibraryCatalog) {
    let directory = tempdir().expect("fixture library directory");
    write_song_with_arrangements(&directory.path().join("Amazing Grace.pro"), arrangements);
    let index = LibraryCatalog::build(directory.path()).expect("fixture library should index");
    (directory, index)
}

pub(super) fn fixture_library() -> (tempfile::TempDir, LibraryCatalog) {
    let directory = tempdir().expect("fixture library directory");
    for name in ["Amazing Grace.pro", "Call to Worship.pro"] {
        write_library_presentation(&directory.path().join(name));
    }
    let index = LibraryCatalog::build(directory.path()).expect("fixture library should index");
    (directory, index)
}

pub(super) fn load_config() -> ProjectConfig {
    parse_project_config_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/workflow/v4_config.json"
    )))
    .expect("fixture config should parse")
}

pub(super) fn load_items() -> Vec<Item> {
    let raw: Vec<FixtureItem> = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/workflow/items.json"
    )))
    .expect("fixture items should parse");
    raw.into_iter().map(FixtureItem::into_item).collect()
}

pub(super) fn scripture_config() -> ProjectConfig {
    parse_project_config_str(
        r#"
            {
              "version": 4,
              "cue_roles": { "scripture": { "slide": "Scripture" } },
              "presentation_types": {
                "scripture": {
                  "kind": "scripture",
                  "content_source": "scripture",
                  "output_strategy": "generate_new",
                  "display": { "kind": "single", "role": "scripture" }
                }
              },
              "item_rules": [{
                "id": "scripture",
                "match": { "title_prefix": ["scripture"] },
                "use_type": "scripture"
              }]
            }
            "#,
    )
    .expect("scripture config should parse")
}

pub(super) fn explicit_library_target_config() -> ProjectConfig {
    parse_project_config_str(
        r#"
            {
              "version": 4,
              "presentation_types": {
                "static_slide": {
                  "kind": "graphic",
                  "content_source": "static",
                  "output_strategy": "preserve_existing"
                },
                "song": {
                  "kind": "song",
                  "content_source": "song",
                  "output_strategy": "preserve_existing"
                }
              },
              "item_rules": [
                {
                  "id": "announcements",
                  "match": { "title_prefix": ["announcements"] },
                  "use_type": "static_slide",
                  "target": { "library_file": "Announcements.pro" }
                },
                {
                  "id": "song",
                  "match": { "title_prefix": ["song"] },
                  "use_type": "song",
                  "target": { "library_file": "Amazing Grace.pro" }
                }
              ]
            }
            "#,
    )
    .expect("explicit target config should parse")
}

pub(super) fn explicit_library_target_items() -> Vec<Item> {
    vec![
        Item {
            id: "announcements".to_string(),
            position: 1,
            title: "Announcements".to_string(),
            description: None,
            category: Category::Graphic,
            note: None,
            song: None,
            scripture: None,
        },
        Item {
            id: "song".to_string(),
            position: 2,
            title: "Song".to_string(),
            description: None,
            category: Category::Song,
            note: None,
            song: Some(Song {
                title: "Amazing Grace".to_string(),
                author: None,
                copyright: None,
                ccli: None,
                themes: None,
                lyrics: None,
                arrangement: None,
            }),
            scripture: None,
        },
    ]
}

pub(super) fn mutable_target_collision_config() -> ProjectConfig {
    parse_project_config_str(
        r#"
            {
              "version": 4,
              "cue_roles": {
                "text": {
                  "slide": "Text",
                  "enter_macro": "Scripture/Prayer",
                  "leader_enter_macro": "Scripture/Prayer (Highlighted)",
                  "speaker_colors": {
                    "leader": "\u0023FEDB4F",
                    "audience": "\u0023FFFFFF"
                  }
                }
              },
              "presentation_types": {
                "edited": {
                  "kind": "liturgy",
                  "content_source": "description",
                  "description_parser": "liturgical",
                  "output_strategy": "edit_in_place",
                  "display": { "kind": "single", "role": "text" }
                },
                "generated": {
                  "kind": "liturgy",
                  "content_source": "description",
                  "description_parser": "liturgical",
                  "output_strategy": "generate_new",
                  "display": { "kind": "single", "role": "text" }
                },
                "existing": {
                  "kind": "graphic",
                  "content_source": "static",
                  "output_strategy": "preserve_existing"
                }
              },
              "item_rules": [
                {
                  "id": "edited",
                  "match": { "title_prefix": ["edited"] },
                  "use_type": "edited",
                  "target": { "library_file": "Weekly Slot.pro" }
                },
                {
                  "id": "generated",
                  "match": { "title_prefix": ["generated"] },
                  "use_type": "generated"
                },
                {
                  "id": "existing",
                  "match": { "title_prefix": ["existing"] },
                  "use_type": "existing",
                  "target": { "library_file": "Reusable.pro" }
                }
              ]
            }
            "#,
    )
    .expect("collision config should parse")
}

pub(super) fn test_text_item(
    id: &str,
    position: usize,
    title: &str,
    description: Option<&str>,
) -> Item {
    Item {
        id: id.to_string(),
        position,
        title: title.to_string(),
        description: description.map(str::to_string),
        category: Category::Text,
        note: None,
        song: None,
        scripture: None,
    }
}

#[derive(Debug, Deserialize)]
struct FixtureItem {
    id: String,
    position: usize,
    title: String,
    description: Option<String>,
    category: FixtureCategory,
    note: Option<String>,
    song: Option<FixtureSong>,
    scripture: Option<FixtureScripture>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
enum FixtureCategory {
    Text,
    Graphic,
    Title,
    Song,
    Other,
}

#[derive(Debug, Deserialize)]
struct FixtureSong {
    title: String,
    author: Option<String>,
    copyright: Option<String>,
    ccli: Option<String>,
    themes: Option<Vec<String>>,
    lyrics: Option<String>,
    arrangement: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FixtureScripture {
    reference: String,
    text: Option<String>,
    translation: Option<String>,
}

impl FixtureItem {
    fn into_item(self) -> Item {
        Item {
            id: self.id,
            position: self.position,
            title: self.title,
            description: self.description,
            category: match self.category {
                FixtureCategory::Text => Category::Text,
                FixtureCategory::Graphic => Category::Graphic,
                FixtureCategory::Title => Category::Title,
                FixtureCategory::Song => Category::Song,
                FixtureCategory::Other => Category::Other,
            },
            note: self.note,
            song: self.song.map(|song| Song {
                title: song.title,
                author: song.author,
                copyright: song.copyright,
                ccli: song.ccli,
                themes: song.themes,
                lyrics: song.lyrics,
                arrangement: song.arrangement,
            }),
            scripture: self.scripture.map(|scripture| Scripture {
                reference: scripture.reference,
                text: scripture.text,
                translation: scripture.translation,
            }),
        }
    }
}
