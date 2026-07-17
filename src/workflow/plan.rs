//! Shared typed workflow plan model.

mod existing;
mod item;
mod render;

#[allow(unused_imports)]
pub use existing::ExistingTransformError;
pub use existing::{BackgroundTransform, CueTransform, ExistingTransform, MacroTransform};
pub use item::{
    ItemKind, OutputKey, PlanDisposition, ReadyAction, ResolvedItemPlan, ReviewContext,
    ScriptureContent, ScriptureRefInfo, ScriptureRequest,
};
// Checked constructor errors remain available at the facade boundary even when
// production callers only need inference to route them into review.
#[allow(unused_imports)]
pub use item::ScripturePlanError;
pub use render::{
    CueMacro, RenderRole, RenderStyle, ResolvedBackground, RestyleMacroPolicy, RestyleMacroRegion,
    RestyleMacroSelector, SpeakerPalette,
};
#[allow(unused_imports)]
pub use render::{IdentifierProblem, RenderPlanError};

#[cfg(test)]
mod tests;
