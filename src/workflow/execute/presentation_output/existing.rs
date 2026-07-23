//! Checked transforms for existing native presentations.

use std::path::{Path, PathBuf};

use crate::propresenter::generated::rv_data;
use crate::propresenter::native_document::OpaquePresentation;
use crate::propresenter::playlist::{PlaylistEntry, SelectedArrangement};
use crate::propresenter::render::apply_application_info;
use crate::propresenter::PresentationSize;
use crate::workflow::plan::{
    BackgroundTransform, CueTransform, ExistingTransform, MacroTransform, ResolvedItemPlan,
    RestyleMacroPolicy, RestyleMacroSelector,
};

use super::super::{BuildServiceError, ServiceBuildExecutor};
use super::target::{
    validate_rendered_presentation_size, write_existing_playlist_presentation, ReviewedRenderTarget,
};

#[derive(Debug)]
pub(in crate::workflow::execute) struct PreparedExistingPresentation {
    pub(in crate::workflow::execute) embedded_data: Vec<u8>,
    pub(in crate::workflow::execute) selected_arrangement: Option<SelectedArrangement>,
    pub(in crate::workflow::execute) file_path: PathBuf,
}

impl ServiceBuildExecutor<'_> {
    /// Apply one checked transform to an existing native presentation.
    pub(in crate::workflow::execute) fn restyle_existing_presentation(
        &self,
        entry: &ResolvedItemPlan,
        source_path: &Path,
        arrangement: Option<&str>,
        transform: &ExistingTransform,
        source_bytes: &[u8],
        target: ReviewedRenderTarget<'_>,
    ) -> Result<(PlaylistEntry, usize), BuildServiceError> {
        let mut editable =
            OpaquePresentation::decode(source_bytes, source_path.display().to_string())?
                .try_into_editable()?;
        let presentation = editable.presentation_mut();
        crate::propresenter::resolution::resize_presentation_canvas(
            presentation,
            target.presentation_size,
        )?;
        validate_rendered_presentation_size(
            presentation,
            target.presentation_size,
            entry.output_key.as_str(),
        )?;
        crate::propresenter::arrangement::clear_stale_selected_arrangement(presentation);
        let selected_arrangement = resolve_selected_arrangement(presentation, arrangement)?;
        if let Some(selected) = selected_arrangement.as_ref() {
            presentation.selected_arrangement = Some(selected_native_uuid(presentation, selected)?);
        }
        if let CueTransform::RetainOperatorPrefix(limit) = transform.cues() {
            crate::propresenter::arrangement::retain_first_operator_cues(presentation, limit)?;
        }
        match (transform.background(), target.background) {
            (BackgroundTransform::Replace(_), Some(background)) => {
                match selected_arrangement.as_ref() {
                    Some(selected) => {
                        crate::propresenter::background::replace_arrangement_entry_backgrounds(
                            presentation,
                            background.path,
                            background.data,
                            self.render_assets.locations().propresenter_root(),
                            selected.uuid(),
                            selected.name(),
                        )?;
                    }
                    None => {
                        crate::propresenter::background::replace_operator_entry_background(
                            presentation,
                            background.path,
                            background.data,
                            self.render_assets.locations().propresenter_root(),
                        )?;
                    }
                }
            }
            (BackgroundTransform::Preserve, None) => {}
            _ => {
                return Err(BuildServiceError::ReviewedBackgroundInvariant {
                    output_key: entry.output_key.to_string(),
                });
            }
        }
        if let MacroTransform::Enforce(policy) = transform.macros() {
            apply_restyle_macro_policy(presentation, policy, self.render_assets.macros())?;
        }
        apply_application_info(
            presentation,
            Some(self.playlist_metadata.application_info()),
        );
        write_existing_playlist_presentation(entry, &editable, target, selected_arrangement)
    }

    pub(in crate::workflow::execute) fn prepare_existing_presentation(
        output_key: &str,
        source_path: &Path,
        arrangement: Option<&str>,
        source_bytes: &[u8],
        presentation_size: PresentationSize,
    ) -> Result<PreparedExistingPresentation, BuildServiceError> {
        let opaque = OpaquePresentation::decode(source_bytes, source_path.display().to_string())?;
        validate_rendered_presentation_size(opaque.presentation(), presentation_size, output_key)?;
        let must_clear_selection = opaque.presentation().arrangements.is_empty()
            && opaque.presentation().selected_arrangement.is_some();
        let selected_arrangement =
            resolve_selected_arrangement(opaque.presentation(), arrangement)?;
        let embedded_data = if selected_arrangement.is_none() && !must_clear_selection {
            opaque.into_bytes()
        } else {
            let mut editable = opaque.try_into_editable()?;
            let presentation = editable.presentation_mut();
            if must_clear_selection {
                crate::propresenter::arrangement::clear_stale_selected_arrangement(presentation);
            }
            if let Some(selected) = selected_arrangement.as_ref() {
                presentation.selected_arrangement =
                    Some(selected_native_uuid(presentation, selected)?);
            }
            editable.encode()?
        };

        Ok(PreparedExistingPresentation {
            embedded_data,
            selected_arrangement,
            file_path: source_path.to_path_buf(),
        })
    }
}

fn selected_native_uuid(
    presentation: &rv_data::Presentation,
    selected: &SelectedArrangement,
) -> Result<rv_data::Uuid, BuildServiceError> {
    crate::propresenter::arrangement::selectable_arrangement_by_identity(
        presentation,
        selected.uuid(),
        selected.name(),
    )
    .map_err(|_| BuildServiceError::ArrangementUnavailable {
        presentation: presentation.name.clone(),
        arrangement: selected.name().to_string(),
    })?
    .native_uuid()
    .cloned()
    .ok_or_else(|| BuildServiceError::ArrangementUnavailable {
        presentation: presentation.name.clone(),
        arrangement: selected.name().to_string(),
    })
}

/// Translate workflow selectors to exact native cue targets before crossing
/// the `ProPresenter` boundary.
pub(in crate::workflow::execute) fn apply_restyle_macro_policy(
    presentation: &mut rv_data::Presentation,
    policy: &RestyleMacroPolicy,
    cache: &crate::propresenter::macros::MacroCache,
) -> Result<bool, crate::propresenter::macros::MacroApplyError> {
    use crate::propresenter::macros::{MacroApplyError, MacroCueTarget};

    let traversal = crate::propresenter::arrangement::checked_operator_cue_indices(presentation)?;
    if traversal.is_empty() {
        return Err(MacroApplyError::MissingOperatorCue);
    }
    let selected_groups =
        crate::propresenter::arrangement::checked_selected_group_entries(presentation)?;
    let mut targets = Vec::with_capacity(policy.regions().len());
    let mut target_indexes = std::collections::HashSet::new();
    for (region_index, region) in policy.regions().iter().enumerate() {
        if cache.find(region.enter_macro()).is_none() {
            return Err(MacroApplyError::Unavailable(
                region.enter_macro().to_string(),
            ));
        }
        let cue_index = match region.selector() {
            RestyleMacroSelector::OperatorCue { index } => traversal
                .get(*index)
                .copied()
                .ok_or_else(|| MacroApplyError::RegionUnavailable {
                    region: region_index,
                    selector: format!("operator cue {index}"),
                })?,
            RestyleMacroSelector::ArrangementGroup {
                index,
                allowed_names,
            } => {
                let group = selected_groups
                    .as_ref()
                    .and_then(|groups| groups.get(*index))
                    .ok_or_else(|| MacroApplyError::RegionUnavailable {
                        region: region_index,
                        selector: format!("selected arrangement group {index}"),
                    })?;
                if !allowed_names.contains(group.name) {
                    return Err(MacroApplyError::UnexpectedGroup {
                        region: region_index,
                        index: *index,
                        actual: group.name.to_string(),
                        allowed: allowed_names.iter().cloned().collect(),
                    });
                }
                group.cue_index
            }
        };
        if !target_indexes.insert(cue_index) {
            return Err(MacroApplyError::DuplicateRegionTarget { cue_index });
        }
        targets.push(MacroCueTarget::new(cue_index, region.enter_macro()));
    }

    crate::propresenter::macros::apply_operator_macro_targets(presentation, &targets, cache)
}

fn resolve_selected_arrangement(
    presentation: &rv_data::Presentation,
    requested_name: Option<&str>,
) -> Result<Option<SelectedArrangement>, BuildServiceError> {
    let Some(name) = requested_name else {
        return Ok(None);
    };
    let resolved = match crate::propresenter::arrangement::selectable_arrangement_by_name(
        presentation,
        name,
    ) {
        Ok(resolved) => resolved,
        Err(crate::propresenter::arrangement::ArrangementSelectionError::Unavailable) => {
            return Err(BuildServiceError::ArrangementUnavailable {
                presentation: presentation.name.clone(),
                arrangement: name.to_string(),
            });
        }
        Err(crate::propresenter::arrangement::ArrangementSelectionError::Ambiguous { matches }) => {
            return Err(BuildServiceError::ArrangementAmbiguous {
                presentation: presentation.name.clone(),
                arrangement: name.to_string(),
                matches,
            });
        }
        Err(crate::propresenter::arrangement::ArrangementSelectionError::Incomplete) => {
            return Err(BuildServiceError::ArrangementIncomplete {
                presentation: presentation.name.clone(),
                arrangement: name.to_string(),
            });
        }
    };
    Ok(Some(SelectedArrangement::new(
        resolved.uuid(),
        resolved.name().to_string(),
    )?))
}
