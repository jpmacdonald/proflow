//! Reviewed Bible lookup and scripture presentation rendering.

use std::path::{Path, PathBuf};

use crate::bible::{
    parse_scripture_ref, BibleCorpusSnapshot, BibleVersion, ScriptureHeader, ScriptureRef, Verse,
};
use crate::propresenter::generated::rv_data;
use crate::propresenter::playlist::{canonical_presentation_name, PlaylistEntry};
use crate::propresenter::render::RenderedPresentation;
use crate::propresenter::text_fit::{CueTextFitSummary, NativeTextFitOracle};
use crate::propresenter::SlideType;
use crate::workflow::approval::CapturedSources;
use crate::workflow::plan::{
    ExpectedMacroRegion, RenderStyle, ResolvedItemPlan, ScriptureContent, ScriptureRefInfo,
    ScriptureRequest,
};
use crate::workflow::presentation_render::{
    render_source_with_native_fit, resolved_macro_regions, CombinedScripturePassage,
    PresentationSource,
};

use super::super::{captured_source_bytes, BuildServiceError, ServiceBuildExecutor};
use super::target::{write_generated_playlist_presentation, ReviewedRenderTarget};

impl ServiceBuildExecutor<'_> {
    pub(in crate::workflow::execute) fn generate_scripture(
        &self,
        entry: &ResolvedItemPlan,
        scripture: &ScriptureContent,
        style: &RenderStyle,
        target: ReviewedRenderTarget<'_>,
        sources: &CapturedSources,
        text_fit: &mut NativeTextFitOracle,
    ) -> Result<
        (
            PlaylistEntry,
            usize,
            Vec<CueTextFitSummary>,
            Vec<ExpectedMacroRegion>,
        ),
        BuildServiceError,
    > {
        let presentation_name =
            canonical_presentation_name(&entry.playlist_name, SlideType::Scripture)?;
        let mut rendered =
            self.render_scripture_source(&presentation_name, scripture, style, sources, text_fit)?;

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
        let macro_regions = resolved_macro_regions(&rendered, style)?;
        let presentation = rendered.into_presentation();
        let (playlist_entry, slides) =
            write_generated_playlist_presentation(entry, &presentation, target)?;
        Ok((playlist_entry, slides, text_fit_evidence, macro_regions))
    }

    fn render_scripture_source(
        &self,
        presentation_name: &str,
        scripture: &ScriptureContent,
        style: &RenderStyle,
        sources: &CapturedSources,
        text_fit: &mut NativeTextFitOracle,
    ) -> Result<RenderedPresentation, BuildServiceError> {
        match scripture.request() {
            ScriptureRequest::Combined(references) => self.render_combined_scripture(
                presentation_name,
                references,
                style,
                sources,
                text_fit,
            ),
            ScriptureRequest::PrefixExcerpt {
                reference,
                display_reference,
                bible_version,
                excerpt_text,
            } => self.render_prefix_scripture(
                presentation_name,
                reference,
                display_reference,
                bible_version,
                excerpt_text,
                style,
                sources,
                text_fit,
            ),
            ScriptureRequest::Single {
                reference,
                bible_version,
            } => self.render_single_scripture(
                presentation_name,
                reference,
                bible_version,
                style,
                sources,
                text_fit,
            ),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_prefix_scripture(
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
        let mut passage = reviewed_passage(
            self.bible_corpora,
            self.render_assets.locations().project_data_root(),
            reference,
            reference_text,
            bible_version,
            sources,
        )?;
        passage.verses = crate::bible::reconcile_prefix_excerpt(&passage.verses, excerpt_text)?;
        let title = format!("Scripture\n{display_reference} {bible_version}");
        let label_prefix = passage.label_prefix();
        let mut rendered = render_source_with_native_fit(
            presentation_name,
            PresentationSource::Scripture {
                title: &title,
                label_prefix: &label_prefix,
                verses: &passage.verses,
            },
            style,
            self.render_assets,
            text_fit,
        )?;
        attach_native_bible_reference(&mut rendered, &passage)?;
        Ok(rendered)
    }

    fn render_combined_scripture(
        &self,
        presentation_name: &str,
        references: &[ScriptureRefInfo],
        style: &RenderStyle,
        sources: &CapturedSources,
        text_fit: &mut NativeTextFitOracle,
    ) -> Result<RenderedPresentation, BuildServiceError> {
        let passages = references
            .iter()
            .map(|reference_info| {
                let reference =
                    parse_scripture_ref(reference_info.reference()).ok_or_else(|| {
                        BuildServiceError::InvalidScriptureReference {
                            reference: reference_info.reference().to_string(),
                        }
                    })?;
                let passage = reviewed_passage(
                    self.bible_corpora,
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
            .collect::<Result<Vec<_>, BuildServiceError>>()?;
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

    fn render_single_scripture(
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
        let passage = reviewed_passage(
            self.bible_corpora,
            self.render_assets.locations().project_data_root(),
            reference,
            reference_text,
            bible_version,
            sources,
        )?;
        let title = format!("Scripture\n{}", passage.header.display());
        let label_prefix = passage.label_prefix();
        let mut rendered = render_source_with_native_fit(
            presentation_name,
            PresentationSource::Scripture {
                title: &title,
                label_prefix: &label_prefix,
                verses: &passage.verses,
            },
            style,
            self.render_assets,
            text_fit,
        )?;
        attach_native_bible_reference(&mut rendered, &passage)?;
        Ok(rendered)
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
    bible: &BibleCorpusSnapshot,
    project_data_root: &Path,
    reference: ScriptureRef,
    reference_text: &str,
    bible_version: &str,
    sources: &CapturedSources,
) -> Result<ReviewedPassage, BuildServiceError> {
    let version = parse_bible_version(bible_version)?;
    let source_path = bible_source_path(project_data_root, version);
    let source_bytes = captured_source_bytes(sources, &source_path)?;
    let (header, verses) = bible.lookup_reviewed_verses(&reference, version, source_bytes)?;
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

fn attach_native_bible_reference(
    rendered: &mut RenderedPresentation,
    passage: &ReviewedPassage,
) -> Result<(), BuildServiceError> {
    let reference = native_bible_reference(passage)?;
    super::target::update_rendered_document(rendered, |presentation| {
        presentation.bible_reference = Some(reference);
        Ok(())
    })
}

fn native_bible_reference(
    passage: &ReviewedPassage,
) -> Result<rv_data::presentation::BibleReference, BuildServiceError> {
    let first_verse = passage
        .verses
        .first()
        .ok_or_else(|| crate::error::Error::Scripture("scripture passage is empty".to_string()))?
        .number;
    let last_verse = passage
        .verses
        .last()
        .ok_or_else(|| crate::error::Error::Scripture("scripture passage is empty".to_string()))?
        .number;
    let book_index = passage.reference.canonical_book_index().ok_or_else(|| {
        crate::error::Error::Scripture(format!(
            "canonical book index is unavailable for {}",
            passage.reference.book
        ))
    })?;
    let chapter = native_range_value(passage.reference.chapter, "chapter")?;
    let first_verse = native_range_value(first_verse, "first verse")?;
    let last_verse = native_range_value(last_verse, "last verse")?;
    let (translation_name, translation_abbreviation) = native_translation(passage.header.version);
    Ok(rv_data::presentation::BibleReference {
        book_index,
        book_name: passage.reference.book.clone(),
        chapter_range: Some(rv_data::IntRange {
            start: chapter,
            end: chapter,
        }),
        // ProPresenter's schema has only one continuous verse range. For a
        // discontinuous selection the native metadata records its outer bounds;
        // cue labels retain the exact selected spans.
        verse_range: Some(rv_data::IntRange {
            start: first_verse,
            end: last_verse,
        }),
        translation_name: translation_name.to_string(),
        translation_display_abbreviation: String::new(),
        translation_internal_abbreviation: translation_abbreviation.to_string(),
        book_key: String::new(),
    })
}

fn native_range_value(value: u32, field: &str) -> Result<i32, BuildServiceError> {
    i32::try_from(value).map_err(|_| {
        crate::error::Error::Scripture(format!(
            "{field} {value} cannot be represented in native Bible metadata"
        ))
        .into()
    })
}

const fn native_translation(version: BibleVersion) -> (&'static str, &'static str) {
    match version {
        BibleVersion::NRSVue => ("New Revised Standard Version Updated Edition", "NRSVue"),
        BibleVersion::NRSV => ("New Revised Standard Version", "NRSV"),
        BibleVersion::NIV => ("New International Version", "NIV"),
        BibleVersion::NKJV => ("New King James Version", "NKJV"),
        BibleVersion::NLT => ("New Living Translation", "NLT"),
        BibleVersion::NASB => ("New American Standard Bible", "NASB"),
        BibleVersion::KJV => ("King James Version", "KJV"),
    }
}

pub(in crate::workflow::execute) fn parse_bible_version(
    name: &str,
) -> Result<BibleVersion, BuildServiceError> {
    BibleVersion::from_name(name)
        .ok_or_else(|| BuildServiceError::UnsupportedBibleVersion(name.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discontinuous_passage_gets_representable_native_metadata() {
        let reference = parse_scripture_ref("Joshua 3:1-5, 9-17")
            .expect("valid discontinuous scripture reference");
        let passage = ReviewedPassage {
            header: ScriptureHeader {
                book: reference.book.clone(),
                chapter: reference.chapter,
                verses: reference.verses.clone(),
                version: BibleVersion::NRSVue,
                missing_verses: Vec::new(),
            },
            reference,
            verses: [1, 2, 3, 4, 5, 9, 10, 11, 12, 13, 14, 15, 16, 17]
                .into_iter()
                .map(|number| Verse {
                    number,
                    text: format!("Verse {number}"),
                })
                .collect(),
        };

        let metadata = native_bible_reference(&passage).expect("representable native metadata");

        assert_eq!(metadata.book_index, 5);
        assert_eq!(metadata.book_name, "Joshua");
        assert_eq!(
            metadata.chapter_range,
            Some(rv_data::IntRange { start: 3, end: 3 })
        );
        assert_eq!(
            metadata.verse_range,
            Some(rv_data::IntRange { start: 1, end: 17 })
        );
        assert_eq!(
            metadata.translation_name,
            "New Revised Standard Version Updated Edition"
        );
        assert_eq!(metadata.translation_internal_abbreviation, "NRSVue");
    }
}
