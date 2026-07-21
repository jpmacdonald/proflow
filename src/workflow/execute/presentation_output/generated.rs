//! Rendering and finalization for generated and edited text presentations.

use std::path::Path;

use crate::propresenter::deserialize::decode_presentation_bytes;
use crate::propresenter::generated::rv_data;
use crate::propresenter::playlist::{canonical_presentation_name, PlaylistEntry};
use crate::propresenter::render::{
    apply_application_info, preserve_edited_document_metadata, preserve_generated_target_metadata,
    RenderedPresentation,
};
use crate::propresenter::text_fit::{CueTextFitSummary, NativeTextFitOracle};
use crate::workflow::description_parser::ParsedContent;
use crate::workflow::plan::{RenderStyle, ResolvedItemPlan};
use crate::workflow::presentation_render::{render_source_with_native_fit, PresentationSource};

use super::super::{BuildServiceError, ServiceBuildExecutor};
use super::target::{
    update_rendered_document, write_generated_playlist_presentation, ReviewedBackgroundAsset,
    ReviewedRenderTarget,
};

impl ServiceBuildExecutor<'_> {
    pub(in crate::workflow::execute) fn edit_description(
        &self,
        entry: &ResolvedItemPlan,
        content: &ParsedContent,
        style: &RenderStyle,
        target: ReviewedRenderTarget<'_>,
        text_fit: &mut NativeTextFitOracle,
    ) -> Result<(PlaylistEntry, usize, Vec<CueTextFitSummary>), BuildServiceError> {
        if content.segments().is_empty() {
            return Err(BuildServiceError::EmptyParsedContent {
                title: entry.pco_title.clone(),
            });
        }
        let source_bytes =
            target
                .existing_bytes
                .ok_or_else(|| BuildServiceError::MissingReviewedSource {
                    path: target.final_path.to_path_buf(),
                })?;
        let existing =
            decode_presentation_bytes(source_bytes, &target.final_path.display().to_string())?;
        let mut rendered =
            self.render_text_presentation(&existing.name, content, style, text_fit)?;
        update_rendered_document(&mut rendered, |presentation| {
            preserve_edited_document_metadata(presentation, &existing);
            Ok(())
        })?;
        self.apply_style(&mut rendered, style, target.background)?;
        update_rendered_document(&mut rendered, |presentation| {
            apply_application_info(
                presentation,
                Some(self.playlist_metadata.application_info()),
            );
            Ok(())
        })?;
        let text_fit_evidence = rendered.text_fit_summary().to_vec();
        let presentation = rendered.into_presentation();
        let (playlist_entry, slides) =
            write_generated_playlist_presentation(entry, &presentation, target)?;
        Ok((playlist_entry, slides, text_fit_evidence))
    }

    pub(in crate::workflow::execute) fn generate_description(
        &self,
        entry: &ResolvedItemPlan,
        content: &ParsedContent,
        style: &RenderStyle,
        target: ReviewedRenderTarget<'_>,
        text_fit: &mut NativeTextFitOracle,
    ) -> Result<(PlaylistEntry, usize, Vec<CueTextFitSummary>), BuildServiceError> {
        if content.segments().is_empty() {
            return Err(BuildServiceError::EmptyParsedContent {
                title: entry.pco_title.clone(),
            });
        }
        let presentation_name =
            canonical_presentation_name(&entry.playlist_name, entry.slide_type())?;
        let mut rendered =
            self.render_text_presentation(&presentation_name, content, style, text_fit)?;
        self.apply_style(&mut rendered, style, target.background)?;
        update_rendered_document(&mut rendered, |presentation| {
            Self::finalize_generated_document(
                presentation,
                target.final_path,
                target.existing_bytes,
                self.playlist_metadata.application_info(),
            )
        })?;
        let text_fit_evidence = rendered.text_fit_summary().to_vec();
        let presentation = rendered.into_presentation();
        let (playlist_entry, slides) =
            write_generated_playlist_presentation(entry, &presentation, target)?;
        Ok((playlist_entry, slides, text_fit_evidence))
    }

    pub(in crate::workflow::execute) fn generate_title(
        &self,
        entry: &ResolvedItemPlan,
        text: &str,
        style: &RenderStyle,
        target: ReviewedRenderTarget<'_>,
        text_fit: &mut NativeTextFitOracle,
    ) -> Result<(PlaylistEntry, usize, Vec<CueTextFitSummary>), BuildServiceError> {
        let presentation_name =
            canonical_presentation_name(&entry.playlist_name, entry.slide_type())?;
        let mut rendered = render_source_with_native_fit(
            &presentation_name,
            PresentationSource::Title { text },
            style,
            self.render_assets,
            text_fit,
        )?;
        self.apply_style(&mut rendered, style, target.background)?;
        update_rendered_document(&mut rendered, |presentation| {
            Self::finalize_generated_document(
                presentation,
                target.final_path,
                target.existing_bytes,
                self.playlist_metadata.application_info(),
            )
        })?;
        let text_fit_evidence = rendered.text_fit_summary().to_vec();
        let presentation = rendered.into_presentation();
        let (playlist_entry, slides) =
            write_generated_playlist_presentation(entry, &presentation, target)?;
        Ok((playlist_entry, slides, text_fit_evidence))
    }

    fn render_text_presentation(
        &self,
        name: &str,
        content: &ParsedContent,
        style: &RenderStyle,
        text_fit: &mut NativeTextFitOracle,
    ) -> Result<RenderedPresentation, BuildServiceError> {
        Ok(render_source_with_native_fit(
            name,
            PresentationSource::Description(content),
            style,
            self.render_assets,
            text_fit,
        )?)
    }

    /// Preserve target-owned metadata, then stamp the current producer.
    pub(in crate::workflow::execute) fn finalize_generated_document(
        presentation: &mut rv_data::Presentation,
        output_path: &Path,
        existing_source_bytes: Option<&[u8]>,
        application_info: &rv_data::ApplicationInfo,
    ) -> Result<(), BuildServiceError> {
        if let Some(source_bytes) = existing_source_bytes {
            let existing =
                decode_presentation_bytes(source_bytes, &output_path.display().to_string())?;
            preserve_generated_target_metadata(presentation, &existing);
        }
        apply_application_info(presentation, Some(application_info));
        Ok(())
    }

    pub(in crate::workflow::execute) fn apply_style(
        &self,
        rendered: &mut RenderedPresentation,
        style: &RenderStyle,
        reviewed_background: Option<ReviewedBackgroundAsset<'_>>,
    ) -> Result<(), BuildServiceError> {
        let output_key = rendered.presentation().name.clone();
        match (style.background(), reviewed_background) {
            (Some(_), Some(background)) => {
                update_rendered_document(rendered, |presentation| {
                    crate::propresenter::background::add_reviewed_background_to_first_cue(
                        presentation,
                        background.path,
                        background.data,
                        self.render_assets.locations().propresenter_root(),
                    )?;
                    Ok(())
                })?;
            }
            (None, None) => {}
            _ => {
                return Err(BuildServiceError::ReviewedBackgroundInvariant { output_key });
            }
        }
        Ok(())
    }
}
