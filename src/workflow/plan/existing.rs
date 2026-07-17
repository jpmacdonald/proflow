//! Checked transforms for existing native presentations.

use std::num::NonZeroUsize;

use serde::Serialize;

use super::{ResolvedBackground, RestyleMacroPolicy};

/// Background behavior for one existing-presentation transform.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "background", rename_all = "snake_case")]
pub enum BackgroundTransform {
    /// Keep every native background action unchanged.
    Preserve,
    /// Replace the presentation's checked entry backgrounds.
    Replace(ResolvedBackground),
}

/// Macro behavior for one existing-presentation transform.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "policy", rename_all = "snake_case")]
pub enum MacroTransform {
    /// Keep every native macro action unchanged.
    Preserve,
    /// Replace native macro transitions with one complete checked policy.
    Enforce(RestyleMacroPolicy),
}

/// Cue-selection behavior for one existing-presentation transform.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "count", rename_all = "snake_case")]
pub enum CueTransform {
    /// Keep the complete operator-visible presentation structure.
    Preserve,
    /// Retain the first non-zero number of operator-visible cue occurrences.
    RetainOperatorPrefix(NonZeroUsize),
}

/// One non-empty, checked transform applied to an existing presentation.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ExistingTransform {
    background: BackgroundTransform,
    macros: MacroTransform,
    cues: CueTransform,
}

impl ExistingTransform {
    /// Build a transform only when at least one native property will change.
    pub fn new(
        background: BackgroundTransform,
        macros: MacroTransform,
        cues: CueTransform,
    ) -> Result<Self, ExistingTransformError> {
        if matches!(background, BackgroundTransform::Preserve)
            && matches!(macros, MacroTransform::Preserve)
            && matches!(cues, CueTransform::Preserve)
        {
            return Err(ExistingTransformError::NoOp);
        }
        Ok(Self {
            background,
            macros,
            cues,
        })
    }

    /// Background operation selected for this presentation.
    pub const fn background(&self) -> &BackgroundTransform {
        &self.background
    }

    /// Macro operation selected for this presentation.
    pub const fn macros(&self) -> &MacroTransform {
        &self.macros
    }

    /// Cue-selection operation selected for this presentation.
    pub const fn cues(&self) -> CueTransform {
        self.cues
    }

    /// Return the exact replacement background, when one is required.
    pub const fn replacement_background(&self) -> Option<&ResolvedBackground> {
        match &self.background {
            BackgroundTransform::Replace(background) => Some(background),
            BackgroundTransform::Preserve => None,
        }
    }

    /// Replace the background component while preserving the other checked operations.
    #[must_use]
    pub fn with_replacement_background(mut self, background: ResolvedBackground) -> Self {
        self.background = BackgroundTransform::Replace(background);
        self
    }
}

/// Invalid existing-presentation transform rejected before planning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ExistingTransformError {
    /// All native properties were configured to remain unchanged.
    #[error("existing presentation transform must change background, macros, or cue selection")]
    NoOp,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_total_no_op() {
        assert_eq!(
            ExistingTransform::new(
                BackgroundTransform::Preserve,
                MacroTransform::Preserve,
                CueTransform::Preserve,
            ),
            Err(ExistingTransformError::NoOp)
        );
    }
}
