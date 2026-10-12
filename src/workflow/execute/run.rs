//! Execution of an approved plan through one filesystem transaction.

use crate::workflow::build_dir::{BuildDir, Classified, Rendered};
use crate::workflow::report::BuildServiceResult;
use crate::workflow::transaction::BuildFileTransaction;
use crate::workflow::transaction::PreparedFileTransaction;
use proflow_propresenter::library::{LibraryCatalog, PreparedLibraryUpdate};
use proflow_propresenter::text_fit::FontProgramSnapshot;

use super::receipt::{
    prepare_build_receipt, receipt_path_for_playlist, verify_sealed_build_artifacts,
    BuildReceiptError, BuildReceiptInputs,
};
use super::rendered_service::collect_build_warnings;
use super::rendered_stage::{
    load_prepared, write_committed, write_prepared, LoadedPrepared, RenderedArtifacts,
};
use super::review::PreparedBuildRequest;
use super::{BuildServiceError, ServiceBuildExecutor};

impl ServiceBuildExecutor<'_> {
    /// Materialize every native artifact while the reviewed source and output
    /// snapshots are still authoritative.
    pub(super) fn prepare_reviewed_service(
        &self,
        inputs: super::review::ReviewedBuildInputs,
        build: BuildDir<Classified>,
    ) -> Result<PreparedBuildRequest, BuildServiceError> {
        let super::review::ReviewedBuildInputs {
            request,
            reviewed,
            presentation_size,
            backgrounds,
            outputs,
            library_candidates: reviewed_candidates,
        } = inputs;
        let bound_request = request.bound();
        let transaction = BuildFileTransaction::from_reviewed(outputs);
        let mut rendered = self.render_plans(
            reviewed.plans(),
            presentation_size,
            &backgrounds,
            reviewed.sources(),
            transaction,
        )?;
        let expected_playlist_path = proflow_propresenter::playlist::playlist_output_path(
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
            playlist_path: playlist_export.path.clone(),
            receipt_path: receipt_path.clone(),
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
        {
            let catalog = self.lock_library()?;
            // A rendered presentation the catalog cannot take is refused now,
            // not at commit.
            catalog_updates(&catalog, &transaction)?;
            if catalog.scan_candidates()? != reviewed_candidates {
                return Err(BuildServiceError::LibraryCandidatesChanged);
            }
        }
        let build = write_prepared(
            build,
            &RenderedArtifacts {
                transaction: &transaction,
                sources: &sources,
                font_programs: &font_programs,
                result: &result,
            },
        )?;
        // The artifacts now live in the build directory. Dropping the
        // transaction removes the copies staged beside the live targets.
        drop(transaction);
        PreparedBuildRequest::from_verified_build(request, plans, result, build)
    }

    /// Commit the verified build `reviewed` names, reading every artifact and
    /// every fingerprint back from its build directory.
    pub(super) fn commit_prepared_service(
        &self,
        reviewed: PreparedBuildRequest,
    ) -> Result<BuildServiceResult, BuildServiceError> {
        let build = reviewed.into_verified_build()?;
        let reviewed_candidates = self.verify_snapshot_stage(&build)?;
        let LoadedPrepared {
            outputs,
            sources,
            font_programs,
            mut result,
        } = load_prepared(&build)?;

        let mut catalog = self.lock_library()?;
        if catalog
            .scan_candidates()?
            .paths()
            .ne(reviewed_candidates.iter().map(std::path::PathBuf::as_path))
        {
            return Err(BuildServiceError::LibraryCandidatesChanged);
        }
        sources.verify()?;
        self.render_assets.verify_current()?;
        self.bible_corpora.verify_current()?;
        font_programs.verify_current()?;
        let transaction =
            PreparedFileTransaction::restage(&build.stage_directory::<Rendered>(), &outputs)?;
        let committed_catalog =
            catalog.with_prepared_updates(&catalog_updates(&catalog, &transaction)?)?;
        let targets = transaction.commit()?;
        *catalog = committed_catalog;
        drop(catalog);
        // The library is the truth of a commit. A build directory that could
        // not record it is reported, not turned into a failed build.
        if let Err(error) = write_committed(build, &targets) {
            result.warnings.push(format!(
                "the build is committed, but its build directory could not record that: {error}"
            ));
        }
        Ok(result)
    }
}

/// Catalog entries for every presentation the transaction will install.
fn catalog_updates(
    catalog: &LibraryCatalog,
    transaction: &PreparedFileTransaction,
) -> Result<Vec<PreparedLibraryUpdate>, BuildServiceError> {
    Ok(transaction
        .presentation_artifacts()?
        .iter()
        .map(|(path, bytes)| catalog.prepare_owned_update(path, bytes))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect())
}
