//! Shared typed workflow plan model.

mod item;

pub(crate) use crate::project_config::{
    BackgroundTransform, CueTransform, ExistingTransform, MacroTransform,
};
pub use item::{
    ItemKind, OutputKey, PlanDisposition, PlanSemanticsError, ReadyAction, ResolvedItemPlan,
    ReviewContext, ScriptureContent, ScriptureRefInfo, ScriptureRequest,
};
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
pub(crate) use item::ScripturePlanError;

#[cfg(test)]
mod tests;
