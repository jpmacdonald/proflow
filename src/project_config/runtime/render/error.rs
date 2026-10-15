//! Typed failures while checking rendering identities and policies.

use super::PresentationSpecError;
use std::fmt;

/// Why a render identifier cannot be used as an exact native lookup key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentifierProblem {
    /// The value has whitespace outside its meaningful content.
    SurroundingWhitespace,
    /// The value contains a character that cannot be displayed or matched safely.
    ControlCharacter,
}

impl fmt::Display for IdentifierProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SurroundingWhitespace => formatter.write_str("surrounding whitespace"),
            Self::ControlCharacter => formatter.write_str("a control character"),
        }
    }
}

pub(in crate::project_config::runtime) fn identifier_problem(
    value: &str,
) -> Option<IdentifierProblem> {
    if value.chars().any(char::is_control) {
        Some(IdentifierProblem::ControlCharacter)
    } else if value.trim() != value {
        Some(IdentifierProblem::SurroundingWhitespace)
    } else {
        None
    }
}

/// Invalid render metadata rejected at the plan boundary.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RenderPlanError {
    /// A restyle policy contained no operator regions.
    #[error("restyle macro policy requires at least one region")]
    EmptyRestyleMacroPolicy,
    /// An arrangement-group selector contained no accepted native names.
    #[error("restyle arrangement-group region {index} requires at least one allowed name")]
    EmptyArrangementGroupNames { index: usize },
    /// Two macro regions selected the same operator cue.
    #[error("restyle macro policy selects operator cue {index} more than once")]
    DuplicateOperatorCueSelector { index: usize },
    /// Two macro regions selected the same arrangement-group occurrence.
    #[error("restyle macro policy selects arrangement group {index} more than once")]
    DuplicateArrangementGroupSelector { index: usize },
    /// An arrangement-group selector name was not an exact native identity.
    #[error("restyle arrangement-group region {index} name '{name}' contains {problem}")]
    InvalidArrangementGroupName {
        index: usize,
        name: String,
        problem: IdentifierProblem,
    },
    /// A cue macro contains no usable name.
    #[error("cue macro name cannot be blank")]
    BlankCueMacro,
    /// A cue macro name cannot be used as an exact native lookup key.
    #[error("cue macro name '{name}' contains {problem}")]
    InvalidCueMacro {
        name: String,
        problem: IdentifierProblem,
    },
    /// The leader-first cue macro contains no usable name.
    #[error("leader-first cue macro name cannot be blank")]
    BlankLeaderCueMacro,
    /// A leader-first macro name cannot be used as an exact native lookup key.
    #[error("leader-first cue macro name '{name}' contains {problem}")]
    InvalidLeaderCueMacro {
        name: String,
        problem: IdentifierProblem,
    },
    /// A render role contains no usable identifier.
    #[error("render role id cannot be blank")]
    BlankRoleId,
    /// A render-role identifier cannot be used as an exact config lookup key.
    #[error("render role id '{role_id}' contains {problem}")]
    InvalidRoleId {
        role_id: String,
        problem: IdentifierProblem,
    },
    /// The renderer's cue-role identity rules rejected a configured role id.
    #[error("render role '{role_id}' is not a valid cue role identity: {source}")]
    RoleIdRejected {
        role_id: String,
        #[source]
        source: PresentationSpecError,
    },
    /// A render role contains no usable theme slide.
    #[error("render role '{role_id}' slide cannot be blank")]
    BlankRoleSlide { role_id: String },
    /// A theme-slide name cannot be used as an exact native lookup key.
    #[error("render role '{role_id}' slide '{slide}' contains {problem}")]
    InvalidRoleSlide {
        role_id: String,
        slide: String,
        problem: IdentifierProblem,
    },
    /// A text-slot binding contains a blank semantic name.
    #[error("render role '{role_id}' has a blank text-slot name ('{slot}')")]
    BlankTextSlotName { role_id: String, slot: String },
    /// A semantic text-slot name cannot be used as an exact lookup key.
    #[error("render role '{role_id}' text-slot name '{slot}' contains {problem}")]
    InvalidTextSlotName {
        role_id: String,
        slot: String,
        problem: IdentifierProblem,
    },
    /// A text-slot binding contains a blank native element name.
    #[error("render role '{role_id}' text slot '{slot}' has a blank element name")]
    BlankTextSlotElement { role_id: String, slot: String },
    /// A native text-element name cannot be used as an exact lookup key.
    #[error("render role '{role_id}' text slot '{slot}' element '{element}' contains {problem}")]
    InvalidTextSlotElement {
        role_id: String,
        slot: String,
        element: String,
        problem: IdentifierProblem,
    },
    /// Two semantic fields target the same native graphics element.
    #[error(
        "render role '{role_id}' text slots '{first_slot}' and '{duplicate_slot}' both target element '{element}'"
    )]
    DuplicateTextSlotElement {
        role_id: String,
        first_slot: String,
        duplicate_slot: String,
        element: String,
    },
    /// Explicit text bindings omit the required semantic body field.
    #[error("render role '{role_id}' has explicit text slots but no required 'body' field")]
    MissingBodyTextSlot { role_id: String },
    /// A responsive role configured only one half of its macro/color contract.
    #[error(
        "render role '{role_id}' must configure a leader-first macro and speaker palette together"
    )]
    IncompleteResponsiveRole { role_id: String },
    /// A responsive role used the same editor color for both speakers.
    #[error("render role '{role_id}' leader and audience colors must differ")]
    IndistinguishableSpeakerColors { role_id: String },
    /// A render style requested a zero-line slide bound.
    #[error("render style max lines per slide cannot be zero")]
    ZeroMaxLines,
    /// Title and content regions reused one semantic role identity.
    #[error("render style title and content roles both use id '{role_id}'")]
    DuplicateRoleId { role_id: String },
}
