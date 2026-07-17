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
use super::plan::ResolvedItemPlan;
use super::presentation_render::PresentationRenderError;
use super::report::BuildServiceResult;
use crate::bible::BibleService;
use crate::paths::BuildLocations;
use crate::planning_center::PlanningCenterClient;
use crate::project_config::ProjectConfig;
use crate::propresenter::arrangement::RetainOperatorCuesError;
use crate::propresenter::background::{
    ArrangementBackgroundError, BackgroundImageError, OperatorEntryBackgroundError,
};
use crate::propresenter::deserialize::ProPresenterError;
use crate::propresenter::library::{LibraryCatalog, LibraryCatalogError};
use crate::propresenter::macros::MacroApplyError;
use crate::propresenter::playlist::{
    canonical_presentation_name, CanonicalPresentationNameError, PlaylistError, PlaylistMetadata,
};
use crate::propresenter::serialize::SerializeError;

mod overrides;
mod playlist_output;
mod presentation_output;
mod render_assets;
mod request;
mod review;
mod run;

pub use overrides::{EntryOverride, OverrideAction, OverrideSlideType};
pub use render_assets::{
    RenderAssetIssue, RenderAssetIssues, RenderAssetSnapshot, RenderAssetSnapshotError,
    ThemeSlideSizeProblem,
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
    /// A contextual workflow failure that has no more specific typed variant.
    #[error("{0}")]
    Message(String),
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
    /// An entry override did not change any semantic property.
    #[error("override for output_key '{output_key}' contains no decision")]
    EmptyOverride {
        /// Stable plan identity targeted by the empty override.
        output_key: String,
    },
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
    /// Scripture source data could not be loaded or queried.
    #[error(transparent)]
    Bible(#[from] crate::error::Error),
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
    #[error("presentation render failed: {0}")]
    PresentationRender(String),
    /// A source file changed after operator review.
    #[error(transparent)]
    SourceReview(#[from] SourceReviewError),
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

impl BuildServiceError {
    pub(super) fn message(msg: impl Into<String>) -> Self {
        Self::Message(msg.into())
    }
}

impl From<PresentationRenderError> for BuildServiceError {
    fn from(error: PresentationRenderError) -> Self {
        Self::PresentationRender(error.to_string())
    }
}

/// Shared executor for full-service builds.
///
/// Every runtime dependency is immutable and explicit. Semantic planning stays
/// pure; IO begins only at identity resolution, review capture, and execution.
pub struct ServiceBuildExecutor<'a> {
    pco_client: &'a PlanningCenterClient,
    bible_service: &'a Arc<Mutex<BibleService>>,
    file_index: &'a Arc<Mutex<LibraryCatalog>>,
    render_assets: &'a RenderAssetSnapshot,
    playlist_metadata: &'a PlaylistMetadata,
}

impl<'a> ServiceBuildExecutor<'a> {
    /// Create a service build executor over explicit immutable dependencies.
    pub const fn new(
        pco_client: &'a PlanningCenterClient,
        bible_service: &'a Arc<Mutex<BibleService>>,
        file_index: &'a Arc<Mutex<LibraryCatalog>>,
        render_assets: &'a RenderAssetSnapshot,
        playlist_metadata: &'a PlaylistMetadata,
    ) -> Self {
        Self {
            pco_client,
            bible_service,
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
        let mappings = self.render_assets.config();
        let request = self.resolve_request_identity(request).await?;
        let items = self
            .pco_client
            .get_service_items(&request.plan_id)
            .await
            .map_err(|error| BuildServiceError::message(error.to_string()))?;
        let index_guard = self.file_index.lock().await;
        let plans = classify::build_plan(
            &items,
            mappings,
            Some(&index_guard),
            request.service_name.as_deref(),
        );
        drop(index_guard);
        let prepared = self
            .review_build_request(request, &plans, mappings.defaults().presentation_size)
            .await?
            .into_prepared()?;
        self.build_prepared_request(prepared).await
    }

    /// Verify and commit exact native artifacts produced by
    /// [`BuildReview::Prepared`].
    pub(crate) async fn build_prepared_request(
        &self,
        reviewed: PreparedBuildRequest,
    ) -> Result<BuildServiceResult, BuildServiceError> {
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

pub(super) fn captured_source_bytes<'a>(
    sources: &'a CapturedSources,
    path: &Path,
) -> Result<&'a [u8], BuildServiceError> {
    sources.bytes(path).ok_or_else(|| {
        BuildServiceError::message(format!(
            "reviewed source capture has no bytes for '{}'",
            path.display()
        ))
    })
}

pub(super) fn unresolved_plan_error(plan: &ResolvedItemPlan) -> BuildServiceError {
    BuildServiceError::message(format!(
        "build blocked by unresolved item {} ('{}'): {}",
        plan.output_key, plan.pco_title, plan.reason
    ))
}

#[cfg(test)]
mod tests;
