//! Typed failures at the presentation rendering boundary.

#[cfg(test)]
use propresenter::text_flow::TextLayoutError;

use super::{
    Error, MacroApplyError, NativeTextRequestError, OperatorTraversalError, PresentationSpecError,
    RenderError, TemplateSlotError, TextFitError, ThemeSlideError,
};

/// Failure in the checked workflow-to-native render phase.
#[derive(Debug, Error)]
pub enum PresentationRenderError {
    /// A configured theme slide could not be resolved safely.
    #[error(transparent)]
    Theme(#[from] ThemeSlideError),
    /// A semantic role or cue specification was invalid.
    #[error(transparent)]
    Spec(#[from] PresentationSpecError),
    /// A semantic specification could not be rendered from its native assets.
    #[error(transparent)]
    Render(#[from] RenderError),
    /// The rendered document's effective operator traversal was invalid.
    #[error(transparent)]
    Traversal(#[from] OperatorTraversalError),
    /// A native text field could not be bound to its semantic name.
    #[error(transparent)]
    Template(#[from] TemplateSlotError),
    /// Checked text layout could not be constructed.
    #[cfg(test)]
    #[error(transparent)]
    TextLayout(#[from] TextLayoutError),
    /// A resolved native text field cannot be measured faithfully.
    #[error(transparent)]
    NativeTextRequest(#[from] NativeTextRequestError),
    /// The persistent `TextKit` helper rejected or could not measure a request.
    #[error(transparent)]
    NativeTextFit(#[from] TextFitError),
    /// A configured cue macro was not available or could not be applied.
    #[error(transparent)]
    Macro(#[from] MacroApplyError),
    /// A role declared explicit fields without declaring the required body.
    #[error("render role '{role}' must map semantic field 'body'")]
    MissingBodyBinding {
        /// Configured semantic role identifier.
        role: String,
    },
    /// Liturgical speaker content was assigned to a role without configured
    /// editor colors.
    #[error("render role '{role}' requires configured speaker colors")]
    MissingSpeakerPalette {
        /// Configured semantic role identifier.
        role: String,
    },
    /// Two independently configured roles claim the same semantic identity.
    #[error("render style contains duplicate cue role id '{role}'")]
    DuplicateRole {
        /// Reused semantic role identifier.
        role: String,
    },
    /// Source content did not produce any cues.
    #[error("presentation source produced no cues")]
    EmptyPresentation,
    /// Combined scripture content was built without its required divider role.
    #[error("combined scripture render is missing its divider role")]
    MissingDividerRole,
    /// A scripture source had no verses to render.
    #[error("scripture passage contains no verses")]
    EmptyScripturePassage,
    /// A scripture source had no operator-visible title.
    #[error("scripture passage title cannot be blank")]
    EmptyScriptureTitle,
    /// A scripture source had no native slide-label prefix.
    #[error("scripture slide-label prefix cannot be blank")]
    EmptyScriptureLabelPrefix,
    /// No nonempty grammatical partition fits the resolved native text box.
    #[error("render role '{role}' cannot fit even one indivisible text fragment")]
    NoFittingTextPartition {
        /// Semantic role whose text box cannot contain the source.
        role: String,
    },
    /// Final native shaping violates the physical box or configured line bound.
    #[error(
        "rendered cue {cue_index} for role '{role}' does not fit: fits_bounds={fits_bounds}, lines={line_count}/{max_lines}, used={used_width:.2}x{used_height:.2}pt"
    )]
    FinalTextOverflow {
        /// Rendered cue index.
        cue_index: usize,
        /// Semantic role used by the cue.
        role: String,
        /// Native helper's physical-bounds result.
        fits_bounds: bool,
        /// Native `TextKit` visual line count.
        line_count: usize,
        /// Configured visual-line maximum.
        max_lines: usize,
        /// Native laid-out width in points.
        used_width: f64,
        /// Native laid-out height in points.
        used_height: f64,
    },
    /// A rendered cue expected from the checked specification is unavailable.
    #[error("rendered cue {cue_index} is unavailable during final text measurement")]
    RenderedCueUnavailable {
        /// Missing zero-based cue index.
        cue_index: usize,
    },
    /// A rendered text cue does not contain exactly one presentation slide.
    #[error("rendered cue {cue_index} does not contain exactly one presentation slide")]
    RenderedPresentationSlideUnavailable {
        /// Invalid zero-based cue index.
        cue_index: usize,
    },
    /// A configured cue macro has no compiled Audience Look destinations.
    #[error("configured macro '{macro_name}' has no resolved audience destinations")]
    MissingAudienceDestinations {
        /// Exact installed macro name.
        macro_name: String,
    },
    /// A macro-selected output theme violates its physical or line bound.
    #[error(
        "rendered cue {cue_index} for role '{role}' overflows audience screen '{screen_name}': fits_bounds={fits_bounds}, lines={line_count}/{max_lines}, used={used_width:.2}x{used_height:.2}pt"
    )]
    AudienceTextOverflow {
        /// Rendered cue index.
        cue_index: usize,
        /// Semantic role used by the cue.
        role: String,
        /// Operator-visible configured audience screen.
        screen_name: String,
        /// Native helper's physical-bounds result.
        fits_bounds: bool,
        /// Native `TextKit` visual line count.
        line_count: usize,
        /// Configured visual-line maximum.
        max_lines: usize,
        /// Native laid-out width in points.
        used_width: f64,
        /// Native laid-out height in points.
        used_height: f64,
    },
    /// One audience screen was measured more than once for a cue.
    #[error(
        "rendered cue {cue_index} produced duplicate evidence for audience screen {screen_uuid}"
    )]
    DuplicateAudienceScreenEvidence {
        /// Rendered cue index.
        cue_index: usize,
        /// Stable native audience-screen UUID.
        screen_uuid: String,
    },
}
