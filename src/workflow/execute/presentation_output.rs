//! Native presentation rendering and per-file export.

use std::path::{Path, PathBuf};

use prost::Message;

use crate::bible::{
    parse_scripture_ref, BibleService, BibleVersion, ScriptureHeader, ScriptureRef, Verse,
};
use crate::propresenter::deserialize::decode_presentation_bytes;
use crate::propresenter::generated::rv_data;
use crate::propresenter::playlist::{
    canonical_presentation_name, PlaylistEntry, PlaylistError, SelectedArrangement,
};
use crate::propresenter::render::{
    apply_application_info, preserve_edited_document_metadata, preserve_generated_target_metadata,
    RenderedPresentation,
};
use crate::propresenter::serialize::write_presentation_file;
use crate::propresenter::{PresentationSize, SlideType};
use crate::workflow::approval::CapturedSources;
use crate::workflow::description_parser::ParsedContent;
use crate::workflow::plan::{
    BackgroundTransform, CueTransform, ExistingTransform, ItemKind, MacroTransform, RenderStyle,
    ResolvedItemPlan, ScriptureContent, ScriptureRefInfo, ScriptureRequest,
};
use crate::workflow::presentation_render::{
    apply_role_macros, render_source, CombinedScripturePassage, PresentationSource,
};

use super::{captured_source_bytes, BuildServiceError, ServiceBuildExecutor};

#[derive(Debug)]
pub(super) struct PreparedExistingPresentation {
    pub(super) embedded_data: Vec<u8>,
    pub(super) selected_arrangement: Option<SelectedArrangement>,
    pub(super) file_path: PathBuf,
}

#[derive(Clone, Copy)]
pub(super) struct ReviewedBackgroundAsset<'a> {
    pub(super) path: &'a Path,
    pub(super) data: &'a [u8],
}

#[derive(Clone, Copy)]
pub(super) struct ReviewedRenderTarget<'a> {
    pub(super) write_path: &'a Path,
    pub(super) final_path: &'a Path,
    pub(super) existing_bytes: Option<&'a [u8]>,
    pub(super) presentation_size: PresentationSize,
    pub(super) background: Option<ReviewedBackgroundAsset<'a>>,
}

impl ServiceBuildExecutor<'_> {
    /// Apply one checked transform to an existing native presentation.
    pub(super) fn restyle_existing_presentation(
        &self,
        entry: &ResolvedItemPlan,
        source_path: &Path,
        arrangement: Option<&str>,
        transform: &ExistingTransform,
        source_bytes: &[u8],
        target: ReviewedRenderTarget<'_>,
    ) -> Result<(PlaylistEntry, usize), BuildServiceError> {
        let mut presentation =
            decode_presentation_bytes(source_bytes, &source_path.display().to_string())?;
        crate::propresenter::resolution::resize_presentation_canvas(
            &mut presentation,
            target.presentation_size,
        )?;
        let normalized_bytes = presentation.encode_to_vec();
        let prepared = Self::prepare_existing_presentation(
            entry.output_key.as_str(),
            source_path,
            arrangement,
            &normalized_bytes,
            target.presentation_size,
        )?;
        if let Some(selected) = prepared.selected_arrangement.as_ref() {
            presentation.selected_arrangement = Some(rv_data::Uuid {
                string: selected.uuid().to_string(),
            });
        }
        let cue_transform = transform.cues();
        if let CueTransform::RetainOperatorPrefix(limit) = cue_transform {
            crate::propresenter::arrangement::retain_first_operator_cues(&mut presentation, limit)?;
        }
        match (transform.background(), target.background) {
            (BackgroundTransform::Replace(_), Some(background)) => {
                match prepared.selected_arrangement.as_ref() {
                    Some(selected) => {
                        crate::propresenter::background::replace_arrangement_entry_backgrounds(
                            &mut presentation,
                            background.path,
                            background.data,
                            self.render_assets.locations().propresenter_root(),
                            selected.uuid(),
                            selected.name(),
                        )?;
                    }
                    None => {
                        crate::propresenter::background::replace_operator_entry_background(
                            &mut presentation,
                            background.path,
                            background.data,
                            self.render_assets.locations().propresenter_root(),
                        )?;
                    }
                }
            }
            (BackgroundTransform::Preserve, None) => {}
            _ => {
                return Err(BuildServiceError::ReviewedBackgroundInvariant {
                    output_key: entry.output_key.to_string(),
                });
            }
        }
        if let MacroTransform::Enforce(policy) = transform.macros() {
            crate::propresenter::macros::apply_operator_macro_policy(
                &mut presentation,
                policy,
                self.render_assets.macros(),
            )?;
        }
        apply_application_info(
            &mut presentation,
            Some(self.playlist_metadata.application_info()),
        );
        validate_rendered_presentation_size(
            &presentation,
            target.presentation_size,
            entry.output_key.as_str(),
        )?;
        write_presentation_file(&presentation, target.write_path)?;

        let playlist_entry = PlaylistEntry::embedded(
            entry.playlist_name.clone(),
            target.final_path.display().to_string(),
            std::fs::read(target.write_path)?,
        )
        .map_err(PlaylistError::from)?
        .with_selected_arrangement(prepared.selected_arrangement)
        .map_err(PlaylistError::from)?;
        Ok((playlist_entry, presentation.cues.len()))
    }

    pub(super) fn edit_description(
        &self,
        entry: &ResolvedItemPlan,
        content: &ParsedContent,
        style: &RenderStyle,
        target: ReviewedRenderTarget<'_>,
    ) -> Result<(PlaylistEntry, usize), BuildServiceError> {
        if content.segments().is_empty() {
            return Err(BuildServiceError::message(format!(
                "No parsed content for edited item '{}'",
                entry.pco_title
            )));
        }
        let source_bytes = target.existing_bytes.ok_or_else(|| {
            BuildServiceError::message(format!(
                "approved source bytes are missing for '{}'",
                target.final_path.display()
            ))
        })?;
        let existing =
            decode_presentation_bytes(source_bytes, &target.final_path.display().to_string())?;
        let mut rendered = self.render_text_presentation(&existing.name, content, style)?;
        preserve_edited_document_metadata(&mut rendered.presentation, &existing);
        self.apply_style(&mut rendered, style, target.background)?;
        apply_application_info(
            &mut rendered.presentation,
            Some(self.playlist_metadata.application_info()),
        );
        validate_rendered_presentation_size(
            &rendered.presentation,
            target.presentation_size,
            entry.output_key.as_str(),
        )?;
        write_presentation_file(&rendered.presentation, target.write_path)?;

        let slide_count = rendered.presentation.cues.len();
        let playlist_entry = PlaylistEntry::embedded(
            entry.playlist_name.clone(),
            target.final_path.display().to_string(),
            std::fs::read(target.write_path)?,
        )
        .map_err(PlaylistError::from)?;
        Ok((playlist_entry, slide_count))
    }

    pub(super) fn generate_description(
        &self,
        entry: &ResolvedItemPlan,
        content: &ParsedContent,
        style: &RenderStyle,
        target: ReviewedRenderTarget<'_>,
    ) -> Result<(PlaylistEntry, usize), BuildServiceError> {
        if content.segments().is_empty() {
            return Err(BuildServiceError::message(format!(
                "No parsed content for generated item '{}'",
                entry.pco_title
            )));
        }
        let presentation_name =
            canonical_presentation_name(&entry.playlist_name, entry.slide_type())?;
        let mut rendered = self.render_text_presentation(&presentation_name, content, style)?;
        self.apply_style(&mut rendered, style, target.background)?;
        Self::finalize_generated_document(
            &mut rendered.presentation,
            target.final_path,
            target.existing_bytes,
            self.playlist_metadata.application_info(),
        )?;
        validate_rendered_presentation_size(
            &rendered.presentation,
            target.presentation_size,
            entry.output_key.as_str(),
        )?;
        write_presentation_file(&rendered.presentation, target.write_path)?;

        let slide_count = rendered.presentation.cues.len();
        let playlist_entry = PlaylistEntry::embedded(
            entry.playlist_name.clone(),
            target.final_path.display().to_string(),
            std::fs::read(target.write_path)?,
        )
        .map_err(PlaylistError::from)?;
        Ok((playlist_entry, slide_count))
    }

    pub(super) fn generate_title(
        &self,
        entry: &ResolvedItemPlan,
        text: &str,
        style: &RenderStyle,
        target: ReviewedRenderTarget<'_>,
    ) -> Result<(PlaylistEntry, usize), BuildServiceError> {
        let presentation_name =
            canonical_presentation_name(&entry.playlist_name, entry.slide_type())?;
        let mut rendered = render_source(
            &presentation_name,
            PresentationSource::Title { text },
            style,
            self.render_assets.themes(),
        )?;
        self.apply_style(&mut rendered, style, target.background)?;
        Self::finalize_generated_document(
            &mut rendered.presentation,
            target.final_path,
            target.existing_bytes,
            self.playlist_metadata.application_info(),
        )?;
        validate_rendered_presentation_size(
            &rendered.presentation,
            target.presentation_size,
            entry.output_key.as_str(),
        )?;
        write_presentation_file(&rendered.presentation, target.write_path)?;
        let slide_count = rendered.presentation.cues.len();
        let playlist_entry = PlaylistEntry::embedded(
            entry.playlist_name.clone(),
            target.final_path.display().to_string(),
            std::fs::read(target.write_path)?,
        )
        .map_err(PlaylistError::from)?;
        Ok((playlist_entry, slide_count))
    }

    fn render_text_presentation(
        &self,
        name: &str,
        content: &ParsedContent,
        style: &RenderStyle,
    ) -> Result<RenderedPresentation, BuildServiceError> {
        Ok(render_source(
            name,
            PresentationSource::Description(content),
            style,
            self.render_assets.themes(),
        )?)
    }

    pub(super) async fn generate_scripture(
        &self,
        entry: &ResolvedItemPlan,
        scripture: &ScriptureContent,
        style: &RenderStyle,
        target: ReviewedRenderTarget<'_>,
        sources: &CapturedSources,
    ) -> Result<(PlaylistEntry, usize), BuildServiceError> {
        if entry.item_kind != ItemKind::Scripture {
            return Err(BuildServiceError::message(format!(
                "Unknown created type for '{}'",
                entry.pco_title
            )));
        }

        let presentation_name =
            canonical_presentation_name(&entry.playlist_name, SlideType::Scripture)?;
        let mut rendered = self
            .render_scripture_source(&presentation_name, scripture, style, sources)
            .await?;

        self.apply_style(&mut rendered, style, target.background)?;
        Self::finalize_generated_document(
            &mut rendered.presentation,
            target.final_path,
            target.existing_bytes,
            self.playlist_metadata.application_info(),
        )?;
        validate_rendered_presentation_size(
            &rendered.presentation,
            target.presentation_size,
            entry.output_key.as_str(),
        )?;
        write_presentation_file(&rendered.presentation, target.write_path)?;

        let playlist_entry = PlaylistEntry::embedded(
            entry.playlist_name.clone(),
            target.final_path.display().to_string(),
            std::fs::read(target.write_path)?,
        )
        .map_err(PlaylistError::from)?;
        Ok((playlist_entry, rendered.presentation.cues.len()))
    }

    async fn render_scripture_source(
        &self,
        presentation_name: &str,
        scripture: &ScriptureContent,
        style: &RenderStyle,
        sources: &CapturedSources,
    ) -> Result<RenderedPresentation, BuildServiceError> {
        match scripture.request() {
            ScriptureRequest::Combined(references) => {
                self.render_combined_scripture(presentation_name, references, style, sources)
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
    ) -> Result<RenderedPresentation, BuildServiceError> {
        let reference = parse_scripture_ref(reference_text).ok_or_else(|| {
            BuildServiceError::message(format!("Cannot parse reference: {reference_text}"))
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
        Ok(render_source(
            presentation_name,
            PresentationSource::Scripture {
                title: &title,
                label_prefix: &label_prefix,
                verses: &passage.verses,
            },
            style,
            self.render_assets.themes(),
        )?)
    }

    async fn render_combined_scripture(
        &self,
        presentation_name: &str,
        references: &[ScriptureRefInfo],
        style: &RenderStyle,
        sources: &CapturedSources,
    ) -> Result<RenderedPresentation, BuildServiceError> {
        let passages = {
            let mut bible = self.bible_service.lock().await;
            references
                .iter()
                .map(|reference_info| {
                    let reference =
                        parse_scripture_ref(reference_info.reference()).ok_or_else(|| {
                            BuildServiceError::message(format!(
                                "Cannot parse: {}",
                                reference_info.reference()
                            ))
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
        Ok(render_source(
            presentation_name,
            PresentationSource::CombinedScripture {
                passages: &passages,
            },
            style,
            self.render_assets.themes(),
        )?)
    }

    async fn render_single_scripture(
        &self,
        presentation_name: &str,
        reference_text: &str,
        bible_version: &str,
        style: &RenderStyle,
        sources: &CapturedSources,
    ) -> Result<RenderedPresentation, BuildServiceError> {
        let reference = parse_scripture_ref(reference_text).ok_or_else(|| {
            BuildServiceError::message(format!("Cannot parse reference: {reference_text}"))
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
        Ok(render_source(
            presentation_name,
            PresentationSource::Scripture {
                title: &title,
                label_prefix: &label_prefix,
                verses: &passage.verses,
            },
            style,
            self.render_assets.themes(),
        )?)
    }

    /// Preserve target-owned metadata, then stamp the current producer.
    pub(super) fn finalize_generated_document(
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

    pub(super) fn prepare_existing_presentation(
        output_key: &str,
        source_path: &Path,
        arrangement: Option<&str>,
        source_bytes: &[u8],
        presentation_size: PresentationSize,
    ) -> Result<PreparedExistingPresentation, BuildServiceError> {
        let embedded_data = source_bytes.to_vec();
        let presentation =
            decode_presentation_bytes(source_bytes, &source_path.display().to_string())?;
        validate_rendered_presentation_size(&presentation, presentation_size, output_key)?;
        let selected_arrangement = if let Some(name) = arrangement {
            let matches = presentation
                .arrangements
                .iter()
                .filter(|arrangement| arrangement.name.eq_ignore_ascii_case(name))
                .collect::<Vec<_>>();
            let arrangement = match matches.as_slice() {
                [] => {
                    return Err(BuildServiceError::ArrangementUnavailable {
                        presentation: presentation.name.clone(),
                        arrangement: name.to_string(),
                    });
                }
                [arrangement] => *arrangement,
                _ => {
                    return Err(BuildServiceError::ArrangementAmbiguous {
                        presentation: presentation.name.clone(),
                        arrangement: name.to_string(),
                        matches: matches.len(),
                    });
                }
            };
            let uuid = crate::propresenter::arrangement::selectable_arrangement_uuid(
                &presentation,
                arrangement,
            )
            .ok_or_else(|| {
                BuildServiceError::message(format!(
                    "arrangement '{name}' in '{}' has incomplete identity or dangling group/cue references",
                    source_path.display()
                ))
            })?;
            Some(
                SelectedArrangement::new(uuid, arrangement.name.clone()).map_err(|error| {
                    BuildServiceError::message(format!(
                        "arrangement '{name}' in '{}' is invalid: {error}",
                        source_path.display()
                    ))
                })?,
            )
        } else {
            None
        };

        Ok(PreparedExistingPresentation {
            embedded_data,
            selected_arrangement,
            file_path: source_path.to_path_buf(),
        })
    }

    pub(super) fn apply_style(
        &self,
        rendered: &mut RenderedPresentation,
        style: &RenderStyle,
        reviewed_background: Option<ReviewedBackgroundAsset<'_>>,
    ) -> Result<(), BuildServiceError> {
        apply_role_macros(rendered, style, self.render_assets.macros())?;
        match (style.background(), reviewed_background) {
            (Some(_), Some(background)) => {
                crate::propresenter::background::add_reviewed_background_to_first_cue(
                    &mut rendered.presentation,
                    background.path,
                    background.data,
                    self.render_assets.locations().propresenter_root(),
                )?;
            }
            (None, None) => {}
            _ => {
                return Err(BuildServiceError::ReviewedBackgroundInvariant {
                    output_key: rendered.presentation.name.clone(),
                });
            }
        }
        Ok(())
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

fn validate_rendered_presentation_size(
    presentation: &rv_data::Presentation,
    expected: PresentationSize,
    output_key: &str,
) -> Result<(), BuildServiceError> {
    let actual = crate::propresenter::resolution::inspect_presentation_size(presentation);
    if actual.matches(expected) {
        Ok(())
    } else {
        Err(BuildServiceError::PresentationSizeInvariant {
            output_key: output_key.to_string(),
            expected,
            actual: actual.describe(),
        })
    }
}

fn bible_source_path(project_data_root: &Path, version: BibleVersion) -> PathBuf {
    project_data_root.join("bibles").join(version.file_name())
}

pub(super) fn parse_bible_version(name: &str) -> Result<BibleVersion, BuildServiceError> {
    BibleVersion::from_name(name)
        .ok_or_else(|| BuildServiceError::UnsupportedBibleVersion(name.to_string()))
}
