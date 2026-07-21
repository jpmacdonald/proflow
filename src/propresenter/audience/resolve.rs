use crate::propresenter::generated::rv_data::{action, macros_document, CollectionElementType};

use super::compile::{compile_look, parse_uuid, resolve_saved_look};
use super::{
    AudienceDestinationError, AudienceDestinationResolver, AudienceLookDestinations,
    NativeIdentityKind,
};

impl AudienceDestinationResolver {
    /// Resolve the single enabled Audience Look action in one installed macro.
    ///
    /// Disabled actions do not execute and are ignored. A macro with zero or
    /// multiple enabled Look actions is not deterministic enough for layout
    /// validation and therefore fails explicitly.
    pub(crate) fn resolve_macro(
        &mut self,
        native_macro: &macros_document::Macro,
    ) -> Result<AudienceLookDestinations, AudienceDestinationError> {
        if let Some((action_index, _)) =
            native_macro
                .actions
                .iter()
                .enumerate()
                .find(|(_, native_action)| {
                    native_action.is_enabled
                        && (native_action.r#type == action::ActionType::Macro as i32
                            || matches!(
                                native_action.action_type_data,
                                Some(action::ActionTypeData::Macro(_))
                            ))
                })
        {
            return Err(AudienceDestinationError::NestedMacroAction {
                macro_name: native_macro.name.clone(),
                action_index,
            });
        }

        let mut enabled_looks = native_macro.actions.iter().filter(|native_action| {
            native_action.is_enabled
                && native_action.r#type == action::ActionType::AudienceLook as i32
        });

        let Some(native_action) = enabled_looks.next() else {
            return Err(AudienceDestinationError::MissingAudienceLookAction {
                macro_name: native_macro.name.clone(),
            });
        };
        if enabled_looks.next().is_some() {
            let count = native_macro
                .actions
                .iter()
                .filter(|native_action| {
                    native_action.is_enabled
                        && native_action.r#type == action::ActionType::AudienceLook as i32
                })
                .count();
            return Err(AudienceDestinationError::AmbiguousAudienceLookActions {
                macro_name: native_macro.name.clone(),
                count,
            });
        }

        let Some(action::ActionTypeData::AudienceLook(audience_look)) =
            native_action.action_type_data.as_ref()
        else {
            return Err(AudienceDestinationError::MissingAudienceLookActionData {
                macro_name: native_macro.name.clone(),
            });
        };
        let identification = audience_look.identification.as_ref().ok_or_else(|| {
            AudienceDestinationError::MissingAudienceLookIdentification {
                macro_name: native_macro.name.clone(),
            }
        })?;
        self.resolve_identification(&native_macro.name, identification)
    }

    /// Resolve one native Audience Look collection identification by UUID.
    ///
    /// The name is checked as corroborating metadata but is never used as a
    /// fallback identity.
    pub(crate) fn resolve_identification(
        &mut self,
        macro_name: &str,
        identification: &CollectionElementType,
    ) -> Result<AudienceLookDestinations, AudienceDestinationError> {
        let native_uuid = identification.parameter_uuid.as_ref().ok_or_else(|| {
            AudienceDestinationError::MissingAudienceLookUuid {
                macro_name: macro_name.to_string(),
                look_name: identification.parameter_name.clone(),
            }
        })?;
        let look_uuid = parse_uuid(
            NativeIdentityKind::MacroAudienceLook,
            &native_uuid.string,
            &identification.parameter_name,
        )
        .map_err(AudienceDestinationError::InvalidIdentity)?;
        let look = resolve_saved_look(
            &self.workspace,
            look_uuid,
            macro_name,
            &identification.parameter_name,
        )?;
        if !identification.parameter_name.is_empty() && identification.parameter_name != look.name {
            return Err(AudienceDestinationError::AudienceLookNameMismatch {
                macro_name: macro_name.to_string(),
                look_uuid,
                macro_look_name: identification.parameter_name.clone(),
                workspace_look_name: look.name.clone(),
            });
        }
        compile_look(look, &self.workspace, &self.show_root, &mut self.themes)
    }
}
