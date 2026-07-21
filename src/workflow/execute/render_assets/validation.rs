//! One deterministic validation pass over configured native render bindings.

use crate::bible::BibleVersion;
use crate::paths::BuildLocations;
use crate::project_config::ProjectConfig;
use crate::propresenter::background::resolve_background_image;
use crate::propresenter::macros::MacroCache;
use crate::propresenter::render::SlideTemplate;
use crate::propresenter::resolution::inspect_slide_size;
use crate::propresenter::theme::ThemeCache;
use crate::propresenter::{audience::PresentationDestination, generated::rv_data};

use super::audience::ConfiguredAudienceDestinations;
use super::{RenderAssetIssue, RenderAssetIssues, RenderAssetSnapshotError, ThemeSlideSizeProblem};

pub(super) fn validate_bindings(
    config: &ProjectConfig,
    locations: &BuildLocations,
    themes: &ThemeCache,
    macros: &MacroCache,
) -> Result<ConfiguredAudienceDestinations, RenderAssetSnapshotError> {
    let mut issues = Vec::new();
    validate_cue_roles(config, themes, &mut issues);
    let audience_destinations =
        ConfiguredAudienceDestinations::capture(config, locations, macros, &mut issues)?;
    validate_audience_text_bindings(config, &audience_destinations, &mut issues);

    for (id, relative_path) in config.backgrounds() {
        if let Err(source) =
            resolve_background_image(locations.project_data_root(), relative_path.as_path())
        {
            issues.push(RenderAssetIssue::Background {
                id: id.to_string(),
                path: relative_path.as_path().to_path_buf(),
                source,
            });
        }
    }

    let bible_root = locations.project_data_root().join("bibles");
    if let Err(source) = crate::bible::validate_bible_corpora(&bible_root) {
        issues.push(RenderAssetIssue::BibleCorpus(source));
    }
    if let Some(version) = config.defaults().bible_version {
        let path = bible_root.join(version.file_name());
        if !path.is_file() {
            issues.push(RenderAssetIssue::MissingBibleCorpus {
                version: BibleVersion::name(version),
                path,
            });
        }
    }

    issues.sort_by_cached_key(ToString::to_string);
    if issues.is_empty() {
        Ok(audience_destinations)
    } else {
        Err(RenderAssetIssues { issues }.into())
    }
}

pub(super) fn validate_audience_text_bindings(
    config: &ProjectConfig,
    destinations: &ConfiguredAudienceDestinations,
    issues: &mut Vec<RenderAssetIssue>,
) {
    for (role_id, role) in config.cue_roles() {
        let macro_names = [
            role.enter_macro.as_deref(),
            role.leader_enter_macro.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect::<std::collections::BTreeSet<_>>();
        for macro_name in macro_names {
            let Some(look) = destinations.for_macro(macro_name) else {
                continue;
            };
            for screen in look.screens() {
                let PresentationDestination::ThemeOverride(destination) = screen.presentation()
                else {
                    continue;
                };
                let slide = rv_data::PresentationSlide {
                    base_slide: Some(destination.base_slide().clone()),
                    ..rv_data::PresentationSlide::default()
                };
                let result = SlideTemplate::inspect(&slide).and_then(|template| {
                    template.validate_native_bindings(role.text_slots.values().map(String::as_str))
                });
                if let Err(source) = result {
                    issues.push(RenderAssetIssue::AudienceTextBinding {
                        role: role_id.clone(),
                        name: macro_name.to_string(),
                        screen_name: screen.screen_name().to_string(),
                        screen_uuid: screen.screen_uuid().to_string(),
                        theme_path: destination.document_path().to_path_buf(),
                        slide_uuid: destination.slide_uuid().to_string(),
                        source,
                    });
                }
            }
        }
    }
}

fn validate_cue_roles(
    config: &ProjectConfig,
    themes: &ThemeCache,
    issues: &mut Vec<RenderAssetIssue>,
) {
    for (role_id, role) in config.cue_roles() {
        let resolved = if role.text_slots.is_empty() {
            themes.text_template(&role.slide).map(|slide| (slide, None))
        } else {
            themes
                .slide_template(&role.slide)
                .map(|template| (template.slide(), Some(template)))
        };

        match resolved {
            Ok((slide, template)) => {
                if let Some(template) = template {
                    for (field, native_slot) in &role.text_slots {
                        if !template.named_slots().any(|name| name == native_slot) {
                            issues.push(RenderAssetIssue::MissingTextSlot {
                                role: role_id.clone(),
                                field: field.clone(),
                                native_slot: native_slot.clone(),
                            });
                        }
                    }
                }
                let expected = config.defaults().presentation_size;
                match inspect_slide_size(slide) {
                    Ok(actual) if actual == expected => {}
                    Ok(actual) => issues.push(RenderAssetIssue::ThemeSlideSize {
                        role: role_id.clone(),
                        slide: role.slide.clone(),
                        expected,
                        problem: ThemeSlideSizeProblem::Mismatch(actual),
                    }),
                    Err(error) => issues.push(RenderAssetIssue::ThemeSlideSize {
                        role: role_id.clone(),
                        slide: role.slide.clone(),
                        expected,
                        problem: ThemeSlideSizeProblem::Invalid(error),
                    }),
                }
            }
            Err(source) => issues.push(RenderAssetIssue::ThemeSlide {
                role: role_id.clone(),
                source,
            }),
        }
    }
}
