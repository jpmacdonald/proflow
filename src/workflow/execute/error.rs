//! Typed service build failures.

use super::{
    receipt, DecisionError, OpenDecision, PresentationContractError, RenderAssetFreshnessError,
    RequestError,
};
use super::{
    ArrangementBackgroundError, BackgroundImageError, BibleCorpusError,
    CanonicalPresentationNameError, Error, FontProgramFreshnessError, GeneratedPresentationError,
    LibraryCatalogError, MacroApplyError, OperatorEntryBackgroundError, OutputReviewError, PathBuf,
    PlanCaptureError, PlanFreshnessError, PlanIdentityError, PlanSemanticsError, PlaylistError,
    PresentationRenderError, ProPresenterError, RetainOperatorCuesError, SelectedArrangementError,
    SerializeError, SourceReviewError,
};

/// Errors raised while resolving, reviewing, or executing a service build.
#[derive(Debug, Error)]
pub enum BuildServiceError {
    /// The request itself was malformed or inconsistent with its plan.
    #[error(transparent)]
    Request(#[from] RequestError),
    /// An immutable Bible corpus changed or could not be read.
    #[error("Bible corpus snapshot failed: {0}")]
    BibleCorpus(#[from] BibleCorpusError),
    /// A restyled existing presentation could not be proved without guessing
    /// its native text mapping.
    #[error("restyled presentation '{presentation}' text-fit proof requires review: {reason}")]
    RestyleTextFit {
        /// Presentation selected for restyling.
        presentation: String,
        /// Exact strict-mapping or native-layout failure.
        reason: String,
    },
    /// A configured arrangement is not present in the rendered presentation.
    #[error("presentation '{presentation}' has no arrangement named '{arrangement}'")]
    ArrangementUnavailable {
        /// Presentation being rendered.
        presentation: String,
        /// Configured arrangement name.
        arrangement: String,
    },
    /// More than one native arrangement has the requested case-insensitive name.
    #[error("presentation '{presentation}' has {matches} arrangements matching '{arrangement}'")]
    ArrangementAmbiguous {
        /// Presentation being rendered.
        presentation: String,
        /// Requested arrangement name.
        arrangement: String,
        /// Number of case-insensitive native matches.
        matches: usize,
    },
    /// A named arrangement exists but its identity or traversal graph is incomplete.
    #[error(
        "presentation '{presentation}' arrangement '{arrangement}' has incomplete identity or dangling group/cue references"
    )]
    ArrangementIncomplete {
        /// Presentation being rendered.
        presentation: String,
        /// Requested arrangement name.
        arrangement: String,
    },
    /// A lookup returned only part of the requested scripture range.
    #[error("scripture lookup for '{reference}' is missing verses {verses:?}")]
    MissingVerses {
        /// Requested scripture reference.
        reference: String,
        /// Verse numbers absent from the selected Bible data.
        verses: Vec<u32>,
    },
    /// Planning Center source inputs changed after preview.
    #[error(transparent)]
    PlanningCenterFreshness(#[from] PlanFreshnessError),
    /// A review state claimed to be blocked but contained no unresolved plan.
    #[error("non-executable review contains no unresolved plan")]
    ReviewStateInvariant,
    /// The remembered decisions could not be read or written.
    #[error(transparent)]
    DecisionMemory(#[from] crate::workflow::decision_memory::DecisionMemoryError),
    /// A default was requested for an item the latest build did not ask about.
    #[error(
        "item '{item}' was not an open decision of the latest build, so there is no question to set a default for; build first"
    )]
    NoOpenQuestion {
        /// Item named by the decision.
        item: String,
    },
    /// Rendering or packaging one plan item failed.
    #[error("{title} ({output_key}): {failure}")]
    Item {
        /// Stable identity of the item.
        output_key: String,
        /// The item's Planning Center title.
        title: String,
        /// What went wrong for it. Shown in the message, not chained as a
        /// source, so a printed error chain names it once.
        failure: Box<Self>,
    },
    /// The plan's recorded decisions could not be read or applied.
    #[error(transparent)]
    Decisions(#[from] DecisionError),
    /// The build cannot be rendered until every listed item has a decision.
    #[error(
        "build blocked by {} unanswered decision(s): {}",
        decisions.len(),
        decisions
            .iter()
            .map(|decision| format!("{} ('{}'): {}", decision.item, decision.title, decision.question))
            .collect::<Vec<_>>()
            .join("; ")
    )]
    UnansweredDecisions {
        /// Every plan item still waiting for a decision, in plan order.
        decisions: Vec<OpenDecision>,
    },
    /// Execution was requested for an item that still requires a human decision.
    #[error("build blocked by unresolved item {output_key} ('{title}'): {reason}")]
    UnresolvedPlan {
        /// Stable plan identity.
        output_key: String,
        /// Operator-visible Planning Center title.
        title: String,
        /// Exact unresolved reason.
        reason: String,
    },
    /// A reviewed source manifest did not contain bytes for an approved path.
    #[error("reviewed source capture has no bytes for '{}'", path.display())]
    MissingReviewedSource {
        /// Approved source path whose immutable bytes were absent.
        path: PathBuf,
    },
    /// Parsed description content unexpectedly contained no semantic segments.
    #[error("parsed content for '{title}' contains no semantic segments")]
    EmptyParsedContent {
        /// Operator-visible item title.
        title: String,
    },
    /// A checked scripture request could not be parsed at execution.
    #[error("cannot parse scripture reference '{reference}'")]
    InvalidScriptureReference {
        /// Exact requested scripture reference.
        reference: String,
    },
    /// A configured theme dependency had no resolvable local file locator.
    #[error("theme slide '{slide}' media dependency has no local file locator: {locator}")]
    ThemeMediaLocatorUnavailable {
        /// Configured theme slide name.
        slide: String,
        /// Native URL as reported by `ProPresenter`.
        locator: String,
    },
    /// Media discovery could not decode an approved source presentation.
    #[error("failed to inspect reviewed media dependencies for '{}': {source}", path.display())]
    ReviewedMediaInspection {
        /// Reviewed source presentation.
        path: PathBuf,
        /// Native protobuf decode failure.
        #[source]
        source: ProPresenterError,
    },
    /// Receipt evidence could not decode the exact final presentation bytes
    /// carried by a playlist entry.
    #[error("failed to inspect final presentation for '{output_key}': {source}")]
    PresentationEvidenceInspection {
        /// Stable reviewed output identity.
        output_key: String,
        /// Native presentation decode failure.
        #[source]
        source: ProPresenterError,
    },
    /// Exact final presentation bytes contain ambiguous or dangling native
    /// references and therefore cannot cross the prepared boundary.
    #[error(
        "final presentation for '{output_key}' has invalid native references: {diagnostics:?}"
    )]
    PresentationStructureDiagnostics {
        /// Stable reviewed output identity.
        output_key: String,
        /// Exact semantic reference diagnostics from the final bytes.
        diagnostics: Vec<propresenter::inspection::PresentationReferenceDiagnostic>,
    },
    /// Exact final native bytes violate the semantic contract derived from the
    /// reviewed plan.
    #[error("final presentation for '{output_key}' violates its reviewed contract: {source}")]
    PresentationContract {
        /// Stable reviewed output identity.
        output_key: String,
        /// First deterministic contract mismatch.
        #[source]
        source: PresentationContractError,
    },
    /// Existing native bytes cannot be represented exactly enough to edit.
    #[error(transparent)]
    NativeEdit(#[from] propresenter::native_document::NativeEditError),
    /// A checked playlist arrangement could not produce an exact effective
    /// operator traversal for receipt evidence.
    #[error("could not derive effective playlist traversal for '{output_key}': {source}")]
    PresentationTraversalEvidence {
        /// Stable plan output identity.
        output_key: String,
        /// Native arrangement/group/cue traversal failure.
        #[source]
        source: propresenter::arrangement::OperatorTraversalError,
    },
    /// Receipt traversal could not bind normalized playlist arrangement
    /// identity back to the exact native UUID spelling.
    #[error("could not resolve playlist arrangement '{arrangement}' for '{output_key}': {source}")]
    PresentationSelectionEvidence {
        /// Stable plan output identity.
        output_key: String,
        /// Exact arrangement display name.
        arrangement: String,
        /// Native arrangement identity failure.
        #[source]
        source: propresenter::arrangement::ArrangementSelectionError,
    },
    /// A presentation-producing plan emitted no exact bytes for evidence.
    #[error("final playlist entry for '{output_key}' has no embedded presentation bytes")]
    MissingPresentationEvidence {
        /// Stable reviewed output identity.
        output_key: String,
    },
    /// A native presentation could not be decoded.
    #[error(transparent)]
    Deserialize(#[from] ProPresenterError),
    /// A native presentation could not be encoded or written.
    #[error(transparent)]
    Serialize(#[from] SerializeError),
    /// A native playlist could not be encoded or written.
    #[error(transparent)]
    Playlist(#[from] PlaylistError),
    /// A presentation output name could not be normalized safely.
    #[error(transparent)]
    PresentationName(#[from] CanonicalPresentationNameError),
    /// Planning Center could not be read or returned an incoherent plan.
    #[error(transparent)]
    PlanningCenter(#[from] crate::planning_center::PlanningCenterError),
    /// Local Bible data could not supply the requested passage.
    #[error(transparent)]
    ScriptureLookup(#[from] crate::bible::ScriptureLookupError),
    /// A scripture passage resolved to no verses at all.
    #[error("scripture passage is empty")]
    EmptyScripturePassage,
    /// The passage's book has no canonical index for native Bible metadata.
    #[error("canonical book index is unavailable for {book}")]
    CanonicalBookIndexUnavailable {
        /// Book name as requested.
        book: String,
    },
    /// A verse or chapter number exceeds the native metadata range.
    #[error("{field} {value} cannot be represented in native Bible metadata")]
    NativeBibleRangeOverflow {
        /// Which number overflowed.
        field: &'static str,
        /// The value that did not fit.
        value: u32,
    },
    /// Planning Center partial-verse text did not prove one safe local cutoff.
    #[error(transparent)]
    ScriptureExcerpt(#[from] crate::bible::ScriptureExcerptError),
    /// Planning Center selected-verse text could not be reconciled safely.
    #[error(transparent)]
    SelectedScriptureExcerpt(#[from] crate::bible::SelectedScriptureExcerptError),
    /// Generated presentation metadata could not be prepared for the library catalog.
    #[error(transparent)]
    LibraryCatalog(#[from] LibraryCatalogError),
    /// A reviewed background could not be resolved or embedded.
    #[error(transparent)]
    Background(#[from] BackgroundImageError),
    /// A structure-preserving background transform could not prove its native
    /// arrangement entry points.
    #[error(transparent)]
    ArrangementBackground(#[from] ArrangementBackgroundError),
    /// An arrangement-less background transform found contradictory native
    /// arrangement state.
    #[error(transparent)]
    OperatorEntryBackground(#[from] OperatorEntryBackgroundError),
    /// A checked operator-cue prefix could not be retained from native structure.
    #[error(transparent)]
    RetainOperatorCues(#[from] RetainOperatorCuesError),
    /// A configured native macro transition could not be applied.
    #[error(transparent)]
    MacroApply(#[from] MacroApplyError),
    /// A legacy presentation could not be normalized to the configured canvas.
    #[error(transparent)]
    PresentationResize(#[from] propresenter::resolution::PresentationResizeError),
    /// A checked presentation specification could not be rendered.
    #[error(transparent)]
    PresentationRender(#[from] PresentationRenderError),
    /// A generated native document failed its final structural proof.
    #[error(transparent)]
    GeneratedPresentation(#[from] GeneratedPresentationError),
    /// A resolved native arrangement could not be represented in playlist metadata.
    #[error(transparent)]
    SelectedArrangement(#[from] SelectedArrangementError),
    /// A source file changed after operator review.
    #[error(transparent)]
    SourceReview(#[from] SourceReviewError),
    /// A deterministic reviewed build receipt could not be produced.
    #[error(transparent)]
    BuildReceipt(#[from] receipt::BuildReceiptError),
    /// A native theme, macro, Workspace, or Look destination changed after the
    /// immutable render snapshot was loaded.
    #[error(transparent)]
    RenderAssetFreshness(#[from] RenderAssetFreshnessError),
    /// A font program used by native text layout changed before commit.
    #[error(transparent)]
    FontProgramFreshness(#[from] FontProgramFreshnessError),
    /// An output target changed after operator review.
    #[error(transparent)]
    OutputReview(#[from] OutputReviewError),
    /// The captured background set disagreed with the approved render plan.
    #[error("reviewed background binding for plan '{output_key}' is inconsistent")]
    ReviewedBackgroundInvariant {
        /// Stable plan identity whose background binding is inconsistent.
        output_key: String,
    },
    /// A rendered presentation does not use the project output dimensions.
    #[error(
        "presentation '{output_key}' violates project size {expected}: {actual}; theme application must follow output-size selection"
    )]
    PresentationSizeInvariant {
        /// Stable reviewed output identity.
        output_key: String,
        /// Project-owned required dimensions.
        expected: propresenter::PresentationSize,
        /// First concrete mismatch found.
        actual: String,
    },
    /// A prepared build was committed through a runtime whose render assets or
    /// Bible corpora differ from the ones it was prepared against.
    #[error(
        "prepared build was committed through a different runtime than it was prepared against"
    )]
    PreparedRuntimeMismatch,
    /// No verified build exists for the plan's current inputs.
    #[error(
        "plan '{plan_id}' has no verified build of its current plan, configuration, library and decisions; build it again"
    )]
    NoVerifiedBuild {
        /// Plan whose build was requested.
        plan_id: String,
    },
    /// The verified build of the plan's current inputs was already committed.
    #[error("the build of plan '{plan_id}' is already committed; build it again to commit again")]
    BuildAlreadyCommitted {
        /// Plan whose build was requested.
        plan_id: String,
    },
    /// The rendered stage of the build directory is not the one this prepared
    /// build was made from.
    #[error(
        "the prepared build was rendered again or changed on disk after preview; preview again"
    )]
    PreparedBuildSuperseded,
    /// A `.pro` file appeared in or vanished from the presentation library
    /// after the build was prepared, so ambiguity review is no longer valid.
    #[error("presentation library candidates changed after preview; preview again")]
    LibraryCandidatesChanged,
    /// The build directory refused a stage or could not be read or written.
    #[error(transparent)]
    BuildDir(#[from] crate::workflow::build_dir::BuildDirError),
    /// A filesystem transaction failed.
    #[error("filesystem transaction failed: {0}")]
    Io(#[from] std::io::Error),
    /// An earlier failure left the library catalog lock poisoned; the process
    /// must restart before it can trust the catalog again.
    #[error("presentation library catalog is unavailable after an earlier failure")]
    LibraryCatalogPoisoned,
}

impl BuildServiceError {
    /// The failure itself, without the item it happened for.
    #[must_use]
    pub fn cause(&self) -> &Self {
        match self {
            Self::Item { failure, .. } => failure.cause(),
            other => other,
        }
    }
}

impl From<PlanSemanticsError> for BuildServiceError {
    fn from(error: PlanSemanticsError) -> Self {
        Self::Request(RequestError::PlanSemantics(error))
    }
}

impl From<PlanCaptureError> for BuildServiceError {
    fn from(error: PlanCaptureError) -> Self {
        match error {
            PlanCaptureError::Identity(PlanIdentityError::NotFound {
                plan_id,
                days_ahead,
            }) => Self::Request(RequestError::PlanNotFound {
                plan_id,
                days_ahead: days_ahead.get(),
            }),
            PlanCaptureError::Api(error) => Self::PlanningCenter(error),
        }
    }
}
