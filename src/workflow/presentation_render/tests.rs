#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use prost::Message;

use super::*;
use crate::paths::{BuildLocationInputs, BuildLocations};
use crate::project_config::{CueRoleConfig, ProjectConfig, RawProjectConfig};
use crate::workflow::plan::{CueMacro, SpeakerPalette};
use propresenter::macros::{cue_has_macro_named, macro_action_name};
use propresenter::rtf::rtf_to_text;
use propresenter::unstable_native::rv_data;

fn fixture_slide() -> rv_data::PresentationSlide {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../propresenter/tests/fixtures/native/templates/scripture-template.pro");
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

#[cfg(target_os = "macos")]
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

mod destinations;

mod content_flow;
