//! Review-bound service build execution.
//!
//! The public flow is intentionally short: resolve transport identities,
//! classify semantic plans, capture an immutable review snapshot, approve that
//! snapshot, then execute it through one filesystem transaction. The child
//! modules own those phase details without introducing runtime wrapper layers.

use std::path::{Path, PathBuf};

use std::sync::Mutex;
use thiserror::Error;

use super::approval::{CapturedSources, OutputReviewError, SourceReviewError};
use super::classify;
use super::plan::{PlanSemanticsError, ResolvedItemPlan};
use super::presentation_render::PresentationRenderError;
use super::report::BuildServiceResult;
use crate::bible::{BibleCorpusError, BibleCorpusSnapshot};
use crate::planning_center::identity::PlanIdentityError;
use crate::planning_center::{PlanCaptureError, PlanFreshnessError, PlanLookup, PlanSnapshot};
use proflow_propresenter::arrangement::RetainOperatorCuesError;
use proflow_propresenter::background::{
    ArrangementBackgroundError, BackgroundImageError, OperatorEntryBackgroundError,
};
use proflow_propresenter::deserialize::ProPresenterError;
use proflow_propresenter::generated_document::GeneratedPresentationError;
use proflow_propresenter::library::{LibraryCatalog, LibraryCatalogError};
use proflow_propresenter::macros::MacroApplyError;
use proflow_propresenter::playlist::{
    canonical_presentation_name, CanonicalPresentationNameError, PlaylistError, PlaylistMetadata,
    SelectedArrangementError,
};
use proflow_propresenter::serialize::SerializeError;
use proflow_propresenter::text_fit::FontProgramFreshnessError;

mod overrides;
mod plan_rendering;
mod playlist_output;
mod presentation_contract;
mod presentation_output;
mod receipt;
mod render_assets;
mod rendered_service;
mod request;
pub(crate) mod restyle_text_fit;
mod review;
mod run;

pub use overrides::{EntryOverride, OverrideAction, OverrideSlideType};
pub use presentation_contract::PresentationContractError;
pub use render_assets::{
    NativeAssetDigest, RenderAssetFingerprint, RenderAssetFingerprintError,
    RenderAssetFreshnessError, RenderAssetIssue, RenderAssetIssues, RenderAssetSnapshot,
    RenderAssetSnapshotError, ThemeSlideSizeProblem,
};
pub use request::{BuildRequest, RequestError};
pub use review::{BuildReview, NeedsReviewBuildRequest, PreparedBuildRequest};

#[cfg(test)]
use self::overrides::{
    apply_override, resolve_requested_plans, validate_request_edits as validate_unique_request_keys,
};
#[cfg(test)]
use self::presentation_output::ReviewedBackgroundAsset;
#[cfg(test)]
use self::request::BoundBuildRequest;
#[cfg(test)]
use super::plan::{ItemKind, PlanDisposition, ReadyAction, RenderStyle, ResolvedBackground};
#[cfg(test)]
use proflow_propresenter::unstable_native::rv_data;
#[cfg(test)]
use uuid::Uuid;

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
        diagnostics: Vec<proflow_propresenter::inspection::PresentationReferenceDiagnostic>,
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
    NativeEdit(#[from] proflow_propresenter::native_document::NativeEditError),
    /// A checked playlist arrangement could not produce an exact effective
    /// operator traversal for receipt evidence.
    #[error("could not derive effective playlist traversal for '{output_key}': {source}")]
    PresentationTraversalEvidence {
        /// Stable plan output identity.
        output_key: String,
        /// Native arrangement/group/cue traversal failure.
        #[source]
        source: proflow_propresenter::arrangement::OperatorTraversalError,
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
        source: proflow_propresenter::arrangement::ArrangementSelectionError,
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
    PresentationResize(#[from] proflow_propresenter::resolution::PresentationResizeError),
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
        expected: proflow_propresenter::PresentationSize,
        /// First concrete mismatch found.
        actual: String,
    },
    /// A filesystem transaction failed.
    #[error("filesystem transaction failed: {0}")]
    Io(#[from] std::io::Error),
    /// An earlier failure left the library catalog lock poisoned; the process
    /// must restart before it can trust the catalog again.
    #[error("presentation library catalog is unavailable after an earlier failure")]
    LibraryCatalogPoisoned,
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

/// Shared executor for full-service builds.
///
/// Every runtime dependency is immutable and explicit, and nothing here talks
/// to Planning Center: the executor describes the lookup it needs through
/// [`Self::plan_lookup`] and consumes the captured [`PlanSnapshot`]. The
/// boundary that performs those reads lives in [`crate::workstation`].
pub struct ServiceBuildExecutor<'a> {
    bible_corpora: &'a BibleCorpusSnapshot,
    file_index: &'a Mutex<LibraryCatalog>,
    render_assets: &'a RenderAssetSnapshot,
    playlist_metadata: &'a PlaylistMetadata,
}

impl<'a> ServiceBuildExecutor<'a> {
    /// Create a service build executor over explicit immutable dependencies.
    pub const fn new(
        bible_corpora: &'a BibleCorpusSnapshot,
        file_index: &'a Mutex<LibraryCatalog>,
        render_assets: &'a RenderAssetSnapshot,
        playlist_metadata: &'a PlaylistMetadata,
    ) -> Self {
        Self {
            bible_corpora,
            file_index,
            render_assets,
            playlist_metadata,
        }
    }

    /// Describe the Planning Center lookup that reviewing `request` needs.
    ///
    /// Identities are validated here so a malformed request never reaches
    /// Planning Center.
    pub fn plan_lookup<'r>(
        &self,
        request: &'r BuildRequest,
    ) -> Result<PlanLookup<'r>, BuildServiceError> {
        request::validate_identity("plan_id", &request.plan_id)?;
        match request.service_name.as_deref() {
            Some(service_name) => {
                request::validate_identity("service_name", service_name)?;
                Ok(PlanLookup::Asserted {
                    plan_id: &request.plan_id,
                    service_name,
                })
            }
            None => Ok(PlanLookup::Upcoming {
                plan_id: &request.plan_id,
                days_ahead: self.render_assets.config().plan_lookahead_days(),
            }),
        }
    }

    /// Classify one captured Planning Center snapshot and materialize the same
    /// reviewed artifacts used by every transport.
    pub fn review_snapshot(
        &self,
        request: BuildRequest,
        planning_center: PlanSnapshot,
    ) -> Result<BuildReview, BuildServiceError> {
        self.render_assets.verify_current()?;
        self.bible_corpora.verify_current()?;
        let request = Self::bind_request_identity(request, &planning_center)?;
        let mappings = self.render_assets.config();
        let index_guard = self.lock_library()?;
        let plans = classify::build_plan(
            planning_center.items(),
            mappings,
            Some(&index_guard),
            Some(planning_center.service_name()),
        );
        drop(index_guard);
        self.review_planned_request(
            request,
            &plans,
            mappings.defaults().presentation_size,
            review::ReviewedPlanningCenterSource::captured(planning_center),
        )
    }

    /// Verify and commit exact native artifacts produced by
    /// [`BuildReview::Prepared`]. `current` is a Planning Center snapshot
    /// refreshed after review; any difference from the reviewed snapshot
    /// rejects the commit.
    pub fn build_prepared_request(
        &self,
        reviewed: PreparedBuildRequest,
        current: &PlanSnapshot,
    ) -> Result<BuildServiceResult, BuildServiceError> {
        reviewed.planning_center_source().verify_current(current)?;
        self.commit_prepared_service(reviewed)
    }

    /// Commit without a Planning Center freshness check, for tests whose
    /// reviewed source is offline.
    #[cfg(test)]
    pub(crate) fn build_prepared_offline(
        &self,
        reviewed: PreparedBuildRequest,
    ) -> Result<BuildServiceResult, BuildServiceError> {
        self.commit_prepared_service(reviewed)
    }

    pub(super) fn lock_library(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, LibraryCatalog>, BuildServiceError> {
        self.file_index
            .lock()
            .map_err(|_| BuildServiceError::LibraryCatalogPoisoned)
    }

    pub(super) fn presentation_target(
        &self,
        plan: &ResolvedItemPlan,
    ) -> Result<PathBuf, BuildServiceError> {
        let name = canonical_presentation_name(&plan.playlist_name, plan.slide_type())?;
        Ok(self
            .render_assets
            .locations()
            .presentation_library()
            .join(format!("{name}.pro")))
    }
}

pub(super) fn captured_source_bytes<'a>(
    sources: &'a CapturedSources,
    path: &Path,
) -> Result<&'a [u8], BuildServiceError> {
    sources
        .bytes(path)
        .ok_or_else(|| BuildServiceError::MissingReviewedSource {
            path: path.to_path_buf(),
        })
}

pub(super) fn unresolved_plan_error(plan: &ResolvedItemPlan) -> BuildServiceError {
    BuildServiceError::UnresolvedPlan {
        output_key: plan.output_key.to_string(),
        title: plan.pco_title.clone(),
        reason: plan.reason.clone(),
    }
}

#[cfg(test)]
mod tests;
