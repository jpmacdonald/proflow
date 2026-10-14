//! Review-bound service build execution.
//!
//! The public flow is intentionally short: resolve transport identities,
//! classify semantic plans, capture an immutable review snapshot, approve that
//! snapshot, then execute it through one filesystem transaction. The child
//! modules own those phase details without introducing runtime wrapper layers.

mod error;
pub use error::BuildServiceError;

use std::path::{Path, PathBuf};

use std::sync::Mutex;
use thiserror::Error;

use super::approval::{CapturedSources, OutputReviewError, SourceReviewError};
use super::classify;
use super::plan::{PlanSemanticsError, ResolvedItemPlan};
use super::presentation_render::PresentationRenderError;
#[cfg(test)]
use super::report::BuildServiceResult;
use crate::bible::{BibleCorpusError, BibleCorpusSnapshot};
use crate::planning_center::identity::PlanIdentityError;
use crate::planning_center::{PlanCaptureError, PlanFreshnessError, PlanLookup, PlanSnapshot};
use propresenter::arrangement::RetainOperatorCuesError;
use propresenter::background::{
    ArrangementBackgroundError, BackgroundImageError, OperatorEntryBackgroundError,
};
use propresenter::deserialize::ProPresenterError;
use propresenter::generated_document::GeneratedPresentationError;
use propresenter::library::{LibraryCatalog, LibraryCatalogError};
use propresenter::macros::MacroApplyError;
use propresenter::playlist::{
    canonical_presentation_name, CanonicalPresentationNameError, PlaylistError, PlaylistMetadata,
    SelectedArrangementError,
};
use propresenter::serialize::SerializeError;
use propresenter::text_fit::FontProgramFreshnessError;

mod build_stages;
mod decision_options;
mod decisions;
mod overrides;
mod plan_rendering;
mod playlist_output;
mod presentation_contract;
mod presentation_output;
mod receipt;
mod render_assets;
mod rendered_service;
mod rendered_stage;
mod request;
pub(crate) mod restyle_text_fit;
mod review;
mod run;

pub use decisions::{
    playlist_export_intent, resolve_override_action, DecidedBy, DecisionChoice, DecisionError,
    DecisionKind, DecisionOption, EntryOverrideAction, ItemDecision, MediaDecision, Remembered,
    ReviewDecisions,
};
pub use overrides::{EntryOverride, OverrideAction, OverrideSlideType};
pub use presentation_contract::PresentationContractError;
pub use render_assets::{
    NativeAssetDigest, RenderAssetFingerprint, RenderAssetFingerprintError,
    RenderAssetFreshnessError, RenderAssetIssue, RenderAssetIssues, RenderAssetSnapshot,
    RenderAssetSnapshotError, ThemeSlideSizeProblem,
};
pub use request::{BuildRequest, RequestError};
pub use review::{BuildReview, NeedsReviewBuildRequest, OpenDecision, PreparedBuildRequest};

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
use propresenter::unstable_native::rv_data;
#[cfg(test)]
use uuid::Uuid;

/// Shared executor for full-service builds.
///
/// Every runtime dependency is immutable and explicit, and nothing here talks
/// to Planning Center: the executor describes the lookup it needs through
/// [`Self::plan_lookup`] and consumes the captured [`PlanSnapshot`]. The
/// boundary that performs those reads lives in [`crate::workstation`].
///
/// The only public way to commit is
/// [`crate::workstation::Workstation::commit_recorded`], which fetches the
/// plan from Planning Center for that commit. The fetched snapshot is hashed
/// into the name of the build directory, so a plan that changed after it was
/// built has no build to commit. The step that takes a snapshot from its
/// caller is crate-private.
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

    /// Where the operator's decisions for `plan_id` are recorded.
    pub fn review_decisions_path(&self, plan_id: &str) -> Result<PathBuf, BuildServiceError> {
        Ok(crate::workflow::build_dir::review_decisions_path(
            &self.builds_root(),
            &crate::workflow::build_dir::BuildPlanId::new(plan_id)?,
        ))
    }

    /// The request that reviewing `plan_id` with its recorded decisions needs.
    ///
    /// A plan with no decisions file is reviewed with no decisions.
    pub fn recorded_request(&self, plan_id: &str) -> Result<BuildRequest, BuildServiceError> {
        Ok(
            ReviewDecisions::read(&self.review_decisions_path(plan_id)?)?
                .into_request(plan_id, self.render_assets.config())?,
        )
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
        let mut index_guard = self.lock_library()?;
        // Ambiguity is decided over the library as it is now, not as it was
        // when the process loaded its catalog. The candidate set is scanned
        // before the rebuild: a file that lands in between is then absent
        // from the recorded set and preparation refuses it.
        let library_candidates = index_guard.scan_candidates()?;
        *index_guard = index_guard.refreshed()?;
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
            library_candidates,
        )
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
