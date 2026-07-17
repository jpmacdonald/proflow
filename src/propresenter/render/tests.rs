#![allow(clippy::expect_used, clippy::float_cmp, clippy::unwrap_used)]

use prost::Message;

use super::*;
use crate::propresenter::presentation_spec::{
    ArrangementName, ArrangementSpec, CueSpec, GroupId, GroupSpec, PresentationSpec, TextBindings,
};

fn text_element(name: &str, text: &str) -> rv_data::slide::Element {
    rv_data::slide::Element {
        element: Some(rv_data::graphics::Element {
            name: name.to_string(),
            text: Some(rv_data::graphics::Text {
                rtf_data: segments_to_rtf_bytes(
                    &[StyledSegment::unstyled(text)],
                    &super::super::rtf::RtfOptions::default(),
                ),
                ..rv_data::graphics::Text::default()
            }),
            ..rv_data::graphics::Element::default()
        }),
        ..rv_data::slide::Element::default()
    }
}

fn template(elements: Vec<rv_data::slide::Element>) -> rv_data::PresentationSlide {
    rv_data::PresentationSlide {
        base_slide: Some(rv_data::Slide {
            elements,
            ..rv_data::Slide::default()
        }),
        ..rv_data::PresentationSlide::default()
    }
}

fn rendered_text(slide: &rv_data::PresentationSlide, index: usize) -> String {
    let data = &slide.base_slide.as_ref().unwrap().elements[index]
        .element
        .as_ref()
        .unwrap()
        .text
        .as_ref()
        .unwrap()
        .rtf_data;
    rtf_to_text(&String::from_utf8_lossy(data)).unwrap_or_default()
}

fn cue_slide(cue: &rv_data::Cue) -> &rv_data::PresentationSlide {
    cue.actions
        .iter()
        .find_map(|action| match &action.action_type_data {
            Some(rv_data::action::ActionTypeData::Slide(slide)) => match &slide.slide {
                Some(rv_data::action::slide_type::Slide::Presentation(slide)) => Some(slide),
                _ => None,
            },
            _ => None,
        })
        .expect("presentation slide action")
}

#[test]
fn named_slot_binding_changes_only_the_selected_field() {
    let slide = template(vec![
        text_element("Name", "Old name"),
        text_element("Title", "Old title"),
    ]);
    let role_id = CueRoleId::new("nametag").expect("role");
    let role = ResolvedCueRole::with_slots(
        role_id.clone(),
        &slide,
        (TextField::new("name").expect("field"), "Name"),
        [(TextField::new("title").expect("field"), "Title")],
    )
    .expect("resolved named fields");
    let bindings = TextBindings::single(
        TextField::new("name").expect("field"),
        vec![StyledSegment::unstyled("Ada Lovelace")],
    );
    let spec = PresentationSpec::new(
        "Nametag",
        GroupSpec::anonymous(CueSpec::text(role_id, bindings), Vec::new()),
        Vec::new(),
    )
    .expect("spec");
    let rendered =
        render_presentation(&spec, &RenderAssets::new(role, Vec::new()).expect("assets"))
            .expect("render");
    let output = cue_slide(&rendered.presentation.cues[0]);

    assert_eq!(rendered_text(output, 0), "Ada Lovelace");
    assert_eq!(rendered_text(output, 1), "Old title");
}

#[test]
fn native_template_instances_have_unique_cue_local_identities() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/propresenter/native/templates/scripture-template.pro");
    let source = rv_data::Presentation::decode(
        std::fs::read(path)
            .expect("native template fixture")
            .as_slice(),
    )
    .expect("native template presentation");
    let template_slide = cue_slide(source.cues.first().expect("template cue")).clone();
    let source_base = template_slide.base_slide.as_ref().expect("source base slide");
    let source_element_uuid = source_base.elements[0]
        .element
        .as_ref()
        .and_then(|element| element.uuid.as_ref())
        .expect("source element identity");
    let role_id = CueRoleId::new("scripture").expect("role");
    let role = ResolvedCueRole::body(role_id.clone(), &template_slide).expect("native body role");
    let body = |text| {
        CueSpec::text(
            role_id.clone(),
            TextBindings::single(TextField::body(), vec![StyledSegment::unstyled(text)]),
        )
    };
    let spec = PresentationSpec::new(
        "Cue-local identities",
        GroupSpec::anonymous(body("First cue"), vec![body("Second cue")]),
        Vec::new(),
    )
    .expect("presentation");

    let rendered =
        render_presentation(&spec, &RenderAssets::new(role, Vec::new()).expect("assets"))
            .expect("render");
    let first = cue_slide(&rendered.presentation.cues[0]);
    let second = cue_slide(&rendered.presentation.cues[1]);
    let first_base = first.base_slide.as_ref().expect("first base slide");
    let second_base = second.base_slide.as_ref().expect("second base slide");
    let first_element_uuid = first_base.elements[0]
        .element
        .as_ref()
        .and_then(|element| element.uuid.as_ref())
        .expect("first element identity");
    let second_element_uuid = second_base.elements[0]
        .element
        .as_ref()
        .and_then(|element| element.uuid.as_ref())
        .expect("second element identity");

    assert_ne!(first_base.uuid, source_base.uuid);
    assert_ne!(second_base.uuid, source_base.uuid);
    assert_ne!(first_base.uuid, second_base.uuid);
    assert_ne!(first_element_uuid, source_element_uuid);
    assert_ne!(second_element_uuid, source_element_uuid);
    assert_ne!(first_element_uuid, second_element_uuid);
}

#[test]
fn replacement_rtf_uses_native_style_and_preserves_rtf_family() {
    let attributes = rv_data::graphics::text::Attributes {
        font: Some(rv_data::Font {
            name: "Avenir Next".to_string(),
            family: "Avenir Next".to_string(),
            size: 64.0,
            bold: true,
            italic: true,
            ..rv_data::Font::default()
        }),
        fill: Some(rv_data::graphics::text::attributes::Fill::TextSolidFill(
            rv_data::Color {
                red: 0.2,
                green: 0.4,
                blue: 0.6,
                alpha: 1.0,
            },
        )),
        ..rv_data::graphics::text::Attributes::default()
    };
    let slide = template(vec![rv_data::slide::Element {
        element: Some(rv_data::graphics::Element {
            name: "Body".to_string(),
            text: Some(rv_data::graphics::Text {
                attributes: Some(attributes.clone()),
                rtf_data: br"{\rtf0\ansi{\fonttbl\f0\froman Times New Roman;}{\colortbl;\red255\green0\blue0;}\pard\qc\f0\fs40\cf1\b0\i0 Old placeholder}"
                    .to_vec(),
                ..rv_data::graphics::Text::default()
            }),
            ..rv_data::graphics::Element::default()
        }),
        ..rv_data::slide::Element::default()
    }]);
    let role_id = CueRoleId::new("content").expect("role");
    let role = ResolvedCueRole::body(role_id.clone(), &slide).expect("body role");
    let spec = PresentationSpec::new(
        "Native style",
        GroupSpec::anonymous(
            CueSpec::text(
                role_id,
                TextBindings::single(
                    TextField::body(),
                    vec![StyledSegment::unstyled("Replacement")],
                ),
            ),
            Vec::new(),
        ),
        Vec::new(),
    )
    .expect("spec");

    let rendered =
        render_presentation(&spec, &RenderAssets::new(role, Vec::new()).expect("assets"))
            .expect("render");
    let output = cue_slide(&rendered.presentation.cues[0]);
    let text = output.base_slide.as_ref().expect("base slide").elements[0]
        .element
        .as_ref()
        .expect("graphics")
        .text
        .as_ref()
        .expect("text");
    let rtf = String::from_utf8_lossy(&text.rtf_data);

    assert_eq!(text.attributes.as_ref(), Some(&attributes));
    assert_eq!(rtf_to_text(&rtf).as_deref(), Some("Replacement"));
    assert!(!rtf.contains("Old placeholder"));
    assert!(rtf.contains(r"\f0\froman\fcharset0 Avenir Next;"));
    assert!(rtf.contains(r"\red51\green102\blue153;"));
    assert!(rtf.contains(r"\f0\fs128"));
    assert!(rtf.contains(r"\b\i"));
    assert!(rtf.contains(r"\pard\qc"));
}

#[test]
fn default_body_ignores_empty_unnamed_helper_fields() {
    let slide = template(vec![
        text_element("Lyrics", "Old lyrics"),
        text_element("", ""),
    ]);
    let role = ResolvedCueRole::body(CueRoleId::new("content").expect("role"), &slide)
        .expect("one meaningful body field");

    assert_eq!(role.fields.get(&TextField::body()), Some(&0));
}

#[test]
fn default_body_rejects_ambiguous_multi_field_templates() {
    let slide = template(vec![text_element("Name", "A"), text_element("Title", "B")]);
    let result = ResolvedCueRole::body(CueRoleId::new("content").expect("role"), &slide);

    assert!(matches!(
        result,
        Err(TemplateSlotError::AmbiguousDefaultSlot { count: 2 })
    ));
}

#[test]
fn renderer_records_generic_role_transitions() {
    let slide = template(vec![text_element("Body", "")]);
    let title_id = CueRoleId::new("title").expect("role");
    let content_id = CueRoleId::new("content").expect("role");
    let title = ResolvedCueRole::body(title_id.clone(), &slide).expect("title role");
    let content = ResolvedCueRole::body(content_id.clone(), &slide).expect("content role");
    let body = |text| TextBindings::single(TextField::body(), vec![StyledSegment::unstyled(text)]);
    let spec = PresentationSpec::new(
        "Transitions",
        GroupSpec::anonymous(
            CueSpec::text(title_id.clone(), body("Title one")),
            vec![
                CueSpec::text(content_id.clone(), body("Content")),
                CueSpec::text(title_id.clone(), body("Title two")),
            ],
        ),
        Vec::new(),
    )
    .expect("spec");
    let rendered = render_presentation(
        &spec,
        &RenderAssets::new(title, vec![content]).expect("assets"),
    )
    .expect("render");

    assert_eq!(rendered.cue_roles.entries(&title_id), &[0, 2]);
    assert_eq!(rendered.cue_roles.entries(&content_id), &[1]);
    assert_eq!(
        rendered
            .cue_roles
            .transitions()
            .iter()
            .map(RoleTransition::cue_index)
            .collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
}

#[test]
fn renderer_derives_role_entries_from_arrangement_traversal() {
    let slide = template(vec![text_element("Body", "")]);
    let shared_id = CueRoleId::new("shared").expect("role");
    let divider_id = CueRoleId::new("divider").expect("role");
    let shared = ResolvedCueRole::body(shared_id.clone(), &slide).expect("shared role");
    let divider = ResolvedCueRole::body(divider_id.clone(), &slide).expect("divider role");
    let first_id = GroupId::new("first").expect("group id");
    let second_id = GroupId::new("second").expect("group id");
    let divider_group_id = GroupId::new("divider").expect("group id");
    let cue = |role: CueRoleId, text| {
        CueSpec::text(
            role,
            TextBindings::single(TextField::body(), vec![StyledSegment::unstyled(text)]),
        )
    };
    let spec = PresentationSpec::new(
        "Canonical macro entries",
        GroupSpec::anonymous_with_id(
            first_id.clone(),
            cue(shared_id.clone(), "First"),
            Vec::new(),
        ),
        vec![
            GroupSpec::anonymous_with_id(
                second_id.clone(),
                cue(shared_id.clone(), "Second"),
                Vec::new(),
            ),
            GroupSpec::anonymous_with_id(
                divider_group_id.clone(),
                cue(divider_id.clone(), "Divider"),
                Vec::new(),
            ),
        ],
    )
    .expect("presentation")
    .with_arrangements(
        vec![
            ArrangementSpec::new("Default", first_id, vec![divider_group_id, second_id])
                .expect("arrangement"),
        ],
        Some(ArrangementName::new("Default").expect("selection")),
    )
    .expect("checked arrangement");

    let rendered = render_presentation(
        &spec,
        &RenderAssets::new(shared, vec![divider]).expect("assets"),
    )
    .expect("render");

    assert_eq!(rendered.cue_roles.entries(&shared_id), &[0, 1]);
    assert_eq!(rendered.cue_roles.entries(&divider_id), &[2]);
    assert_eq!(
        rendered
            .cue_roles
            .transitions()
            .iter()
            .map(RoleTransition::cue_index)
            .collect::<Vec<_>>(),
        vec![0, 2, 1]
    );
}

#[test]
fn renderer_constructs_only_declared_cues_and_groups() {
    let slide = template(vec![text_element("Body", "")]);
    let role_id = CueRoleId::new("content").expect("role");
    let role = ResolvedCueRole::body(role_id.clone(), &slide).expect("role template");
    let cue = || {
        CueSpec::text(
            role_id.clone(),
            TextBindings::single(TextField::body(), vec![StyledSegment::unstyled("Text")]),
        )
    };
    let spec = PresentationSpec::new(
        "Groups",
        GroupSpec::anonymous(cue(), vec![cue()]),
        vec![GroupSpec::named("Second", cue(), Vec::new()).expect("named group")],
    )
    .expect("spec");
    let directory = tempfile::tempdir().expect("temporary directory");
    let groups_path = directory.path().join("Groups");
    let application_group_id = uuid::Uuid::new_v4().to_string();
    let installed_group = rv_data::Group {
        uuid: Some(rv_data::Uuid {
            string: application_group_id.clone(),
        }),
        name: "Second".to_string(),
        color: Some(rv_data::Color {
            red: 0.2,
            green: 0.3,
            blue: 0.4,
            alpha: 1.0,
        }),
        ..rv_data::Group::default()
    };
    std::fs::write(
        &groups_path,
        rv_data::ProGroupsDocument {
            groups: vec![installed_group],
        }
        .encode_to_vec(),
    )
    .expect("write groups");
    let groups = GroupCatalog::load_from(&groups_path).expect("load groups");
    let assets = RenderAssets::new(role, Vec::new())
        .expect("assets")
        .with_group_catalog(&groups);
    let rendered = render_presentation(&spec, &assets).expect("render");

    assert_eq!(rendered.presentation.cues.len(), 3);
    assert_eq!(rendered.presentation.cue_groups.len(), 2);
    assert_eq!(rendered.presentation.cue_groups[0].cue_identifiers.len(), 2);
    assert_eq!(rendered.presentation.cue_groups[1].cue_identifiers.len(), 1);
    assert_eq!(
        rendered.presentation.cue_groups[1]
            .group
            .as_ref()
            .map(|group| group.name.as_str()),
        Some("Second")
    );
    assert_eq!(
        rendered.presentation.cue_groups[1]
            .group
            .as_ref()
            .and_then(|group| group.application_group_identifier.as_ref())
            .map(|uuid| uuid.string.as_str()),
        Some(application_group_id.as_str())
    );
}

#[test]
fn named_group_requires_exact_installed_metadata() {
    let slide = template(vec![text_element("Body", "")]);
    let role_id = CueRoleId::new("content").expect("role");
    let role = ResolvedCueRole::body(role_id.clone(), &slide).expect("role template");
    let cue = CueSpec::text(
        role_id,
        TextBindings::single(TextField::body(), vec![StyledSegment::unstyled("Text")]),
    );
    let spec = PresentationSpec::new(
        "Groups",
        GroupSpec::named("Verse 1", cue, Vec::new()).expect("named group"),
        Vec::new(),
    )
    .expect("spec");

    assert!(matches!(
        render_presentation(
            &spec,
            &RenderAssets::new(role, Vec::new()).expect("assets")
        ),
        Err(RenderError::MissingGroup { group }) if group == "Verse 1"
    ));
}

#[test]
fn arrangements_round_trip_ordered_repeated_group_references_and_selection() {
    let slide = template(vec![text_element("Body", "")]);
    let role_id = CueRoleId::new("content").expect("role");
    let role = ResolvedCueRole::body(role_id.clone(), &slide).expect("role template");
    let verse_id = GroupId::new("verse-1").expect("group id");
    let chorus_id = GroupId::new("chorus").expect("group id");
    let cue = |text| {
        CueSpec::text(
            role_id.clone(),
            TextBindings::single(TextField::body(), vec![StyledSegment::unstyled(text)]),
        )
    };
    let spec = PresentationSpec::new(
        "Arrangement Fidelity",
        GroupSpec::anonymous_with_id(verse_id.clone(), cue("Verse"), Vec::new()),
        vec![GroupSpec::anonymous_with_id(
            chorus_id.clone(),
            cue("Chorus"),
            Vec::new(),
        )],
    )
    .expect("presentation")
    .with_arrangements(
        vec![
            ArrangementSpec::new("Default", verse_id, vec![chorus_id.clone(), chorus_id])
                .expect("arrangement"),
        ],
        Some(ArrangementName::new("Default").expect("selection")),
    )
    .expect("checked arrangement");

    let rendered =
        render_presentation(&spec, &RenderAssets::new(role, Vec::new()).expect("assets"))
            .expect("render");
    let decoded = rv_data::Presentation::decode(rendered.presentation.encode_to_vec().as_slice())
        .expect("native round trip");
    let group_ids = decoded
        .cue_groups
        .iter()
        .map(|group| {
            group
                .group
                .as_ref()
                .and_then(|group| group.uuid.as_ref())
                .map(|uuid| uuid.string.clone())
                .expect("rendered group identity")
        })
        .collect::<Vec<_>>();
    let arrangement = decoded.arrangements.first().expect("arrangement");

    assert_eq!(
        arrangement
            .group_identifiers
            .iter()
            .map(|uuid| uuid.string.as_str())
            .collect::<Vec<_>>(),
        vec![
            group_ids[0].as_str(),
            group_ids[1].as_str(),
            group_ids[1].as_str(),
        ]
    );
    assert_eq!(
        decoded
            .selected_arrangement
            .as_ref()
            .map(|uuid| uuid.string.as_str()),
        arrangement.uuid.as_ref().map(|uuid| uuid.string.as_str())
    );
    assert_eq!(
        crate::propresenter::arrangement::operator_cue_indices(&decoded),
        vec![0, 1, 1]
    );
}

#[test]
fn producer_neutral_envelope_omits_producer_dependent_metadata() {
    let presentation = producer_neutral_presentation("Native Envelope");

    assert!(matches!(
        presentation.background,
        Some(rv_data::Background {
            is_enabled: false,
            fill: None
        })
    ));
    assert!(presentation.application_info.is_none());
    assert!(presentation.chord_chart.is_none());
    assert!(presentation.ccli.is_some());
    assert_eq!(
        presentation.timeline.map(|timeline| timeline.duration),
        Some(300.0)
    );
}

#[test]
fn generated_target_preservation_keeps_timeline_settings_without_stale_cue_references() {
    let mut existing = producer_neutral_presentation("Existing");
    existing.category = "Liturgy".to_string();
    existing.notes = "Preserve this".to_string();
    let audio_action = rv_data::Action {
        name: "Retained audio".to_string(),
        ..rv_data::Action::default()
    };
    let stale_cue = rv_data::presentation::timeline::Cue {
        trigger_time: 1.0,
        name: "stale cue".to_string(),
        trigger_info: None,
    };
    existing.timeline = Some(rv_data::presentation::Timeline {
        cues: vec![stale_cue.clone()],
        duration: 42.0,
        r#loop: true,
        audio_action: Some(audio_action.clone()),
        timecode_enable: true,
        timecode_offset: 8.5,
        cues_v2: vec![stale_cue],
    });
    let mut replacement = producer_neutral_presentation("Existing");

    preserve_generated_target_metadata(&mut replacement, &existing);

    assert_eq!(replacement.uuid, existing.uuid);
    assert_eq!(replacement.category, "Liturgy");
    assert_eq!(replacement.notes, "Preserve this");
    let timeline = replacement.timeline.expect("native timeline");
    assert!(timeline.cues.is_empty());
    assert!(timeline.cues_v2.is_empty());
    assert_eq!(timeline.duration, 42.0);
    assert!(timeline.r#loop);
    assert_eq!(timeline.audio_action.as_ref(), Some(&audio_action));
    assert!(timeline.timecode_enable);
    assert_eq!(timeline.timecode_offset, 8.5);
}

#[test]
fn generated_target_does_not_inherit_stale_content_semantics() {
    let mut existing = producer_neutral_presentation("Old song");
    existing.chord_chart = Some(rv_data::Url {
        platform: 9,
        ..rv_data::Url::default()
    });
    existing.ccli = Some(rv_data::presentation::Ccli {
        author: "Stale author".to_string(),
        ..rv_data::presentation::Ccli::default()
    });
    existing.bible_reference = Some(rv_data::presentation::BibleReference {
        book_name: "Stale book".to_string(),
        ..rv_data::presentation::BibleReference::default()
    });
    existing.multi_tracks_licensing = Some(rv_data::presentation::MultiTracksLicensing {
        song_identifier: 42,
        ..rv_data::presentation::MultiTracksLicensing::default()
    });
    existing.music_key = "Stale key".to_string();
    existing.music = Some(rv_data::presentation::Music {
        original_music_key: "Stale original key".to_string(),
        ..rv_data::presentation::Music::default()
    });
    let mut replacement = producer_neutral_presentation("New liturgy");

    preserve_generated_target_metadata(&mut replacement, &existing);
    apply_application_info(
        &mut replacement,
        Some(&rv_data::ApplicationInfo {
            platform: rv_data::application_info::Platform::Macos as i32,
            ..rv_data::ApplicationInfo::default()
        }),
    );

    assert_eq!(
        replacement.chord_chart.as_ref().map(|url| url.platform),
        Some(rv_data::url::Platform::Macos as i32)
    );
    assert_eq!(
        replacement.ccli.as_ref().map(|ccli| ccli.author.as_str()),
        Some("")
    );
    assert!(replacement.bible_reference.is_none());
    assert!(replacement.multi_tracks_licensing.is_none());
    assert!(replacement.music_key.is_empty());
    assert!(replacement.music.is_none());

    preserve_edited_document_metadata(&mut replacement, &existing);

    assert_eq!(replacement.chord_chart, existing.chord_chart);
    assert_eq!(replacement.ccli, existing.ccli);
    assert_eq!(replacement.bible_reference, existing.bible_reference);
    assert_eq!(
        replacement.multi_tracks_licensing,
        existing.multi_tracks_licensing
    );
    assert_eq!(replacement.music_key, existing.music_key);
    assert_eq!(replacement.music, existing.music);
}

#[test]
fn fresh_chord_chart_platform_follows_the_captured_producer() {
    for (producer, expected) in [
        (
            rv_data::application_info::Platform::Macos,
            rv_data::url::Platform::Macos,
        ),
        (
            rv_data::application_info::Platform::Windows,
            rv_data::url::Platform::Win32,
        ),
    ] {
        let mut presentation = producer_neutral_presentation("Fresh");
        apply_application_info(
            &mut presentation,
            Some(&rv_data::ApplicationInfo {
                platform: producer as i32,
                ..rv_data::ApplicationInfo::default()
            }),
        );

        assert_eq!(
            presentation.chord_chart.as_ref().map(|url| url.platform),
            Some(expected as i32)
        );
    }

    let mut unknown = producer_neutral_presentation("Unknown producer");
    apply_application_info(&mut unknown, None);
    assert!(unknown.chord_chart.is_none());
}
