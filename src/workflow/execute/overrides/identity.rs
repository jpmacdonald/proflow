//! Check source identities before applying operator overrides.

use super::{
    validate_identity, validate_path_identity, ReadyAction, RequestError, ResolvedItemPlan,
};

pub(super) fn validate_plan_identity(plan: &ResolvedItemPlan) -> Result<(), RequestError> {
    validate_identity("plan output_key", plan.output_key.as_str())?;
    validate_identity("plan playlist_name", &plan.playlist_name)?;
    let Some(action) = plan.preview_action() else {
        return Ok(());
    };
    match action {
        ReadyAction::UseExisting {
            file_path,
            arrangement,
        }
        | ReadyAction::RestyleExisting {
            file_path,
            arrangement,
            ..
        } => {
            validate_path_identity("plan file_path", file_path)?;
            if let Some(arrangement) = arrangement {
                validate_identity("plan arrangement", arrangement)?;
            }
        }
        ReadyAction::EditDescription { file_path, .. } => {
            validate_path_identity("plan file_path", file_path)?;
        }
        ReadyAction::GenerateDescription { .. }
        | ReadyAction::GenerateScripture { .. }
        | ReadyAction::GenerateTitle { .. } => {}
    }
    Ok(())
}
