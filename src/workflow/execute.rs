//! Review-bound service build execution.
//!
//! The public flow is intentionally short: resolve transport identities,
//! classify semantic plans, capture an immutable review snapshot, approve that
//! snapshot, then execute it through one filesystem transaction. The child
//! modules own those phase details without introducing runtime wrapper layers.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use thiserror::Error;
use tokio::sync::Mutex;

use super::approval::{CapturedSources, OutputReviewError, SourceReviewError};
use super::classify;
use super::plan::PlanSemanticsError;
use super::plan::ResolvedItemPlan;
use super::presentation_render::PresentationRenderError;
use super::report::BuildServiceResult;
use crate::bible::{BibleCorpusError, BibleCorpusSnapshot};
use crate::planning_center::identity::{resolve_plan_identity, PlanIdentityError};
use crate::planning_center::PlanningCenterClient;
use crate::planning_center::{PlanFreshnessError, PlanSnapshot};
use crate::propresenter::arrangement::RetainOperatorCuesError;
use crate::propresenter::background::{
    ArrangementBackgroundError, BackgroundImageError, OperatorEntryBackgroundError,
};
use crate::propresenter::deserialize::ProPresenterError;
use crate::propresenter::generated_document::GeneratedPresentationError;
use crate::propresenter::library::{LibraryCatalog, LibraryCatalogError};
use crate::propresenter::macros::MacroApplyError;
use crate::propresenter::playlist::{
    canonical_presentation_name, CanonicalPresentationNameError, PlaylistError, PlaylistMetadata,
    SelectedArrangementError,
};
use crate::propresenter::serialize::SerializeError;
use crate::propresenter::text_fit::FontProgramFreshnessError;

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
pub use request::BuildRequest;
pub use review::{BuildReview, NeedsReviewBuildRequest, PreparedBuildRequest};

#[cfg(test)]
use self::overrides::{
    apply_override, resolve_requested_plans, validate_request_edits as validate_unique_request_keys,
};
#[cfg(test)]
use self::presentation_output::{parse_bible_version, ReviewedBackgroundAsset};
#[cfg(test)]
use self::request::BoundBuildRequest;
#[cfg(test)]
use super::plan::{ItemKind, PlanDisposition, ReadyAction, RenderStyle, ResolvedBackground};
#[cfg(test)]
use crate::propresenter::generated::rv_data;
#[cfg(test)]
use uuid::Uuid;

/// Errors raised while resolving, reviewing, or executing a service build.
#[derive(Debug, Error)]
pub enum BuildServiceError {
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
    /// A requested translation identifier is not supported by the local Bible data.
    #[error("unsupported Bible version '{0}'")]
    UnsupportedBibleVersion(String),
    /// A lookup returned only part of the requested scripture range.
    #[error("scripture lookup for '{reference}' is missing verses {verses:?}")]
    MissingVerses {
        /// Requested scripture reference.
        reference: String,
        /// Verse numbers absent from the selected Bible data.
        verses: Vec<u32>,
    },
    /// A required request identity was absent at the checked transition.
    #[error("build request {field} must be a non-empty resolved value before review")]
    UnresolvedIdentity {
        /// Missing or empty request field.
        field: &'static str,
    },
    /// A request identity could not be preserved as an exact lookup key.
    #[error("{field} must be non-empty, unpadded, and contain no control characters")]
    InvalidIdentity {
        /// Invalid request, override, path, arrangement, or title field.
        field: &'static str,
    },
    /// Planning Center did not return the requested plan in the supported
    /// service-build lookahead window.
    #[error("plan '{plan_id}' was not found in the next {days_ahead} days")]
    PlanNotFound {
        /// Stable Planning Center plan identity.
        plan_id: String,
        /// Lookahead window searched for authoritative metadata.
        days_ahead: i64,
    },
    /// A caller-supplied service type disagreed with Planning Center metadata.
    #[error(
        "service_name mismatch for plan '{plan_id}': caller supplied '{supplied}', Planning Center reports '{actual}'"
    )]
    ServiceNameMismatch {
        /// Stable Planning Center plan identity.
        plan_id: String,
        /// Caller-supplied assertion.
        supplied: String,
        /// Authoritative Planning Center service type.
        actual: String,
    },
    /// A captured source was accidentally paired with a different request.
    #[error(
        "Planning Center snapshot identity mismatch: request named '{requested}', captured '{captured}'"
    )]
    PlanningCenterSnapshotIdentity {
        /// Plan identity supplied by the request.
        requested: String,
        /// Plan identity captured from Planning Center.
        captured: String,
    },
    /// Planning Center source inputs changed after preview.
    #[error(transparent)]
    PlanningCenterFreshness(#[from] PlanFreshnessError),
    /// An entry override did not change any semantic property.
    #[error("override for output_key '{output_key}' contains no decision")]
    EmptyOverride {
        /// Stable plan identity targeted by the empty override.
        output_key: String,
    },
    /// The same request key was supplied more than once for one edit kind.
    #[error("duplicate {kind} '{key}'")]
    DuplicateRequestEdit {
        /// Operator edit collection containing the duplicate.
        kind: &'static str,
        /// Repeated stable output key.
        key: String,
    },
    /// One output was requested in mutually exclusive edit collections.
    #[error("output_key '{output_key}' cannot be both skipped and overridden")]
    ConflictingRequestEdits {
        /// Stable output identity with contradictory decisions.
        output_key: String,
    },
    /// The classifier emitted duplicate stable output identities.
    #[error("duplicate plan output_keys: {}", keys.join(", "))]
    DuplicatePlanOutputKeys {
        /// Sorted duplicate identities.
        keys: Vec<String>,
    },
    /// A request attempted to skip outputs absent from the reviewed plan.
    #[error("unknown skip_output_keys: {}", keys.join(", "))]
    UnknownSkipOutputKeys {
        /// Sorted unknown identities.
        keys: Vec<String>,
    },
    /// A request attempted to override outputs absent from the reviewed plan.
    #[error("unknown override output_keys: {}", keys.join(", "))]
    UnknownOverrideOutputKeys {
        /// Sorted unknown identities.
        keys: Vec<String>,
    },
    /// An override action is incompatible with the planned semantic content.
    #[error("override for '{output_key}' cannot {intent}")]
    UnsupportedOverride {
        /// Stable plan identity being overridden.
        output_key: String,
        /// Specific incompatible transition.
        intent: &'static str,
    },
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
    /// An operator decision contradicted the content carried by its planned
    /// presentation action.
    #[error(transparent)]
    PlanSemantics(#[from] PlanSemanticsError),
    /// A portable-media source could not be resolved to one stable file.
    #[error("failed to resolve reviewed media source '{}': {source}", path.display())]
    MediaSource {
        /// Requested media path.
        path: PathBuf,
        /// Filesystem resolution failure.
        #[source]
        source: std::io::Error,
    },
    /// A portable-media source resolved to a non-file path.
    #[error("reviewed media source is not a regular file: {}", path.display())]
    MediaSourceNotFile {
        /// Canonical media path.
        path: PathBuf,
    },
    /// A reviewed presentation source could not be resolved to one stable file.
    #[error("failed to resolve reviewed presentation source '{}': {source}", path.display())]
    PresentationSource {
        /// Requested presentation path.
        path: PathBuf,
        /// Filesystem resolution failure.
        #[source]
        source: std::io::Error,
    },
    /// A reviewed presentation source resolved to a non-file path.
    #[error("reviewed presentation source is not a regular file: {}", path.display())]
    PresentationSourceNotFile {
        /// Canonical presentation path.
        path: PathBuf,
    },
    /// A reviewed presentation source did not use the native `.pro` suffix.
    #[error("reviewed presentation source must be a .pro file: {}", path.display())]
    PresentationSourceExtension {
        /// Canonical presentation path.
        path: PathBuf,
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
        diagnostics: Vec<crate::propresenter::inspection::PresentationReferenceDiagnostic>,
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
    NativeEdit(#[from] crate::propresenter::native_document::NativeEditError),
    /// A checked playlist arrangement could not produce an exact effective
    /// operator traversal for receipt evidence.
    #[error("could not derive effective playlist traversal for '{output_key}': {source}")]
    PresentationTraversalEvidence {
        /// Stable plan output identity.
        output_key: String,
        /// Native arrangement/group/cue traversal failure.
        #[source]
        source: crate::propresenter::arrangement::OperatorTraversalError,
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
        source: crate::propresenter::arrangement::ArrangementSelectionError,
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
    /// A shared application data source could not be loaded or queried.
    #[error(transparent)]
    Application(#[from] crate::error::Error),
    /// Planning Center partial-verse text did not prove one safe local cutoff.
    #[error(transparent)]
    ScriptureExcerpt(#[from] crate::bible::ScriptureExcerptError),
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
    PresentationResize(#[from] crate::propresenter::resolution::PresentationResizeError),
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
        expected: crate::propresenter::PresentationSize,
        /// First concrete mismatch found.
        actual: String,
    },
    /// A filesystem transaction failed.
    #[error("filesystem transaction failed: {0}")]
    Io(#[from] std::io::Error),
}

/// Shared executor for full-service builds.
///
/// Every runtime dependency is immutable and explicit. Semantic planning stays
/// pure; IO begins only at identity resolution, review capture, and execution.
pub struct ServiceBuildExecutor<'a> {
    pco_client: &'a PlanningCenterClient,
    bible_corpora: &'a BibleCorpusSnapshot,
    file_index: &'a Arc<Mutex<LibraryCatalog>>,
    render_assets: &'a RenderAssetSnapshot,
    playlist_metadata: &'a PlaylistMetadata,
}

impl<'a> ServiceBuildExecutor<'a> {
    /// Create a service build executor over explicit immutable dependencies.
    pub const fn new(
        pco_client: &'a PlanningCenterClient,
        bible_corpora: &'a BibleCorpusSnapshot,
        file_index: &'a Arc<Mutex<LibraryCatalog>>,
        render_assets: &'a RenderAssetSnapshot,
        playlist_metadata: &'a PlaylistMetadata,
    ) -> Self {
        Self {
            pco_client,
            bible_corpora,
            file_index,
            render_assets,
            playlist_metadata,
        }
    }

    /// Resolve and classify one Planning Center plan, then build it through the
    /// same review boundary used by explicit previews.
    pub async fn build_service(
        &self,
        request: &BuildRequest,
    ) -> Result<BuildServiceResult, BuildServiceError> {
        let prepared = self
            .review_service_request(request.clone())
            .await?
            .into_prepared()?;
        self.build_prepared_request(prepared).await
    }

    /// Capture one Planning Center snapshot, classify it, and materialize the
    /// same reviewed artifacts used by every transport.
    pub(crate) async fn review_service_request(
        &self,
        request: BuildRequest,
    ) -> Result<BuildReview, BuildServiceError> {
        self.render_assets.verify_current()?;
        self.bible_corpora.verify_current()?;
        let plan_id = request::required_identity("plan_id", request.plan_id.clone())?;
        let planning_center = if let Some(service_name) = request.service_name.as_deref() {
            request::validate_identity("service_name", service_name)?;
            self.pco_client
                .capture_plan_snapshot(&plan_id, service_name)
                .await?
        } else {
            let days_ahead = self.render_assets.config().plan_lookahead_days();
            let (services, plans) = self.pco_client.get_upcoming_services(days_ahead).await?;
            let identity = resolve_plan_identity(&services, &plans, &plan_id, days_ahead)
                .map_err(map_plan_identity_error)?;
            let items = self.pco_client.get_service_items(&plan_id).await?;
            let discovered = PlanSnapshot::from_resolved(identity, items);
            self.pco_client.refresh_plan_snapshot(&discovered).await?
        };
        let request = Self::bind_request_identity(request, &planning_center)?;
        let mappings = self.render_assets.config();
        let index_guard = self.file_index.lock().await;
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
        .await
    }

    /// Verify and commit exact native artifacts produced by
    /// [`BuildReview::Prepared`].
    pub(crate) async fn build_prepared_request(
        &self,
        reviewed: PreparedBuildRequest,
    ) -> Result<BuildServiceResult, BuildServiceError> {
        #[cfg(test)]
        if !reviewed
            .planning_center_source()
            .should_verify_before_commit()
        {
            return self.commit_prepared_service(reviewed).await;
        }

        let current = self
            .pco_client
            .refresh_plan_snapshot(reviewed.planning_center_source().snapshot())
            .await?;
        reviewed.planning_center_source().verify_current(&current)?;
        self.commit_prepared_service(reviewed).await
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

fn map_plan_identity_error(error: PlanIdentityError) -> BuildServiceError {
    match error {
        PlanIdentityError::NotFound {
            plan_id,
            days_ahead,
        } => BuildServiceError::PlanNotFound {
            plan_id,
            days_ahead: days_ahead.get(),
        },
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
