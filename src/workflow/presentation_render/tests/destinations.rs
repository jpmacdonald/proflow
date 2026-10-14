use super::*;

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
    use propresenter::presentation_spec::{
        CueSpec, GroupSpec, PresentationSpec, TextBindings, TextField,
    };
    use propresenter::render::{render_presentation, RenderAssets};
    use propresenter::rtf::StyledSegment;

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
    let role_id = propresenter::presentation_spec::CueRoleId::new("content").expect("valid role");
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

    super::super::text_fit::retain_final_text_fit(
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
    use propresenter::presentation_spec::{
        CueSpec, GroupSpec, PresentationSpec, TextBindings, TextField,
    };
    use propresenter::render::{render_presentation, RenderAssets};
    use propresenter::rtf::StyledSegment;

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
    let role_id = propresenter::presentation_spec::CueRoleId::new("content").expect("valid role");
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

    super::super::text_fit::retain_final_text_fit(
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
    // Keep both metric styles inside the narrow Stream destination. This
    // replacement is measured directly rather than paginated again.
    text.rtf_data =
        br"{\rtf1\ansi\deff0{\fonttbl{\f0\fswiss Helvetica;}}\f0\fs48 A {\b B}}".to_vec();
    crate::workflow::execute::restyle_text_fit::prove_restyled_text_fit_for_test(
        &mixed_metrics,
        &assets,
        &mut oracle,
    )
    .expect("theme destination canonicalizes existing inline source styles");
}
