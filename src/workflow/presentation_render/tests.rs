#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use prost::Message;

use super::*;
use crate::paths::{BuildLocationInputs, BuildLocations};
use crate::project_config::{CueRoleConfig, ProjectConfig, RawProjectConfig};
use crate::propresenter::generated::rv_data;
use crate::propresenter::macros::{cue_has_macro_named, macro_action_name};
use crate::propresenter::rtf::rtf_to_text;
use crate::workflow::plan::{CueMacro, SpeakerPalette};

fn fixture_slide() -> rv_data::PresentationSlide {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/propresenter/native/templates/scripture-template.pro");
    let bytes = std::fs::read(path).expect("read presentation fixture");
    let presentation =
        rv_data::Presentation::decode(bytes.as_slice()).expect("decode presentation fixture");
    presentation
        .cues
        .iter()
        .flat_map(|cue| &cue.actions)
        .find_map(|action| match &action.action_type_data {
            Some(rv_data::action::ActionTypeData::Slide(slide)) => match &slide.slide {
                Some(rv_data::action::slide_type::Slide::Presentation(slide)) => {
                    Some(slide.clone())
                }
                _ => None,
            },
            _ => None,
        })
        .expect("fixture presentation slide")
}

fn two_field_slide() -> rv_data::PresentationSlide {
    let mut slide = fixture_slide();
    let base = slide.base_slide.as_mut().expect("base slide");
    let mut unrelated = base.elements[0].clone();
    unrelated.element.as_mut().expect("unrelated element").uuid = Some(rv_data::Uuid {
        string: "5DFD516B-CDF8-4FC2-A26F-FBB43E2330C5".to_string(),
    });
    base.elements[0]
        .element
        .as_mut()
        .expect("body element")
        .name = "Body".to_string();
    unrelated.element.as_mut().expect("unrelated element").name = "Footer".to_string();
    base.elements.push(unrelated);
    slide
}

fn set_element_metrics(element: &mut rv_data::slide::Element, name: &str, width: f64, height: f64) {
    let graphics = element.element.as_mut().expect("graphics element");
    graphics.name = name.to_string();
    graphics.bounds = Some(rv_data::graphics::Rect {
        size: Some(rv_data::graphics::Size { width, height }),
        ..rv_data::graphics::Rect::default()
    });
    let text = graphics.text.as_mut().expect("text element");
    text.margins = Some(rv_data::graphics::EdgeInsets::default());
    text.attributes = Some(rv_data::graphics::text::Attributes {
        font: Some(rv_data::Font {
            size: 20.0,
            ..rv_data::Font::default()
        }),
        paragraph_style: Some(rv_data::graphics::text::attributes::Paragraph {
            line_height_multiple: 1.0,
            ..rv_data::graphics::text::attributes::Paragraph::default()
        }),
        ..rv_data::graphics::text::Attributes::default()
    });
}

fn differently_sized_fields() -> rv_data::PresentationSlide {
    let mut slide = fixture_slide();
    let base = slide.base_slide.as_mut().expect("base slide");
    let mut body = base.elements[0].clone();
    body.element.as_mut().expect("body element").uuid = Some(rv_data::Uuid {
        string: "4D33AA7C-725E-4938-A660-8D67B38C8CB8".to_string(),
    });
    set_element_metrics(&mut base.elements[0], "Footer", 2_000.0, 400.0);
    set_element_metrics(&mut body, "Body", 200.0, 20.0);
    base.elements.push(body);
    slide
}

fn theme_cache(slides: Vec<(&str, rv_data::PresentationSlide)>) -> (tempfile::TempDir, ThemeCache) {
    let root = tempfile::tempdir().expect("temporary theme root");
    let directory = root.path().join("Themes/Test");
    std::fs::create_dir_all(&directory).expect("create theme directory");
    let document = rv_data::template::Document {
        application_info: None,
        slides: slides
            .into_iter()
            .map(|(name, slide)| rv_data::template::Slide {
                base_slide: slide.base_slide,
                name: name.to_string(),
                actions: Vec::new(),
            })
            .collect(),
    };
    std::fs::write(directory.join("Theme"), document.encode_to_vec()).expect("write theme");
    let cache =
        ThemeCache::load_from_dir(Some("Test"), &root.path().join("Themes")).expect("load theme");
    (root, cache)
}

const SCREEN_UUID: &str = "11111111-1111-4111-8111-111111111111";
const LOOK_UUID: &str = "22222222-2222-4222-8222-222222222222";
const AUDIENCE_SLIDE_UUID: &str = "33333333-3333-4333-8333-333333333333";

fn render_asset_locations(root: &Path) -> BuildLocations {
    let data = root.join("data");
    let library = root.join("library");
    let propresenter = root.join("ProPresenter");
    std::fs::create_dir_all(data.join("bibles")).expect("create bible root");
    std::fs::create_dir_all(&library).expect("create library");
    std::fs::create_dir_all(&propresenter).expect("create ProPresenter root");
    BuildLocations::from_inputs(BuildLocationInputs {
        project_data_root: data,
        presentation_library: library.clone(),
        playlist_output: library,
        propresenter_root: propresenter.clone(),
        themes: propresenter.join("Themes"),
        macros: propresenter.join("Configuration/Macros"),
    })
    .expect("checked locations")
}

fn text_slide(name: &str, width: f64, height: f64) -> rv_data::PresentationSlide {
    let mut slide = fixture_slide();
    let base = slide.base_slide.as_mut().expect("text slide base");
    set_element_metrics(&mut base.elements[0], name, width, height);
    slide
}

fn write_source_theme(
    locations: &BuildLocations,
    additional_slides: Vec<(&str, rv_data::PresentationSlide)>,
) {
    let slides = std::iter::once(("Content", text_slide("Body", 1_400.0, 700.0)))
        .chain(additional_slides)
        .map(|(name, slide)| rv_data::template::Slide {
            base_slide: slide.base_slide,
            name: name.to_string(),
            actions: Vec::new(),
        })
        .collect();
    let document = rv_data::template::Document {
        slides,
        ..rv_data::template::Document::default()
    };
    let path = locations.themes().join("Source Theme/Theme");
    std::fs::create_dir_all(path.parent().expect("source theme parent"))
        .expect("create source theme");
    std::fs::write(path, document.encode_to_vec()).expect("write source theme");
}

fn write_audience_theme(locations: &BuildLocations, width: f64, height: f64) -> PathBuf {
    let mut slide = text_slide("Audience Body", width, height);
    slide.base_slide.as_mut().expect("audience base").uuid = Some(rv_data::Uuid {
        string: AUDIENCE_SLIDE_UUID.to_string(),
    });
    let document = rv_data::template::Document {
        slides: vec![rv_data::template::Slide {
            base_slide: slide.base_slide,
            name: "Stream Content".to_string(),
            actions: Vec::new(),
        }],
        ..rv_data::template::Document::default()
    };
    let path = locations.themes().join("Audience Theme/Theme");
    std::fs::create_dir_all(path.parent().expect("audience theme parent"))
        .expect("create audience theme");
    std::fs::write(&path, document.encode_to_vec()).expect("write audience theme");
    path
}

fn audience_look_macro(name: &str, index: usize) -> rv_data::macros_document::Macro {
    use rv_data::action::{self, ActionTypeData};

    rv_data::macros_document::Macro {
        uuid: Some(rv_data::Uuid {
            string: format!("render-fixture-macro-{index}"),
        }),
        name: name.to_string(),
        actions: vec![rv_data::Action {
            is_enabled: true,
            r#type: action::ActionType::AudienceLook as i32,
            action_type_data: Some(ActionTypeData::AudienceLook(action::AudienceLookType {
                identification: Some(rv_data::CollectionElementType {
                    parameter_uuid: Some(rv_data::Uuid {
                        string: LOOK_UUID.to_string(),
                    }),
                    parameter_name: "Lyrics".to_string(),
                    parent_collection: None,
                }),
            })),
            ..rv_data::Action::default()
        }],
        ..rv_data::macros_document::Macro::default()
    }
}

fn write_macros(locations: &BuildLocations, macro_names: &[&str]) {
    let document = rv_data::MacrosDocument {
        macros: macro_names
            .iter()
            .enumerate()
            .map(|(index, name)| audience_look_macro(name, index))
            .collect(),
        ..rv_data::MacrosDocument::default()
    };
    std::fs::create_dir_all(locations.macros().parent().expect("macro parent"))
        .expect("create macro directory");
    std::fs::write(locations.macros(), document.encode_to_vec()).expect("write macros");
}

fn write_workspace(locations: &BuildLocations, audience_theme_path: &Path) {
    use rv_data::pro_audience_look::ProScreenLook;
    use rv_data::pro_presenter_screen;

    let file_url = rv_data::Url {
        storage: Some(rv_data::url::Storage::AbsoluteString(format!(
            "file://{}",
            audience_theme_path.display()
        ))),
        ..rv_data::Url::default()
    };
    let workspace = rv_data::ProPresenterWorkspace {
        pro_screens: vec![rv_data::ProPresenterScreen {
            uuid: Some(rv_data::Uuid {
                string: SCREEN_UUID.to_string(),
            }),
            name: "Stream".to_string(),
            screen_type: pro_presenter_screen::ScreenType::Audience as i32,
            ..rv_data::ProPresenterScreen::default()
        }],
        audience_looks: vec![rv_data::ProAudienceLook {
            uuid: Some(rv_data::Uuid {
                string: LOOK_UUID.to_string(),
            }),
            name: "Lyrics".to_string(),
            screen_looks: vec![ProScreenLook {
                pro_screen_uuid: Some(rv_data::Uuid {
                    string: SCREEN_UUID.to_string(),
                }),
                presentation_foreground_enabled: true,
                template_document_file_path: Some(file_url),
                template_slide_uuid: Some(rv_data::Uuid {
                    string: AUDIENCE_SLIDE_UUID.to_string(),
                }),
                ..ProScreenLook::default()
            }],
            ..rv_data::ProAudienceLook::default()
        }],
        ..rv_data::ProPresenterWorkspace::default()
    };
    std::fs::write(locations.workspace(), workspace.encode_to_vec()).expect("write workspace");
}

fn render_asset_config(macro_names: &[&str]) -> ProjectConfig {
    let mut raw = RawProjectConfig::default();
    raw.defaults.theme = Some("Source Theme".to_string());
    for (index, macro_name) in macro_names.iter().enumerate() {
        raw.cue_roles.insert(
            format!("fixture-role-{index}"),
            CueRoleConfig {
                slide: "Content".to_string(),
                text_slots: BTreeMap::from([("body".to_string(), "Body".to_string())]),
                enter_macro: Some((*macro_name).to_string()),
                leader_enter_macro: None,
                speaker_colors: None,
            },
        );
    }
    ProjectConfig::try_from(raw).expect("valid render fixture config")
}

fn render_assets(
    additional_slides: Vec<(&str, rv_data::PresentationSlide)>,
    macro_names: &[&str],
    audience_size: (f64, f64),
) -> (tempfile::TempDir, RenderAssetSnapshot) {
    let root = tempfile::tempdir().expect("temporary render assets");
    let locations = render_asset_locations(root.path());
    write_source_theme(&locations, additional_slides);
    let audience_theme_path = write_audience_theme(&locations, audience_size.0, audience_size.1);
    write_macros(&locations, macro_names);
    write_workspace(&locations, &audience_theme_path);
    let assets = RenderAssetSnapshot::load(render_asset_config(macro_names), locations)
        .expect("load render assets");
    (root, assets)
}

fn render_assets_with_narrow_stream() -> (tempfile::TempDir, RenderAssetSnapshot) {
    render_assets(Vec::new(), &["Song"], (220.0, 48.0))
}

fn render_with_configured_macros(
    name: &str,
    source: PresentationSource<'_>,
    style: &RenderStyle,
    additional_slides: Vec<(&str, rv_data::PresentationSlide)>,
    macro_names: &[&str],
) -> RenderedPresentation {
    let (_root, assets) = render_assets(additional_slides, macro_names, (1_400.0, 700.0));
    render_source_with_fit(
        name,
        source,
        style,
        assets.themes(),
        Some(&assets),
        &mut DiagnosticRenderTextFit,
    )
    .expect("render with macros before retaining final text-fit proof")
}

fn role(
    id: &str,
    slide: &str,
    text_slots: BTreeMap<String, String>,
    cue_macro: Option<CueMacro>,
) -> RenderRole {
    RenderRole::new(
        id.to_string(),
        slide.to_string(),
        text_slots,
        cue_macro,
        None,
    )
    .expect("valid render role")
}

fn liturgy_role(id: &str, slide: &str, cue_macro: CueMacro) -> RenderRole {
    RenderRole::new(
        id.to_string(),
        slide.to_string(),
        BTreeMap::new(),
        Some(cue_macro),
        Some(SpeakerPalette::new((254, 219, 79), (255, 255, 255))),
    )
    .expect("valid liturgy role")
}

fn parsed_segment(text: &str, speaker: SpeakerRole) -> ParsedSegment {
    ParsedSegment {
        text: text.to_string(),
        speaker,
        bold: None,
        italic: None,
    }
}

fn description(text: &str, title: Option<&str>) -> ParsedContent {
    ParsedContent::new(
        vec![ParsedSegment {
            text: text.to_string(),
            speaker: SpeakerRole::Neutral,
            bold: None,
            italic: None,
        }],
        title.map(str::to_string),
    )
}

fn body_rtf(slide: &rv_data::PresentationSlide, name: &str) -> Vec<u8> {
    slide
        .base_slide
        .as_ref()
        .expect("base slide")
        .elements
        .iter()
        .find_map(|element| {
            let graphics = element.element.as_ref()?;
            (graphics.name == name)
                .then(|| graphics.text.as_ref().map(|text| text.rtf_data.clone()))
                .flatten()
        })
        .expect("named text element")
}

fn rendered_slide(rendered: &RenderedPresentation, index: usize) -> &rv_data::PresentationSlide {
    rendered
        .presentation()
        .cues
        .get(index)
        .and_then(|cue| cue.actions.first())
        .and_then(|action| action.action_type_data.as_ref())
        .and_then(|action| match action {
            rv_data::action::ActionTypeData::Slide(slide) => slide.slide.as_ref(),
            _ => None,
        })
        .and_then(|slide| match slide {
            rv_data::action::slide_type::Slide::Presentation(slide) => Some(slide),
            rv_data::action::slide_type::Slide::Prop(_) => None,
        })
        .expect("rendered presentation slide")
}

fn first_rendered_slide(rendered: &RenderedPresentation) -> &rv_data::PresentationSlide {
    rendered_slide(rendered, 0)
}

fn slide_labels(rendered: &RenderedPresentation) -> Vec<Option<&str>> {
    rendered
        .presentation()
        .cues
        .iter()
        .map(|cue| {
            cue.actions
                .iter()
                .find_map(|action| action.label.as_ref().map(|label| label.text.as_str()))
        })
        .collect()
}

fn assert_resolved_macro_regions(
    rendered: &RenderedPresentation,
    style: &RenderStyle,
    expected: &[(usize, &str)],
) {
    let resolved = resolved_macro_regions(rendered, style)
        .expect("lower rendered role transitions into the final contract");
    let actual = resolved
        .iter()
        .map(|region| {
            let ExpectedMacroSelector::OperatorCue { index } = &region.selector else {
                panic!("generated render must use operator-cue selectors");
            };
            (*index, region.macro_name.as_str())
        })
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
}

fn assert_alternating_liturgy_macro_contract(rendered: &RenderedPresentation, style: &RenderStyle) {
    assert_resolved_macro_regions(
        rendered,
        style,
        &[
            (0, "Scripture/Prayer"),
            (1, "Scripture/Prayer (Highlighted)"),
            (2, "Scripture/Prayer"),
            (3, "Scripture/Prayer (Highlighted)"),
            (4, "Scripture/Prayer"),
        ],
    );
}

fn rendered_text(rendered: &RenderedPresentation, index: usize) -> String {
    let slide = rendered_slide(rendered, index);
    slide
        .base_slide
        .as_ref()
        .expect("base slide")
        .elements
        .iter()
        .find_map(|element| {
            let text = element.element.as_ref()?.text.as_ref()?;
            rtf_to_text(&String::from_utf8_lossy(&text.rtf_data))
        })
        .expect("rendered text")
}

fn first_presentation_slide_mut(
    presentation: &mut rv_data::Presentation,
) -> &mut rv_data::PresentationSlide {
    presentation.cues[0]
        .actions
        .iter_mut()
        .find_map(|action| {
            let rv_data::action::ActionTypeData::Slide(slide) = action.action_type_data.as_mut()?
            else {
                return None;
            };
            let rv_data::action::slide_type::Slide::Presentation(slide) = slide.slide.as_mut()?
            else {
                return None;
            };
            Some(slide)
        })
        .expect("presentation slide")
}

#[test]
fn explicit_body_slot_changes_only_the_named_native_field() {
    let template = two_field_slide();
    let original_footer = body_rtf(&template, "Footer");
    let (_root, themes) = theme_cache(vec![("Content", template)]);
    let style = RenderStyle::new(
        None,
        role(
            "body-role",
            "Content",
            BTreeMap::from([("body".to_string(), "Body".to_string())]),
            None,
        ),
        None,
        None,
    )
    .expect("valid style");

    let content = description("Replacement body", None);
    let rendered = render_source(
        "Named fields",
        PresentationSource::Description(&content),
        &style,
        &themes,
    )
    .expect("render named field");
    let slide = first_rendered_slide(&rendered);

    assert_eq!(
        rtf_to_text(&String::from_utf8_lossy(&body_rtf(slide, "Body"))).as_deref(),
        Some("Replacement body")
    );
    assert_eq!(body_rtf(slide, "Footer"), original_footer);
}

#[test]
fn final_fit_proves_each_field_of_a_multi_field_cue_independently() {
    use crate::propresenter::presentation_spec::{
        CueSpec, GroupSpec, PresentationSpec, TextBindings, TextField,
    };
    use crate::propresenter::render::{render_presentation, RenderAssets};
    use crate::propresenter::rtf::StyledSegment;

    let (_root, themes) = theme_cache(vec![("Content", two_field_slide())]);
    let style = RenderStyle::new(
        None,
        role(
            "content",
            "Content",
            BTreeMap::from([
                ("body".to_string(), "Body".to_string()),
                ("footer".to_string(), "Footer".to_string()),
            ]),
            None,
        ),
        None,
        Some(7),
    )
    .expect("valid style");
    let role_id =
        crate::propresenter::presentation_spec::CueRoleId::new("content").expect("valid role");
    let resolved = resolve_role(style.content(), role_id.clone(), &themes).expect("resolve role");
    let assets = RenderAssets::new(resolved, Vec::new()).expect("render assets");
    let bindings = TextBindings::new(
        (
            TextField::body(),
            vec![StyledSegment::unstyled("Main words")],
        ),
        [(
            TextField::new("footer").expect("footer field"),
            vec![StyledSegment::unstyled("Footer words")],
        )],
    )
    .expect("multi-field bindings");
    let spec = PresentationSpec::new(
        "Multi field",
        GroupSpec::anonymous(CueSpec::text(role_id, bindings), Vec::new()),
        Vec::new(),
    )
    .expect("presentation spec");
    let mut rendered = render_presentation(&spec, &assets).expect("render presentation");

    super::text_fit::retain_final_text_fit(
        &spec,
        &assets,
        &mut rendered,
        &style,
        None,
        7,
        &mut DiagnosticRenderTextFit,
    )
    .expect("prove every field");

    assert_eq!(rendered.text_fit_summary().len(), 1);
    assert_eq!(rendered.text_fit_summary()[0].destination_count(), 2);
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "the regression assembles one complete two-field source and audience fixture"
)]
fn final_fit_proves_each_multi_field_audience_destination_independently() {
    use crate::propresenter::presentation_spec::{
        CueSpec, GroupSpec, PresentationSpec, TextBindings, TextField,
    };
    use crate::propresenter::render::{render_presentation, RenderAssets};
    use crate::propresenter::rtf::StyledSegment;

    let root = tempfile::tempdir().expect("temporary render assets");
    let locations = render_asset_locations(root.path());
    let source_slide = two_field_slide();
    let source_document = rv_data::template::Document {
        slides: vec![rv_data::template::Slide {
            base_slide: source_slide.base_slide,
            name: "Content".to_string(),
            actions: Vec::new(),
        }],
        ..Default::default()
    };
    let source_path = locations.themes().join("Source Theme/Theme");
    std::fs::create_dir_all(source_path.parent().expect("source parent"))
        .expect("create source theme");
    std::fs::write(source_path, source_document.encode_to_vec()).expect("write source theme");

    let mut audience_slide = two_field_slide();
    audience_slide
        .base_slide
        .as_mut()
        .expect("audience base")
        .uuid = Some(rv_data::Uuid {
        string: AUDIENCE_SLIDE_UUID.to_string(),
    });
    let audience_document = rv_data::template::Document {
        slides: vec![rv_data::template::Slide {
            base_slide: audience_slide.base_slide,
            name: "Stream Content".to_string(),
            actions: Vec::new(),
        }],
        ..Default::default()
    };
    let audience_path = locations.themes().join("Audience Theme/Theme");
    std::fs::create_dir_all(audience_path.parent().expect("audience parent"))
        .expect("create audience theme");
    std::fs::write(&audience_path, audience_document.encode_to_vec())
        .expect("write audience theme");
    write_macros(&locations, &["Song"]);
    write_workspace(&locations, &audience_path);

    let slots = BTreeMap::from([
        ("body".to_string(), "Body".to_string()),
        ("footer".to_string(), "Footer".to_string()),
    ]);
    let mut raw = RawProjectConfig::default();
    raw.defaults.theme = Some("Source Theme".to_string());
    raw.cue_roles.insert(
        "content".to_string(),
        CueRoleConfig {
            slide: "Content".to_string(),
            text_slots: slots.clone(),
            enter_macro: Some("Song".to_string()),
            leader_enter_macro: None,
            speaker_colors: None,
        },
    );
    let snapshot = RenderAssetSnapshot::load(
        ProjectConfig::try_from(raw).expect("valid config"),
        locations,
    )
    .expect("load render assets");
    let style = RenderStyle::new(
        None,
        role(
            "content",
            "Content",
            slots,
            Some(CueMacro::new("Song".to_string(), None).expect("macro")),
        ),
        None,
        Some(7),
    )
    .expect("valid style");
    let role_id =
        crate::propresenter::presentation_spec::CueRoleId::new("content").expect("valid role");
    let resolved = resolve_role(style.content(), role_id.clone(), snapshot.themes())
        .expect("resolve source role");
    let assets = RenderAssets::new(resolved, Vec::new()).expect("render assets");
    let bindings = TextBindings::new(
        (
            TextField::body(),
            vec![StyledSegment::unstyled("Main words")],
        ),
        [(
            TextField::new("footer").expect("footer field"),
            vec![StyledSegment::unstyled("Footer words")],
        )],
    )
    .expect("multi-field bindings");
    let spec = PresentationSpec::new(
        "Multi field destinations",
        GroupSpec::anonymous(CueSpec::text(role_id, bindings), Vec::new()),
        Vec::new(),
    )
    .expect("presentation spec");
    let mut rendered = render_presentation(&spec, &assets).expect("render presentation");

    super::text_fit::retain_final_text_fit(
        &spec,
        &assets,
        &mut rendered,
        &style,
        Some(&snapshot),
        7,
        &mut DiagnosticRenderTextFit,
    )
    .expect("prove all field destinations");

    assert_eq!(rendered.text_fit_summary()[0].destination_count(), 4);
}

#[test]
fn explicit_body_geometry_controls_content_splitting() {
    let template = differently_sized_fields();
    let original_footer = body_rtf(&template, "Footer");
    let (_root, themes) = theme_cache(vec![("Content", template)]);
    let style = RenderStyle::new(
        None,
        role(
            "body-role",
            "Content",
            BTreeMap::from([("body".to_string(), "Body".to_string())]),
            None,
        ),
        None,
        Some(8),
    )
    .expect("valid style");
    let text = std::iter::repeat_n("bounded words", 20)
        .collect::<Vec<_>>()
        .join(" ");
    let content = description(&text, None);

    let rendered = render_source(
        "Body metrics",
        PresentationSource::Description(&content),
        &style,
        &themes,
    )
    .expect("render from selected body geometry");

    assert!(rendered.presentation().cues.len() > 1);
    assert_eq!(
        rendered.text_fit_summary().len(),
        rendered.presentation().cues.len()
    );
    assert!(rendered
        .text_fit_summary()
        .iter()
        .all(|summary| summary.destination_count() == 1));
    let rendered_bodies = (0..rendered.presentation().cues.len())
        .map(|index| {
            let slide = rendered_slide(&rendered, index);
            assert_eq!(body_rtf(slide, "Footer"), original_footer);
            rtf_to_text(&String::from_utf8_lossy(&body_rtf(slide, "Body")))
                .expect("visible body text")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        rendered_bodies
            .join(" ")
            .split_whitespace()
            .collect::<Vec<_>>(),
        text.split_whitespace().collect::<Vec<_>>()
    );
}

#[test]
fn stream_override_capacity_forces_a_shorter_successful_partition() {
    let (_root, assets) = render_assets_with_narrow_stream();
    let style = RenderStyle::new(
        None,
        role(
            "content",
            "Content",
            BTreeMap::from([("body".to_string(), "Body".to_string())]),
            Some(CueMacro::new("Song".to_string(), None).expect("song macro")),
        ),
        None,
        Some(7),
    )
    .expect("valid stream-aware style");
    let content = description(
        "The first sentence has enough words to exercise wrapping naturally. The second sentence also carries meaningful content, while the final clause confirms that every destination participates in partition selection.",
        None,
    );

    let source_only = render_source(
        "Source-only capacity",
        PresentationSource::Description(&content),
        &style,
        assets.themes(),
    )
    .expect("source theme accepts the full paragraph");
    let with_stream = render_source_with_fit(
        "Stream-aware capacity",
        PresentationSource::Description(&content),
        &style,
        assets.themes(),
        Some(&assets),
        &mut DiagnosticRenderTextFit,
    )
    .expect("narrow stream should force a valid shorter partition");

    assert_eq!(source_only.presentation().cues.len(), 1);
    assert!(with_stream.presentation().cues.len() > 1);
    assert_eq!(
        with_stream.text_fit_summary().len(),
        with_stream.presentation().cues.len()
    );
    assert!(with_stream
        .text_fit_summary()
        .iter()
        .all(|summary| summary.destination_count() == 2));
    assert!(cue_has_macro_named(
        &with_stream.presentation().cues[0],
        "Song"
    ));
}

#[cfg(target_os = "macos")]
#[test]
fn strict_restyle_proof_measures_active_macro_screens_and_rejects_ambiguous_text() {
    let (_root, assets) = render_assets_with_narrow_stream();
    let style = RenderStyle::new(
        None,
        role(
            "content",
            "Content",
            BTreeMap::from([("body".to_string(), "Body".to_string())]),
            Some(CueMacro::new("Song".to_string(), None).expect("song macro")),
        ),
        None,
        Some(7),
    )
    .expect("valid restyle-proof style");
    let content = description(
        "The first sentence creates a useful lyric slide. The second sentence proves that the active macro continues across later cues, and the last clause keeps the stream destination bounded.",
        None,
    );
    let mut oracle = NativeTextFitOracle::start_bundled().expect("native TextKit oracle");
    let mut rendered = render_source_with_native_fit(
        "Strict restyle proof",
        PresentationSource::Description(&content),
        &style,
        &assets,
        &mut oracle,
    )
    .expect("render stream-aware source");

    let proof = crate::workflow::execute::restyle_text_fit::prove_restyled_text_fit_for_test(
        rendered.presentation(),
        &assets,
        &mut oracle,
    )
    .expect("prove every restyled destination");
    assert_eq!(proof.len(), rendered.presentation().cues.len());
    assert!(proof.iter().all(|summary| summary.destination_count() == 2));

    let mut rerouted = rendered.presentation().clone();
    let macro_identification = rerouted.cues[0]
        .actions
        .iter_mut()
        .find_map(|action| {
            let rv_data::action::ActionTypeData::Macro(macro_action) =
                action.action_type_data.as_mut()?
            else {
                return None;
            };
            macro_action.identification.as_mut()
        })
        .expect("entry macro identification");
    macro_identification.parameter_name = "Different Destination".to_string();
    assert!(matches!(
        rendered.replace_preserving_role_mapping(rerouted),
        Err(RenderError::MeasuredCueDestinationChanged { cue_index: 0 })
    ));

    let mut ambiguous = rendered.presentation().clone();
    let slide = first_presentation_slide_mut(&mut ambiguous);
    let base = slide.base_slide.as_mut().expect("base slide");
    let mut duplicate = base.elements[0].clone();
    duplicate.element.as_mut().expect("text graphics").uuid = Some(rv_data::Uuid {
        string: "55555555-5555-4555-8555-555555555555".to_string(),
    });
    base.elements.push(duplicate);

    let error = crate::workflow::execute::restyle_text_fit::prove_restyled_text_fit_for_test(
        &ambiguous,
        &assets,
        &mut oracle,
    )
    .expect_err("multi-text restyle mapping must require review");
    assert!(error.contains("2 nonempty text elements"));

    let mut mixed_metrics = rendered.presentation().clone();
    let text = first_presentation_slide_mut(&mut mixed_metrics)
        .base_slide
        .as_mut()
        .and_then(|slide| {
            slide
                .elements
                .iter_mut()
                .find_map(|element| element.element.as_mut()?.text.as_mut())
        })
        .expect("one visible text field");
    text.rtf_data =
        br"{\rtf1\ansi\deff0{\fonttbl{\f0\fswiss Helvetica;}}\f0\fs48 Plain {\b inline emphasis}}"
            .to_vec();

    let error = crate::workflow::execute::restyle_text_fit::prove_restyled_text_fit_for_test(
        &mixed_metrics,
        &assets,
        &mut oracle,
    )
    .expect_err("mixed metric runs need proprietary audience-style mapping");
    assert!(error.contains("metric-affecting text runs"));
}

#[test]
fn scripture_continuations_keep_their_native_verse_label() {
    let (_root, themes) = theme_cache(vec![("Scripture", fixture_slide())]);
    let style = RenderStyle::new(
        None,
        role("content", "Scripture", BTreeMap::new(), None),
        None,
        Some(1),
    )
    .expect("valid style");
    let verses = [Verse {
        number: 12,
        text: std::iter::repeat_n("unpunctuated", 40)
            .collect::<Vec<_>>()
            .join(" "),
    }];

    let rendered = render_source(
        "John 3v12 NRSVue",
        PresentationSource::Scripture {
            title: "John 3:12 NRSVue",
            label_prefix: "John 3:",
            verses: &verses,
        },
        &style,
        &themes,
    )
    .expect("render scripture");
    let labels = slide_labels(&rendered);

    assert!(labels.len() > 2);
    assert_eq!(labels[0], None);
    assert!(labels[1..].iter().all(|label| *label == Some("John 3:12")));
}

#[test]
fn mixed_responses_share_one_slide_and_use_the_first_speakers_macro() {
    let style = RenderStyle::new(
        None,
        liturgy_role(
            "liturgy",
            "Liturgy",
            CueMacro::new(
                "Scripture/Prayer".to_string(),
                Some("Scripture/Prayer (Highlighted)".to_string()),
            )
            .expect("valid cue macros"),
        ),
        None,
        Some(7),
    )
    .expect("valid style");
    for (first, second, expected_macro) in [
        (
            SpeakerRole::Leader,
            SpeakerRole::Audience,
            "Scripture/Prayer (Highlighted)",
        ),
        (
            SpeakerRole::Audience,
            SpeakerRole::Leader,
            "Scripture/Prayer",
        ),
    ] {
        let content = ParsedContent::new(
            vec![
                parsed_segment("First response.", first),
                parsed_segment("", SpeakerRole::Neutral),
                parsed_segment("Second response.", second),
            ],
            None,
        );
        let rendered = render_with_configured_macros(
            "Mixed liturgy",
            PresentationSource::Description(&content),
            &style,
            vec![("Liturgy", fixture_slide())],
            &["Scripture/Prayer", "Scripture/Prayer (Highlighted)"],
        );

        assert_eq!(rendered.presentation().cues.len(), 1);
        assert!(cue_has_macro_named(
            &rendered.presentation().cues[0],
            expected_macro
        ));
        let rtf = rendered_slide(&rendered, 0)
            .base_slide
            .as_ref()
            .expect("base slide")
            .elements
            .iter()
            .find_map(|element| element.element.as_ref()?.text.as_ref())
            .expect("rendered text");
        let rtf = String::from_utf8_lossy(&rtf.rtf_data);
        assert!(rtf.contains("\\red254\\green219\\blue79"));
        assert!(rtf.contains("\\red255\\green255\\blue255"));
        assert!(rendered_text(&rendered, 0).contains("First response"));
        assert!(rendered_text(&rendered, 0).contains("Second response"));
    }
}

#[test]
fn long_question_answer_flow_separates_roles_and_keeps_the_answer_front_loaded() {
    let style = RenderStyle::new(
        None,
        liturgy_role(
            "liturgy",
            "Liturgy",
            CueMacro::new(
                "Scripture/Prayer".to_string(),
                Some("Scripture/Prayer (Highlighted)".to_string()),
            )
            .expect("valid cue macros"),
        ),
        Some(role(
            "title",
            "Title",
            BTreeMap::new(),
            Some(CueMacro::new("Name Tag/Title".to_string(), None).expect("title macro")),
        )),
        Some(2),
    )
    .expect("valid style");
    let answer = "A. Do take care of all our physical needs so that we come to know that you are the only source of everything good, and that neither our work and worry, nor your gifts, can do us any good without your blessing. And so help us to give up our trust in creatures and trust in you alone.";
    let content = ParsedContent::new(
        vec![
            parsed_segment("Q. What is our hope?", SpeakerRole::Leader),
            parsed_segment(answer, SpeakerRole::Audience),
        ],
        Some("Affirmation of Faith".to_string()),
    );

    let rendered = render_with_configured_macros(
        "Catechism",
        PresentationSource::Description(&content),
        &style,
        vec![("Title", fixture_slide()), ("Liturgy", fixture_slide())],
        &[
            "Name Tag/Title",
            "Scripture/Prayer",
            "Scripture/Prayer (Highlighted)",
        ],
    );

    assert!(rendered.presentation().cues.len() >= 3);
    assert_eq!(rendered_text(&rendered, 1), "Q. What is our hope?");
    assert!(!rendered_text(&rendered, 1).contains("A."));
    assert!(cue_has_macro_named(
        &rendered.presentation().cues[1],
        "Scripture/Prayer (Highlighted)"
    ));
    assert!(cue_has_macro_named(
        &rendered.presentation().cues[2],
        "Scripture/Prayer"
    ));
    let answer_slides = (2..rendered.presentation().cues.len())
        .map(|index| rendered_text(&rendered, index))
        .collect::<Vec<_>>();
    let answer_word_counts = answer_slides
        .iter()
        .map(|slide| slide.split_whitespace().count())
        .collect::<Vec<_>>();
    assert_eq!(
        answer_slides
            .join(" ")
            .split_whitespace()
            .collect::<String>(),
        answer.split_whitespace().collect::<String>()
    );
    assert!(
        answer_word_counts.last().is_some_and(|count| *count >= 3),
        "tiny catechism tail: {answer_slides:?}"
    );
    assert!(
        answer_word_counts
            .first()
            .zip(answer_word_counts.last())
            .is_some_and(|(first, last)| first >= last),
        "catechism answer should trend front-heavy: {answer_slides:?}"
    );
    assert!(rendered
        .presentation()
        .cues
        .iter()
        .all(|cue| { cue.actions.iter().filter_map(macro_action_name).count() <= 1 }));
}

#[test]
fn each_explicit_question_answer_pair_is_partitioned_from_its_own_answer() {
    let style = RenderStyle::new(
        None,
        liturgy_role(
            "liturgy",
            "Liturgy",
            CueMacro::new(
                "Scripture/Prayer".to_string(),
                Some("Scripture/Prayer (Highlighted)".to_string()),
            )
            .expect("valid cue macros"),
        ),
        None,
        Some(1),
    )
    .expect("valid style");
    let content = ParsedContent::new(
        vec![
            parsed_segment("ALL: Preamble.", SpeakerRole::Audience),
            parsed_segment("Q. First question?", SpeakerRole::Leader),
            parsed_segment("A. First answer.", SpeakerRole::Audience),
            parsed_segment("Q. Second question?", SpeakerRole::Leader),
            parsed_segment("A. Second answer.", SpeakerRole::Audience),
        ],
        None,
    );
    let layout = TextLayout::new(20, 1).expect("valid layout");

    let slides = pack_description_for_slides_estimated(&content, layout);
    let text = slides
        .iter()
        .map(|slide| {
            slide
                .iter()
                .map(|segment| segment.text.as_str())
                .collect::<Vec<_>>()
                .join(" | ")
        })
        .collect::<Vec<_>>();

    assert_eq!(
        text,
        vec![
            "ALL: Preamble.",
            "Q. First question?",
            "A. First answer.",
            "Q. Second question?",
            "A. Second answer.",
        ]
    );
    assert_eq!(
        slides
            .iter()
            .map(|slide| slide[0].speaker)
            .collect::<Vec<_>>(),
        vec![
            SpeakerRole::Audience,
            SpeakerRole::Leader,
            SpeakerRole::Audience,
            SpeakerRole::Leader,
            SpeakerRole::Audience,
        ]
    );

    let rendered = render_with_configured_macros(
        "Multiple catechism pairs",
        PresentationSource::Description(&content),
        &style,
        vec![("Liturgy", fixture_slide())],
        &["Scripture/Prayer", "Scripture/Prayer (Highlighted)"],
    );
    assert_eq!(rendered.presentation().cues.len(), 5);
    assert_alternating_liturgy_macro_contract(&rendered, &style);
    for (index, expected) in [
        "Scripture/Prayer",
        "Scripture/Prayer (Highlighted)",
        "Scripture/Prayer",
        "Scripture/Prayer (Highlighted)",
        "Scripture/Prayer",
    ]
    .into_iter()
    .enumerate()
    {
        assert!(cue_has_macro_named(
            &rendered.presentation().cues[index],
            expected
        ));
        assert_eq!(
            rendered.presentation().cues[index]
                .actions
                .iter()
                .filter_map(macro_action_name)
                .count(),
            1
        );
    }
}

#[test]
fn macros_follow_configured_role_transitions_instead_of_fixed_role_names() {
    let style = RenderStyle::new(
        None,
        role(
            "paragraph-region",
            "Paragraph",
            BTreeMap::new(),
            Some(CueMacro::new("Body Macro".to_string(), None).expect("body macro")),
        ),
        Some(role(
            "heading-region",
            "Heading",
            BTreeMap::new(),
            Some(CueMacro::new("Heading Macro".to_string(), None).expect("heading macro")),
        )),
        None,
    )
    .expect("valid style");
    let content = description("Body", Some("Heading"));
    let rendered = render_with_configured_macros(
        "Role macros",
        PresentationSource::Description(&content),
        &style,
        vec![("Heading", fixture_slide()), ("Paragraph", fixture_slide())],
        &["Heading Macro", "Body Macro"],
    );

    assert_eq!(rendered.cue_roles().transitions().len(), 2);
    assert_eq!(
        rendered.cue_roles().transitions()[0].role().as_str(),
        "heading-region"
    );
    assert_eq!(rendered.cue_roles().transitions()[0].cue_index(), 0);
    assert_eq!(
        rendered.cue_roles().transitions()[1].role().as_str(),
        "paragraph-region"
    );
    assert_eq!(rendered.cue_roles().transitions()[1].cue_index(), 1);
    assert!(cue_has_macro_named(
        &rendered.presentation().cues[0],
        "Heading Macro"
    ));
    assert!(cue_has_macro_named(
        &rendered.presentation().cues[1],
        "Body Macro"
    ));
    assert_eq!(
        rendered.presentation().cues[0]
            .actions
            .iter()
            .filter_map(macro_action_name)
            .collect::<Vec<_>>(),
        vec!["Heading Macro"]
    );
    assert_eq!(
        rendered.presentation().cues[1]
            .actions
            .iter()
            .filter_map(macro_action_name)
            .collect::<Vec<_>>(),
        vec!["Body Macro"]
    );
}
