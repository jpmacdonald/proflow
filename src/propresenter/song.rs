//! Song presentation structure repair.
//!
//! ProPresenter song files are arrangement-driven: operators expect a leading
//! background/title group, lyric groups in the middle, and a trailing blank
//! group. Older or imported files sometimes have cues but no arrangement, or a
//! single catch-all group. This module normalizes that structure without
//! touching lyric text.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use regex::Regex;
use uuid::Uuid;

use super::background;
use super::generated::rv_data::{self, action, presentation};
use super::macros::MacroCache;
use super::rtf::{extract_rtf_options, rtf_to_text, segments_to_rtf_bytes, StyledSegment};
use super::template::{clone_slide_with_text, replace_cue_slide_template_preserving_text};

/// Song presentation behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SongKind {
    /// Regular worship/contemporary song: blank background group starts the song.
    Worship,
    /// Hymn: title group starts the song, usually including a hymn number.
    Hymn,
    /// Anthem/choir/solo piece: title group starts the song with credits.
    Anthem,
}

/// Inputs for normalizing a song presentation.
pub struct SongStructureOptions<'a> {
    /// Song subtype.
    pub kind: SongKind,
    /// Optional background image to set on the first boundary cue.
    pub background_image: Option<&'a Path>,
    /// Macro to apply to worship background cues and first lyric cues.
    pub song_macro: Option<&'a str>,
    /// Macro to apply to hymn/anthem title cues.
    pub title_macro: Option<&'a str>,
    /// Macro lookup cache.
    pub macro_cache: Option<&'a MacroCache>,
    /// Optional title text for hymn/anthem boundary cues.
    pub title_text: Option<&'a str>,
    /// Projector-facing template for lyric/blank song cues.
    pub song_template: Option<&'a rv_data::PresentationSlide>,
    /// Projector-facing template for hymn/anthem title cues.
    pub title_template: Option<&'a rv_data::PresentationSlide>,
}

/// Result of a normalization pass.
#[derive(Debug, Clone, Default)]
pub struct SongStructureReport {
    /// Whether the presentation was changed.
    pub changed: bool,
    /// The arrangement UUID that should be used from playlist entries.
    pub arrangement_uuid: Option<Uuid>,
}

/// Infer a song kind from the PCO title, library path, and existing content.
#[must_use]
pub fn infer_song_kind(
    pco_title: &str,
    file_path: Option<&str>,
    presentation: Option<&rv_data::Presentation>,
) -> SongKind {
    let mut text = format!("{pco_title} ");
    if let Some(path) = file_path {
        text.push_str(path);
        text.push(' ');
    }
    if let Some(presentation) = presentation {
        text.push_str(&presentation.name);
        text.push(' ');
        for cue in &presentation.cues {
            text.push_str(&cue_text(cue));
            text.push(' ');
        }
    }

    let normalized = text.to_lowercase();
    if normalized.contains("anthem")
        || normalized.contains("choir")
        || normalized.contains("soloist")
        || normalized.contains("solo:")
        || normalized.contains("arr.")
        || normalized.contains("arranged")
    {
        return SongKind::Anthem;
    }

    if normalized.contains("[hymn]")
        || normalized.contains("hymn #")
        || leading_hymn_number(pco_title).is_some()
    {
        return SongKind::Hymn;
    }

    SongKind::Worship
}

/// Build operator-facing title text for hymn/anthem songs from PCO context.
#[must_use]
pub fn title_text_from_context(
    pco_title: &str,
    presentation_name: &str,
    kind: SongKind,
) -> Option<String> {
    match kind {
        SongKind::Worship => None,
        SongKind::Hymn => leading_hymn_number(pco_title)
            .map(|(number, title)| format!("{}\nHymn #{}", clean_song_title(&title), number))
            .or_else(|| {
                let title = clean_song_title(presentation_name.trim_start_matches("[Hymn]"));
                (!title.is_empty()).then_some(title)
            }),
        SongKind::Anthem => {
            let after_role = pco_title
                .split_once(':')
                .map_or(pco_title, |(_, rest)| rest)
                .trim();
            let (title, credit) = after_role
                .rsplit_once(',')
                .map_or((after_role, ""), |(title, credit)| {
                    (title.trim(), credit.trim())
                });
            let title = if title.is_empty() {
                presentation_name
            } else {
                title
            };
            if credit.is_empty() {
                Some(clean_song_title(title))
            } else {
                Some(format!("{}\n{}", clean_song_title(title), credit))
            }
        }
    }
}

/// Normalize a song presentation to operator-friendly arrangement structure.
#[allow(clippy::too_many_lines)]
pub fn ensure_song_structure(
    presentation: &mut rv_data::Presentation,
    options: &SongStructureOptions<'_>,
) -> SongStructureReport {
    let mut report = SongStructureReport::default();
    if presentation.cues.is_empty() {
        return report;
    }

    let had_arrangements = !presentation.arrangements.is_empty();
    let mut ordered_cue_ids = ordered_cue_ids(presentation);
    if ordered_cue_ids.is_empty() {
        ordered_cue_ids = presentation
            .cues
            .iter()
            .filter_map(|cue| cue.uuid.as_ref().map(|uuid| uuid.string.clone()))
            .collect();
    }
    if ordered_cue_ids.is_empty() {
        return report;
    }

    let mut boundary = find_existing_boundary_groups(presentation);
    let desired_title_text = options.title_text.map_or_else(
        || title_text_for_song(presentation, options.kind),
        str::to_string,
    );

    if !had_arrangements && boundary.background_group_uuid.is_none() {
        let background_cue_id = match options.kind {
            SongKind::Worship => find_worship_background_cue(presentation, &ordered_cue_ids)
                .unwrap_or_else(|| {
                    let template_id = first_existing_cue_id(presentation, &ordered_cue_ids);
                    let cue_id = push_blank_cue(presentation, template_id.as_deref());
                    ordered_cue_ids.insert(0, cue_id.clone());
                    cue_id
                }),
            SongKind::Hymn | SongKind::Anthem => find_title_cue(presentation, &ordered_cue_ids)
                .unwrap_or_else(|| {
                    let template_id = first_existing_cue_id(presentation, &ordered_cue_ids);
                    let cue_id =
                        push_title_cue(presentation, template_id.as_deref(), &desired_title_text);
                    ordered_cue_ids.insert(0, cue_id.clone());
                    report.changed = true;
                    cue_id
                }),
        };
        let group_uuid = push_single_cue_group(presentation, "Background", &background_cue_id);
        boundary.background_group_uuid = Some(group_uuid);
        boundary.background_cue_id = Some(background_cue_id);
        report.changed = true;
    }

    if !had_arrangements && boundary.blank_group_uuid.is_none() {
        let blank_cue_id = find_blank_cue(presentation, &ordered_cue_ids).unwrap_or_else(|| {
            let template_id = boundary
                .background_cue_id
                .clone()
                .or_else(|| first_existing_cue_id(presentation, &ordered_cue_ids));
            let cue_id = push_blank_cue(presentation, template_id.as_deref());
            ordered_cue_ids.push(cue_id.clone());
            report.changed = true;
            cue_id
        });
        let group_uuid = push_single_cue_group(presentation, "Blank", &blank_cue_id);
        boundary.blank_group_uuid = Some(group_uuid);
        boundary.blank_cue_id = Some(blank_cue_id);
        report.changed = true;
    }

    if had_arrangements {
        ensure_selected_arrangement(presentation, &mut report);
    } else {
        let content_groups = content_group_uuids(presentation, &boundary);
        let content_groups = if content_groups.is_empty() {
            let content_ids = ordered_cue_ids
                .iter()
                .filter(|cue_id| Some(cue_id.as_str()) != boundary.background_cue_id.as_deref())
                .filter(|cue_id| Some(cue_id.as_str()) != boundary.blank_cue_id.as_deref())
                .filter(|cue_id| cue_by_uuid(presentation, cue_id).is_some())
                .cloned()
                .collect::<Vec<_>>();

            if content_ids.is_empty() {
                Vec::new()
            } else {
                vec![push_multi_cue_group(presentation, "Lyrics", &content_ids)]
            }
        } else {
            content_groups
        };

        let mut arrangement_group_ids = Vec::new();
        if let Some(group_uuid) = &boundary.background_group_uuid {
            arrangement_group_ids.push(group_uuid.clone());
        }
        arrangement_group_ids.extend(content_groups);
        if let Some(group_uuid) = &boundary.blank_group_uuid {
            arrangement_group_ids.push(group_uuid.clone());
        }

        if !arrangement_group_ids.is_empty() {
            let arrangement_uuid = Uuid::new_v4();
            presentation.arrangements.push(presentation::Arrangement {
                uuid: Some(rv_data::Uuid {
                    string: arrangement_uuid.to_string(),
                }),
                name: "Default".to_string(),
                group_identifiers: arrangement_group_ids
                    .iter()
                    .map(|uuid| rv_data::Uuid {
                        string: uuid.clone(),
                    })
                    .collect(),
            });
            presentation.selected_arrangement = Some(rv_data::Uuid {
                string: arrangement_uuid.to_string(),
            });
            report.arrangement_uuid = Some(arrangement_uuid);
            report.changed = true;
        }
    }

    apply_song_boundary_style(presentation, options, &boundary, &mut report);
    if matches!(options.kind, SongKind::Hymn | SongKind::Anthem) {
        apply_song_title_text(presentation, &boundary, &desired_title_text, &mut report);
    }

    if report.arrangement_uuid.is_none() {
        report.arrangement_uuid = selected_or_default_arrangement(presentation)
            .and_then(|arrangement| arrangement.uuid.as_ref())
            .and_then(|uuid| Uuid::parse_str(&uuid.string).ok());
    }

    report
}

#[derive(Debug, Clone, Default)]
struct BoundaryGroups {
    background_group_uuid: Option<String>,
    background_cue_id: Option<String>,
    blank_group_uuid: Option<String>,
    blank_cue_id: Option<String>,
}

fn find_existing_boundary_groups(presentation: &rv_data::Presentation) -> BoundaryGroups {
    let mut boundary = BoundaryGroups::default();
    for group in &presentation.cue_groups {
        let Some(group_data) = &group.group else {
            continue;
        };
        let Some(group_uuid) = &group_data.uuid else {
            continue;
        };
        let group_name = group_data.name.to_lowercase();
        if boundary.background_group_uuid.is_none()
            && matches!(group_name.as_str(), "background" | "title")
        {
            boundary.background_group_uuid = Some(group_uuid.string.clone());
            boundary.background_cue_id = group
                .cue_identifiers
                .first()
                .map(|uuid| uuid.string.clone());
        }
        if boundary.blank_group_uuid.is_none() && group_name == "blank" {
            boundary.blank_group_uuid = Some(group_uuid.string.clone());
            boundary.blank_cue_id = group
                .cue_identifiers
                .first()
                .map(|uuid| uuid.string.clone());
        }
    }
    boundary
}

fn ordered_cue_ids(presentation: &rv_data::Presentation) -> Vec<String> {
    if let Some(arrangement) = selected_or_default_arrangement(presentation) {
        let group_index = cue_group_index_by_uuid(presentation);
        let mut ids = Vec::new();
        for group_id in &arrangement.group_identifiers {
            if let Some(group) = group_index
                .get(group_id.string.as_str())
                .and_then(|idx| presentation.cue_groups.get(*idx))
            {
                ids.extend(group.cue_identifiers.iter().map(|uuid| uuid.string.clone()));
            }
        }
        if !ids.is_empty() {
            return ids;
        }
    }

    let mut ids = Vec::new();
    for group in &presentation.cue_groups {
        ids.extend(group.cue_identifiers.iter().map(|uuid| uuid.string.clone()));
    }
    ids
}

fn content_group_uuids(
    presentation: &rv_data::Presentation,
    boundary: &BoundaryGroups,
) -> Vec<String> {
    let excluded_group_ids = [
        boundary.background_group_uuid.as_deref(),
        boundary.blank_group_uuid.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect::<HashSet<_>>();

    let excluded_cue_ids = [
        boundary.background_cue_id.as_deref(),
        boundary.blank_cue_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect::<HashSet<_>>();

    presentation
        .cue_groups
        .iter()
        .filter_map(|group| {
            let group_uuid = group.group.as_ref()?.uuid.as_ref()?.string.as_str();
            if excluded_group_ids.contains(group_uuid) {
                return None;
            }
            if group.cue_identifiers.is_empty() {
                return None;
            }
            if group
                .cue_identifiers
                .iter()
                .any(|cue_id| excluded_cue_ids.contains(cue_id.string.as_str()))
            {
                return None;
            }
            Some(group_uuid.to_string())
        })
        .collect()
}

fn apply_song_boundary_style(
    presentation: &mut rv_data::Presentation,
    options: &SongStructureOptions<'_>,
    boundary: &BoundaryGroups,
    report: &mut SongStructureReport,
) {
    let cue_index = cue_index_by_uuid(presentation);
    let Some(background_cue_idx) = boundary
        .background_cue_id
        .as_deref()
        .and_then(|cue_id| cue_index.get(cue_id).copied())
    else {
        return;
    };

    if let Some(image_path) = options.background_image {
        if let Some(cue) = presentation.cues.get_mut(background_cue_idx) {
            if background::ensure_background_on_cue(cue, image_path) {
                report.changed = true;
            }
        }
    }

    match options.kind {
        SongKind::Worship => {
            if let (Some(template), Some(cue)) = (
                options.song_template,
                presentation.cues.get_mut(background_cue_idx),
            ) {
                if replace_cue_slide_template_preserving_text(cue, template) {
                    report.changed = true;
                }
            }
        }
        SongKind::Hymn | SongKind::Anthem => {
            if let (Some(template), Some(cue)) = (
                options.title_template,
                presentation.cues.get_mut(background_cue_idx),
            ) {
                if replace_cue_slide_template_preserving_text(cue, template) {
                    report.changed = true;
                }
            }
            if let Some(template) = options.song_template {
                for content_idx in content_cue_indexes(presentation, boundary) {
                    if let Some(cue) = presentation.cues.get_mut(content_idx) {
                        if replace_cue_slide_template_preserving_text(cue, template) {
                            report.changed = true;
                        }
                    }
                }
            }
        }
    }

    match options.kind {
        SongKind::Worship => remove_song_macros_after_background(presentation, boundary, report),
        SongKind::Hymn | SongKind::Anthem => {
            remove_song_macros_after_first_content(presentation, boundary, report);
        }
    }

    let Some(cache) = options.macro_cache else {
        return;
    };

    match options.kind {
        SongKind::Worship => {
            if let (Some(song_macro), Some(cue)) = (
                options.song_macro,
                presentation.cues.get_mut(background_cue_idx),
            ) {
                if super::macros::ensure_macro_prefix_on_cue(cue, "Song", song_macro, cache) {
                    report.changed = true;
                }
            }
        }
        SongKind::Hymn | SongKind::Anthem => {
            if let (Some(title_macro), Some(cue)) = (
                options.title_macro,
                presentation.cues.get_mut(background_cue_idx),
            ) {
                if super::macros::replace_macro_on_cue(cue, title_macro, cache) {
                    report.changed = true;
                }
            }
            if let Some((content_idx, song_macro)) =
                first_content_cue_index(presentation, boundary).zip(options.song_macro)
            {
                if let Some(cue) = presentation.cues.get_mut(content_idx) {
                    if super::macros::ensure_macro_prefix_on_cue(cue, "Song", song_macro, cache) {
                        report.changed = true;
                    }
                }
            }
        }
    }
}

fn remove_song_macros_after_background(
    presentation: &mut rv_data::Presentation,
    boundary: &BoundaryGroups,
    report: &mut SongStructureReport,
) {
    let mut indexes = content_cue_indexes(presentation, boundary);
    if let Some(blank_idx) = boundary
        .blank_cue_id
        .as_deref()
        .and_then(|cue_id| cue_index_by_uuid(presentation).get(cue_id).copied())
    {
        indexes.push(blank_idx);
    }
    remove_song_macros_from_indexes(presentation, indexes, report);
}

fn remove_song_macros_after_first_content(
    presentation: &mut rv_data::Presentation,
    boundary: &BoundaryGroups,
    report: &mut SongStructureReport,
) {
    let mut indexes = content_cue_indexes(presentation, boundary);
    if !indexes.is_empty() {
        indexes.remove(0);
    }
    if let Some(blank_idx) = boundary
        .blank_cue_id
        .as_deref()
        .and_then(|cue_id| cue_index_by_uuid(presentation).get(cue_id).copied())
    {
        indexes.push(blank_idx);
    }
    remove_song_macros_from_indexes(presentation, indexes, report);
}

fn remove_song_macros_from_indexes(
    presentation: &mut rv_data::Presentation,
    indexes: Vec<usize>,
    report: &mut SongStructureReport,
) {
    for idx in indexes {
        if let Some(cue) = presentation.cues.get_mut(idx) {
            if super::macros::remove_macro_prefix_actions(cue, "Song") {
                report.changed = true;
            }
        }
    }
}

fn apply_song_title_text(
    presentation: &mut rv_data::Presentation,
    boundary: &BoundaryGroups,
    title_text: &str,
    report: &mut SongStructureReport,
) {
    if title_text.trim().is_empty() {
        return;
    }
    let cue_index = cue_index_by_uuid(presentation);
    let Some(background_cue_idx) = boundary
        .background_cue_id
        .as_deref()
        .and_then(|cue_id| cue_index.get(cue_id).copied())
    else {
        return;
    };
    let Some(cue) = presentation.cues.get_mut(background_cue_idx) else {
        return;
    };
    let current_text = cue_text(cue);
    if !is_blank_text(&current_text) && !title_text_needs_rewrite(&current_text, title_text) {
        return;
    }
    if replace_cue_slide_text(cue, title_text) {
        report.changed = true;
    }
}

fn first_content_cue_index(
    presentation: &rv_data::Presentation,
    boundary: &BoundaryGroups,
) -> Option<usize> {
    content_cue_indexes(presentation, boundary)
        .into_iter()
        .next()
}

fn content_cue_indexes(
    presentation: &rv_data::Presentation,
    boundary: &BoundaryGroups,
) -> Vec<usize> {
    let excluded = [
        boundary.background_cue_id.as_deref(),
        boundary.blank_cue_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect::<HashSet<_>>();

    let cue_index = cue_index_by_uuid(presentation);
    let mut indexes = Vec::new();
    for cue_id in ordered_cue_ids(presentation) {
        if excluded.contains(cue_id.as_str()) {
            continue;
        }
        if let Some(idx) = cue_index.get(cue_id.as_str()) {
            indexes.push(*idx);
        }
    }
    indexes
}

fn find_worship_background_cue(
    presentation: &rv_data::Presentation,
    ordered_cue_ids: &[String],
) -> Option<String> {
    let cue_index = cue_index_by_uuid(presentation);
    ordered_cue_ids.iter().find_map(|cue_id| {
        let cue = presentation.cues.get(*cue_index.get(cue_id.as_str())?)?;
        let text = cue_text(cue);
        let has_background = cue
            .actions
            .iter()
            .any(background::is_background_media_action);
        let has_song_macro = super::macros::cue_has_macro_prefix(cue, "Song");
        (is_blank_text(&text) && (has_background || has_song_macro)).then(|| cue_id.clone())
    })
}

fn find_title_cue(
    presentation: &rv_data::Presentation,
    ordered_cue_ids: &[String],
) -> Option<String> {
    let cue_index = cue_index_by_uuid(presentation);
    let title = presentation.name.to_lowercase();
    ordered_cue_ids.iter().find_map(|cue_id| {
        let cue = presentation.cues.get(*cue_index.get(cue_id.as_str())?)?;
        let text = cue_text(cue).to_lowercase();
        let has_title_macro = super::macros::cue_has_macro_named(cue, "Name Tag/Title");
        let looks_like_title = !is_blank_text(&text)
            && (has_title_macro
                || text.contains("hymn #")
                || text.contains("arr.")
                || text.contains(&title));
        looks_like_title.then(|| cue_id.clone())
    })
}

fn find_blank_cue(
    presentation: &rv_data::Presentation,
    ordered_cue_ids: &[String],
) -> Option<String> {
    let cue_index = cue_index_by_uuid(presentation);
    ordered_cue_ids.iter().rev().find_map(|cue_id| {
        let cue = presentation.cues.get(*cue_index.get(cue_id.as_str())?)?;
        is_blank_text(&cue_text(cue)).then(|| cue_id.clone())
    })
}

fn push_blank_cue(
    presentation: &mut rv_data::Presentation,
    template_cue_id: Option<&str>,
) -> String {
    push_cue_from_existing(
        presentation,
        template_cue_id,
        &[StyledSegment::unstyled("")],
    )
}

fn push_title_cue(
    presentation: &mut rv_data::Presentation,
    template_cue_id: Option<&str>,
    title: &str,
) -> String {
    push_cue_from_existing(
        presentation,
        template_cue_id,
        &[StyledSegment::unstyled(title)],
    )
}

fn push_cue_from_existing(
    presentation: &mut rv_data::Presentation,
    template_cue_id: Option<&str>,
    segments: &[StyledSegment],
) -> String {
    let template = template_cue_id
        .and_then(|cue_id| cue_by_uuid(presentation, cue_id))
        .and_then(extract_presentation_slide)
        .or_else(|| {
            presentation
                .cues
                .iter()
                .find_map(extract_presentation_slide)
        });

    let cue_uuid = Uuid::new_v4();
    let actions = template.map_or_else(Vec::new, |template| {
        vec![rv_data::Action {
            uuid: Some(rv_data::Uuid {
                string: Uuid::new_v4().to_string(),
            }),
            name: String::new(),
            label: None,
            delay_time: 0.0,
            old_type: None,
            is_enabled: true,
            layer_identification: None,
            duration: 0.0,
            r#type: action::ActionType::PresentationSlide as i32,
            action_type_data: Some(action::ActionTypeData::Slide(action::SlideType {
                slide: Some(action::slide_type::Slide::Presentation(
                    clone_slide_with_text(&template, segments),
                )),
            })),
        }]
    });

    presentation.cues.push(rv_data::Cue {
        uuid: Some(rv_data::Uuid {
            string: cue_uuid.to_string(),
        }),
        name: String::new(),
        actions,
        completion_target_type: rv_data::cue::CompletionTargetType::None as i32,
        completion_target_uuid: None,
        completion_action_type: rv_data::cue::CompletionActionType::Last as i32,
        completion_action_uuid: None,
        trigger_time: None,
        hot_key: Some(rv_data::HotKey {
            code: 0,
            control_identifier: String::new(),
        }),
        pending_imports: Vec::new(),
        is_enabled: true,
        completion_time: 0.0,
    });

    cue_uuid.to_string()
}

fn replace_cue_slide_text(cue: &mut rv_data::Cue, text: &str) -> bool {
    for action in &mut cue.actions {
        let Some(action::ActionTypeData::Slide(slide_type)) = &mut action.action_type_data else {
            continue;
        };
        let Some(action::slide_type::Slide::Presentation(slide)) = &mut slide_type.slide else {
            continue;
        };
        let mut wrote_primary = false;
        if let Some(base_slide) = &mut slide.base_slide {
            for slide_element in &mut base_slide.elements {
                let Some(graphics_element) = &mut slide_element.element else {
                    continue;
                };
                let Some(graphics_text) = &mut graphics_element.text else {
                    continue;
                };
                let rtf_options = extract_rtf_options(&graphics_text.rtf_data).unwrap_or_default();
                let segments = if wrote_primary {
                    vec![StyledSegment::unstyled("")]
                } else {
                    wrote_primary = true;
                    vec![StyledSegment::unstyled(text)]
                };
                graphics_text.rtf_data = segments_to_rtf_bytes(&segments, &rtf_options);
            }
            base_slide.uuid = Some(rv_data::Uuid {
                string: Uuid::new_v4().to_string(),
            });
        }
        return true;
    }
    false
}

fn push_single_cue_group(
    presentation: &mut rv_data::Presentation,
    group_name: &str,
    cue_id: &str,
) -> String {
    push_multi_cue_group(presentation, group_name, &[cue_id.to_string()])
}

fn push_multi_cue_group(
    presentation: &mut rv_data::Presentation,
    group_name: &str,
    cue_ids: &[String],
) -> String {
    let group_uuid = Uuid::new_v4();
    presentation.cue_groups.push(presentation::CueGroup {
        group: Some(rv_data::Group {
            uuid: Some(rv_data::Uuid {
                string: group_uuid.to_string(),
            }),
            name: group_name.to_string(),
            color: None,
            hot_key: None,
            application_group_identifier: None,
            application_group_name: String::new(),
        }),
        cue_identifiers: cue_ids
            .iter()
            .map(|cue_id| rv_data::Uuid {
                string: cue_id.clone(),
            })
            .collect(),
    });
    group_uuid.to_string()
}

fn selected_or_default_arrangement(
    presentation: &rv_data::Presentation,
) -> Option<&presentation::Arrangement> {
    if let Some(selected) = &presentation.selected_arrangement {
        if let Some(arrangement) = presentation.arrangements.iter().find(|arrangement| {
            arrangement
                .uuid
                .as_ref()
                .is_some_and(|uuid| uuid.string == selected.string)
        }) {
            return Some(arrangement);
        }
    }

    presentation
        .arrangements
        .iter()
        .find(|arrangement| arrangement.name.eq_ignore_ascii_case("Default"))
        .or_else(|| presentation.arrangements.first())
}

fn ensure_selected_arrangement(
    presentation: &mut rv_data::Presentation,
    report: &mut SongStructureReport,
) {
    let Some(arrangement) = selected_or_default_arrangement(presentation) else {
        return;
    };
    let arrangement_uuid = arrangement.uuid.clone();
    report.arrangement_uuid = arrangement_uuid
        .as_ref()
        .and_then(|uuid| Uuid::parse_str(&uuid.string).ok());

    if presentation.selected_arrangement.is_none() {
        presentation.selected_arrangement = arrangement_uuid;
        report.changed = true;
    }
}

fn cue_group_index_by_uuid(presentation: &rv_data::Presentation) -> HashMap<String, usize> {
    let mut map = HashMap::new();
    for (idx, cue_group) in presentation.cue_groups.iter().enumerate() {
        let Some(group) = &cue_group.group else {
            continue;
        };
        let Some(uuid) = &group.uuid else {
            continue;
        };
        map.insert(uuid.string.clone(), idx);
    }
    map
}

fn cue_index_by_uuid(presentation: &rv_data::Presentation) -> HashMap<String, usize> {
    let mut map = HashMap::new();
    for (idx, cue) in presentation.cues.iter().enumerate() {
        if let Some(uuid) = &cue.uuid {
            map.insert(uuid.string.clone(), idx);
        }
    }
    map
}

fn cue_by_uuid<'a>(
    presentation: &'a rv_data::Presentation,
    cue_id: &str,
) -> Option<&'a rv_data::Cue> {
    presentation
        .cues
        .iter()
        .find(|cue| cue.uuid.as_ref().is_some_and(|uuid| uuid.string == cue_id))
}

fn first_existing_cue_id(
    presentation: &rv_data::Presentation,
    ordered_cue_ids: &[String],
) -> Option<String> {
    let cue_index = cue_index_by_uuid(presentation);
    ordered_cue_ids
        .iter()
        .find(|cue_id| cue_index.contains_key(cue_id.as_str()))
        .cloned()
}

fn extract_presentation_slide(cue: &rv_data::Cue) -> Option<rv_data::PresentationSlide> {
    cue.actions.iter().find_map(|action| {
        let Some(action::ActionTypeData::Slide(slide_type)) = &action.action_type_data else {
            return None;
        };
        let Some(action::slide_type::Slide::Presentation(slide)) = &slide_type.slide else {
            return None;
        };
        Some(slide.clone())
    })
}

fn cue_text(cue: &rv_data::Cue) -> String {
    let mut texts = Vec::new();
    for action in &cue.actions {
        let Some(action::ActionTypeData::Slide(slide_type)) = &action.action_type_data else {
            continue;
        };
        let Some(action::slide_type::Slide::Presentation(slide)) = &slide_type.slide else {
            continue;
        };
        let Some(base_slide) = &slide.base_slide else {
            continue;
        };
        for element in &base_slide.elements {
            let Some(graphics) = &element.element else {
                continue;
            };
            let Some(text) = &graphics.text else {
                continue;
            };
            let rtf = String::from_utf8_lossy(&text.rtf_data);
            if let Some(text) = rtf_to_text(&rtf) {
                texts.push(text);
            }
        }
    }
    texts.join("\n")
}

fn is_blank_text(text: &str) -> bool {
    !text.chars().any(char::is_alphanumeric)
}

fn title_text_needs_rewrite(current_text: &str, title_text: &str) -> bool {
    let Some(first_line) = title_text.lines().find(|line| !line.trim().is_empty()) else {
        return false;
    };
    current_text.match_indices(first_line).nth(1).is_some()
}

fn title_text_for_song(presentation: &rv_data::Presentation, kind: SongKind) -> String {
    match kind {
        SongKind::Hymn => {
            if let Some((number, title)) = leading_hymn_number(&presentation.name) {
                format!("{title}\nHymn #{number}")
            } else {
                presentation.name.trim_start_matches("[Hymn] ").to_string()
            }
        }
        SongKind::Anthem | SongKind::Worship => presentation.name.clone(),
    }
}

fn leading_hymn_number(text: &str) -> Option<(String, String)> {
    let re = Regex::new(r"^\s*#?(\d{1,4})\s+(.+)$").ok()?;
    let captures = re.captures(text)?;
    let number = captures.get(1)?.as_str().to_string();
    let title = captures.get(2)?.as_str().trim().to_string();
    Some((number, title))
}

fn clean_song_title(title: &str) -> String {
    title.trim().trim_start_matches("[Hymn]").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(id: &str) -> rv_data::Cue {
        rv_data::Cue {
            uuid: Some(rv_data::Uuid {
                string: id.to_string(),
            }),
            actions: Vec::new(),
            ..rv_data::Cue::default()
        }
    }

    fn group(name: &str, id: &str, cue_ids: &[&str]) -> presentation::CueGroup {
        presentation::CueGroup {
            group: Some(rv_data::Group {
                uuid: Some(rv_data::Uuid {
                    string: id.to_string(),
                }),
                name: name.to_string(),
                ..rv_data::Group::default()
            }),
            cue_identifiers: cue_ids
                .iter()
                .map(|cue_id| rv_data::Uuid {
                    string: (*cue_id).to_string(),
                })
                .collect(),
        }
    }

    #[test]
    fn creates_default_arrangement_for_existing_split_song_groups() {
        let mut presentation = rv_data::Presentation {
            name: "We Walk by Faith and Not by Sight".to_string(),
            cues: vec![cue("title"), cue("v1"), cue("v2"), cue("blank")],
            cue_groups: vec![
                group("Background", "g-title", &["title"]),
                group("Verse 1", "g-v1", &["v1"]),
                group("Verse 2", "g-v2", &["v2"]),
                group("Blank", "g-blank", &["blank"]),
            ],
            ..rv_data::Presentation::default()
        };

        let report = ensure_song_structure(
            &mut presentation,
            &SongStructureOptions {
                kind: SongKind::Hymn,
                background_image: None,
                song_macro: None,
                title_macro: None,
                macro_cache: None,
                title_text: None,
                song_template: None,
                title_template: None,
            },
        );

        assert!(report.changed);
        assert_eq!(presentation.arrangements.len(), 1);
        let groups = &presentation.arrangements[0].group_identifiers;
        assert_eq!(groups.len(), 4);
        assert_eq!(groups[0].string, "g-title");
        assert_eq!(groups[3].string, "g-blank");
        assert!(presentation.selected_arrangement.is_some());
    }

    #[test]
    fn splits_single_group_song_into_boundary_arrangement() {
        let mut presentation = rv_data::Presentation {
            name: "I Am Not My Own - Courtney".to_string(),
            cues: vec![cue("title"), cue("v1"), cue("v2"), cue("blank")],
            cue_groups: vec![group("", "g-all", &["title", "v1", "v2", "blank"])],
            ..rv_data::Presentation::default()
        };

        let report = ensure_song_structure(
            &mut presentation,
            &SongStructureOptions {
                kind: SongKind::Anthem,
                background_image: None,
                song_macro: None,
                title_macro: None,
                macro_cache: None,
                title_text: None,
                song_template: None,
                title_template: None,
            },
        );

        assert!(report.changed);
        assert_eq!(presentation.arrangements.len(), 1);
        let arrangement_groups = &presentation.arrangements[0].group_identifiers;
        assert_eq!(arrangement_groups.len(), 3);
        let names = arrangement_groups
            .iter()
            .filter_map(|group_id| {
                presentation
                    .cue_groups
                    .iter()
                    .find(|group| {
                        group
                            .group
                            .as_ref()
                            .and_then(|group| group.uuid.as_ref())
                            .is_some_and(|uuid| uuid.string == group_id.string)
                    })
                    .and_then(|group| group.group.as_ref())
                    .map(|group| group.name.as_str())
            })
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["Background", "Lyrics", "Blank"]);
    }

    #[test]
    fn keeps_existing_arrangement_sequence_intact() {
        let mut presentation = rv_data::Presentation {
            name: "May The Peoples Praise You".to_string(),
            selected_arrangement: Some(rv_data::Uuid {
                string: "arr".to_string(),
            }),
            arrangements: vec![presentation::Arrangement {
                uuid: Some(rv_data::Uuid {
                    string: "arr".to_string(),
                }),
                name: "Default".to_string(),
                group_identifiers: vec!["g-bg", "g-v1", "g-chorus", "g-blank", "g-v2", "g-chorus"]
                    .into_iter()
                    .map(|id| rv_data::Uuid {
                        string: id.to_string(),
                    })
                    .collect(),
            }],
            cues: vec![cue("bg"), cue("v1"), cue("chorus"), cue("blank"), cue("v2")],
            cue_groups: vec![
                group("Background", "g-bg", &["bg"]),
                group("Verse 1", "g-v1", &["v1"]),
                group("Chorus", "g-chorus", &["chorus"]),
                group("Blank", "g-blank", &["blank"]),
                group("Verse 2", "g-v2", &["v2"]),
            ],
            ..rv_data::Presentation::default()
        };
        let before = presentation.arrangements[0].group_identifiers.clone();

        let report = ensure_song_structure(
            &mut presentation,
            &SongStructureOptions {
                kind: SongKind::Worship,
                background_image: None,
                song_macro: None,
                title_macro: None,
                macro_cache: None,
                title_text: None,
                song_template: None,
                title_template: None,
            },
        );

        assert!(!report.changed);
        assert_eq!(presentation.arrangements[0].group_identifiers, before);
    }

    #[test]
    fn worship_song_keeps_song_macro_only_on_background_cue() {
        let mut presentation = rv_data::Presentation {
            name: "This Is Amazing Grace".to_string(),
            selected_arrangement: Some(rv_data::Uuid {
                string: "arr".to_string(),
            }),
            arrangements: vec![presentation::Arrangement {
                uuid: Some(rv_data::Uuid {
                    string: "arr".to_string(),
                }),
                name: "Default".to_string(),
                group_identifiers: vec!["g-bg", "g-v1", "g-v2", "g-blank"]
                    .into_iter()
                    .map(|id| rv_data::Uuid {
                        string: id.to_string(),
                    })
                    .collect(),
            }],
            cues: vec![cue("bg"), cue("v1"), cue("v2"), cue("blank")],
            cue_groups: vec![
                group("Background", "g-bg", &["bg"]),
                group("Verse 1", "g-v1", &["v1"]),
                group("Verse 2", "g-v2", &["v2"]),
                group("Blank", "g-blank", &["blank"]),
            ],
            ..rv_data::Presentation::default()
        };
        presentation.cues[0]
            .actions
            .push(super::super::macros::make_macro_action(
                "Song",
                "00000000-0000-0000-0000-000000000001",
            ));
        presentation.cues[1]
            .actions
            .push(super::super::macros::make_macro_action(
                "Song",
                "00000000-0000-0000-0000-000000000001",
            ));
        presentation.cues[2]
            .actions
            .push(super::super::macros::make_macro_action(
                "Song",
                "00000000-0000-0000-0000-000000000001",
            ));
        presentation.cues[3]
            .actions
            .push(super::super::macros::make_macro_action(
                "Song",
                "00000000-0000-0000-0000-000000000001",
            ));

        let report = ensure_song_structure(
            &mut presentation,
            &SongStructureOptions {
                kind: SongKind::Worship,
                background_image: None,
                song_macro: None,
                title_macro: None,
                macro_cache: None,
                title_text: None,
                song_template: None,
                title_template: None,
            },
        );

        assert!(report.changed);
        assert!(super::super::macros::cue_has_macro_prefix(
            &presentation.cues[0],
            "Song"
        ));
        assert!(!super::super::macros::cue_has_macro_prefix(
            &presentation.cues[1],
            "Song"
        ));
        assert!(!super::super::macros::cue_has_macro_prefix(
            &presentation.cues[2],
            "Song"
        ));
        assert!(!super::super::macros::cue_has_macro_prefix(
            &presentation.cues[3],
            "Song"
        ));
    }

    #[test]
    fn titled_song_keeps_song_macro_only_on_first_content_cue() {
        let mut presentation = rv_data::Presentation {
            name: "[Hymn] Praise Ye the Lord".to_string(),
            selected_arrangement: Some(rv_data::Uuid {
                string: "arr".to_string(),
            }),
            arrangements: vec![presentation::Arrangement {
                uuid: Some(rv_data::Uuid {
                    string: "arr".to_string(),
                }),
                name: "Default".to_string(),
                group_identifiers: vec!["g-title", "g-v1", "g-v2", "g-blank"]
                    .into_iter()
                    .map(|id| rv_data::Uuid {
                        string: id.to_string(),
                    })
                    .collect(),
            }],
            cues: vec![cue("title"), cue("v1"), cue("v2"), cue("blank")],
            cue_groups: vec![
                group("Title", "g-title", &["title"]),
                group("Verse 1", "g-v1", &["v1"]),
                group("Verse 2", "g-v2", &["v2"]),
                group("Blank", "g-blank", &["blank"]),
            ],
            ..rv_data::Presentation::default()
        };
        presentation.cues[1]
            .actions
            .push(super::super::macros::make_macro_action(
                "Song",
                "00000000-0000-0000-0000-000000000001",
            ));
        presentation.cues[2]
            .actions
            .push(super::super::macros::make_macro_action(
                "Song",
                "00000000-0000-0000-0000-000000000001",
            ));
        presentation.cues[3]
            .actions
            .push(super::super::macros::make_macro_action(
                "Song",
                "00000000-0000-0000-0000-000000000001",
            ));

        let report = ensure_song_structure(
            &mut presentation,
            &SongStructureOptions {
                kind: SongKind::Hymn,
                background_image: None,
                song_macro: None,
                title_macro: None,
                macro_cache: None,
                title_text: None,
                song_template: None,
                title_template: None,
            },
        );

        assert!(report.changed);
        assert!(super::super::macros::cue_has_macro_prefix(
            &presentation.cues[1],
            "Song"
        ));
        assert!(!super::super::macros::cue_has_macro_prefix(
            &presentation.cues[2],
            "Song"
        ));
        assert!(!super::super::macros::cue_has_macro_prefix(
            &presentation.cues[3],
            "Song"
        ));
    }
}
