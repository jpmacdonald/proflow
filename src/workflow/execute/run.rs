//! Execution of an approved plan through one filesystem transaction.

#[cfg(test)]
use std::path::Path;

use crate::propresenter::library::PreparedLibraryUpdate;
use crate::propresenter::text_fit::FontProgramSnapshot;
#[cfg(test)]
use crate::propresenter::text_fit::TextFitContractSummary;
use crate::workflow::report::BuildServiceResult;
use crate::workflow::transaction::BuildFileTransaction;
use crate::workflow::transaction::PreparedFileTransaction;

use super::receipt::{
    prepare_build_receipt, receipt_path_for_playlist, verify_sealed_build_artifacts,
    BuildReceiptError, BuildReceiptInputs,
};
use super::rendered_service::collect_build_warnings;
use super::review::PreparedBuildRequest;
use super::{BuildServiceError, ServiceBuildExecutor};

impl ServiceBuildExecutor<'_> {
    /// Materialize every native artifact while the reviewed source and output
    /// snapshots are still authoritative.
    pub(super) async fn prepare_reviewed_service(
        &self,
        inputs: super::review::ReviewedBuildInputs,
    ) -> Result<PreparedBuildRequest, BuildServiceError> {
        let super::review::ReviewedBuildInputs {
            request,
            reviewed,
            presentation_size,
            backgrounds,
            outputs,
        } = inputs;
        let bound_request = request.bound();
        let transaction = BuildFileTransaction::from_reviewed(outputs);
        let mut rendered = self
            .render_plans(
                reviewed.plans(),
                presentation_size,
                &backgrounds,
                reviewed.sources(),
                transaction,
            )
            .await?;
        let expected_playlist_path = crate::propresenter::playlist::playlist_output_path(
            self.render_assets.locations().playlist_output(),
            &bound_request.playlist_name,
        );
        let receipt_path = receipt_path_for_playlist(&expected_playlist_path)?;
        let staged_receipt = rendered.transaction.stage_reviewed(&receipt_path)?;
        let playlist_export = self.stage_playlist(
            bound_request,
            reviewed.sources(),
            &rendered.playlist_entries,
            &mut rendered.transaction,
        )?;

        let mut warnings = collect_build_warnings(&rendered.summary_entries);
        warnings.extend(playlist_export.evidence.warnings().iter().cloned());
        let (plans, sources) = reviewed.into_verified_parts()?;
        let font_programs = FontProgramSnapshot::capture(
            rendered
                .summary_entries
                .iter()
                .flat_map(|entry| &entry.text_fit_evidence),
        )?;
        let staged_artifacts = rendered
            .transaction
            .staged_artifacts()
            .map_err(|source| BuildReceiptError::InspectStagedArtifacts { source })?;
        let receipt = prepare_build_receipt(BuildReceiptInputs {
            receipt_target: &receipt_path,
            playlist_target: &playlist_export.path,
            playlist_name: &bound_request.playlist_name,
            package_mode: bound_request.playlist_export.mode(),
            planning_center: request.planning_center_source().snapshot(),
            playlist_metadata: self.playlist_metadata,
            playlist_export: &playlist_export.evidence,
            render_assets: self.render_assets.fingerprint(),
            text_fit_contract: &rendered.text_fit_contract,
            sources: &sources,
            staged: &staged_artifacts,
            entries: &rendered.summary_entries,
        })?;
        receipt.write_to(&staged_receipt)?;
        let result = BuildServiceResult {
            playlist_path: playlist_export.path.display().to_string(),
            receipt_path: receipt_path.display().to_string(),
            receipt_revision: receipt.revision().to_string(),
            text_fit_contract: rendered.text_fit_contract,
            package_mode: bound_request.playlist_export.mode(),
            media_asset_count: playlist_export.evidence.media_asset_count(),
            total_items: rendered.playlist_entries.len(),
            entries: rendered.summary_entries,
            generated_count: rendered.counts.generated,
            library_count: rendered.counts.library,
            skipped_count: rendered.counts.skipped,
            warnings,
        };

        let transaction = rendered.transaction.seal()?;
        verify_sealed_build_artifacts(&receipt_path, &receipt, &staged_artifacts, &transaction)?;
        self.render_assets.verify_current()?;
        let artifacts = transaction.presentation_artifacts()?;
        let catalog_updates = {
            let catalog = self.file_index.lock().await;
            artifacts
                .iter()
                .map(|(path, bytes)| catalog.prepare_owned_update(path, bytes))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .flatten()
                .collect()
        };

        let prepared = PreparedService {
            transaction,
            catalog_updates,
            font_programs,
            result,
        };
        Ok(PreparedBuildRequest::from_materialized(
            request, plans, sources, prepared,
        ))
    }

    pub(super) async fn commit_prepared_service(
        &self,
        reviewed: PreparedBuildRequest,
    ) -> Result<BuildServiceResult, BuildServiceError> {
        let (sources, prepared) = reviewed.into_commit_parts();
        let PreparedService {
            transaction,
            catalog_updates,
            font_programs,
            result,
        } = prepared;
        let mut catalog = self.file_index.lock().await;
        let committed_catalog = catalog.with_prepared_updates(&catalog_updates)?;
        sources.verify()?;
        self.render_assets.verify_current()?;
        font_programs.verify_current()?;
        transaction.commit()?;
        *catalog = committed_catalog;
        drop(catalog);
        Ok(result)
    }
}

pub(super) struct PreparedService {
    transaction: PreparedFileTransaction,
    catalog_updates: Vec<PreparedLibraryUpdate>,
    font_programs: FontProgramSnapshot,
    result: BuildServiceResult,
}

#[cfg(test)]
impl PreparedService {
    pub(super) fn artifact_bytes(&self, target: &Path) -> std::io::Result<Option<Vec<u8>>> {
        self.transaction.staged_bytes_for(target)
    }

    pub(super) fn offline_test(transaction: PreparedFileTransaction) -> Self {
        Self {
            transaction,
            catalog_updates: Vec::new(),
            font_programs: FontProgramSnapshot::diagnostic(),
            result: BuildServiceResult {
                playlist_path: String::new(),
                receipt_path: String::new(),
                receipt_revision: String::new(),
                text_fit_contract: TextFitContractSummary::diagnostic(),
                package_mode: crate::propresenter::playlist::PlaylistExportMode::LibraryLinks,
                media_asset_count: 0,
                total_items: 0,
                entries: Vec::new(),
                generated_count: 0,
                library_count: 0,
                skipped_count: 0,
                warnings: Vec::new(),
            },
        }
    }
}

impl std::fmt::Debug for PreparedService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedService")
            .field("transaction", &self.transaction)
            .field("playlist_path", &self.result.playlist_path)
            .finish_non_exhaustive()
    }
}
