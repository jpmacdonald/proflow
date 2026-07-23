use super::super::*;
use crate::workflow::plan::{
    BackgroundTransform, CueTransform, ExistingTransform, MacroTransform, RenderRole, RenderStyle,
    RestyleMacroSelector,
};
use std::{collections::BTreeSet, path::Path};

fn production_config() -> ProjectConfig {
    parse_project_config_str(include_str!("../../../data/proflow.config.json"))
        .expect("repo config should parse")
}

fn restyle(
    config: &ProjectConfig,
    type_key: &str,
    expected_kind: ItemKind,
    expected_source: ExistingSource,
) -> (ExistingTransform, Option<String>) {
    let Some(PresentationPolicy::RestyleExisting {
        kind,
        source,
        arrangement,
        transform,
    }) = config.presentation_policy(type_key)
    else {
        panic!("{type_key} must compile to a native restyle policy");
    };
    assert_eq!(kind, expected_kind, "wrong kind for {type_key}");
    assert_eq!(source, expected_source, "wrong source for {type_key}");
    (transform.for_service(None), arrangement.for_service(None))
}

fn description_style(
    config: &ProjectConfig,
    type_key: &str,
    expected_kind: ItemKind,
    expected_parser: DescriptionParserKind,
    expected_strategy: OutputStrategy,
) -> RenderStyle {
    let Some(policy) = config.presentation_policy(type_key) else {
        panic!("missing production policy {type_key}");
    };
    let (kind, parser, render) = match (expected_strategy, policy) {
        (
            OutputStrategy::EditInPlace,
            PresentationPolicy::EditDescription {
                kind,
                parser,
                render,
            },
        )
        | (
            OutputStrategy::GenerateNew,
            PresentationPolicy::GenerateDescription {
                kind,
                parser,
                render,
            },
        ) => (kind, parser, render),
        _ => panic!("wrong description strategy for {type_key}"),
    };
    assert_eq!(kind, expected_kind, "wrong kind for {type_key}");
    assert_eq!(parser, expected_parser, "wrong parser for {type_key}");
    render.for_service(None)
}

fn assert_background(style: &RenderStyle, expected_id: &str, expected_file: &str) {
    let background = style
        .background()
        .expect("managed render recipe must resolve a background");
    assert_eq!(background.id().as_str(), expected_id);
    assert_eq!(background.file().as_path(), Path::new(expected_file));
}

fn assert_default_background(transform: &ExistingTransform) {
    let BackgroundTransform::Replace(background) = transform.background() else {
        panic!("expected the managed default background");
    };
    assert_eq!(background.id().as_str(), "default");
    assert_eq!(
        background.file().as_path(),
        Path::new("backgrounds/default.png")
    );
}

fn assert_operator_macros(transform: &ExistingTransform, expected: &[(usize, &str)]) {
    let MacroTransform::Enforce(policy) = transform.macros() else {
        panic!("expected enforced operator macro transitions");
    };
    assert_eq!(policy.regions().len(), expected.len());
    for (region, &(index, macro_name)) in policy.regions().iter().zip(expected) {
        assert_eq!(
            region.selector(),
            &RestyleMacroSelector::OperatorCue { index }
        );
        assert_eq!(region.enter_macro(), macro_name);
    }
}

fn assert_arrangement_macros(transform: &ExistingTransform, expected: &[(usize, &[&str], &str)]) {
    let MacroTransform::Enforce(policy) = transform.macros() else {
        panic!("expected enforced arrangement macro transitions");
    };
    assert_eq!(policy.regions().len(), expected.len());
    for (region, &(expected_index, expected_names, expected_macro)) in
        policy.regions().iter().zip(expected)
    {
        let RestyleMacroSelector::ArrangementGroup {
            index,
            allowed_names,
        } = region.selector()
        else {
            panic!("expected an arrangement-group selector");
        };
        assert_eq!(*index, expected_index);
        assert_eq!(
            allowed_names,
            &expected_names
                .iter()
                .map(|name| (*name).to_string())
                .collect::<BTreeSet<_>>()
        );
        assert_eq!(region.enter_macro(), expected_macro);
    }
}

fn assert_static_recipes(config: &ProjectConfig) {
    assert!(config.presentation_policy("person_nametag").is_none());

    let (graphic, arrangement) = restyle(
        config,
        "static_graphic",
        ItemKind::Graphic,
        ExistingSource::Static,
    );
    assert_eq!(arrangement, None);
    assert_eq!(graphic.background(), &BackgroundTransform::Preserve);
    assert_eq!(graphic.cues(), CueTransform::Preserve);
    assert_operator_macros(&graphic, &[(0, "Graphics")]);

    let (title, arrangement) = restyle(
        config,
        "title_static",
        ItemKind::Nametag,
        ExistingSource::Static,
    );
    assert_eq!(arrangement, None);
    assert_default_background(&title);
    assert!(matches!(
        title.cues(),
        CueTransform::RetainOperatorPrefix(limit) if limit.get() == 1
    ));
    assert_operator_macros(&title, &[(0, "Name Tag/Title")]);

    let (preserved, arrangement) = restyle(
        config,
        "preserved_liturgy_static",
        ItemKind::Liturgy,
        ExistingSource::Static,
    );
    assert_eq!(arrangement, None);
    assert_default_background(&preserved);
    assert_eq!(preserved.macros(), &MacroTransform::Preserve);
    assert_eq!(preserved.cues(), CueTransform::Preserve);

    for (type_key, content_macro) in [
        ("leader_liturgy_static", "Scripture/Prayer (Highlighted)"),
        ("audience_liturgy_static", "Scripture/Prayer"),
    ] {
        let (transform, arrangement) =
            restyle(config, type_key, ItemKind::Liturgy, ExistingSource::Static);
        assert_eq!(arrangement, None);
        assert_default_background(&transform);
        assert_eq!(transform.cues(), CueTransform::Preserve);
        assert_operator_macros(&transform, &[(0, "Name Tag/Title"), (1, content_macro)]);
    }

    let (titled_song, arrangement) = restyle(
        config,
        "titled_song_static",
        ItemKind::Song,
        ExistingSource::Static,
    );
    assert_eq!(arrangement, None);
    assert_default_background(&titled_song);
    assert_eq!(titled_song.cues(), CueTransform::Preserve);
    assert_operator_macros(&titled_song, &[(0, "Name Tag/Title"), (1, "Song")]);
}

fn assert_song_recipes(config: &ProjectConfig) {
    let (song, arrangement) = restyle(config, "song", ItemKind::Song, ExistingSource::Song);
    assert_eq!(arrangement, None);
    assert_default_background(&song);
    assert_eq!(song.cues(), CueTransform::Preserve);
    assert_arrangement_macros(&song, &[(0, &["Background", "Blank", "Title"], "Song")]);

    let (hymn, arrangement) = restyle(config, "hymn", ItemKind::Song, ExistingSource::Song);
    assert_eq!(arrangement, None);
    assert_default_background(&hymn);
    assert_eq!(hymn.cues(), CueTransform::Preserve);
    assert_arrangement_macros(
        &hymn,
        &[
            (0, &["Background", "Title"], "Name Tag/Title"),
            (1, &["Verse", "Verse 1"], "Song"),
        ],
    );

    let (doxology, arrangement) = restyle(
        config,
        "doxology_with_prayer",
        ItemKind::Song,
        ExistingSource::Static,
    );
    assert_eq!(arrangement.as_deref(), Some("& Prayer of Dedication"));
    assert_default_background(&doxology);
    assert_eq!(doxology.cues(), CueTransform::Preserve);
    assert_arrangement_macros(
        &doxology,
        &[(0, &["Group"], "Name Tag/Title"), (1, &["Verse"], "Song")],
    );
}

fn assert_generated_recipes(config: &ProjectConfig) {
    let Some(PresentationPolicy::GenerateScripture { render }) =
        config.presentation_policy("scripture")
    else {
        panic!("scripture must remain generated content");
    };
    let scripture = render.for_service(None);
    assert_background(&scripture, "default", "backgrounds/default.png");
    assert_eq!(scripture.max_lines_per_slide(), Some(7));
    assert_eq!(scripture.content().id(), "scripture_prayer");
    let scripture_macro = scripture
        .content()
        .cue_macro()
        .expect("scripture content entry macro");
    assert_eq!(scripture_macro.enter(), "Scripture/Prayer");
    assert_eq!(scripture_macro.leader_enter(), None);
    assert_eq!(scripture.content().speaker_palette(), None);
    let scripture_title = scripture.title().expect("scripture title cue");
    assert_eq!(scripture_title.id(), "title");
    assert_eq!(
        scripture_title
            .cue_macro()
            .expect("scripture title entry macro")
            .enter(),
        "Name Tag/Title"
    );

    let responsive = description_style(
        config,
        "liturgical_edited",
        ItemKind::Liturgy,
        DescriptionParserKind::Liturgical,
        OutputStrategy::EditInPlace,
    );
    assert_background(&responsive, "default", "backgrounds/default.png");
    assert_eq!(responsive.max_lines_per_slide(), Some(8));
    assert_eq!(responsive.title().map(RenderRole::id), Some("title"));
    assert_eq!(responsive.content().id(), "responsive_scripture_prayer");
    let macro_pair = responsive
        .content()
        .cue_macro()
        .expect("responsive entry macro pair");
    assert_eq!(macro_pair.enter(), "Scripture/Prayer");
    assert_eq!(
        macro_pair.leader_enter(),
        Some("Scripture/Prayer (Highlighted)")
    );
    let palette = responsive
        .content()
        .speaker_palette()
        .expect("responsive speaker palette");
    assert_eq!(palette.leader(), (254, 219, 79));
    assert_eq!(palette.audience(), (255, 255, 255));

    for (type_key, parser) in [
        ("liturgical_generated", DescriptionParserKind::Liturgical),
        (
            "liturgical_audience_generated",
            DescriptionParserKind::LiturgicalAudience,
        ),
    ] {
        let style = description_style(
            config,
            type_key,
            ItemKind::Liturgy,
            parser,
            OutputStrategy::GenerateNew,
        );
        assert_background(&style, "default", "backgrounds/default.png");
        assert_eq!(style.max_lines_per_slide(), Some(8));
        assert_eq!(style.title().map(RenderRole::id), Some("title"));
        assert_eq!(style.content().id(), "responsive_scripture_prayer");
    }

    for (type_key, strategy) in [
        ("content_nametag", OutputStrategy::EditInPlace),
        ("generated_content_nametag", OutputStrategy::GenerateNew),
    ] {
        let style = description_style(
            config,
            type_key,
            ItemKind::Nametag,
            DescriptionParserKind::ContentNametag,
            strategy,
        );
        assert_background(&style, "default", "backgrounds/default.png");
        assert_eq!(style.max_lines_per_slide(), Some(4));
        assert!(style.title().is_none());
        assert_eq!(style.content().id(), "title");
    }

    let sermon = description_style(
        config,
        "sermon_title",
        ItemKind::Nametag,
        DescriptionParserKind::ContentNametag,
        OutputStrategy::GenerateNew,
    );
    assert_background(&sermon, "sermon", "backgrounds/sermon.png");
    assert_eq!(sermon.max_lines_per_slide(), Some(4));
    assert!(sermon.title().is_none());
    assert_eq!(sermon.content().id(), "title");
}

fn assert_required_graphics(config: &ProjectConfig) {
    assert_eq!(
        config
            .as_raw()
            .required_playlist_items
            .iter()
            .map(|item| (
                item.id.as_str(),
                item.use_type.as_str(),
                item.library_file.as_str(),
                item.placement,
                item.service_group.as_deref(),
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                "pre_post_service",
                "static_graphic",
                "Pre-Service, Post-Service Slides.pro",
                RequiredPlaylistPlacement::Start,
                Some("primary"),
            ),
            (
                "stephen_minister",
                "static_graphic",
                "Stephen Minister Slide.pro",
                RequiredPlaylistPlacement::End,
                Some("primary"),
            ),
        ]
    );
}

#[test]
fn repo_config_compiles_to_the_reviewed_presentation_contract() {
    let config = production_config();
    assert_eq!(config.as_raw().version, 4);
    assert_eq!(
        config.defaults().speaker_fallback_rule.as_deref(),
        Some("lords_prayer")
    );
    assert_eq!(
        config
            .as_raw()
            .item_rules
            .iter()
            .filter(|rule| rule.tier != RuleTier::Primary)
            .map(|rule| rule.id.as_str())
            .collect::<Vec<_>>(),
        vec![
            "offertory_detail_music",
            "prayer_of_confession",
            "affirmation_of_faith",
            "organ_prelude",
            "organ_postlude",
            "welcome_bundle",
            "traditional_hymns",
            "all_songs",
        ]
    );
    assert_eq!(
        config
            .as_raw()
            .item_rules
            .iter()
            .find(|rule| rule.id == "all_songs")
            .map(|rule| rule.tier),
        Some(RuleTier::CatchAll)
    );
    assert!(validate_project_config(config.as_raw()).is_empty());
    assert_static_recipes(&config);
    assert_song_recipes(&config);
    assert_generated_recipes(&config);
    assert_required_graphics(&config);
}
