//! Shared service build execution.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Read a file, logging a warning on failure instead of silently discarding the error.
fn read_file_optional(path: &Path) -> Option<Vec<u8>> {
    match std::fs::read(path) {
        Ok(data) => Some(data),
        Err(e) => {
            eprintln!("Warning: failed to read {}: {e}", path.display());
            None
        }
    }
}

use thiserror::Error;
use tokio::sync::Mutex;
use uuid::Uuid;

use super::classify;
use super::description_parser::to_styled_segments;
use super::plan::{ContentSource, ItemKind, PlanAction, ResolvedItemPlan};
use super::report::{BuildServiceEntry, BuildServiceResult};
use crate::bible::{parse_scripture_ref, BibleService, BibleVersion};
use crate::paths::find_data_subdir;
use crate::planning_center::PlanningCenterClient;
use crate::project_config::ProjectConfig;
use crate::propresenter::deserialize::{read_presentation_file, ProPresenterError};
use crate::propresenter::macros::MacroCache;
use crate::propresenter::playlist::{
    build_playlist, canonical_presentation_name, playlist_output_path, write_playlist_file,
    PlaylistEntry, PlaylistError,
};
use crate::propresenter::rtf::StyledSegment;
use crate::propresenter::serialize::{write_presentation_file, SerializeError};
use crate::propresenter::template::{
    build_combined_scripture_presentation, build_scripture_presentation_dual_template,
    edit_existing_presentation, pack_segments_for_slides, ScripturePassage, ThemeCache,
    DEFAULT_MAX_LINES_PER_SLIDE,
};
use crate::propresenter::SlideType;
use crate::utils::file_index::FileIndex;

/// Input for single ad-hoc presentation generation.
#[derive(Debug, Clone)]
pub(crate) struct SingleGenerateRequest {
    pub slide_type: SlideType,
    pub name: String,
    pub scripture_reference: Option<String>,
    pub bible_version: Option<String>,
    pub content: Option<Vec<StyledSegment>>,
    pub title_text: Option<String>,
    pub background: Option<String>,
    pub arrangement: Option<String>,
}

/// Result of a single presentation generation.
#[derive(Debug)]
pub(crate) struct SingleGenerateResult {
    pub output_path: PathBuf,
    pub slide_count: usize,
}

/// Per-entry override applied during service build execution.
#[derive(Debug, Clone, Default)]
pub(crate) struct EntryOverride {
    pub output_key: String,
    pub playlist_name: Option<String>,
    pub background: Option<String>,
    pub arrangement: Option<String>,
}

/// Input arguments for the shared service build workflow.
#[derive(Debug, Clone, Default)]
pub(crate) struct BuildRequest {
    pub plan_id: String,
    pub service_name: Option<String>,
    pub playlist_name: Option<String>,
    pub skip_output_keys: Vec<String>,
    pub overrides: Vec<EntryOverride>,
}

/// Errors raised while executing a service build.
#[derive(Debug, Error)]
pub(crate) enum BuildServiceError {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Deserialize(#[from] ProPresenterError),
    #[error(transparent)]
    Serialize(#[from] SerializeError),
    #[error(transparent)]
    Playlist(#[from] PlaylistError),
    #[error(transparent)]
    Bible(#[from] crate::error::Error),
}

impl BuildServiceError {
    fn message(msg: impl Into<String>) -> Self {
        Self::Message(msg.into())
    }
}

/// Shared executor for full-service builds.
pub(crate) struct ServiceBuildExecutor<'a> {
    pco_client: &'a PlanningCenterClient,
    bible_service: &'a Arc<Mutex<BibleService>>,
    file_index: &'a Arc<Mutex<Option<FileIndex>>>,
    template_cache: &'a Arc<Mutex<ThemeCache>>,
    macro_cache: &'a MacroCache,
    library_path: Option<&'a Path>,
}

impl<'a> ServiceBuildExecutor<'a> {
    /// Create a new service build executor over shared runtime dependencies.
    pub(crate) fn new(
        pco_client: &'a PlanningCenterClient,
        bible_service: &'a Arc<Mutex<BibleService>>,
        file_index: &'a Arc<Mutex<Option<FileIndex>>>,
        template_cache: &'a Arc<Mutex<ThemeCache>>,
        macro_cache: &'a MacroCache,
        library_path: Option<&'a Path>,
    ) -> Self {
        Self {
            pco_client,
            bible_service,
            file_index,
            template_cache,
            macro_cache,
            library_path,
        }
    }

    /// Execute a full service build from plan/config inputs.
    pub(crate) async fn build_service(
        &self,
        request: &BuildRequest,
        mappings: &ProjectConfig,
    ) -> Result<BuildServiceResult, BuildServiceError> {
        let items = self
            .pco_client
            .get_service_items(&request.plan_id)
            .await
            .map_err(|e| BuildServiceError::message(e.to_string()))?;

        let index_guard = self.file_index.lock().await;
        let plans = classify::build_plan(
            &items,
            mappings,
            index_guard.as_ref(),
            request.service_name.as_deref(),
        );
        drop(index_guard);

        let skip_set: HashSet<&str> = request
            .skip_output_keys
            .iter()
            .map(String::as_str)
            .collect();
        let override_map: HashMap<&str, &EntryOverride> = request
            .overrides
            .iter()
            .map(|entry| (entry.output_key.as_str(), entry))
            .collect();

        let mut playlist_entries: Vec<PlaylistEntry> = Vec::new();
        let mut summary_entries: Vec<BuildServiceEntry> = Vec::new();
        let mut generated_count = 0usize;
        let mut library_count = 0usize;
        let mut skipped_count = 0usize;

        for plan in &plans {
            if skip_set.contains(plan.output_key.as_str()) {
                skipped_count += 1;
                summary_entries.push(BuildServiceEntry {
                    output_key: plan.output_key.clone(),
                    position: plan.position,
                    name: plan.pco_title.clone(),
                    action: "skipped (user)".to_string(),
                    file_path: None,
                    slides: None,
                });
                continue;
            }

            let entry_override = override_map.get(plan.output_key.as_str()).copied();
            let effective_plan = apply_override(plan, entry_override);

            match effective_plan.action {
                PlanAction::Skip => {
                    skipped_count += 1;
                    summary_entries.push(BuildServiceEntry {
                        output_key: effective_plan.output_key.clone(),
                        position: effective_plan.position,
                        name: effective_plan.pco_title.clone(),
                        action: format!("skipped: {}", effective_plan.reason),
                        file_path: None,
                        slides: None,
                    });
                }
                PlanAction::NeedsReview => {
                    skipped_count += 1;
                    summary_entries.push(BuildServiceEntry {
                        output_key: effective_plan.output_key.clone(),
                        position: effective_plan.position,
                        name: effective_plan.pco_title.clone(),
                        action: format!("uncertain: {}", effective_plan.reason),
                        file_path: None,
                        slides: None,
                    });
                }
                PlanAction::UseExisting => {
                    let file_path = effective_plan.file_path.clone().unwrap_or_default();
                    let embedded_data = read_file_optional(Path::new(&file_path));
                    let arrangement_uuid = if embedded_data.is_some() {
                        Self::resolve_arrangement_uuid(
                            &file_path,
                            effective_plan.style.arrangement.as_deref(),
                        )
                    } else {
                        None
                    };

                    let file_stem = Path::new(&file_path)
                        .file_stem()
                        .and_then(|segment| segment.to_str())
                        .unwrap_or(&effective_plan.playlist_name);

                    playlist_entries.push(PlaylistEntry {
                        name: file_stem.to_string(),
                        slide_type: effective_plan.slide_type(),
                        from_matched_file: true,
                        presentation_path: file_path.clone(),
                        arrangement_uuid,
                        embedded_data,
                    });

                    library_count += 1;
                    summary_entries.push(BuildServiceEntry {
                        output_key: effective_plan.output_key.clone(),
                        position: effective_plan.position,
                        name: effective_plan.playlist_name.clone(),
                        action: "library".to_string(),
                        file_path: Some(file_path),
                        slides: None,
                    });
                }
                PlanAction::EditInPlace => {
                    let (playlist_entry, slides) =
                        self.generate_from_description(&effective_plan).await?;
                    let generated_path = playlist_entry.presentation_path.clone();
                    playlist_entries.push(playlist_entry);

                    generated_count += 1;
                    summary_entries.push(BuildServiceEntry {
                        output_key: effective_plan.output_key.clone(),
                        position: effective_plan.position,
                        name: effective_plan.playlist_name.clone(),
                        action: "edited".to_string(),
                        file_path: Some(generated_path),
                        slides: Some(slides),
                    });
                }
                PlanAction::GenerateNew => {
                    let (playlist_entry, slides) = match &effective_plan.content_source {
                        ContentSource::Scripture { .. } => {
                            self.generate_scripture(&effective_plan).await?
                        }
                        ContentSource::Description { .. } | ContentSource::None => {
                            self.generate_from_description(&effective_plan).await?
                        }
                    };
                    let generated_path = playlist_entry.presentation_path.clone();
                    playlist_entries.push(playlist_entry);

                    generated_count += 1;
                    summary_entries.push(BuildServiceEntry {
                        output_key: effective_plan.output_key.clone(),
                        position: effective_plan.position,
                        name: effective_plan.playlist_name.clone(),
                        action: "generated".to_string(),
                        file_path: Some(generated_path),
                        slides: Some(slides),
                    });
                }
            }
        }

        let playlist_name = self.resolve_playlist_name(request).await;
        let playlist = build_playlist(&playlist_name, &playlist_entries);
        let output_path = playlist_output_path(self.library_path, &playlist_name);
        write_playlist_file(&playlist, &playlist_entries, &output_path)?;

        Ok(BuildServiceResult {
            playlist_path: output_path.display().to_string(),
            entries: summary_entries,
            total_items: playlist_entries.len(),
            generated_count,
            library_count,
            skipped_count,
        })
    }

    /// Generate a single ad-hoc presentation from user-provided content.
    pub(crate) async fn generate_single(
        &self,
        request: &SingleGenerateRequest,
    ) -> Result<SingleGenerateResult, BuildServiceError> {
        let presentation_name = canonical_presentation_name(&request.name, request.slide_type);

        let mut presentation = if request.slide_type == SlideType::Scripture {
            let ref_str = request.scripture_reference.as_deref().ok_or_else(|| {
                BuildServiceError::message(
                    "scripture_reference is required for scripture slide type",
                )
            })?;
            let reference = parse_scripture_ref(ref_str)
                .ok_or_else(|| BuildServiceError::message("Could not parse scripture reference"))?;
            let version = parse_bible_version(request.bible_version.as_deref());

            let (header, verses) = self
                .bible_service
                .lock()
                .await
                .lookup_verses(&reference, version)?;

            if !header.missing_verses.is_empty() {
                eprintln!(
                    "Warning: {ref_str}: missing verses {:?}",
                    header.missing_verses
                );
            }

            let slide_name = match request.slide_type {
                SlideType::Scripture => "scripture",
                SlideType::Lyrics => "song",
                _ => "info",
            };
            let template_slide = self
                .template_cache
                .lock()
                .await
                .get(slide_name)
                .cloned()
                .ok_or_else(|| {
                    BuildServiceError::message(format!("No template slide: {slide_name}"))
                })?;

            crate::propresenter::template::build_scripture_presentation(
                &presentation_name,
                &template_slide,
                &verses,
                Some(&header.display()),
            )
            .ok_or_else(|| BuildServiceError::message("Failed to build scripture presentation"))?
        } else {
            let segments = request.content.as_deref().ok_or_else(|| {
                BuildServiceError::message("content is required for non-scripture slide types")
            })?;

            let slide_name = match request.slide_type {
                SlideType::Scripture => "scripture",
                SlideType::Lyrics => "song",
                _ => "info",
            };
            let template_slide = self
                .template_cache
                .lock()
                .await
                .get(slide_name)
                .cloned()
                .ok_or_else(|| {
                    BuildServiceError::message(format!("No template slide: {slide_name}"))
                })?;

            crate::propresenter::template::build_presentation_from_template_with_options(
                &presentation_name,
                &template_slide,
                segments,
                45,
                DEFAULT_MAX_LINES_PER_SLIDE,
                request.title_text.as_deref(),
            )
            .ok_or_else(|| {
                BuildServiceError::message("Failed to build presentation from template")
            })?
        };

        if let Some(ref bg_category) = request.background {
            Self::apply_background(&mut presentation, bg_category);
        }

        if let Some(ref arr_name) = request.arrangement {
            crate::propresenter::arrangement::select_arrangement_by_name(
                &mut presentation,
                arr_name,
            );
        }

        let output_path = self.output_presentation_path(&presentation_name);
        write_presentation_file(&presentation, &output_path)?;
        self.refresh_file_index(&output_path).await;

        Ok(SingleGenerateResult {
            slide_count: presentation.cues.len(),
            output_path,
        })
    }

    async fn generate_from_description(
        &self,
        entry: &ResolvedItemPlan,
    ) -> Result<(PlaylistEntry, usize), BuildServiceError> {
        let segments: Vec<StyledSegment> = entry
            .parsed_content()
            .map(to_styled_segments)
            .unwrap_or_default();

        if segments.is_empty() {
            if let Some(ref file_path) = entry.file_path {
                let embedded_data = read_file_optional(Path::new(file_path));
                let file_stem = Path::new(file_path)
                    .file_stem()
                    .and_then(|segment| segment.to_str())
                    .unwrap_or(&entry.playlist_name);
                return Ok((
                    PlaylistEntry {
                        name: file_stem.to_string(),
                        slide_type: entry.slide_type(),
                        from_matched_file: true,
                        presentation_path: file_path.clone(),
                        arrangement_uuid: None,
                        embedded_data,
                    },
                    0,
                ));
            }

            return Err(BuildServiceError::message(format!(
                "No parsed content and no library file for edited item '{}'",
                entry.pco_title
            )));
        }

        let title_text = entry
            .parsed_content()
            .and_then(|content| content.title_text.clone());

        if let Some(ref file_path) = entry.file_path {
            let existing = read_presentation_file(file_path)?;
            let mut presentation =
                edit_existing_presentation(&existing, &segments, title_text.as_deref())
                    .ok_or_else(|| {
                        BuildServiceError::message(format!(
                            "Failed to edit presentation '{}' — no template slide found",
                            entry.playlist_name
                        ))
                    })?;

            if let Some(ref background) = entry.style.background {
                Self::apply_background(&mut presentation, background);
            }

            let style = maybe_upgrade_highlighted_macro(&entry.style, &segments);
            self.apply_macros(&mut presentation, &style);

            let output_path = PathBuf::from(file_path);
            write_presentation_file(&presentation, &output_path)?;

            let slide_count = presentation.cues.len();
            let embedded_data = read_file_optional(&output_path);
            let file_stem = output_path
                .file_stem()
                .and_then(|segment| segment.to_str())
                .unwrap_or(&entry.playlist_name);

            return Ok((
                PlaylistEntry {
                    name: file_stem.to_string(),
                    slide_type: entry.slide_type(),
                    from_matched_file: true,
                    presentation_path: output_path.display().to_string(),
                    arrangement_uuid: None,
                    embedded_data,
                },
                slide_count,
            ));
        }

        let slide_name = entry.style.template_name.clone().unwrap_or_else(|| {
            match entry.item_type.as_deref() {
                Some("scripture") => "scripture",
                Some("song") => "song",
                _ => "info",
            }
            .to_string()
        });

        let template_slide = self
            .template_cache
            .lock()
            .await
            .get(&slide_name)
            .cloned()
            .ok_or_else(|| {
                BuildServiceError::message(format!("No template slide: {slide_name}"))
            })?;

        let presentation_name =
            canonical_presentation_name(&entry.playlist_name, entry.slide_type());

        let (wrap_col, max_lines) =
            crate::propresenter::template::extract_slide_metrics(&template_slide)
                .map_or((45, DEFAULT_MAX_LINES_PER_SLIDE), |metrics| {
                    (metrics.chars_per_line, metrics.max_lines)
                });

        let slide_groups = pack_segments_for_slides(&segments, wrap_col, max_lines);
        let mut all_slide_segments: Vec<Vec<StyledSegment>> = Vec::new();
        if let Some(ref title) = title_text {
            if !title.is_empty() {
                all_slide_segments.push(vec![StyledSegment::unstyled(title)]);
            }
        }
        all_slide_segments.extend(slide_groups);

        let mut presentation = crate::propresenter::template::assemble_presentation(
            &presentation_name,
            &template_slide,
            &all_slide_segments,
        );

        if let Some(ref background) = entry.style.background {
            Self::apply_background(&mut presentation, background);
        }

        let style = maybe_upgrade_highlighted_macro(&entry.style, &segments);
        self.apply_macros(&mut presentation, &style);

        if let Some(ref arrangement) = entry.style.arrangement {
            crate::propresenter::arrangement::select_arrangement_by_name(
                &mut presentation,
                arrangement,
            );
        }

        let output_path = self.output_presentation_path(&presentation_name);
        write_presentation_file(&presentation, &output_path)?;
        self.refresh_file_index(&output_path).await;

        let slide_count = presentation.cues.len();
        let embedded_data = read_file_optional(&output_path);

        Ok((
            PlaylistEntry {
                name: presentation_name,
                slide_type: entry.slide_type(),
                from_matched_file: false,
                presentation_path: output_path.display().to_string(),
                arrangement_uuid: None,
                embedded_data,
            },
            slide_count,
        ))
    }

    async fn generate_scripture(
        &self,
        entry: &ResolvedItemPlan,
    ) -> Result<(PlaylistEntry, usize), BuildServiceError> {
        if entry.item_kind != ItemKind::Scripture {
            return Err(BuildServiceError::message(format!(
                "Unknown created type for '{}'",
                entry.pco_title
            )));
        }

        let content_slide_name = entry
            .style
            .template_name
            .clone()
            .unwrap_or_else(|| "scripture".to_string());

        let title_slide_name = entry
            .style
            .title_template
            .clone()
            .unwrap_or_else(|| content_slide_name.clone());

        let mut cache = self.template_cache.lock().await;
        let content_template = cache.get(&content_slide_name).cloned().ok_or_else(|| {
            BuildServiceError::message(format!("No template slide: {content_slide_name}"))
        })?;
        let title_template = if title_slide_name == content_slide_name {
            content_template.clone()
        } else {
            cache
                .get(&title_slide_name)
                .cloned()
                .unwrap_or_else(|| content_template.clone())
        };
        drop(cache);

        let presentation_name =
            canonical_presentation_name(&entry.playlist_name, SlideType::Scripture);

        let mut missing_warnings = Vec::new();
        let scripture = entry.scripture_content().ok_or_else(|| {
            BuildServiceError::message(format!(
                "No scripture source configured for '{}'",
                entry.pco_title
            ))
        })?;
        let mut presentation = if !scripture.references.is_empty() {
            let mut passages = Vec::new();
            let mut bible = self.bible_service.lock().await;

            for ref_info in &scripture.references {
                let reference = parse_scripture_ref(&ref_info.reference).ok_or_else(|| {
                    BuildServiceError::message(format!("Cannot parse: {}", ref_info.reference))
                })?;
                let version = parse_bible_version(Some(&ref_info.version));
                let (header, verses) = bible.lookup_verses(&reference, version)?;

                if !header.missing_verses.is_empty() {
                    missing_warnings.push(format!(
                        "{}: missing verses {:?}",
                        ref_info.reference, header.missing_verses
                    ));
                }

                passages.push(ScripturePassage {
                    title: header.display(),
                    verses,
                });
            }

            drop(bible);

            build_combined_scripture_presentation(&presentation_name, &content_template, &passages)
                .ok_or_else(|| {
                    BuildServiceError::message(
                        "Failed to build combined scripture presentation".to_string(),
                    )
                })?
        } else {
            let reference_text = scripture.reference.as_deref().ok_or_else(|| {
                BuildServiceError::message(format!(
                    "No scripture reference for '{}'",
                    entry.pco_title
                ))
            })?;
            let reference = parse_scripture_ref(reference_text).ok_or_else(|| {
                BuildServiceError::message(format!("Cannot parse reference: {reference_text}"))
            })?;
            let version = parse_bible_version(scripture.bible_version.as_deref());

            let (header, verses) = self
                .bible_service
                .lock()
                .await
                .lookup_verses(&reference, version)?;

            if !header.missing_verses.is_empty() {
                missing_warnings.push(format!(
                    "{reference_text}: missing verses {:?}",
                    header.missing_verses
                ));
            }

            let title = format!("Scripture\n{}", header.display());
            build_scripture_presentation_dual_template(
                &presentation_name,
                &title_template,
                &content_template,
                &verses,
                Some(&title),
            )
            .ok_or_else(|| {
                BuildServiceError::message("Failed to build scripture presentation".to_string())
            })?
        };

        if let Some(ref background) = entry.style.background {
            Self::apply_background(&mut presentation, background);
        }

        self.apply_macros(&mut presentation, &entry.style);

        let output_path = self.output_presentation_path(&presentation_name);
        write_presentation_file(&presentation, &output_path)?;
        self.refresh_file_index(&output_path).await;

        if !missing_warnings.is_empty() {
            eprintln!("Warning: {}", missing_warnings.join("; "));
        }

        Ok((
            PlaylistEntry {
                name: presentation_name,
                slide_type: SlideType::Scripture,
                from_matched_file: false,
                presentation_path: output_path.display().to_string(),
                arrangement_uuid: None,
                embedded_data: read_file_optional(&output_path),
            },
            presentation.cues.len(),
        ))
    }

    async fn resolve_playlist_name(&self, request: &BuildRequest) -> String {
        if let Some(name) = &request.playlist_name {
            return name.clone();
        }

        let service_name = request.service_name.as_deref().unwrap_or("Service");
        self.resolve_plan_date(&request.plan_id).await.map_or_else(
            || service_name.to_string(),
            |date| format!("{} - {}", date.format("%B %-d, %Y"), service_name),
        )
    }

    async fn resolve_plan_date(&self, plan_id: &str) -> Option<chrono::DateTime<chrono::Utc>> {
        match self.pco_client.get_upcoming_services(60).await {
            Ok((_, plans)) => plans
                .iter()
                .find(|plan| plan.id == plan_id)
                .map(|plan| plan.date),
            Err(e) => {
                eprintln!("Warning: could not resolve plan date for {plan_id}: {e}");
                None
            }
        }
    }

    async fn refresh_file_index(&self, output_path: &Path) {
        if let Some(ref mut index) = *self.file_index.lock().await {
            index.add_entry(output_path);
        }
    }

    fn output_presentation_path(&self, presentation_name: &str) -> PathBuf {
        self.library_path
            .unwrap_or_else(|| Path::new("."))
            .join(format!("{presentation_name}.pro"))
    }

    fn resolve_arrangement_uuid(file_path: &str, arrangement_name: Option<&str>) -> Option<Uuid> {
        let name = arrangement_name?;
        let data = read_file_optional(Path::new(file_path))?;
        let presentation =
            <crate::propresenter::generated::rv_data::Presentation as prost::Message>::decode(
                data.as_slice(),
            )
            .ok()?;

        let target = name.to_lowercase();
        presentation
            .arrangements
            .iter()
            .find(|arrangement| arrangement.name.to_lowercase() == target)
            .and_then(|arrangement| arrangement.uuid.as_ref())
            .and_then(|uuid| Uuid::parse_str(&uuid.string).ok())
    }

    fn apply_macros(
        &self,
        presentation: &mut crate::propresenter::generated::rv_data::Presentation,
        style: &super::plan::PresentationStyle,
    ) {
        if let Some(ref macro_name) = style.macro_name {
            crate::propresenter::macros::add_macro_to_first_cue(
                presentation,
                macro_name,
                self.macro_cache,
            );
        }
        if let Some(ref content_macro) = style.content_macro {
            crate::propresenter::macros::add_macro_to_content_cues(
                presentation,
                content_macro,
                self.macro_cache,
            );
        }
    }

    fn apply_background(
        presentation: &mut crate::propresenter::generated::rv_data::Presentation,
        background_category: &str,
    ) {
        let category = match background_category.to_lowercase().as_str() {
            "sermon" => crate::propresenter::background::BackgroundCategory::Sermon,
            _ => crate::propresenter::background::BackgroundCategory::Default,
        };
        let data_dir = find_data_subdir("");
        match crate::propresenter::background::resolve_background_image(&data_dir, category) {
            Some(image_path) => {
                crate::propresenter::background::add_background_to_first_cue(
                    presentation,
                    &image_path,
                );
            }
            None => {
                eprintln!(
                    "Warning: no background image found for '{background_category}' in {}",
                    data_dir.join("backgrounds").display()
                );
            }
        }
    }
}

fn apply_override(
    entry: &ResolvedItemPlan,
    override_entry: Option<&EntryOverride>,
) -> ResolvedItemPlan {
    let mut effective = entry.clone();
    if let Some(override_entry) = override_entry {
        if let Some(ref playlist_name) = override_entry.playlist_name {
            effective.playlist_name = playlist_name.clone();
        }
        if let Some(ref background) = override_entry.background {
            effective.style.background = Some(background.clone());
        }
        if let Some(ref arrangement) = override_entry.arrangement {
            effective.style.arrangement = Some(arrangement.clone());
        }
    }
    effective
}

/// When all content segments are uniformly colored (e.g. all-yellow Prayer of
/// Confession text), upgrade `Scripture/Prayer` content_macro to
/// `Scripture/Prayer (Highlighted)`. ProPresenter doesn't apply styling correctly
/// when the entire slide is highlighted with the regular macro.
fn maybe_upgrade_highlighted_macro(
    style: &super::plan::PresentationStyle,
    segments: &[StyledSegment],
) -> super::plan::PresentationStyle {
    const BASE_MACRO: &str = "Scripture/Prayer";
    const HIGHLIGHTED_MACRO: &str = "Scripture/Prayer (Highlighted)";

    let content_segments: Vec<_> = segments.iter().filter(|s| !s.text.is_empty()).collect();
    let needs_upgrade = style
        .content_macro
        .as_deref()
        .is_some_and(|m| m == BASE_MACRO)
        && !content_segments.is_empty()
        && content_segments.iter().all(|s| s.color.is_some());

    if needs_upgrade {
        let mut upgraded = style.clone();
        upgraded.content_macro = Some(HIGHLIGHTED_MACRO.to_string());
        upgraded
    } else {
        style.clone()
    }
}

fn parse_bible_version(version_text: Option<&str>) -> BibleVersion {
    version_text
        .and_then(BibleVersion::from_text)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

    use super::*;
    use serde::Deserialize;

    use crate::workflow::plan::{PresentationStyle, ResolvedItemPlan};

    #[test]
    fn apply_override_updates_name_background_and_arrangement() {
        let entry = ResolvedItemPlan {
            output_key: "3:main".to_string(),
            position: 3,
            pco_title: "Call to Worship".to_string(),
            playlist_name: "Call to Worship".to_string(),
            action: PlanAction::EditInPlace,
            style: PresentationStyle {
                background: Some("default".to_string()),
                arrangement: Some("Base".to_string()),
                ..PresentationStyle::default()
            },
            ..ResolvedItemPlan::default()
        };
        let override_entry = EntryOverride {
            output_key: "3:main".to_string(),
            playlist_name: Some("Weekly Call to Worship".to_string()),
            background: Some("sermon".to_string()),
            arrangement: Some("Override".to_string()),
        };

        let effective = apply_override(&entry, Some(&override_entry));

        assert_eq!(effective.playlist_name, "Weekly Call to Worship");
        assert_eq!(effective.style.background.as_deref(), Some("sermon"));
        assert_eq!(effective.style.arrangement.as_deref(), Some("Override"));
    }

    #[derive(Debug, Deserialize)]
    struct EntryFixture {
        output_key: String,
        position: usize,
        pco_title: String,
        playlist_name: String,
        status: FixtureStatus,
        #[serde(default)]
        reason: String,
        file_path: Option<String>,
        background: Option<String>,
        arrangement: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "snake_case")]
    enum FixtureStatus {
        Used,
        Created,
        Edited,
        Skipped,
        Uncertain,
    }

    #[derive(Debug, Deserialize)]
    struct OverrideFixture {
        output_key: String,
        playlist_name: Option<String>,
        background: Option<String>,
        arrangement: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    struct EntryOverrideFixture {
        entry: EntryFixture,
        #[serde(rename = "override")]
        override_entry: OverrideFixture,
    }

    #[test]
    fn apply_override_uses_fixture_data_for_build_adjacent_inputs() {
        let fixture: EntryOverrideFixture = serde_json::from_str(include_str!(
            "../../tests/fixtures/workflow/entry_override.json"
        ))
        .expect("fixture override should parse");

        let entry = ResolvedItemPlan {
            output_key: fixture.entry.output_key,
            position: fixture.entry.position,
            pco_title: fixture.entry.pco_title,
            playlist_name: fixture.entry.playlist_name,
            action: match fixture.entry.status {
                FixtureStatus::Used => PlanAction::UseExisting,
                FixtureStatus::Created => PlanAction::GenerateNew,
                FixtureStatus::Edited => PlanAction::EditInPlace,
                FixtureStatus::Skipped => PlanAction::Skip,
                FixtureStatus::Uncertain => PlanAction::NeedsReview,
            },
            reason: fixture.entry.reason,
            file_path: fixture.entry.file_path,
            style: PresentationStyle {
                background: fixture.entry.background,
                arrangement: fixture.entry.arrangement,
                ..PresentationStyle::default()
            },
            ..ResolvedItemPlan::default()
        };

        let override_entry = EntryOverride {
            output_key: fixture.override_entry.output_key,
            playlist_name: fixture.override_entry.playlist_name,
            background: fixture.override_entry.background,
            arrangement: fixture.override_entry.arrangement,
        };

        let effective = apply_override(&entry, Some(&override_entry));

        assert_eq!(effective.playlist_name, "Weekly Call to Worship");
        assert_eq!(effective.style.background.as_deref(), Some("sermon"));
        assert_eq!(effective.style.arrangement.as_deref(), Some("Override"));
        assert_eq!(
            effective.file_path.as_deref(),
            Some("/tmp/fixture/Call to Worship.pro")
        );
    }
}
