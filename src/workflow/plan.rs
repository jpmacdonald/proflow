//! Shared typed workflow plan model.

mod action;
mod item;
mod output_key;
mod presentation_contract;
mod scripture;

pub use crate::project_config::ItemKind;
pub(crate) use crate::project_config::{
    BackgroundTransform, CueTransform, ExistingTransform, MacroTransform,
};
pub use action::{ReadyAction, ReviewContext};
pub use item::{PlanDisposition, PlanSemanticsError, ResolvedItemPlan};
pub use output_key::OutputKey;
pub use presentation_contract::{
    ExpectedCueCount, ExpectedMacroPolicy, ExpectedMacroRegion, ExpectedMacroSelector,
    ExpectedPresentationContract,
};
pub use scripture::{ScriptureContent, ScriptureRefInfo, ScriptureRequest};
// Checked constructor errors remain available at the facade boundary even when
// production callers only need inference to route them into review.
#[cfg(test)]
pub(crate) use crate::project_config::{CueMacro, RestyleMacroRegion, SpeakerPalette};
#[cfg(test)]
pub(crate) use crate::project_config::{IdentifierProblem, RenderPlanError};
pub(crate) use crate::project_config::{
    RenderRole, RenderStyle, ResolvedBackground, RestyleMacroPolicy, RestyleMacroSelector,
};
#[cfg(test)]
pub(crate) use scripture::ScripturePlanError;

#[cfg(test)]
mod tests;
