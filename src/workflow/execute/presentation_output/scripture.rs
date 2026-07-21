//! Reviewed Bible lookup and scripture presentation rendering.

use std::path::{Path, PathBuf};

use crate::bible::{
    parse_scripture_ref, BibleService, BibleVersion, ScriptureHeader, ScriptureRef, Verse,
};
use crate::propresenter::playlist::{canonical_presentation_name, PlaylistEntry};
use crate::propresenter::render::RenderedPresentation;
use crate::propresenter::text_fit::{CueTextFitSummary, NativeTextFitOracle};
use crate::propresenter::SlideType;
use crate::workflow::approval::CapturedSources;
use crate::workflow::plan::{
    RenderStyle, ResolvedItemPlan, ScriptureContent, ScriptureRefInfo, ScriptureRequest,
};
use crate::workflow::presentation_render::{
    render_source_with_native_fit, CombinedScripturePassage, PresentationSource,
};

use super::super::{captured_source_bytes, BuildServiceError, ServiceBuildExecutor};
use super::target::{write_generated_playlist_presentation, ReviewedRenderTarget};

impl ServiceBuildExecutor<'_> {
    pub(in crate::workflow::execute) async fn generate_scripture(
        &self,
        entry: &ResolvedItemPlan,
        scripture: &ScriptureContent,
        style: &RenderStyle,
        target: ReviewedRenderTarget<'_>,
        sources: &CapturedSources,
        text_fit: &mut NativeTextFitOracle,
    ) -> Result<(PlaylistEntry, usize, Vec<CueTextFitSummary>), BuildServiceError> {
        let presentation_name =
            canonical_presentation_name(&entry.playlist_name, SlideType::Scripture)?;
        let mut rendered = self
            .render_scripture_source(&presentation_name, scripture, style, sources, text_fit)
            .await?;

        self.apply_style(&mut rendered, style, target.background)?;
        super::target::update_rendered_document(&mut rendered, |presentation| {
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

    async fn render_scripture_source(
        &self,
        presentation_name: &str,
        scripture: &ScriptureContent,
        style: &RenderStyle,
        sources: &CapturedSources,
        text_fit: &mut NativeTextFitOracle,
    ) -> Result<RenderedPresentation, BuildServiceError> {
        match scripture.request() {
            ScriptureRequest::Combined(references) => {
                self.render_combined_scripture(
                    presentation_name,
                    references,
                    style,
                    sources,
                    text_fit,
                )
                .await
            }
            ScriptureRequest::PrefixExcerpt {
                reference,
                display_reference,
                bible_version,
                excerpt_text,
            } => {
                self.render_prefix_scripture(
                    presentation_name,
                    reference,
                    display_reference,
                    bible_version,
                    excerpt_text,
                    style,
                    sources,
                    text_fit,
                )
                .await
            }
            ScriptureRequest::Single {
                reference,
                bible_version,
            } => {
                self.render_single_scripture(
                    presentation_name,
                    reference,
                    bible_version,
                    style,
                    sources,
                    text_fit,
                )
                .await
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn render_prefix_scripture(
        &self,
        presentation_name: &str,
        reference_text: &str,
        display_reference: &str,
        bible_version: &str,
        excerpt_text: &str,
        style: &RenderStyle,
        sources: &CapturedSources,
        text_fit: &mut NativeTextFitOracle,
    ) -> Result<RenderedPresentation, BuildServiceError> {
        let reference = parse_scripture_ref(reference_text).ok_or_else(|| {
            BuildServiceError::InvalidScriptureReference {
                reference: reference_text.to_string(),
            }
        })?;
        let mut passage = {
            let mut bible = self.bible_service.lock().await;
            reviewed_passage(
                &mut bible,
                self.render_assets.locations().project_data_root(),
                reference,
                reference_text,
                bible_version,
                sources,
            )?
        };
        passage.verses = crate::bible::reconcile_prefix_excerpt(&passage.verses, excerpt_text)?;
        let title = format!("Scripture\n{display_reference} {bible_version}");
        let label_prefix = passage.label_prefix();
        Ok(render_source_with_native_fit(
            presentation_name,
            PresentationSource::Scripture {
                title: &title,
                label_prefix: &label_prefix,
                verses: &passage.verses,
            },
            style,
            self.render_assets,
            text_fit,
        )?)
    }

    async fn render_combined_scripture(
        &self,
        presentation_name: &str,
        references: &[ScriptureRefInfo],
        style: &RenderStyle,
        sources: &CapturedSources,
        text_fit: &mut NativeTextFitOracle,
    ) -> Result<RenderedPresentation, BuildServiceError> {
        let passages =
            {
                let mut bible = self.bible_service.lock().await;
                references
                    .iter()
                    .map(|reference_info| {
                        let reference = parse_scripture_ref(reference_info.reference())
                            .ok_or_else(|| BuildServiceError::InvalidScriptureReference {
                                reference: reference_info.reference().to_string(),
                            })?;
                        let passage = reviewed_passage(
                            &mut bible,
                            self.render_assets.locations().project_data_root(),
                            reference,
                            reference_info.reference(),
                            reference_info.version(),
                            sources,
                        )?;
                        CombinedScripturePassage::new(
                            passage.header.display(),
                            passage.label_prefix(),
                            passage.verses,
                        )
                        .map_err(BuildServiceError::from)
                    })
                    .collect::<Result<Vec<_>, BuildServiceError>>()?
            };
        Ok(render_source_with_native_fit(
            presentation_name,
            PresentationSource::CombinedScripture {
                passages: &passages,
            },
            style,
            self.render_assets,
            text_fit,
        )?)
    }

    async fn render_single_scripture(
        &self,
        presentation_name: &str,
        reference_text: &str,
        bible_version: &str,
        style: &RenderStyle,
        sources: &CapturedSources,
        text_fit: &mut NativeTextFitOracle,
    ) -> Result<RenderedPresentation, BuildServiceError> {
        let reference = parse_scripture_ref(reference_text).ok_or_else(|| {
            BuildServiceError::InvalidScriptureReference {
                reference: reference_text.to_string(),
            }
        })?;
        let passage = {
            let mut bible = self.bible_service.lock().await;
            reviewed_passage(
                &mut bible,
                self.render_assets.locations().project_data_root(),
                reference,
                reference_text,
                bible_version,
                sources,
            )?
        };
        let title = format!("Scripture\n{}", passage.header.display());
        let label_prefix = passage.label_prefix();
        Ok(render_source_with_native_fit(
            presentation_name,
            PresentationSource::Scripture {
                title: &title,
                label_prefix: &label_prefix,
                verses: &passage.verses,
            },
            style,
            self.render_assets,
            text_fit,
        )?)
    }
}

struct ReviewedPassage {
    reference: ScriptureRef,
    header: ScriptureHeader,
    verses: Vec<Verse>,
}

impl ReviewedPassage {
    fn label_prefix(&self) -> String {
        format!("{} {}:", self.reference.book, self.reference.chapter)
    }
}

fn reviewed_passage(
    bible: &mut BibleService,
    project_data_root: &Path,
    reference: ScriptureRef,
    reference_text: &str,
    bible_version: &str,
    sources: &CapturedSources,
) -> Result<ReviewedPassage, BuildServiceError> {
    let version = parse_bible_version(bible_version)?;
    let source_path = bible_source_path(project_data_root, version);
    let source_bytes = captured_source_bytes(sources, &source_path)?;
    let (header, verses) = bible.lookup_verses_from_bytes(&reference, version, source_bytes)?;
    if !header.missing_verses.is_empty() {
        return Err(BuildServiceError::MissingVerses {
            reference: reference_text.to_string(),
            verses: header.missing_verses,
        });
    }
    Ok(ReviewedPassage {
        reference,
        header,
        verses,
    })
}

fn bible_source_path(project_data_root: &Path, version: BibleVersion) -> PathBuf {
    project_data_root.join("bibles").join(version.file_name())
}

pub(in crate::workflow::execute) fn parse_bible_version(
    name: &str,
) -> Result<BibleVersion, BuildServiceError> {
    BibleVersion::from_name(name)
        .ok_or_else(|| BuildServiceError::UnsupportedBibleVersion(name.to_string()))
}
