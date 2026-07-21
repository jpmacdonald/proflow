//! Native presentation rendering and per-file export.
//!
//! The child modules follow the existing execution phases: checked transforms
//! for native presentations, generated text, reviewed scripture lookup, and the
//! final encoding boundary.

mod existing;
mod generated;
mod scripture;
mod target;

#[cfg(test)]
pub(super) use existing::apply_restyle_macro_policy;
#[cfg(test)]
pub(super) use scripture::parse_bible_version;
pub(super) use target::{ReviewedBackgroundAsset, ReviewedRenderTarget};
