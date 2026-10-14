//! Compiled runtime presentation policy.
//!
//! The JSON contract deliberately remains a convenient editable structure.
//! This module performs its one semantic translation into variants that cannot
//! express unsupported source/output combinations.

mod compile;
pub(super) use compile::{compile_presentation_policies, compile_render_roles};

use std::collections::BTreeSet;

mod classification;
mod existing;
mod identity;
mod render;

pub use classification::{
    compile_classifications, compile_required_playlist_items, ClassificationTier,
    CompiledClassification, CompiledDecision, CompiledDirectTarget, CompiledExpansionStep,
    CompiledRequiredPlaylistItem, CompiledRuleOutcome, CompiledSpeakerTarget, ItemMatchInput,
    ResolvedPresentationType, ResolvedRequiredPresentation,
};
pub use existing::{BackgroundTransform, CueTransform, ExistingTransform, MacroTransform};
pub use identity::{MacroName, PresentationTypeKey, ThemeSlideName};
#[cfg(test)]
pub use render::{
    CueMacro, IdentifierProblem, RenderPlanError, RestyleMacroRegion, SpeakerPalette,
};
#[cfg(not(test))]
use render::{CueMacro, RestyleMacroRegion, SpeakerPalette};
pub use render::{
    RenderRole, RenderStyle, ResolvedBackground, RestyleMacroPolicy, RestyleMacroSelector,
};

use super::{
    ConfigValidationIssue, ContentSourceKind, DescriptionParserKind, DisplayBindingConfig,
    ItemKind, OutputStrategy, OverrideWhen, PresentationTypeConfig, RawProjectConfig,
    RestyleMacroSelectorConfig,
};

/// One checked presentation behavior used by classification.
#[derive(Debug, Clone)]
pub enum PresentationPolicy {
    /// Always omit a matching item.
    Skip { kind: ItemKind },
    /// Preserve an explicit human decision boundary.
    Review(ReviewPolicy),
    /// Preserve one explicitly exempt native presentation without modifying it.
    PreserveExisting {
        kind: ItemKind,
        source: ExistingSource,
        arrangement: ArrangementPolicy,
    },
    /// Reuse native content and structure while enforcing the reviewed background.
    RestyleExisting {
        kind: ItemKind,
        source: ExistingSource,
        arrangement: ArrangementPolicy,
        transform: ExistingTransformPolicy,
    },
    /// Parse description content and replace an existing presentation.
    EditDescription {
        kind: ItemKind,
        parser: DescriptionParserKind,
        render: RenderPolicy,
    },
    /// Parse description content and create a presentation.
    GenerateDescription {
        kind: ItemKind,
        parser: DescriptionParserKind,
        render: RenderPolicy,
    },
    /// Fetch scripture content and create a presentation.
    GenerateScripture { render: RenderPolicy },
}

impl PresentationPolicy {
    /// Conceptual item kind reported to the operator.
    pub(crate) const fn kind(&self) -> ItemKind {
        match self {
            Self::Skip { kind }
            | Self::PreserveExisting { kind, .. }
            | Self::RestyleExisting { kind, .. }
            | Self::EditDescription { kind, .. }
            | Self::GenerateDescription { kind, .. } => *kind,
            Self::Review(review) => review.kind(),
            Self::GenerateScripture { .. } => ItemKind::Scripture,
        }
    }

    /// Whether a rule may select one exact existing library file for this policy.
    const fn accepts_library_file_target(&self) -> bool {
        matches!(
            self,
            Self::PreserveExisting { .. }
                | Self::RestyleExisting { .. }
                | Self::EditDescription { .. }
        )
    }

    /// Whether a contextual decision may choose an existing presentation.
    const fn accepts_existing_file_decision(&self) -> bool {
        matches!(
            self,
            Self::PreserveExisting { .. } | Self::RestyleExisting { .. }
        )
    }
}

/// Content family accepted by the use-existing operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExistingSource {
    Static,
    Song,
}

/// Source-specific context retained when configuration requires review.
#[derive(Debug, Clone)]
pub enum ReviewPolicy {
    Static {
        kind: ItemKind,
    },
    Description {
        kind: ItemKind,
        parser: DescriptionParserKind,
        render: Option<Box<RenderPolicy>>,
    },
    Scripture,
    Song,
}

impl ReviewPolicy {
    pub(crate) const fn kind(&self) -> ItemKind {
        match self {
            Self::Static { kind } | Self::Description { kind, .. } => *kind,
            Self::Scripture => ItemKind::Scripture,
            Self::Song => ItemKind::Song,
        }
    }
}

/// A complete checked style plus the only service-scoped substitutions it can
/// accept: alternative backgrounds.
#[derive(Debug, Clone)]
pub struct RenderPolicy {
    base: RenderStyle,
    overrides: Vec<ServiceRenderStyle>,
}

/// A checked native transform plus service-scoped background substitutions.
#[derive(Debug, Clone)]
pub struct ExistingTransformPolicy {
    base: ExistingTransform,
    background_overrides: Vec<ServiceBackground>,
}

impl ExistingTransformPolicy {
    /// Resolve the complete transform without consulting the raw configuration.
    pub(crate) fn for_service(&self, service_name: Option<&str>) -> ExistingTransform {
        self.background_overrides
            .iter()
            .rev()
            .find(|entry| entry.scope.matches(service_name))
            .map_or_else(
                || self.base.clone(),
                |entry| {
                    self.base
                        .clone()
                        .with_replacement_background(entry.background.clone())
                },
            )
    }
}

#[derive(Debug, Clone)]
struct ServiceBackground {
    scope: ServiceScope,
    background: ResolvedBackground,
}

impl RenderPolicy {
    /// Resolve a complete style without consulting raw config or revalidating
    /// cue-role relationships.
    pub(crate) fn for_service(&self, service_name: Option<&str>) -> RenderStyle {
        self.overrides
            .iter()
            .rev()
            .find(|entry| entry.scope.matches(service_name))
            .map_or_else(|| self.base.clone(), |entry| entry.style.clone())
    }
}

#[derive(Debug, Clone)]
struct ServiceRenderStyle {
    scope: ServiceScope,
    style: RenderStyle,
}

/// Base arrangement and scoped alternatives for an existing presentation.
#[derive(Debug, Clone)]
pub struct ArrangementPolicy {
    base: Option<String>,
    overrides: Vec<ServiceArrangement>,
}

impl ArrangementPolicy {
    pub(crate) fn for_service(&self, service_name: Option<&str>) -> Option<String> {
        self.overrides
            .iter()
            .rev()
            .find(|entry| entry.scope.matches(service_name))
            .map_or(self.base.as_ref(), |entry| Some(&entry.arrangement))
            .cloned()
    }
}

#[derive(Debug, Clone)]
struct ServiceArrangement {
    scope: ServiceScope,
    arrangement: String,
}

/// A precomputed service-name predicate. `None` means every service, including
/// planning calls where no service name is available.
#[derive(Debug, Clone)]
struct ServiceScope {
    service_types: Option<BTreeSet<String>>,
}

impl ServiceScope {
    fn matches(&self, service_name: Option<&str>) -> bool {
        match (&self.service_types, service_name) {
            (None, _) => true,
            (Some(service_types), Some(service_name)) => {
                service_types.contains(&service_name.to_ascii_lowercase())
            }
            (Some(_), None) => false,
        }
    }
}

#[cfg(test)]
mod tests;
