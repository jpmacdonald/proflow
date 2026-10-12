//! Classification for generated scripture presentation types.

use crate::bible::BibleVersion;
use crate::planning_center::types::Item;
use crate::project_config::PresentationTypeKey;
use crate::project_config::ProjectConfig;

use super::expansion::{resolve_speaker, SpeakerResolution};
use crate::workflow::classify_matching::strip_speaker;
use crate::workflow::plan::{
    ItemKind, OutputKey, PlanDisposition, ReadyAction, RenderStyle, ResolvedItemPlan,
    ReviewContext, ScriptureContent, ScriptureRefInfo,
};
use crate::workflow::scripture::{
    parse_prefix_scripture_ref, parse_scripture_refs, validate_translation_declarations,
    ParsedScriptureRef, ScriptureRefsError,
};

#[allow(clippy::too_many_lines)]
pub(super) fn build_scripture_plan(
    output_key: OutputKey,
    type_key: &PresentationTypeKey,
    item: &Item,
    style: RenderStyle,
    config: &ProjectConfig,
) -> ResolvedItemPlan {
    let speaker = item.title.match_indices(')').find_map(|(end, _)| {
        match resolve_speaker(&item.title[..=end], None, config) {
            SpeakerResolution::Resolved(name) => Some(name),
            SpeakerResolution::Missing | SpeakerResolution::Unrecognized => None,
        }
    });
    let known_speaker = speaker.as_deref();
    let configured_default = config.defaults().bible_version;
    let parsed_refs = match scripture_source(item, configured_default, known_speaker)
        .and_then(|(source, default)| parse_scripture_refs(source, default, known_speaker))
    {
        Ok(references) => references,
        Err(error) => {
            if matches!(error, ScriptureRefsError::PartialVerse(_)) {
                if let Some(plan) = build_prefix_excerpt_plan(
                    output_key.clone(),
                    type_key,
                    item,
                    style,
                    configured_default,
                    known_speaker,
                ) {
                    return plan;
                }
            }
            return ResolvedItemPlan::new(
                output_key,
                item.position,
                item.title.clone(),
                strip_speaker(&item.title),
                error.to_string(),
                ItemKind::Scripture,
                Some(type_key.clone()),
                PlanDisposition::NeedsReview(ReviewContext::new(None)),
            );
        }
    };

    if parsed_refs.len() > 1 {
        let ref_infos = parsed_refs
            .iter()
            .map(|reference| ScriptureRefInfo::new(reference.reference.clone(), reference.version))
            .collect::<Result<Vec<_>, _>>();
        let ref_infos = match ref_infos {
            Ok(ref_infos) => ref_infos,
            Err(error) => {
                return ResolvedItemPlan::new(
                    output_key,
                    item.position,
                    item.title.clone(),
                    strip_speaker(&item.title),
                    error.to_string(),
                    ItemKind::Scripture,
                    Some(type_key.clone()),
                    PlanDisposition::NeedsReview(ReviewContext::new(None)),
                );
            }
        };
        let first_version = ref_infos[0].version();
        let same_version = ref_infos
            .iter()
            .all(|reference| reference.version() == first_version);
        let combined_name = if same_version {
            format!(
                "{} {}",
                ref_infos
                    .iter()
                    .map(|reference| reference.reference().replace(':', "v"))
                    .collect::<Vec<_>>()
                    .join(", "),
                first_version.name()
            )
        } else {
            ref_infos
                .iter()
                .map(|reference| {
                    format!(
                        "{} {}",
                        reference.reference().replace(':', "v"),
                        reference.version().name()
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        let version_summary = if same_version {
            first_version.name().to_string()
        } else {
            "mixed versions".to_string()
        };
        let reference_count = ref_infos.len();
        let Some(scripture) = ScriptureContent::combined(ref_infos) else {
            return ResolvedItemPlan::new(
                output_key,
                item.position,
                item.title.clone(),
                combined_name,
                "Combined scripture source requires at least two references".to_string(),
                ItemKind::Scripture,
                Some(type_key.clone()),
                PlanDisposition::NeedsReview(ReviewContext::new(None)),
            );
        };

        return ResolvedItemPlan::new(
            output_key,
            item.position,
            item.title.clone(),
            combined_name,
            format!(
                "Generate combined scripture slides ({reference_count} refs, {version_summary})"
            ),
            ItemKind::Scripture,
            Some(type_key.clone()),
            PlanDisposition::Ready(ReadyAction::GenerateScripture { scripture, style }),
        );
    }

    let parsed_ref = &parsed_refs[0];
    if names_selected_verses(&item.title) {
        return build_selected_excerpt_plan(output_key, type_key, item, style, parsed_ref);
    }
    let scripture = match ScriptureContent::single(parsed_ref.reference.clone(), parsed_ref.version)
    {
        Ok(scripture) => scripture,
        Err(error) => {
            return ResolvedItemPlan::new(
                output_key,
                item.position,
                item.title.clone(),
                strip_speaker(&item.title),
                error.to_string(),
                ItemKind::Scripture,
                Some(type_key.clone()),
                PlanDisposition::NeedsReview(ReviewContext::new(None)),
            );
        }
    };

    ResolvedItemPlan::new(
        output_key,
        item.position,
        item.title.clone(),
        format!("{} {}", parsed_ref.reference, parsed_ref.version.name()),
        format!("Generate scripture slides ({})", parsed_ref.version.name()),
        ItemKind::Scripture,
        Some(type_key.clone()),
        PlanDisposition::Ready(ReadyAction::GenerateScripture { scripture, style }),
    )
}

fn build_selected_excerpt_plan(
    output_key: OutputKey,
    type_key: &PresentationTypeKey,
    item: &Item,
    style: RenderStyle,
    parsed: &ParsedScriptureRef,
) -> ResolvedItemPlan {
    let excerpt_text = item
        .scripture
        .as_ref()
        .and_then(|scripture| scripture.text.as_deref())
        .or(item.description.as_deref())
        .map(str::trim)
        .filter(|text| !text.is_empty());
    let Some(excerpt_text) = excerpt_text else {
        return ResolvedItemPlan::new(
            output_key,
            item.position,
            item.title.clone(),
            format!("{} {}", parsed.reference, parsed.version.name()),
            "Selected-verse scripture requires numbered Planning Center description text"
                .to_string(),
            ItemKind::Scripture,
            Some(type_key.clone()),
            PlanDisposition::NeedsReview(ReviewContext::new(None)),
        );
    };
    let scripture = match ScriptureContent::selected_excerpt(
        parsed.reference.clone(),
        parsed.version,
        excerpt_text.to_string(),
    ) {
        Ok(scripture) => scripture,
        Err(error) => {
            return ResolvedItemPlan::new(
                output_key,
                item.position,
                item.title.clone(),
                format!("{} {}", parsed.reference, parsed.version.name()),
                error.to_string(),
                ItemKind::Scripture,
                Some(type_key.clone()),
                PlanDisposition::NeedsReview(ReviewContext::new(None)),
            );
        }
    };
    let action = ReadyAction::GenerateScripture { scripture, style };
    ResolvedItemPlan::new(
        output_key,
        item.position,
        item.title.clone(),
        format!("{} {}", parsed.reference, parsed.version.name()),
        "Validate Planning Center selected-verse text against the local Bible corpus".to_string(),
        ItemKind::Scripture,
        Some(type_key.clone()),
        PlanDisposition::NeedsReview(ReviewContext::new(Some(action))),
    )
}

fn names_selected_verses(title: &str) -> bool {
    let title = title.to_ascii_lowercase();
    title.contains("selected vv") || title.contains("selected verses")
}

fn build_prefix_excerpt_plan(
    output_key: OutputKey,
    type_key: &PresentationTypeKey,
    item: &Item,
    style: RenderStyle,
    configured_default: Option<BibleVersion>,
    known_speaker: Option<&str>,
) -> Option<ResolvedItemPlan> {
    let (source, default) = scripture_source(item, configured_default, known_speaker).ok()?;
    let parsed = parse_prefix_scripture_ref(source, default, known_speaker).ok()?;
    let excerpt_text = item
        .scripture
        .as_ref()
        .and_then(|scripture| scripture.text.as_deref())
        .or(item.description.as_deref())?
        .trim();
    let scripture = ScriptureContent::prefix_excerpt(
        parsed.reference,
        parsed.display_reference.clone(),
        parsed.version,
        excerpt_text.to_string(),
    )
    .ok()?;
    let action = ReadyAction::GenerateScripture { scripture, style };
    Some(ResolvedItemPlan::new(
        output_key,
        item.position,
        item.title.clone(),
        format!("{} {}", parsed.display_reference, parsed.version.name()),
        "Validate Planning Center partial-verse text against the local Bible corpus".to_string(),
        ItemKind::Scripture,
        Some(type_key.clone()),
        PlanDisposition::NeedsReview(ReviewContext::new(Some(action))),
    ))
}

/// Structured translations are explicit source declarations, never defaults.
/// A title declaration must agree before either whole or partial references can
/// become proposals. Without a structured translation, the title remains the
/// authority: source normalization derives that reference from the title and
/// may retain only its first passage.
fn scripture_source<'a>(
    item: &'a Item,
    configured_default: Option<BibleVersion>,
    known_speaker: Option<&str>,
) -> Result<(&'a str, Option<BibleVersion>), ScriptureRefsError> {
    let Some(structured) = item.scripture.as_ref() else {
        return Ok((&item.title, configured_default));
    };
    let Some(translation) = structured.translation.as_deref() else {
        return Ok((&item.title, configured_default));
    };
    let translation = translation.trim();
    let version = BibleVersion::from_name(translation)
        .ok_or_else(|| ScriptureRefsError::UnsupportedVersion(translation.to_string()))?;
    validate_translation_declarations(&item.title, version, known_speaker)?;
    let reference = structured.reference.trim();
    if reference.is_empty() {
        return Err(ScriptureRefsError::Missing);
    }
    validate_translation_declarations(reference, version, known_speaker)?;
    Ok((reference, Some(version)))
}
