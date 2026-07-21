use std::path::{Path, PathBuf};

use prost::Message;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::model::{ResolvedThemeTemplate, ThemeDocument};
use super::{
    AudienceDestinationError, AudienceLookDestinations, AudienceScreenDestination,
    AudienceWorkspaceError, InvalidNativeIdentity, NativeIdentityKind, PresentationDestination,
    ThemeDestination,
};
use crate::propresenter::generated::rv_data::{
    self, pro_presenter_screen, ProAudienceLook, ProPresenterScreen, ProPresenterWorkspace,
};
use crate::propresenter::native_url::{NativeFileLocator, NativeFileResolution};

pub(super) fn resolve_saved_look<'a>(
    workspace: &'a ProPresenterWorkspace,
    look_uuid: Uuid,
    macro_name: &str,
    look_name: &str,
) -> Result<&'a ProAudienceLook, AudienceDestinationError> {
    let matches = workspace
        .audience_looks
        .iter()
        .filter(|look| {
            look.uuid
                .as_ref()
                .and_then(|native| Uuid::parse_str(&native.string).ok())
                == Some(look_uuid)
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [look] => Ok(look),
        [] => Err(AudienceDestinationError::DanglingAudienceLook {
            macro_name: macro_name.to_string(),
            look_uuid,
            look_name: look_name.to_string(),
        }),
        [first, rest @ ..] => Err(AudienceWorkspaceError::DuplicateLookUuid {
            uuid: look_uuid,
            first_name: first.name.clone(),
            duplicate_name: rest[0].name.clone(),
        }
        .into()),
    }
}

pub(super) fn compile_look(
    look: &ProAudienceLook,
    workspace: &ProPresenterWorkspace,
    show_root: &Path,
    themes: &mut std::collections::HashMap<PathBuf, ThemeDocument>,
) -> Result<AudienceLookDestinations, AudienceDestinationError> {
    let native_uuid =
        look.uuid
            .as_ref()
            .ok_or_else(|| AudienceWorkspaceError::MissingLookUuid {
                name: look.name.clone(),
            })?;
    let uuid = parse_uuid(
        NativeIdentityKind::AudienceLook,
        &native_uuid.string,
        &look.name,
    )
    .map_err(AudienceWorkspaceError::from)?;
    let mut seen_screens = std::collections::HashSet::new();
    let mut destinations = Vec::new();

    for native in &look.screen_looks {
        let screen_uuid = native.pro_screen_uuid.as_ref().ok_or_else(|| {
            AudienceWorkspaceError::MissingLookScreenUuid {
                look_name: look.name.clone(),
            }
        })?;
        let screen_uuid = parse_uuid(
            NativeIdentityKind::LookScreen,
            &screen_uuid.string,
            &look.name,
        )
        .map_err(AudienceWorkspaceError::from)?;
        if !seen_screens.insert(screen_uuid) {
            return Err(AudienceWorkspaceError::DuplicateLookScreen {
                look_name: look.name.clone(),
                screen_uuid,
            }
            .into());
        }
        let screen = resolve_screen(&workspace.pro_screens, screen_uuid, &look.name)?;
        let kind =
            pro_presenter_screen::ScreenType::try_from(screen.screen_type).map_err(|_| {
                AudienceWorkspaceError::UnknownScreenType {
                    screen_uuid,
                    screen_name: screen.name.clone(),
                    raw_type: screen.screen_type,
                }
            })?;

        if kind != pro_presenter_screen::ScreenType::Audience
            || !native.presentation_foreground_enabled
        {
            continue;
        }

        let presentation =
            compile_presentation_destination(look, &screen.name, native, show_root, themes)?;
        destinations.push(AudienceScreenDestination {
            screen_uuid,
            screen_name: screen.name.clone(),
            presentation,
        });
    }

    destinations.sort_unstable_by(|first, second| {
        first
            .screen_name
            .cmp(&second.screen_name)
            .then_with(|| first.screen_uuid.cmp(&second.screen_uuid))
    });
    Ok(AudienceLookDestinations {
        uuid,
        name: look.name.clone(),
        screens: destinations,
    })
}

fn resolve_screen<'a>(
    screens: &'a [ProPresenterScreen],
    screen_uuid: Uuid,
    look_name: &str,
) -> Result<&'a ProPresenterScreen, AudienceWorkspaceError> {
    let matches = screens
        .iter()
        .filter(|screen| {
            screen
                .uuid
                .as_ref()
                .and_then(|native| Uuid::parse_str(&native.string).ok())
                == Some(screen_uuid)
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [screen] => Ok(screen),
        [] => Err(AudienceWorkspaceError::DanglingScreen {
            look_name: look_name.to_string(),
            screen_uuid,
        }),
        [first, rest @ ..] => Err(AudienceWorkspaceError::DuplicateScreenUuid {
            uuid: screen_uuid,
            first_name: first.name.clone(),
            duplicate_name: rest[0].name.clone(),
        }),
    }
}

fn compile_presentation_destination(
    look: &ProAudienceLook,
    screen_name: &str,
    native: &rv_data::pro_audience_look::ProScreenLook,
    show_root: &Path,
    themes: &mut std::collections::HashMap<PathBuf, ThemeDocument>,
) -> Result<PresentationDestination, AudienceWorkspaceError> {
    match (
        native.template_document_file_path.as_ref(),
        native.template_slide_uuid.as_ref(),
    ) {
        (None, None) => Ok(PresentationDestination::SourcePresentation),
        (Some(document), Some(native_slide_uuid)) => {
            let locator = NativeFileLocator::from_url(document).ok_or_else(|| {
                AudienceWorkspaceError::InvalidThemeDocumentUrl {
                    look_name: look.name.clone(),
                    screen_name: screen_name.to_string(),
                }
            })?;
            let source_url = locator.source().to_string();
            let document_path = match locator.resolve(Some(show_root)) {
                NativeFileResolution::Available(path) => path,
                NativeFileResolution::Missing(path) => {
                    return Err(AudienceWorkspaceError::MissingThemeDocument {
                        look_name: look.name.clone(),
                        screen_name: screen_name.to_string(),
                        path,
                    });
                }
                NativeFileResolution::Unresolved => {
                    return Err(AudienceWorkspaceError::UnresolvedThemeDocument {
                        look_name: look.name.clone(),
                        screen_name: screen_name.to_string(),
                        source_url,
                    });
                }
            };
            let slide_uuid = parse_uuid(
                NativeIdentityKind::ThemeSlide,
                &native_slide_uuid.string,
                &format!("{} / {screen_name}", look.name),
            )?;
            let (template, document_sha256) = resolve_theme_template(
                themes,
                &document_path,
                slide_uuid,
                &look.name,
                screen_name,
            )?;
            let base_slide = template.native.base_slide.clone().ok_or_else(|| {
                AudienceWorkspaceError::MissingThemeBaseSlide {
                    look_name: look.name.clone(),
                    screen_name: screen_name.to_string(),
                    path: document_path.clone(),
                    slide_uuid,
                }
            })?;
            Ok(PresentationDestination::ThemeOverride(Box::new(
                ThemeDestination {
                    document_path,
                    document_sha256,
                    slide_uuid,
                    template: template.native,
                    base_slide,
                    template_bytes: template.bytes,
                },
            )))
        }
        (document, slide) => Err(AudienceWorkspaceError::IncompleteThemeOverride {
            look_name: look.name.clone(),
            screen_name: screen_name.to_string(),
            has_document: document.is_some(),
            has_slide: slide.is_some(),
        }),
    }
}

fn resolve_theme_template(
    themes: &mut std::collections::HashMap<PathBuf, ThemeDocument>,
    path: &Path,
    slide_uuid: Uuid,
    look_name: &str,
    screen_name: &str,
) -> Result<(ResolvedThemeTemplate, [u8; 32]), AudienceWorkspaceError> {
    let document = match themes.entry(path.to_path_buf()) {
        std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
        std::collections::hash_map::Entry::Vacant(entry) => {
            let data = std::fs::read(path).map_err(|source| {
                AudienceWorkspaceError::ReadThemeDocument {
                    path: path.to_path_buf(),
                    source,
                }
            })?;
            let document =
                rv_data::template::Document::decode(data.as_slice()).map_err(|source| {
                    AudienceWorkspaceError::DecodeThemeDocument {
                        path: path.to_path_buf(),
                        source,
                    }
                })?;
            let mut templates =
                std::collections::HashMap::<Uuid, Vec<ResolvedThemeTemplate>>::new();
            for template in document.slides {
                let Some(uuid) = template
                    .base_slide
                    .as_ref()
                    .and_then(|slide| slide.uuid.as_ref())
                    .and_then(|native| Uuid::parse_str(&native.string).ok())
                else {
                    continue;
                };
                templates
                    .entry(uuid)
                    .or_default()
                    .push(ResolvedThemeTemplate::new(template));
            }
            entry.insert(ThemeDocument {
                source_sha256: Sha256::digest(&data).into(),
                templates,
            })
        }
    };

    let matches = document
        .templates
        .get(&slide_uuid)
        .map(Vec::as_slice)
        .unwrap_or_default();
    match matches {
        [template] => Ok((template.clone(), document.source_sha256)),
        [] => Err(AudienceWorkspaceError::DanglingThemeSlide {
            look_name: look_name.to_string(),
            screen_name: screen_name.to_string(),
            path: path.to_path_buf(),
            slide_uuid,
        }),
        duplicates => Err(AudienceWorkspaceError::AmbiguousThemeSlide {
            look_name: look_name.to_string(),
            screen_name: screen_name.to_string(),
            path: path.to_path_buf(),
            slide_uuid,
            count: duplicates.len(),
        }),
    }
}

pub(super) fn parse_uuid(
    kind: NativeIdentityKind,
    value: &str,
    name: &str,
) -> Result<Uuid, InvalidNativeIdentity> {
    Uuid::parse_str(value).map_err(|_| InvalidNativeIdentity {
        kind,
        name: name.to_string(),
        value: value.to_string(),
    })
}
