//! Classification for checked description-backed and static policies.

use super::file_stem;
use crate::planning_center::types::Item;
use crate::project_config::{DescriptionParserKind, ItemKind};
use crate::propresenter::library::LibraryCatalog;
use crate::workflow::classify_matching::strip_speaker;
use crate::workflow::description_parser::{self, ParsedContent};
use crate::workflow::library_search::{resolve_exact_library_file, ExactLibraryFileMatch};
use crate::workflow::plan::{
    ExistingTransform, OutputKey, PlanDisposition, ReadyAction, RenderStyle, ResolvedItemPlan,
    ReviewContext,
};

pub(super) enum StaticPolicy {
    Review,
    PreserveExisting {
        arrangement: Option<String>,
    },
    RestyleExisting {
        arrangement: Option<String>,
        transform: ExistingTransform,
    },
}

pub(super) enum DescriptionPolicy {
    Review { render: Option<RenderStyle> },
    Edit { render: RenderStyle },
    Generate { render: RenderStyle },
}

pub(super) fn build_static_plan(
    output_key: OutputKey,
    type_key: &str,
    kind: ItemKind,
    policy: StaticPolicy,
    item: &Item,
    target_library_file: Option<&str>,
    file_index: Option<&LibraryCatalog>,
) -> ResolvedItemPlan {
    let target_match = target_library_file.map(|name| resolve_exact_library_file(file_index, name));
    let found = unique_path(target_match.as_ref());
    let (disposition, reason) = match policy {
        StaticPolicy::Review => (
            PlanDisposition::NeedsReview(ReviewContext::new(found.clone().map(|file_path| {
                ReadyAction::UseExisting {
                    file_path,
                    arrangement: None,
                }
            }))),
            "Configured to require review".to_string(),
        ),
        StaticPolicy::PreserveExisting { arrangement } => {
            resolve_existing_action(found.clone(), target_match.as_ref(), arrangement, None)
        }
        StaticPolicy::RestyleExisting {
            arrangement,
            transform,
        } => resolve_existing_action(
            found.clone(),
            target_match.as_ref(),
            arrangement,
            Some(transform),
        ),
    };
    finish_plan(
        output_key,
        type_key,
        kind,
        item,
        target_library_file,
        target_match.as_ref(),
        found.as_deref(),
        disposition,
        reason,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "the checked output identity and concrete description inputs remain explicit at this compiler boundary"
)]
pub(super) fn build_description_plan(
    output_key: OutputKey,
    type_key: &str,
    kind: ItemKind,
    parser: DescriptionParserKind,
    policy: DescriptionPolicy,
    item: &Item,
    target_library_file: Option<&str>,
    file_index: Option<&LibraryCatalog>,
) -> ResolvedItemPlan {
    let target_match = target_library_file.map(|name| resolve_exact_library_file(file_index, name));
    let found = unique_path(target_match.as_ref());
    let parse_result = match (parser, item.description.as_deref()) {
        (DescriptionParserKind::ContentNametag, description) => {
            Some(description_parser::parse_description(
                description.unwrap_or_default(),
                &item.title,
                parser,
            ))
        }
        (
            DescriptionParserKind::Liturgical | DescriptionParserKind::LiturgicalAudience,
            Some(description),
        ) => Some(description_parser::parse_description(
            description,
            &item.title,
            parser,
        )),
        (DescriptionParserKind::Liturgical | DescriptionParserKind::LiturgicalAudience, None) => {
            None
        }
    };
    let (parsed_content, parse_error) = match parse_result {
        Some(Ok(content)) => (content, None),
        Some(Err(error)) => (None, Some(error.to_string())),
        None => (None, None),
    };
    let target_ambiguous = matches!(target_match, Some(ExactLibraryFileMatch::Ambiguous));
    let (disposition, mut reason) = match policy {
        DescriptionPolicy::Review { render } => {
            configured_review_action(found.clone(), parsed_content, render)
        }
        DescriptionPolicy::Edit { render } => {
            edit_description_action(parsed_content, found.clone(), target_ambiguous, render)
        }
        DescriptionPolicy::Generate { render } => {
            generate_description_action(parsed_content, render)
        }
    };
    if matches!(disposition, PlanDisposition::NeedsReview(_)) {
        if let Some(error) = parse_error {
            reason = error;
        }
    }
    finish_plan(
        output_key,
        type_key,
        kind,
        item,
        target_library_file,
        target_match.as_ref(),
        found.as_deref(),
        disposition,
        reason,
    )
}

fn unique_path(target: Option<&ExactLibraryFileMatch>) -> Option<std::path::PathBuf> {
    match target {
        Some(ExactLibraryFileMatch::Unique(path)) => Some(path.into()),
        Some(ExactLibraryFileMatch::Missing | ExactLibraryFileMatch::Ambiguous) | None => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn finish_plan(
    output_key: OutputKey,
    type_key: &str,
    kind: ItemKind,
    item: &Item,
    target_library_file: Option<&str>,
    target_match: Option<&ExactLibraryFileMatch>,
    found: Option<&std::path::Path>,
    disposition: PlanDisposition,
    mut reason: String,
) -> ResolvedItemPlan {
    if let (Some(target), Some(ExactLibraryFileMatch::Missing | ExactLibraryFileMatch::Ambiguous)) =
        (target_library_file, target_match)
    {
        reason = format!("{reason}: {target}");
    }
    ResolvedItemPlan {
        output_key,
        position: item.position,
        pco_title: item.title.clone(),
        playlist_name: found.map_or_else(
            || target_library_file.map_or_else(|| strip_speaker(&item.title), file_stem),
            |path| file_stem(&path.display().to_string()),
        ),
        reason,
        item_kind: kind,
        item_type: Some(type_key.to_string()),
        disposition,
    }
}

fn configured_review_action(
    file_path: Option<std::path::PathBuf>,
    parsed_content: Option<ParsedContent>,
    render: Option<RenderStyle>,
) -> (PlanDisposition, String) {
    let proposed_action = match (file_path, parsed_content, render) {
        (Some(file_path), Some(parsed_content), Some(style)) => {
            Some(ReadyAction::EditDescription {
                file_path,
                parsed_content,
                style,
            })
        }
        (None, Some(parsed_content), Some(style)) => Some(ReadyAction::GenerateDescription {
            parsed_content,
            style,
        }),
        (Some(file_path), None, _) => Some(ReadyAction::UseExisting {
            file_path,
            arrangement: None,
        }),
        _ => None,
    };
    (
        PlanDisposition::NeedsReview(ReviewContext::new(proposed_action)),
        "Configured to require review".to_string(),
    )
}

fn resolve_existing_action(
    file_path: Option<std::path::PathBuf>,
    target_match: Option<&ExactLibraryFileMatch>,
    arrangement: Option<String>,
    restyle: Option<ExistingTransform>,
) -> (PlanDisposition, String) {
    if matches!(target_match, Some(ExactLibraryFileMatch::Ambiguous)) {
        (
            PlanDisposition::NeedsReview(ReviewContext::new(None)),
            "Configured existing file is ambiguous".to_string(),
        )
    } else if let Some(file_path) = file_path {
        let (action, reason) = match restyle {
            None => (
                ReadyAction::UseExisting {
                    file_path,
                    arrangement,
                },
                "Explicit graphic/media exemption".to_string(),
            ),
            Some(transform) => {
                let reason = transform.replacement_background().map_or_else(
                    || "Library match; applying existing presentation transform".to_string(),
                    |background| {
                        format!("Library match; applying background '{}'", background.id())
                    },
                );
                (
                    ReadyAction::RestyleExisting {
                        file_path,
                        arrangement,
                        transform,
                    },
                    reason,
                )
            }
        };
        (PlanDisposition::Ready(action), reason)
    } else {
        (
            PlanDisposition::NeedsReview(ReviewContext::new(None)),
            "Configured existing file not found".to_string(),
        )
    }
}

fn edit_description_action(
    parsed_content: Option<ParsedContent>,
    file_path: Option<std::path::PathBuf>,
    target_ambiguous: bool,
    style: RenderStyle,
) -> (PlanDisposition, String) {
    if parsed_content.is_none() {
        let proposed_action = file_path.map(|file_path| ReadyAction::UseExisting {
            file_path,
            arrangement: None,
        });
        return (
            PlanDisposition::NeedsReview(ReviewContext::new(proposed_action)),
            "No description content to edit".to_string(),
        );
    }
    if target_ambiguous {
        let proposed_action =
            parsed_content.map(|parsed_content| ReadyAction::GenerateDescription {
                parsed_content,
                style,
            });
        return (
            PlanDisposition::NeedsReview(ReviewContext::new(proposed_action)),
            "Edit-in-place target is ambiguous".to_string(),
        );
    }
    match (file_path, parsed_content) {
        (Some(file_path), Some(parsed_content)) => (
            PlanDisposition::Ready(ReadyAction::EditDescription {
                file_path,
                parsed_content,
                style,
            }),
            "Content updated from description".to_string(),
        ),
        (None, Some(parsed_content)) => (
            PlanDisposition::NeedsReview(ReviewContext::new(Some(
                ReadyAction::GenerateDescription {
                    parsed_content,
                    style,
                },
            ))),
            "Edit-in-place target not found".to_string(),
        ),
        (_, None) => (
            PlanDisposition::NeedsReview(ReviewContext::new(None)),
            "No description content to edit".to_string(),
        ),
    }
}

fn generate_description_action(
    parsed_content: Option<ParsedContent>,
    style: RenderStyle,
) -> (PlanDisposition, String) {
    parsed_content.map_or_else(
        || {
            (
                PlanDisposition::NeedsReview(ReviewContext::new(None)),
                "No description content to generate".to_string(),
            )
        },
        |parsed_content| {
            (
                PlanDisposition::Ready(ReadyAction::GenerateDescription {
                    parsed_content,
                    style,
                }),
                "Generate from description content".to_string(),
            )
        },
    )
}
