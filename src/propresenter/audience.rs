//! Checked resolution of macro-selected Audience Looks.
//!
//! A presentation cue does not directly identify the theme used by every
//! output. Its macro selects an Audience Look, and that Look maps each audience
//! screen to either the source presentation or a specific theme slide. This
//! module compiles that native reference graph into immutable destinations for
//! layout validation. It never changes the workspace, macros, themes, or live
//! `ProPresenter` state.

#[path = "audience/compile.rs"]
mod compile;
#[path = "audience/error.rs"]
mod error;
#[path = "audience/load.rs"]
mod load;
#[path = "audience/model.rs"]
mod model;
#[path = "audience/resolve.rs"]
mod resolve;

pub use error::{
    AudienceDestinationError, AudienceWorkspaceError, AudienceWorkspaceLoadError,
    InvalidNativeIdentity, NativeIdentityKind,
};
pub(crate) use model::AudienceDestinationResolver;
pub use model::{
    AudienceLookDestinations, AudienceScreenDestination, PresentationDestination, ThemeDestination,
};

#[cfg(test)]
#[path = "audience/tests.rs"]
mod tests;
