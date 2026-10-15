//! Audit capabilities required by selected native transformations.

use super::{LibraryCatalog, ReadyAction, ResolvedItemPlan};

pub(super) fn audit_existing_transform_capabilities(
    entries: &mut [ResolvedItemPlan],
    file_index: Option<&LibraryCatalog>,
) {
    let Some(file_index) = file_index else {
        return;
    };
    for plan in entries {
        let Some(ReadyAction::RestyleExisting {
            file_path,
            arrangement,
            transform,
        }) = plan.ready_action()
        else {
            continue;
        };
        let Some(entry) = file_index.entry_at(file_path) else {
            continue;
        };
        let capabilities = entry.transform_capabilities();
        let problem = if !capabilities.exact_editable() {
            Some(
                "native bytes contain data this ProFlow schema cannot preserve exactly".to_string(),
            )
        } else if matches!(
            transform.background(),
            crate::workflow::plan::BackgroundTransform::Replace(_)
        ) && !capabilities.background_entries_editable()
        {
            Some("one or more native background entry cues cannot be resolved uniquely".to_string())
        } else {
            transform_traversal_problem(capabilities, arrangement.as_deref(), transform)
        };
        if let Some(problem) = problem {
            plan.require_review(format!(
                "Existing presentation transform is not supported safely: {problem}"
            ));
        }
    }
}

pub(super) fn transform_traversal_problem(
    capabilities: &propresenter::library::LibraryTransformCapabilities,
    arrangement: Option<&str>,
    transform: &crate::workflow::plan::ExistingTransform,
) -> Option<String> {
    let Some(traversal) = capabilities.traversal(arrangement) else {
        return Some(arrangement.map_or_else(
            || "presentation has no checked operator traversal".to_string(),
            |name| format!("arrangement '{name}' has no checked operator traversal"),
        ));
    };
    if let crate::workflow::plan::CueTransform::RetainOperatorPrefix(count) = transform.cues() {
        if count.get() > traversal.cue_count() {
            return Some(format!(
                "requested cue prefix {} exceeds the checked traversal length {}",
                count,
                traversal.cue_count()
            ));
        }
    }
    let crate::workflow::plan::MacroTransform::Enforce(policy) = transform.macros() else {
        return None;
    };
    for region in policy.regions() {
        match region.selector() {
            crate::workflow::plan::RestyleMacroSelector::OperatorCue { index }
                if *index >= traversal.cue_count() =>
            {
                return Some(format!(
                    "macro selector operator cue {index} exceeds the checked traversal length {}",
                    traversal.cue_count()
                ));
            }
            crate::workflow::plan::RestyleMacroSelector::ArrangementGroup {
                index,
                allowed_names,
            } => {
                let Some(actual) = traversal.group_names().get(*index) else {
                    return Some(format!(
                        "macro selector arrangement group {index} is unavailable"
                    ));
                };
                if !allowed_names.contains(actual) {
                    return Some(format!(
                        "macro selector arrangement group {index} resolved to unexpected group '{actual}'"
                    ));
                }
            }
            crate::workflow::plan::RestyleMacroSelector::OperatorCue { .. } => {}
        }
    }
    None
}
