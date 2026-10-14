//! Construction and accessors for reviewed, prepared build requests.

use super::{
    BuildDir, BuildServiceError, BuildServiceResult, PlanSnapshot, PlaylistExportMode,
    PlaylistMediaAsset, PreparedBuildRequest, Rendered, ResolvedItemPlan, ReviewedRequest,
    Verified,
};
use std::path::Path;

impl PreparedBuildRequest {
    /// Name the verified build a finished render or an earlier review left.
    pub(in crate::workflow::execute) fn from_verified_build(
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
    #[cfg(test)]
    pub(in crate::workflow::execute) fn into_verified_build(
        self,
    ) -> Result<BuildDir<Verified>, BuildServiceError> {
        if self.build.verify_stage::<Rendered>()? != self.rendered {
            return Err(BuildServiceError::PreparedBuildSuperseded);
        }
        Ok(self.build)
    }

    /// Identity of the rendered artifacts this review produced. A commit that
    /// names it installs these artifacts or nothing.
    pub fn revision(&self) -> String {
        self.rendered.to_string()
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

    /// The report of the verified build.
    pub fn into_result(self) -> BuildServiceResult {
        self.result
    }

    #[cfg(test)]
    pub(crate) fn prepared_artifact_bytes(
        &self,
        target: &Path,
    ) -> Result<Option<Vec<u8>>, BuildServiceError> {
        super::super::rendered_stage::artifact_bytes(&self.build, target)
    }

    #[cfg(test)]
    pub(crate) fn has_reviewed_source(&self, source: &Path) -> bool {
        super::super::rendered_stage::load_prepared(&self.build)
            .is_ok_and(|prepared| prepared.sources.contains(source))
    }
}
