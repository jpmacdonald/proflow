//! Classification for generated scripture presentation types.

use crate::bible::BibleVersion;
use crate::planning_center::types::Item;
use crate::workflow::classify_matching::strip_speaker;
use crate::workflow::plan::{
    ItemKind, OutputKey, PlanDisposition, ReadyAction, RenderStyle, ResolvedItemPlan,
    ReviewContext, ScriptureContent, ScriptureRefInfo,
};
use crate::workflow::scripture::{
    parse_prefix_scripture_ref, parse_scripture_refs, ParsedScriptureRef,
};

#[allow(clippy::too_many_lines)]
pub(super) fn build_scripture_plan(
    output_key: OutputKey,
    type_key: &str,
    item: &Item,
    style: RenderStyle,
    configured_default: Option<BibleVersion>,
) -> ResolvedItemPlan {
    let parsed_refs = match item_scripture_refs(item, configured_default) {
        Ok(references) => references,
        Err(error) => {
            if let Some(plan) = build_prefix_excerpt_plan(
                output_key.clone(),
                type_key,
                item,
                style,
                configured_default,
            ) {
                return plan;
            }
            return ResolvedItemPlan::new(
                output_key,
                item.position,
                item.title.clone(),
                strip_speaker(&item.title),
                error,
                ItemKind::Scripture,
                Some(type_key.to_string()),
                PlanDisposition::NeedsReview(ReviewContext::new(None)),
            );
        }
    };

    if parsed_refs.len() > 1 {
        let ref_infos = parsed_refs
            .iter()
            .map(|reference| {
                ScriptureRefInfo::new(reference.reference.clone(), reference.version.clone())
            })
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
                    Some(type_key.to_string()),
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
                "{} {first_version}",
                ref_infos
                    .iter()
                    .map(|reference| reference.reference().replace(':', "v"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        } else {
            ref_infos
                .iter()
                .map(|reference| {
                    format!(
                        "{} {}",
                        reference.reference().replace(':', "v"),
                        reference.version()
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        let version_summary = if same_version {
            first_version.to_string()
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
                Some(type_key.to_string()),
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
            Some(type_key.to_string()),
            PlanDisposition::Ready(ReadyAction::GenerateScripture { scripture, style }),
        );
    }

    let parsed_ref = &parsed_refs[0];
    let scripture =
        match ScriptureContent::single(parsed_ref.reference.clone(), parsed_ref.version.clone()) {
            Ok(scripture) => scripture,
            Err(error) => {
                return ResolvedItemPlan::new(
                    output_key,
                    item.position,
                    item.title.clone(),
                    strip_speaker(&item.title),
                    error.to_string(),
                    ItemKind::Scripture,
                    Some(type_key.to_string()),
                    PlanDisposition::NeedsReview(ReviewContext::new(None)),
                );
            }
        };

    ResolvedItemPlan::new(
        output_key,
        item.position,
        item.title.clone(),
        format!("{} {}", parsed_ref.reference, parsed_ref.version),
        format!("Generate scripture slides ({})", parsed_ref.version),
        ItemKind::Scripture,
        Some(type_key.to_string()),
        PlanDisposition::Ready(ReadyAction::GenerateScripture { scripture, style }),
    )
}

fn build_prefix_excerpt_plan(
    output_key: OutputKey,
    type_key: &str,
    item: &Item,
    style: RenderStyle,
    configured_default: Option<BibleVersion>,
) -> Option<ResolvedItemPlan> {
    let structured_version = match item
        .scripture
        .as_ref()
        .and_then(|scripture| scripture.translation.as_deref())
    {
        Some(version) => Some(BibleVersion::from_name(version.trim())?),
        None => None,
    };
    let default_version = structured_version.or(configured_default);
    let parsed = parse_prefix_scripture_ref(&item.title, default_version)
        .ok()
        .or_else(|| {
            item.scripture.as_ref().and_then(|structured| {
                parse_prefix_scripture_ref(&structured.reference, default_version).ok()
            })
        })?;
    let excerpt_text = item
        .scripture
        .as_ref()
        .and_then(|scripture| scripture.text.as_deref())
        .or(item.description.as_deref())?
        .trim();
    let scripture = ScriptureContent::prefix_excerpt(
        parsed.reference,
        parsed.display_reference.clone(),
        parsed.version.clone(),
        excerpt_text.to_string(),
    )
    .ok()?;
    let action = ReadyAction::GenerateScripture { scripture, style };
    Some(ResolvedItemPlan::new(
        output_key,
        item.position,
        item.title.clone(),
        format!("{} {}", parsed.display_reference, parsed.version),
        "Validate Planning Center partial-verse text against the local Bible corpus".to_string(),
        ItemKind::Scripture,
        Some(type_key.to_string()),
        PlanDisposition::NeedsReview(ReviewContext::new(Some(action))),
    ))
}

fn item_scripture_refs(
    item: &Item,
    configured_default: Option<BibleVersion>,
) -> Result<Vec<ParsedScriptureRef>, String> {
    let Some(structured) = item.scripture.as_ref() else {
        return parse_scripture_refs(&item.title, configured_default)
            .map_err(|error| error.to_string());
    };
    let Some(translation) = structured.translation.as_deref() else {
        return parse_scripture_refs(&item.title, configured_default)
            .map_err(|error| error.to_string());
    };
    let translation = translation.trim();
    let Some(version) = BibleVersion::all()
        .iter()
        .copied()
        .find(|version| version.name().eq_ignore_ascii_case(translation))
    else {
        return Err(format!("Unsupported Bible version '{translation}'"));
    };
    let reference = structured.reference.trim();
    if reference.is_empty() {
        return Err("No scripture reference".to_string());
    }
    parse_scripture_refs(&format!("{reference} {}", version.name()), None)
        .map_err(|error| error.to_string())
}
