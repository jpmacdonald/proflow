//! Review capture and approval state transitions.

use std::path::Path;

use crate::planning_center::{PlanFreshnessError, PlanSnapshot};
use proflow_propresenter::library::LibraryCandidates;
use proflow_propresenter::playlist::{
    playlist_output_path, PlaylistExportMode, PlaylistMediaAsset,
};
use proflow_propresenter::PresentationSize;

use super::build_stages::ReviewedDecisions;
use super::overrides::resolve_requested_plans;
use super::receipt::receipt_path_for_playlist;
use super::request::{BoundBuildRequest, BuildRequest, RequestError};
use super::{BuildServiceError, ServiceBuildExecutor};
use crate::workflow::build_dir::{BuildDir, Rendered, SealDigest, Verified};
use crate::workflow::plan::{ReadyAction, ResolvedItemPlan};
use crate::workflow::report::BuildServiceResult;

mod path_ownership;
mod scripture_excerpt;
mod source_capture;

use path_ownership::{validate_reviewed_path_ownership, PlannedOutputTarget, ReviewedOutputOwner};
use scripture_excerpt::reconcile_description_scripture_excerpts;
use source_capture::{reviewed_theme_media_paths, validate_reviewed_background_bindings};
pub(super) use source_capture::{ReviewedBackgroundPath, ReviewedBuildInputs};

/// Result of resolving and reviewing one build request.
///
/// Only [`Self::Prepared`] carries a value accepted by the build boundary.
/// Unresolved decisions therefore cannot compile as executable input.
#[derive(Debug)]
pub enum BuildReview {
    /// At least one semantic item still requires an operator decision.
    NeedsReview(Box<NeedsReviewBuildRequest>),
    /// Every native presentation and the playlist package were materialized.
    Prepared(Box<PreparedBuildRequest>),
}

/// A non-executable review retained only for operator-facing diagnostics.
#[derive(Debug)]
pub struct NeedsReviewBuildRequest {
    request: ReviewedRequest,
    plans: Vec<ResolvedItemPlan>,
}

/// One fully resolved review whose native artifacts are verified on disk.
///
/// The artifacts, the bytes they replace, and every fingerprint commit checks
/// live in the build directory this value names. What it holds in memory is
/// only what a caller displays, plus the identity of the rendered stage it was
/// made from: commit refuses a build directory whose rendered stage is no
/// longer that one.
///
/// Fields are private so callers cannot manufacture an executable request from
/// plans that did not pass capture, rendering, packaging, and transaction seal.
#[derive(Debug)]
pub struct PreparedBuildRequest {
    request: ReviewedRequest,
    plans: Vec<ResolvedItemPlan>,
    result: BuildServiceResult,
    build: BuildDir<Verified>,
    rendered: SealDigest,
}

/// Planning Center source captured before classification and retained through
/// the only executable state transition.
#[derive(Debug)]
pub(super) struct ReviewedPlanningCenterSource {
    snapshot: PlanSnapshot,
}

/// One checked request bound to the exact Planning Center source from which
/// its service identity and semantic plans were derived.
#[derive(Debug)]
pub(super) struct ReviewedRequest {
    request: BoundBuildRequest,
    planning_center: ReviewedPlanningCenterSource,
}

impl ReviewedRequest {
    fn new(
        request: BoundBuildRequest,
        planning_center: ReviewedPlanningCenterSource,
    ) -> Result<Self, BuildServiceError> {
        if request.plan_id != planning_center.snapshot().plan_id() {
            return Err(BuildServiceError::from(
                RequestError::PlanningCenterSnapshotIdentity {
                    requested: request.plan_id,
                    captured: planning_center.snapshot().plan_id().to_string(),
                },
            ));
        }
        Ok(Self {
            request,
            planning_center,
        })
    }

    pub(super) const fn bound(&self) -> &BoundBuildRequest {
        &self.request
    }

    pub(super) const fn planning_center_source(&self) -> &ReviewedPlanningCenterSource {
        &self.planning_center
    }

    #[cfg(test)]
    pub(super) fn offline(request: BuildRequest) -> Result<Self, BuildServiceError> {
        let planning_center = ReviewedPlanningCenterSource::offline(&request);
        Self::new(BoundBuildRequest::try_from(request)?, planning_center)
    }
}

impl ReviewedPlanningCenterSource {
    pub(super) const fn captured(snapshot: PlanSnapshot) -> Self {
        Self { snapshot }
    }

    #[cfg(test)]
    pub(super) fn offline(request: &BuildRequest) -> Self {
        Self {
            snapshot: PlanSnapshot::offline(
                &request.plan_id,
                request.service_name.as_deref().unwrap_or("Offline service"),
            ),
        }
    }

    pub(super) const fn snapshot(&self) -> &PlanSnapshot {
        &self.snapshot
    }

    pub(super) fn verify_current(&self, current: &PlanSnapshot) -> Result<(), PlanFreshnessError> {
        self.snapshot.verify_current(current)
    }
}

impl BuildReview {
    const fn request(&self) -> &BoundBuildRequest {
        match self {
            Self::NeedsReview(review) => &review.request.request,
            Self::Prepared(prepared) => &prepared.request.request,
        }
    }

    const fn planning_center(&self) -> &ReviewedPlanningCenterSource {
        match self {
            Self::NeedsReview(review) => &review.request.planning_center,
            Self::Prepared(prepared) => &prepared.request.planning_center,
        }
    }

    /// Return the exact effective plans rendered in the operator preview.
    pub fn plans(&self) -> &[ResolvedItemPlan] {
        match self {
            Self::NeedsReview(review) => &review.plans,
            Self::Prepared(prepared) => &prepared.plans,
        }
    }

    /// Exact materialized build report when every presentation and package has
    /// already crossed the prepared boundary.
    pub fn materialized_result(&self) -> Option<&crate::workflow::report::BuildServiceResult> {
        match self {
            Self::Prepared(prepared) => Some(&prepared.result),
            Self::NeedsReview(_) => None,
        }
    }

    /// Stable Planning Center plan identity bound to this request.
    pub fn plan_id(&self) -> &str {
        &self.request().plan_id
    }

    /// Planning Center service type resolved before preview approval.
    pub fn service_name(&self) -> &str {
        &self.request().service_name
    }

    /// Final playlist display/file identity resolved before preview approval.
    pub fn playlist_name(&self) -> &str {
        &self.request().playlist_name
    }

    /// Planning Center title captured before classification.
    pub fn plan_title(&self) -> &str {
        self.planning_center().snapshot().plan_title()
    }

    /// Scheduled service date captured before classification.
    pub const fn date(&self) -> chrono::DateTime<chrono::Utc> {
        self.planning_center().snapshot().date()
    }

    /// Playlist package mode included in the reviewed request.
    pub const fn playlist_export_mode(&self) -> PlaylistExportMode {
        match self {
            Self::NeedsReview(review) => review.request.request.playlist_export.mode(),
            Self::Prepared(prepared) => prepared.request.request.playlist_export.mode(),
        }
    }

    /// Additional portable-media sources explicitly included in the request.
    pub fn additional_media_assets(&self) -> &[PlaylistMediaAsset] {
        self.request().playlist_export.media_assets()
    }

    /// Consume a resolved review, or return the first unresolved semantic item.
    pub fn into_prepared(self) -> Result<PreparedBuildRequest, BuildServiceError> {
        match self {
            Self::Prepared(prepared) => Ok(*prepared),
            Self::NeedsReview(review) => {
                let plan = review
                    .plans
                    .iter()
                    .find(|plan| plan.needs_review())
                    .ok_or(BuildServiceError::ReviewStateInvariant)?;
                Err(super::unresolved_plan_error(plan))
            }
        }
    }
}

impl PreparedBuildRequest {
    /// Name the verified build a finished render or an earlier review left.
    pub(super) fn from_verified_build(
        request: ReviewedRequest,
        plans: Vec<ResolvedItemPlan>,
        result: BuildServiceResult,
        build: BuildDir<Verified>,
    ) -> Result<Self, BuildServiceError> {
        let rendered = build.verify_stage::<Rendered>()?;
        Ok(Self {
            request,
            plans,
            result,
            build,
            rendered,
        })
    }

    /// Hand the build directory to commit, if its rendered stage is still the
    /// one this value was made from.
    pub(super) fn into_verified_build(self) -> Result<BuildDir<Verified>, BuildServiceError> {
        if self.build.verify_stage::<Rendered>()? != self.rendered {
            return Err(BuildServiceError::PreparedBuildSuperseded);
        }
        self.build.verify_stage::<Verified>()?;
        Ok(self.build)
    }

    pub(super) const fn planning_center_source(&self) -> &ReviewedPlanningCenterSource {
        &self.request.planning_center
    }

    /// The Planning Center snapshot these artifacts were reviewed against.
    pub const fn plan_snapshot(&self) -> &PlanSnapshot {
        self.request.planning_center.snapshot()
    }

    /// Exact effective plans from which the artifacts were prepared.
    pub fn plans(&self) -> &[ResolvedItemPlan] {
        &self.plans
    }

    /// Stable Planning Center plan identity bound to these artifacts.
    pub fn plan_id(&self) -> &str {
        &self.request.request.plan_id
    }

    /// Planning Center service type bound before preparation.
    pub fn service_name(&self) -> &str {
        &self.request.request.service_name
    }

    /// Final playlist display/file identity bound before preparation.
    pub fn playlist_name(&self) -> &str {
        &self.request.request.playlist_name
    }

    /// Playlist package mode bound before preparation.
    pub const fn playlist_export_mode(&self) -> PlaylistExportMode {
        self.request.request.playlist_export.mode()
    }

    /// Additional portable-media sources explicitly bound during review.
    pub fn additional_media_assets(&self) -> &[PlaylistMediaAsset] {
        self.request.request.playlist_export.media_assets()
    }

    /// Directory holding every stage of this build.
    pub fn build_directory(&self) -> &Path {
        self.build.path()
    }

    #[cfg(test)]
    pub(crate) fn prepared_artifact_bytes(
        &self,
        target: &Path,
    ) -> Result<Option<Vec<u8>>, BuildServiceError> {
        super::rendered_stage::artifact_bytes(&self.build, target)
    }

    #[cfg(test)]
    pub(crate) fn has_reviewed_source(&self, source: &Path) -> bool {
        super::rendered_stage::load_prepared(&self.build)
            .is_ok_and(|prepared| prepared.sources.contains(source))
    }

    /// A verified build with no artifacts, for tests of the bookkeeping
    /// around prepared builds. It belongs to no runtime and cannot commit.
    #[cfg(test)]
    pub(crate) fn offline_test(
        builds_root: &Path,
        plan_id: &str,
        service_name: &str,
        playlist_name: &str,
    ) -> Result<Self, BuildServiceError> {
        use crate::workflow::approval::{OutputManifest, SourceManifest};
        use crate::workflow::build_dir::{BuildInputs, BuildPlanId, Classified, Snapshotted};
        use crate::workflow::transaction::BuildFileTransaction;
        use proflow_propresenter::text_fit::{FontProgramSnapshot, TextFitContractSummary};

        let request = ReviewedRequest::offline(BuildRequest {
            plan_id: plan_id.to_string(),
            service_name: Some(service_name.to_string()),
            playlist_name: Some(playlist_name.to_string()),
            ..BuildRequest::default()
        })?;
        let build: BuildDir<Snapshotted> = BuildDir::open(
            builds_root,
            &BuildPlanId::new(plan_id)?,
            &BuildInputs {
                plan_snapshot: b"offline",
                render_assets: b"offline",
                bible_corpora: b"offline",
                library: b"offline",
                review_decisions: playlist_name.as_bytes(),
            },
        )?
        .write(|_| Ok(()))?;
        let build: BuildDir<Classified> = build.write(|_| Ok(()))?;
        let result = BuildServiceResult {
            playlist_path: std::path::PathBuf::new(),
            receipt_path: std::path::PathBuf::new(),
            receipt_revision: String::new(),
            text_fit_contract: TextFitContractSummary::diagnostic(),
            package_mode: PlaylistExportMode::LibraryLinks,
            media_asset_count: 0,
            total_items: 0,
            entries: Vec::new(),
            generated_count: 0,
            library_count: 0,
            skipped_count: 0,
            warnings: Vec::new(),
        };
        let transaction =
            BuildFileTransaction::from_reviewed(OutputManifest::capture(std::iter::empty())?)
                .seal()?;
        let build = super::rendered_stage::write_prepared(
            build,
            &super::rendered_stage::RenderedArtifacts {
                transaction: &transaction,
                sources: &SourceManifest::recorded(std::iter::empty()),
                font_programs: &FontProgramSnapshot::diagnostic(),
                result: &result,
            },
        )?;
        Self::from_verified_build(request, Vec::new(), result, build)
    }
}

impl ServiceBuildExecutor<'_> {
    /// Bind a fully resolved request to the effective plans, source bytes, and
    /// output target states shown during preview.
    pub(super) fn review_planned_request(
        &self,
        mut request: BuildRequest,
        plans: &[ResolvedItemPlan],
        presentation_size: PresentationSize,
        planning_center: ReviewedPlanningCenterSource,
        library_candidates: LibraryCandidates,
    ) -> Result<BuildReview, BuildServiceError> {
        self.render_assets.verify_current()?;
        let skip_output_keys = std::mem::take(&mut request.skip_output_keys);
        let overrides = std::mem::take(&mut request.overrides);
        let request = BoundBuildRequest::try_from(request)?;
        let mut plans = resolve_requested_plans(plans, &skip_output_keys, &overrides)?;
        reconcile_description_scripture_excerpts(&mut plans, self.bible_corpora)?;
        let build = self.classified_build(
            &ReviewedDecisions {
                request: &request,
                skip_output_keys: &skip_output_keys,
                overrides: &overrides,
            },
            planning_center.snapshot(),
            &library_candidates,
            &plans,
        )?;
        if plans.iter().any(ResolvedItemPlan::needs_review) {
            return Ok(BuildReview::NeedsReview(Box::new(
                NeedsReviewBuildRequest {
                    request: ReviewedRequest::new(request, planning_center)?,
                    plans,
                },
            )));
        }
        // A fully resolved review always renders. A render left by an earlier
        // review of the same inputs is not reused: it may predate this binary
        // or an input the hash does not cover.
        let build = build.clear_later_stages()?;
        let mut additional_sources = request
            .playlist_export
            .media_assets()
            .iter()
            .map(|asset| asset.source_path.clone())
            .collect::<Vec<_>>();
        if matches!(
            request.playlist_export.mode(),
            PlaylistExportMode::PortableImport
        ) {
            for path in reviewed_theme_media_paths(self, &plans)? {
                additional_sources.push(path);
            }
        }
        let (output_targets, existing_generated_targets) =
            self.planned_output_targets(&request, &plans)?;
        validate_reviewed_path_ownership(
            &plans,
            self.render_assets.locations().project_data_root(),
            &additional_sources,
            &output_targets,
        )?;
        additional_sources.extend(existing_generated_targets);
        let reviewed = ReviewedBuildInputs::capture(
            ReviewedRequest::new(request, planning_center)?,
            plans,
            presentation_size,
            self.render_assets.locations().project_data_root(),
            self.render_assets.locations().propresenter_root(),
            additional_sources,
            output_targets.into_iter().map(|target| target.physical),
            library_candidates,
        )?;
        validate_reviewed_background_bindings(&reviewed)?;
        let prepared = self.prepare_reviewed_service(reviewed, build)?;
        Ok(BuildReview::Prepared(Box::new(prepared)))
    }

    /// Every path the build will write, with the plan that owns it, and the
    /// generated targets that already exist and are therefore also sources.
    fn planned_output_targets(
        &self,
        request: &BoundBuildRequest,
        plans: &[ResolvedItemPlan],
    ) -> Result<(Vec<PlannedOutputTarget>, Vec<std::path::PathBuf>), BuildServiceError> {
        let playlist_path = playlist_output_path(
            self.render_assets.locations().playlist_output(),
            &request.playlist_name,
        );
        let receipt_path = receipt_path_for_playlist(&playlist_path)?;
        let mut output_targets = vec![
            PlannedOutputTarget::resolve(ReviewedOutputOwner::Receipt, &receipt_path)?,
            PlannedOutputTarget::resolve(ReviewedOutputOwner::Playlist, &playlist_path)?,
        ];
        let mut existing_generated_targets = Vec::new();
        for plan in plans {
            match plan.ready_action() {
                Some(
                    ReadyAction::RestyleExisting { file_path, .. }
                    | ReadyAction::EditDescription { file_path, .. },
                ) => {
                    output_targets.push(PlannedOutputTarget::resolve(
                        ReviewedOutputOwner::Plan(plan.output_key.to_string()),
                        file_path,
                    )?);
                }
                Some(
                    ReadyAction::GenerateDescription { .. }
                    | ReadyAction::GenerateScripture { .. }
                    | ReadyAction::GenerateTitle { .. },
                ) => {
                    let target = self.presentation_target(plan)?;
                    if target.is_file() {
                        existing_generated_targets.push(target.clone());
                    }
                    output_targets.push(PlannedOutputTarget::resolve(
                        ReviewedOutputOwner::Plan(plan.output_key.to_string()),
                        &target,
                    )?);
                }
                Some(ReadyAction::UseExisting { .. }) | None => {}
            }
        }
        Ok((output_targets, existing_generated_targets))
    }

    #[cfg(test)]
    pub(crate) fn review_build_request(
        &self,
        request: BuildRequest,
        plans: &[ResolvedItemPlan],
        presentation_size: PresentationSize,
    ) -> Result<BuildReview, BuildServiceError> {
        let planning_center = ReviewedPlanningCenterSource::offline(&request);
        let library_candidates = self.lock_library()?.scan_candidates()?;
        self.review_planned_request(
            request,
            plans,
            presentation_size,
            planning_center,
            library_candidates,
        )
    }
}
