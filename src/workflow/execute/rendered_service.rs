//! Rendered-service aggregation and receipt-facing presentation evidence.

use crate::propresenter::inspection::{
    summarize_presentation_structure, PresentationStructureSummary,
};
use crate::propresenter::playlist::PlaylistEntry;
use crate::propresenter::text_fit::{CueTextFitSummary, TextFitContractSummary};
use crate::workflow::plan::{ExpectedMacroPolicy, ExpectedMacroRegion, ResolvedItemPlan};
use crate::workflow::report::{BuildServiceEntry, PlaylistSelectionSummary};
use crate::workflow::transaction::BuildFileTransaction;

use super::BuildServiceError;

pub(super) struct RenderedService {
    pub(super) transaction: BuildFileTransaction,
    pub(super) text_fit_contract: TextFitContractSummary,
    pub(super) playlist_entries: Vec<PlaylistEntry>,
    pub(super) summary_entries: Vec<BuildServiceEntry>,
    pub(super) counts: BuildCounts,
}

impl RenderedService {
    pub(super) fn new(
        transaction: BuildFileTransaction,
        text_fit_contract: TextFitContractSummary,
    ) -> Self {
        Self {
            transaction,
            text_fit_contract,
            playlist_entries: Vec::new(),
            summary_entries: Vec::new(),
            counts: BuildCounts::default(),
        }
    }

    pub(super) fn record(
        &mut self,
        plan: &ResolvedItemPlan,
        presentation_size: crate::propresenter::PresentationSize,
        rendered: RenderedPlan,
    ) -> Result<(), BuildServiceError> {
        let mut expected_presentation =
            crate::workflow::plan::ExpectedPresentationContract::from_plan(plan, presentation_size)
                .map_err(|_| BuildServiceError::ReviewStateInvariant)?;
        match rendered {
            RenderedPlan::Generated {
                playlist_entry,
                mut summary,
                text_fit_evidence,
                resolved_macro_regions,
            } => {
                summary.text_fit_evidence = text_fit_evidence;
                if let Some(regions) = resolved_macro_regions {
                    expected_presentation
                        .as_mut()
                        .ok_or(BuildServiceError::ReviewStateInvariant)?
                        .macros = ExpectedMacroPolicy::Exact(regions);
                }
                let expected = expected_presentation
                    .as_ref()
                    .ok_or(BuildServiceError::ReviewStateInvariant)?;
                let (structure, selection) =
                    presentation_evidence(&playlist_entry, &summary.output_key, expected)?;
                summary.expected_presentation = expected_presentation;
                summary.presentation_structure = Some(structure);
                summary.playlist_selection = selection;
                self.playlist_entries.push(playlist_entry);
                self.summary_entries.push(summary);
                self.counts.generated += 1;
            }
            RenderedPlan::Library {
                playlist_entry,
                mut summary,
            } => {
                let expected = expected_presentation
                    .as_ref()
                    .ok_or(BuildServiceError::ReviewStateInvariant)?;
                let (structure, selection) =
                    presentation_evidence(&playlist_entry, &summary.output_key, expected)?;
                summary.expected_presentation = expected_presentation;
                summary.presentation_structure = Some(structure);
                summary.playlist_selection = selection;
                self.playlist_entries.push(playlist_entry);
                self.summary_entries.push(summary);
                self.counts.library += 1;
            }
            RenderedPlan::Skipped(summary) => {
                debug_assert!(expected_presentation.is_none());
                self.summary_entries.push(summary);
                self.counts.skipped += 1;
            }
        }
        Ok(())
    }
}

fn presentation_evidence(
    entry: &PlaylistEntry,
    output_key: &str,
    expected: &crate::workflow::ExpectedPresentationContract,
) -> Result<
    (
        PresentationStructureSummary,
        Option<PlaylistSelectionSummary>,
    ),
    BuildServiceError,
> {
    let bytes =
        entry
            .embedded_data()
            .ok_or_else(|| BuildServiceError::MissingPresentationEvidence {
                output_key: output_key.to_string(),
            })?;
    let presentation = crate::propresenter::deserialize::decode_presentation_bytes(
        bytes,
        entry.presentation_path(),
    )
    .map_err(|source| BuildServiceError::PresentationEvidenceInspection {
        output_key: output_key.to_string(),
        source,
    })?;
    let summary = summarize_presentation_structure(&presentation);
    if !summary.reference_diagnostics.is_empty() {
        return Err(BuildServiceError::PresentationStructureDiagnostics {
            output_key: output_key.to_string(),
            diagnostics: summary.reference_diagnostics,
        });
    }
    let mut effective = presentation.clone();
    let selection = entry
        .selected_arrangement()
        .map(
            |selected| -> Result<PlaylistSelectionSummary, BuildServiceError> {
                let native_uuid =
                    crate::propresenter::arrangement::selectable_arrangement_by_identity(
                        &presentation,
                        selected.uuid(),
                        selected.name(),
                    )
                    .map_err(|source| BuildServiceError::PresentationSelectionEvidence {
                        output_key: output_key.to_string(),
                        arrangement: selected.name().to_string(),
                        source,
                    })?
                    .native_uuid()
                    .cloned()
                    .ok_or_else(|| BuildServiceError::PresentationSelectionEvidence {
                        output_key: output_key.to_string(),
                        arrangement: selected.name().to_string(),
                        source: crate::propresenter::arrangement::ArrangementSelectionError::Unavailable,
                    })?;
                effective.selected_arrangement = Some(native_uuid);
                let operator_cue_indexes =
                    crate::propresenter::arrangement::checked_operator_cue_indices(&effective)
                        .map_err(|source| BuildServiceError::PresentationTraversalEvidence {
                            output_key: output_key.to_string(),
                            source,
                        })?;
                Ok(PlaylistSelectionSummary {
                    arrangement_uuid: selected.uuid().to_string(),
                    arrangement_name: selected.name().to_string(),
                    operator_cue_indexes,
                })
            },
        )
        .transpose()?;
    super::presentation_contract::validate_final_presentation(
        &effective,
        &summary,
        entry
            .selected_arrangement()
            .map(crate::propresenter::playlist::SelectedArrangement::name),
        expected,
    )
    .map_err(|source| BuildServiceError::PresentationContract {
        output_key: output_key.to_string(),
        source,
    })?;
    Ok((summary, selection))
}

#[derive(Default)]
pub(super) struct BuildCounts {
    pub(super) generated: usize,
    pub(super) library: usize,
    pub(super) skipped: usize,
}

pub(super) enum RenderedPlan {
    Generated {
        playlist_entry: PlaylistEntry,
        summary: BuildServiceEntry,
        text_fit_evidence: Vec<CueTextFitSummary>,
        resolved_macro_regions: Option<Vec<ExpectedMacroRegion>>,
    },
    Library {
        playlist_entry: PlaylistEntry,
        summary: BuildServiceEntry,
    },
    Skipped(BuildServiceEntry),
}

pub(super) fn skipped_summary(plan: &ResolvedItemPlan) -> BuildServiceEntry {
    BuildServiceEntry {
        output_key: plan.output_key.to_string(),
        position: plan.position,
        name: plan.pco_title.clone(),
        action: format!("skipped: {}", plan.reason),
        file_path: None,
        slides: None,
        expected_presentation: None,
        presentation_structure: None,
        playlist_selection: None,
        text_fit_evidence: Vec::new(),
        warnings: Vec::new(),
    }
}

pub(super) fn library_summary(plan: &ResolvedItemPlan, file_path: String) -> BuildServiceEntry {
    BuildServiceEntry {
        output_key: plan.output_key.to_string(),
        position: plan.position,
        name: plan.playlist_name.clone(),
        action: "library".to_string(),
        file_path: Some(file_path),
        slides: None,
        expected_presentation: None,
        presentation_structure: None,
        playlist_selection: None,
        text_fit_evidence: Vec::new(),
        warnings: Vec::new(),
    }
}

pub(super) fn edited_summary(
    plan: &ResolvedItemPlan,
    file_path: String,
    slides: usize,
) -> BuildServiceEntry {
    BuildServiceEntry {
        output_key: plan.output_key.to_string(),
        position: plan.position,
        name: plan.playlist_name.clone(),
        action: "edited".to_string(),
        file_path: Some(file_path),
        slides: Some(slides),
        expected_presentation: None,
        presentation_structure: None,
        playlist_selection: None,
        text_fit_evidence: Vec::new(),
        warnings: zero_slide_warnings(slides),
    }
}

pub(super) fn restyled_summary(
    plan: &ResolvedItemPlan,
    file_path: String,
    slides: usize,
) -> BuildServiceEntry {
    BuildServiceEntry {
        output_key: plan.output_key.to_string(),
        position: plan.position,
        name: plan.playlist_name.clone(),
        action: "restyled".to_string(),
        file_path: Some(file_path),
        slides: Some(slides),
        expected_presentation: None,
        presentation_structure: None,
        playlist_selection: None,
        text_fit_evidence: Vec::new(),
        warnings: zero_slide_warnings(slides),
    }
}

pub(super) fn generated_plan(
    plan: &ResolvedItemPlan,
    playlist_entry: PlaylistEntry,
    slides: usize,
    text_fit_evidence: Vec<CueTextFitSummary>,
    resolved_macro_regions: Vec<ExpectedMacroRegion>,
) -> RenderedPlan {
    let file_path = playlist_entry.presentation_path().to_string();
    RenderedPlan::Generated {
        playlist_entry,
        summary: BuildServiceEntry {
            output_key: plan.output_key.to_string(),
            position: plan.position,
            name: plan.playlist_name.clone(),
            action: "generated".to_string(),
            file_path: Some(file_path),
            slides: Some(slides),
            expected_presentation: None,
            presentation_structure: None,
            playlist_selection: None,
            text_fit_evidence: Vec::new(),
            warnings: zero_slide_warnings(slides),
        },
        text_fit_evidence,
        resolved_macro_regions: Some(resolved_macro_regions),
    }
}

fn zero_slide_warnings(slides: usize) -> Vec<String> {
    if slides == 0 {
        vec!["presentation has zero slides".to_string()]
    } else {
        Vec::new()
    }
}

pub(super) fn collect_build_warnings(entries: &[BuildServiceEntry]) -> Vec<String> {
    entries
        .iter()
        .flat_map(|entry| {
            entry
                .warnings
                .iter()
                .map(|warning| format!("{}: {warning}", entry.output_key))
        })
        .collect()
}
