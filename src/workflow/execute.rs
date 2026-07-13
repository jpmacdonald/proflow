//! Shared service build execution.
#![allow(clippy::too_many_lines, missing_docs)]

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use thiserror::Error;
use tokio::sync::Mutex;
use uuid::Uuid;

use super::approval::{
    OutputManifest, OutputReviewError, ReviewedServicePlan, SourceManifest, SourceReviewError,
};
use super::classify;
use super::description_parser::to_styled_segments;
use super::plan::{
    ContentSource, ItemKind, PlanAction, ResolvedBackground, ResolvedItemPlan, ScriptureRequest,
};
use super::report::{BuildServiceEntry, BuildServiceResult};
use super::transaction::BuildFileTransaction;
use crate::bible::{parse_scripture_ref, BibleService, BibleVersion};
use crate::paths::data_root;
use crate::planning_center::PlanningCenterClient;
use crate::project_config::ProjectConfig;
use crate::propresenter::background::BackgroundImageError;
use crate::propresenter::deserialize::ProPresenterError;
use crate::propresenter::generated::rv_data;
use crate::propresenter::macros::{add_macro_to_cue_entries, MacroApplyError, MacroCache};
use crate::propresenter::media::presentation_media_dependencies_from_bytes;
use crate::propresenter::package::PlaylistPackageMode;
use crate::propresenter::playlist::{
    build_playlist, canonical_presentation_name, playlist_output_path,
    write_playlist_file_with_reviewed_media, PlaylistEntry, PlaylistError, PlaylistMediaAsset,
    PlaylistMetadata, SelectedArrangement,
};
use crate::propresenter::rtf::StyledSegment;
use crate::propresenter::serialize::{write_presentation_file, SerializeError};
use crate::propresenter::template::{
    apply_application_info, assemble_presentation_with_title_template_and_roles,
    build_combined_scripture_presentation_dual_template_with_roles,
    build_scripture_presentation_dual_template_with_roles, pack_segments_for_slides,
    preserve_presentation_envelope, RenderedPresentation, ScripturePassage, ThemeCache,
    ThemeSlideError, DEFAULT_MAX_LINES_PER_SLIDE,
};
use crate::propresenter::SlideType;
use crate::utils::file_index::FileIndex;

/// Per-entry override applied during service build execution.
#[derive(Debug, Clone, Default)]
pub struct EntryOverride {
    pub output_key: String,
    pub action: Option<PlanAction>,
    pub playlist_name: Option<String>,
    pub file_path: Option<String>,
    pub slide_type: Option<OverrideSlideType>,
    pub background: Option<ResolvedBackground>,
    pub arrangement: Option<String>,
}

/// Semantic slide role accepted at service-build boundaries.
#[derive(Debug, Clone, Copy, serde::Deserialize, schemars::JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OverrideSlideType {
    /// Generic text presentation.
    Text,
    /// Song lyric presentation.
    #[serde(alias = "song")]
    Lyrics,
    /// Scripture presentation.
    Scripture,
    /// Title presentation.
    Title,
    /// Graphic presentation.
    Graphic,
    /// Person or content nametag presentation.
    #[serde(alias = "person_nametag")]
    Nametag,
}

impl std::str::FromStr for OverrideSlideType {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "text" => Ok(Self::Text),
            "lyrics" | "song" => Ok(Self::Lyrics),
            "scripture" => Ok(Self::Scripture),
            "title" => Ok(Self::Title),
            "graphic" => Ok(Self::Graphic),
            "nametag" | "person_nametag" => Ok(Self::Nametag),
            _ => Err(format!(
                "unknown slide type '{value}'; expected text, lyrics, scripture, title, graphic, or nametag"
            )),
        }
    }
}

/// Input arguments for the shared service build workflow.
#[derive(Debug, Clone, Default)]
pub struct BuildRequest {
    pub plan_id: String,
    pub service_name: Option<String>,
    pub playlist_name: Option<String>,
    pub skip_output_keys: Vec<String>,
    pub overrides: Vec<EntryOverride>,
    pub playlist_package_mode: PlaylistPackageMode,
    pub media_assets: Vec<PlaylistMediaAsset>,
}

/// One complete build request bound to the decisions, source bytes, and output
/// target states shown in an operator preview.
///
/// The request fields are private and the playlist/service identities are
/// required before capture, so execution cannot add overrides, skips, media, or
/// a newly resolved date after approval.
#[derive(Debug)]
pub struct ReviewedBuildRequest {
    request: BoundBuildRequest,
    reviewed: ReviewedServicePlan,
    presentation_size: crate::propresenter::PresentationSize,
    backgrounds: Vec<ReviewedBackgroundPath>,
    outputs: OutputManifest,
}

impl ReviewedBuildRequest {
    fn capture(
        mut request: BoundBuildRequest,
        plans: Vec<ResolvedItemPlan>,
        presentation_size: crate::propresenter::PresentationSize,
        project_data_root: &Path,
        additional_sources: impl IntoIterator<Item = PathBuf>,
        output_targets: impl IntoIterator<Item = PathBuf>,
    ) -> Result<Self, BuildServiceError> {
        let backgrounds = resolve_reviewed_backgrounds(&plans, project_data_root)?;
        let mut additional_sources = additional_sources.into_iter().collect::<Vec<_>>();
        additional_sources.extend(backgrounds.iter().map(|background| background.path.clone()));
        let mut reviewed = ReviewedServicePlan::capture_with_additional_sources(
            plans,
            project_data_root,
            additional_sources,
        )?;
        if matches!(
            request.playlist_package_mode,
            PlaylistPackageMode::ExportPortable
        ) {
            for background in &backgrounds {
                if !request
                    .media_assets
                    .iter()
                    .any(|asset| asset.source_path == background.path)
                {
                    request
                        .media_assets
                        .push(PlaylistMediaAsset::new(&background.path));
                }
            }
            let media_paths = reviewed_media_source_paths(&reviewed)?;
            reviewed.extend_sources(media_paths.iter().cloned())?;
            for path in media_paths {
                if !request
                    .media_assets
                    .iter()
                    .any(|asset| asset.source_path == path)
                {
                    request.media_assets.push(PlaylistMediaAsset::new(path));
                }
            }
        }
        let outputs = OutputManifest::capture(output_targets)?;
        Ok(Self {
            request,
            reviewed,
            presentation_size,
            backgrounds,
            outputs,
        })
    }

    /// Return the exact effective plans rendered in the operator preview.
    pub fn plans(&self) -> &[ResolvedItemPlan] {
        self.reviewed.plans()
    }

    /// Stable Planning Center plan identity bound to this request.
    pub fn plan_id(&self) -> &str {
        &self.request.plan_id
    }

    /// Planning Center service type resolved before preview approval.
    pub fn service_name(&self) -> &str {
        &self.request.service_name
    }

    /// Final playlist display/file identity resolved before preview approval.
    pub fn playlist_name(&self) -> &str {
        &self.request.playlist_name
    }

    /// Playlist package mode included in the reviewed request.
    pub const fn playlist_package_mode(&self) -> PlaylistPackageMode {
        self.request.playlist_package_mode
    }

    /// Explicit portable-media sources included in the reviewed request.
    pub fn media_assets(&self) -> &[PlaylistMediaAsset] {
        &self.request.media_assets
    }

    #[cfg(test)]
    pub(crate) fn offline_test(
        plan_id: &str,
        service_name: &str,
        playlist_name: &str,
    ) -> Result<Self, BuildServiceError> {
        let request = BoundBuildRequest::try_from(BuildRequest {
            plan_id: plan_id.to_string(),
            service_name: Some(service_name.to_string()),
            playlist_name: Some(playlist_name.to_string()),
            ..BuildRequest::default()
        })?;
        Self::capture(
            request,
            Vec::new(),
            crate::propresenter::PresentationSize::FULL_HD,
            Path::new("."),
            std::iter::empty(),
            std::iter::empty(),
        )
    }
}

#[derive(Debug)]
struct BoundBuildRequest {
    plan_id: String,
    service_name: String,
    playlist_name: String,
    playlist_package_mode: PlaylistPackageMode,
    media_assets: Vec<PlaylistMediaAsset>,
}

impl TryFrom<BuildRequest> for BoundBuildRequest {
    type Error = BuildServiceError;

    fn try_from(request: BuildRequest) -> Result<Self, Self::Error> {
        let plan_id = required_identity("plan_id", request.plan_id)?;
        let service_name = required_identity(
            "service_name",
            request
                .service_name
                .ok_or(BuildServiceError::UnresolvedIdentity {
                    field: "service_name",
                })?,
        )?;
        let playlist_name = required_identity(
            "playlist_name",
            request
                .playlist_name
                .ok_or(BuildServiceError::UnresolvedIdentity {
                    field: "playlist_name",
                })?,
        )?;
        if matches!(
            request.playlist_package_mode,
            PlaylistPackageMode::LibraryLocal
        ) && !request.media_assets.is_empty()
        {
            return Err(BuildServiceError::message(
                "media_assets require export_portable package mode",
            ));
        }
        let media_assets = request
            .media_assets
            .into_iter()
            .map(|mut asset| {
                asset.source_path = canonical_media_source(&asset.source_path)?;
                Ok(asset)
            })
            .collect::<Result<Vec<_>, BuildServiceError>>()?;
        Ok(Self {
            plan_id,
            service_name,
            playlist_name,
            playlist_package_mode: request.playlist_package_mode,
            media_assets,
        })
    }
}

fn required_identity(field: &'static str, value: String) -> Result<String, BuildServiceError> {
    if value.trim().is_empty() {
        Err(BuildServiceError::UnresolvedIdentity { field })
    } else {
        Ok(value)
    }
}

fn canonical_media_source(path: &Path) -> Result<PathBuf, BuildServiceError> {
    let canonical = path
        .canonicalize()
        .map_err(|source| BuildServiceError::MediaSource {
            path: path.to_path_buf(),
            source,
        })?;
    if canonical.is_file() {
        Ok(canonical)
    } else {
        Err(BuildServiceError::MediaSourceNotFile { path: canonical })
    }
}

#[derive(Debug)]
struct PreparedExistingPresentation {
    embedded_data: Vec<u8>,
    selected_arrangement: Option<SelectedArrangement>,
    file_path: PathBuf,
}

/// Validated operation emitted by the preflight phase. File requirements are
/// carried by the variants that need them, so execution cannot observe an
/// action with a missing or contradictory path.
#[derive(Debug)]
enum ExecutableAction {
    Skip { summary: String },
    UseExisting { file_path: PathBuf },
    EditInPlace { file_path: PathBuf },
    GenerateNew,
}

#[derive(Debug)]
struct ExecutablePlan {
    plan: ResolvedItemPlan,
    action: ExecutableAction,
    reviewed_background: Option<PathBuf>,
}

#[derive(Debug)]
struct ReviewedBackgroundPath {
    output_key: String,
    path: PathBuf,
}

#[derive(Clone, Copy)]
struct ReviewedBackgroundAsset<'a> {
    path: &'a Path,
    data: &'a [u8],
}

#[derive(Clone, Copy)]
struct ReviewedRenderTarget<'a> {
    write_path: &'a Path,
    final_path: &'a Path,
    existing_bytes: Option<&'a [u8]>,
    presentation_size: crate::propresenter::PresentationSize,
    background: Option<ReviewedBackgroundAsset<'a>>,
}

/// Plans that passed semantic preflight with source and output snapshots bound
/// to this execution attempt.
#[derive(Debug)]
struct ApprovedServicePlan {
    request: BoundBuildRequest,
    executable_plans: Vec<ExecutablePlan>,
    presentation_size: crate::propresenter::PresentationSize,
    sources: SourceManifest,
    outputs: OutputManifest,
}

/// Errors raised while executing a service build.
#[derive(Debug, Error)]
pub enum BuildServiceError {
    #[error("{0}")]
    Message(String),
    /// A configured arrangement is not present in the rendered presentation.
    #[error("presentation '{presentation}' has no arrangement named '{arrangement}'")]
    ArrangementUnavailable {
        /// Presentation being rendered.
        presentation: String,
        /// Configured arrangement name.
        arrangement: String,
    },
    /// A requested translation identifier is not supported by the local Bible data.
    #[error("unsupported Bible version '{0}'")]
    UnsupportedBibleVersion(String),
    /// A lookup returned only part of the requested scripture range.
    #[error("scripture lookup for '{reference}' is missing verses {verses:?}")]
    MissingVerses {
        /// Requested scripture reference.
        reference: String,
        /// Verse numbers absent from the selected Bible data.
        verses: Vec<u32>,
    },
    /// A request reached classification/review without one concrete identity.
    #[error("build request {field} must be a non-empty resolved value before review")]
    UnresolvedIdentity {
        /// Missing or empty request field.
        field: &'static str,
    },
    /// A portable-media source could not be resolved to one stable file.
    #[error("failed to resolve reviewed media source '{}': {source}", path.display())]
    MediaSource {
        /// Requested media path.
        path: PathBuf,
        /// Filesystem resolution failure.
        #[source]
        source: std::io::Error,
    },
    /// A portable-media source resolved to a non-file path.
    #[error("reviewed media source is not a regular file: {}", path.display())]
    MediaSourceNotFile {
        /// Canonical media path.
        path: PathBuf,
    },
    #[error(transparent)]
    Deserialize(#[from] ProPresenterError),
    #[error(transparent)]
    Serialize(#[from] SerializeError),
    #[error(transparent)]
    Playlist(#[from] PlaylistError),
    #[error(transparent)]
    Bible(#[from] crate::error::Error),
    #[error(transparent)]
    Background(#[from] BackgroundImageError),
    #[error(transparent)]
    Macro(#[from] MacroApplyError),
    #[error(transparent)]
    ThemeSlide(#[from] ThemeSlideError),
    #[error(transparent)]
    SourceReview(#[from] SourceReviewError),
    #[error(transparent)]
    OutputReview(#[from] OutputReviewError),
    #[error("reviewed background binding for plan '{output_key}' is inconsistent")]
    ReviewedBackgroundInvariant { output_key: String },
    /// Rendered slides did not materialize the project output dimensions.
    #[error(
        "presentation '{output_key}' violates project size {expected}: {actual}; theme application must follow output-size selection"
    )]
    PresentationSizeInvariant {
        /// Stable reviewed output identity.
        output_key: String,
        /// Project-owned required dimensions.
        expected: crate::propresenter::PresentationSize,
        /// First concrete mismatch found.
        actual: String,
    },
    #[error("filesystem transaction failed: {0}")]
    Io(#[from] std::io::Error),
}

impl BuildServiceError {
    fn message(msg: impl Into<String>) -> Self {
        Self::Message(msg.into())
    }
}

/// Shared executor for full-service builds.
pub struct ServiceBuildExecutor<'a> {
    pco_client: &'a PlanningCenterClient,
    bible_service: &'a Arc<Mutex<BibleService>>,
    file_index: &'a Arc<Mutex<Option<FileIndex>>>,
    template_cache: &'a ThemeCache,
    macro_cache: &'a MacroCache,
    playlist_metadata: &'a PlaylistMetadata,
    playlist_output_dir: Option<&'a Path>,
    generated_presentation_dir: Option<&'a Path>,
    project_data_root: PathBuf,
}

impl<'a> ServiceBuildExecutor<'a> {
    /// Create a new service build executor over shared runtime dependencies.
    #[allow(
        clippy::too_many_arguments,
        reason = "the constructor exposes each immutable runtime dependency explicitly"
    )]
    pub fn new(
        pco_client: &'a PlanningCenterClient,
        bible_service: &'a Arc<Mutex<BibleService>>,
        file_index: &'a Arc<Mutex<Option<FileIndex>>>,
        template_cache: &'a ThemeCache,
        macro_cache: &'a MacroCache,
        playlist_metadata: &'a PlaylistMetadata,
        playlist_output_dir: Option<&'a Path>,
        generated_presentation_dir: Option<&'a Path>,
    ) -> Self {
        Self {
            pco_client,
            bible_service,
            file_index,
            template_cache,
            macro_cache,
            playlist_metadata,
            playlist_output_dir,
            generated_presentation_dir,
            project_data_root: data_root(),
        }
    }

    /// Execute a full service build from plan/config inputs.
    pub async fn build_service(
        &self,
        request: &BuildRequest,
        mappings: &ProjectConfig,
    ) -> Result<BuildServiceResult, BuildServiceError> {
        let request = self.resolve_request_identity(request).await?;
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

        let reviewed =
            self.review_build_request(request, &plans, mappings.defaults.presentation_size)?;
        self.build_reviewed_request(reviewed).await
    }

    /// Capture and execute resolved decisions through the same source-integrity
    /// boundary used by reviewed MCP builds.
    pub async fn build_resolved_service(
        &self,
        request: &BuildRequest,
        plans: &[ResolvedItemPlan],
        presentation_size: crate::propresenter::PresentationSize,
    ) -> Result<BuildServiceResult, BuildServiceError> {
        let request = self.resolve_request_identity(request).await?;
        let reviewed = self.review_build_request(request, plans, presentation_size)?;
        self.build_reviewed_request(reviewed).await
    }

    /// Bind a fully resolved request to the effective plans, source bytes, and
    /// output target states shown during preview.
    pub fn review_build_request(
        &self,
        mut request: BuildRequest,
        plans: &[ResolvedItemPlan],
        presentation_size: crate::propresenter::PresentationSize,
    ) -> Result<ReviewedBuildRequest, BuildServiceError> {
        let skip_output_keys = std::mem::take(&mut request.skip_output_keys);
        let overrides = std::mem::take(&mut request.overrides);
        let request = BoundBuildRequest::try_from(request)?;
        let plans = resolve_requested_plans(plans, &skip_output_keys, &overrides)?;
        let mut additional_sources = request
            .media_assets
            .iter()
            .map(|asset| asset.source_path.clone())
            .collect::<Vec<_>>();
        let mut output_targets = vec![playlist_output_path(
            self.playlist_output_dir,
            &request.playlist_name,
        )];
        for plan in &plans {
            match plan.action {
                PlanAction::GenerateNew => {
                    let target = self.presentation_target(plan);
                    if target.is_file() {
                        additional_sources.push(target.clone());
                    }
                    output_targets.push(target);
                }
                PlanAction::EditInPlace => {
                    output_targets.extend(plan.file_path.as_deref().map(PathBuf::from));
                }
                PlanAction::UseExisting | PlanAction::Skip | PlanAction::NeedsReview => {}
            }
        }
        ReviewedBuildRequest::capture(
            request,
            plans,
            presentation_size,
            &self.project_data_root,
            additional_sources,
            output_targets,
        )
    }

    /// Execute exactly one request previously produced by
    /// [`Self::review_build_request`].
    pub async fn build_reviewed_request(
        &self,
        reviewed: ReviewedBuildRequest,
    ) -> Result<BuildServiceResult, BuildServiceError> {
        let approved = Self::approve_service_plan(reviewed)?;
        self.build_approved_service(approved).await
    }

    fn approve_service_plan(
        reviewed: ReviewedBuildRequest,
    ) -> Result<ApprovedServicePlan, BuildServiceError> {
        let ReviewedBuildRequest {
            request,
            reviewed,
            presentation_size,
            backgrounds,
            outputs,
        } = reviewed;
        let (plans, reviewed_sources) = reviewed.into_verified_parts()?;
        let mut executable_plans = prepare_build(&plans)?;
        for executable in &mut executable_plans {
            let reviewed_background = backgrounds
                .iter()
                .find(|background| background.output_key == executable.plan.output_key)
                .map(|background| background.path.clone());
            if executable.plan.style.background.is_some() != reviewed_background.is_some() {
                return Err(BuildServiceError::ReviewedBackgroundInvariant {
                    output_key: executable.plan.output_key.clone(),
                });
            }
            executable.reviewed_background = reviewed_background;
        }

        Ok(ApprovedServicePlan {
            request,
            executable_plans,
            presentation_size,
            sources: reviewed_sources,
            outputs,
        })
    }

    async fn build_approved_service(
        &self,
        approved: ApprovedServicePlan,
    ) -> Result<BuildServiceResult, BuildServiceError> {
        approved.sources.verify()?;
        approved.outputs.verify()?;
        let ApprovedServicePlan {
            request,
            executable_plans,
            presentation_size,
            sources,
            outputs,
        } = approved;

        let mut playlist_entries: Vec<PlaylistEntry> = Vec::new();
        let mut summary_entries: Vec<BuildServiceEntry> = Vec::new();
        let package_media_assets = request.media_assets.clone();
        let mut generated_count = 0usize;
        let mut library_count = 0usize;
        let mut skipped_count = 0usize;

        let mut transaction = BuildFileTransaction::new();

        for executable in executable_plans {
            let ExecutablePlan {
                plan: effective_plan,
                action,
                reviewed_background,
            } = executable;
            let reviewed_background = if let Some(path) = reviewed_background.as_deref() {
                Some(ReviewedBackgroundAsset {
                    path,
                    data: approved_source_bytes(&sources, path)?,
                })
            } else {
                None
            };
            match action {
                ExecutableAction::Skip { summary } => {
                    skipped_count += 1;
                    summary_entries.push(BuildServiceEntry {
                        output_key: effective_plan.output_key.clone(),
                        position: effective_plan.position,
                        name: effective_plan.pco_title.clone(),
                        action: summary,
                        file_path: None,
                        slides: None,
                        warnings: Vec::new(),
                    });
                }
                ExecutableAction::UseExisting { file_path } => {
                    let source_bytes = approved_source_bytes(&sources, &file_path)?;
                    let prepared = Self::prepare_existing_presentation(
                        &effective_plan,
                        &file_path,
                        source_bytes,
                        presentation_size,
                    )?;

                    let prepared_path = prepared.file_path.display().to_string();

                    playlist_entries.push(PlaylistEntry {
                        name: effective_plan.playlist_name.clone(),
                        slide_type: effective_plan.slide_type(),
                        from_matched_file: true,
                        presentation_path: prepared_path.clone(),
                        selected_arrangement: prepared.selected_arrangement,
                        user_music_key: None,
                        embedded_data: Some(prepared.embedded_data),
                    });

                    library_count += 1;
                    summary_entries.push(BuildServiceEntry {
                        output_key: effective_plan.output_key.clone(),
                        position: effective_plan.position,
                        name: effective_plan.playlist_name.clone(),
                        action: "library".to_string(),
                        file_path: Some(prepared_path),
                        slides: None,
                        warnings: Vec::new(),
                    });
                }
                ExecutableAction::EditInPlace { file_path } => {
                    let final_path = file_path;
                    outputs.verify_target(&final_path)?;
                    let staged_path = transaction.stage_for(&final_path)?;
                    let source_bytes = approved_source_bytes(&sources, &final_path)?;
                    let target = ReviewedRenderTarget {
                        write_path: &staged_path,
                        final_path: &final_path,
                        existing_bytes: Some(source_bytes),
                        presentation_size,
                        background: reviewed_background,
                    };
                    let (playlist_entry, slides) =
                        self.generate_from_description(&effective_plan, target)?;
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
                        warnings: zero_slide_warnings(slides),
                    });
                }
                ExecutableAction::GenerateNew => {
                    let final_path = self.presentation_target(&effective_plan);
                    outputs.verify_target(&final_path)?;
                    let staged_path = transaction.stage_for(&final_path)?;
                    let existing_target_bytes = sources.bytes(&final_path);
                    let target = ReviewedRenderTarget {
                        write_path: &staged_path,
                        final_path: &final_path,
                        existing_bytes: existing_target_bytes,
                        presentation_size,
                        background: reviewed_background,
                    };
                    let (playlist_entry, slides) = match &effective_plan.content_source {
                        ContentSource::Scripture { .. } => {
                            self.generate_scripture(&effective_plan, target, &sources)
                                .await?
                        }
                        ContentSource::Description { .. } | ContentSource::None => {
                            self.generate_from_description(&effective_plan, target)?
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
                        warnings: zero_slide_warnings(slides),
                    });
                }
            }
        }

        let playlist_name = request.playlist_name.clone();
        let playlist = build_playlist(&playlist_name, &playlist_entries, self.playlist_metadata);
        let output_path = playlist_output_path(self.playlist_output_dir, &playlist_name);
        let media_asset_count = package_media_assets.len();
        outputs.verify_target(&output_path)?;
        let staged_playlist_path = transaction.stage_for(&output_path)?;
        let reviewed_media_assets = if matches!(
            request.playlist_package_mode,
            PlaylistPackageMode::ExportPortable
        ) {
            for path in discovered_media_paths(&playlist_entries)? {
                if sources.bytes(&path).is_none() {
                    return Err(BuildServiceError::message(format!(
                        "portable media source was not captured by the reviewed request: {}",
                        path.display()
                    )));
                }
            }
            sources.verify()?;
            package_media_assets
                .iter()
                .map(|asset| {
                    let bytes = approved_source_bytes(&sources, &asset.source_path)?;
                    asset.bind_reviewed(bytes).map_err(BuildServiceError::from)
                })
                .collect::<Result<Vec<_>, BuildServiceError>>()?
        } else {
            Vec::new()
        };
        write_playlist_file_with_reviewed_media(
            &playlist,
            &playlist_entries,
            &staged_playlist_path,
            request.playlist_package_mode,
            &reviewed_media_assets,
        )?;
        // Recheck every path-backed source after rendering. Portable media is
        // written from the reviewed bytes above, so a transient path change
        // cannot introduce unreviewed archive content.
        sources.verify()?;
        outputs.verify()?;
        let committed_paths = transaction.commit()?;
        for path in committed_paths
            .iter()
            .filter(|path| path.extension().is_some_and(|extension| extension == "pro"))
        {
            self.refresh_file_index(path).await;
        }
        let warnings = collect_build_warnings(&summary_entries);

        Ok(BuildServiceResult {
            playlist_path: output_path.display().to_string(),
            package_mode: request.playlist_package_mode,
            media_asset_count,
            entries: summary_entries,
            total_items: playlist_entries.len(),
            generated_count,
            library_count,
            skipped_count,
            warnings,
        })
    }

    fn generate_from_description(
        &self,
        entry: &ResolvedItemPlan,
        target: ReviewedRenderTarget<'_>,
    ) -> Result<(PlaylistEntry, usize), BuildServiceError> {
        let segments: Vec<StyledSegment> = entry
            .parsed_content()
            .map(to_styled_segments)
            .unwrap_or_default();
        let title_only_generation = is_title_only_generation(entry) && entry.file_path.is_none();
        let title_text = entry
            .parsed_content()
            .and_then(|content| content.title_text.clone())
            .or_else(|| title_only_generation.then(|| entry.playlist_name.clone()));

        if segments.is_empty() && !title_only_generation {
            return Err(BuildServiceError::message(format!(
                "No parsed content for generated item '{}'",
                entry.pco_title
            )));
        }

        let title_template = if let Some(ref title_slide_name) = entry.style.title_slide {
            Some(self.template_cache.text_template(title_slide_name)?.clone())
        } else {
            None
        };

        if let Some(ref file_path) = entry.file_path {
            let source_bytes = target.existing_bytes.ok_or_else(|| {
                BuildServiceError::message(format!(
                    "approved source bytes are missing for '{file_path}'"
                ))
            })?;
            let existing = <rv_data::Presentation as prost::Message>::decode(source_bytes)
                .map_err(|error| {
                    BuildServiceError::message(format!(
                        "failed to decode approved presentation '{file_path}': {error}"
                    ))
                })?;
            let content_slide_name = entry.style.content_slide.as_deref().ok_or_else(|| {
                BuildServiceError::message(format!(
                    "edited item '{}' has no configured content cue role",
                    entry.output_key
                ))
            })?;
            let content_template = self
                .template_cache
                .text_template(content_slide_name)?
                .clone();

            let mut rendered = build_description_presentation_with_templates(
                &existing.name,
                &content_template,
                title_template.as_ref(),
                &segments,
                title_text.as_deref(),
                entry.style.max_lines_per_slide,
            )
            .ok_or_else(|| {
                BuildServiceError::message(format!(
                    "Failed to edit presentation '{}'",
                    entry.playlist_name
                ))
            })?;
            preserve_presentation_envelope(&mut rendered.presentation, &existing);

            self.apply_style(
                &mut rendered,
                &entry.style,
                all_content_segments_colored(&segments),
                target.background,
            )?;
            apply_application_info(
                &mut rendered.presentation,
                Some(self.playlist_metadata.application_info()),
            );

            validate_rendered_presentation_size(
                &rendered.presentation,
                target.presentation_size,
                &entry.output_key,
            )?;
            write_presentation_file(&rendered.presentation, target.write_path)?;

            let slide_count = rendered.presentation.cues.len();
            let embedded_data = std::fs::read(target.write_path)?;
            let file_stem = target
                .final_path
                .file_stem()
                .and_then(|segment| segment.to_str())
                .unwrap_or(&entry.playlist_name);

            return Ok((
                PlaylistEntry {
                    name: file_stem.to_string(),
                    slide_type: entry.slide_type(),
                    from_matched_file: true,
                    presentation_path: target.final_path.display().to_string(),
                    selected_arrangement: None,
                    user_music_key: None,
                    embedded_data: Some(embedded_data),
                },
                slide_count,
            ));
        }

        let presentation_name =
            canonical_presentation_name(&entry.playlist_name, entry.slide_type());
        // Description-generated items often keep the same presentation name
        // while their weekly body text changes. Rebuild them from source every
        // time, preserving the existing UUID on write, instead of reusing stale
        // content by filename alone.

        let slide_name = entry.style.content_slide.as_deref().ok_or_else(|| {
            BuildServiceError::message(format!(
                "generated item '{}' has no configured content cue role",
                entry.output_key
            ))
        })?;

        let template_slide = self.template_cache.text_template(slide_name)?.clone();

        let mut rendered = build_description_presentation_with_templates(
            &presentation_name,
            &template_slide,
            title_template.as_ref(),
            &segments,
            title_text.as_deref(),
            entry.style.max_lines_per_slide,
        )
        .ok_or_else(|| {
            BuildServiceError::message(format!(
                "Failed to build presentation '{}'",
                entry.playlist_name
            ))
        })?;

        self.apply_style(
            &mut rendered,
            &entry.style,
            all_content_segments_colored(&segments),
            target.background,
        )?;

        Self::finalize_generated_document(
            &mut rendered.presentation,
            target.final_path,
            target.existing_bytes,
            self.playlist_metadata.application_info(),
        )?;
        validate_rendered_presentation_size(
            &rendered.presentation,
            target.presentation_size,
            &entry.output_key,
        )?;
        write_presentation_file(&rendered.presentation, target.write_path)?;

        let slide_count = rendered.presentation.cues.len();
        let embedded_data = std::fs::read(target.write_path)?;

        Ok((
            PlaylistEntry {
                name: presentation_name,
                slide_type: entry.slide_type(),
                from_matched_file: false,
                presentation_path: target.final_path.display().to_string(),
                selected_arrangement: None,
                user_music_key: None,
                embedded_data: Some(embedded_data),
            },
            slide_count,
        ))
    }

    #[allow(clippy::if_not_else)]
    async fn generate_scripture(
        &self,
        entry: &ResolvedItemPlan,
        target: ReviewedRenderTarget<'_>,
        sources: &SourceManifest,
    ) -> Result<(PlaylistEntry, usize), BuildServiceError> {
        if entry.item_kind != ItemKind::Scripture {
            return Err(BuildServiceError::message(format!(
                "Unknown created type for '{}'",
                entry.pco_title
            )));
        }

        let content_slide_name = entry.style.content_slide.as_deref().ok_or_else(|| {
            BuildServiceError::message(format!(
                "scripture item '{}' has no configured content cue role",
                entry.output_key
            ))
        })?;

        let title_slide_name = entry
            .style
            .title_slide
            .as_deref()
            .unwrap_or(content_slide_name);

        let content_template = self
            .template_cache
            .text_template(content_slide_name)?
            .clone();
        let title_template = if title_slide_name == content_slide_name {
            content_template.clone()
        } else {
            self.template_cache.text_template(title_slide_name)?.clone()
        };

        let presentation_name =
            canonical_presentation_name(&entry.playlist_name, SlideType::Scripture);
        // Scripture files are generated from source text and formatting rules.
        // Rebuild them when a playlist is built so fixes to packing/theme
        // behavior are reflected without requiring manual file deletion. The
        // existing presentation UUID is preserved before write.

        let scripture = entry.scripture_content().ok_or_else(|| {
            BuildServiceError::message(format!(
                "No scripture source configured for '{}'",
                entry.pco_title
            ))
        })?;
        let mut rendered = match scripture.request() {
            ScriptureRequest::Combined(references) => {
                let mut passages = Vec::new();
                let mut bible = self.bible_service.lock().await;

                for ref_info in references {
                    let reference = parse_scripture_ref(&ref_info.reference).ok_or_else(|| {
                        BuildServiceError::message(format!("Cannot parse: {}", ref_info.reference))
                    })?;
                    let version = parse_bible_version(&ref_info.version)?;
                    let source_path = bible_source_path(&self.project_data_root, version);
                    let source_bytes = approved_source_bytes(sources, &source_path)?;
                    let (header, verses) =
                        bible.lookup_verses_from_bytes(&reference, version, source_bytes)?;

                    if !header.missing_verses.is_empty() {
                        return Err(BuildServiceError::MissingVerses {
                            reference: ref_info.reference.clone(),
                            verses: header.missing_verses,
                        });
                    }

                    passages.push(ScripturePassage {
                        title: header.display(),
                        verses,
                    });
                }

                drop(bible);

                build_combined_scripture_presentation_dual_template_with_roles(
                    &presentation_name,
                    &title_template,
                    &content_template,
                    &passages,
                    entry.style.max_lines_per_slide,
                )
                .ok_or_else(|| {
                    BuildServiceError::message(
                        "Failed to build combined scripture presentation".to_string(),
                    )
                })?
            }
            ScriptureRequest::Single {
                reference: reference_text,
                bible_version,
            } => {
                let reference = parse_scripture_ref(reference_text).ok_or_else(|| {
                    BuildServiceError::message(format!("Cannot parse reference: {reference_text}"))
                })?;
                let version = parse_bible_version(bible_version)?;

                let source_path = bible_source_path(&self.project_data_root, version);
                let source_bytes = approved_source_bytes(sources, &source_path)?;
                let (header, verses) = self.bible_service.lock().await.lookup_verses_from_bytes(
                    &reference,
                    version,
                    source_bytes,
                )?;

                if !header.missing_verses.is_empty() {
                    return Err(BuildServiceError::MissingVerses {
                        reference: reference_text.to_string(),
                        verses: header.missing_verses,
                    });
                }

                let title = format!("Scripture\n{}", header.display());
                build_scripture_presentation_dual_template_with_roles(
                    &presentation_name,
                    &title_template,
                    &content_template,
                    &verses,
                    Some(&title),
                    entry.style.max_lines_per_slide,
                )
                .ok_or_else(|| {
                    BuildServiceError::message("Failed to build scripture presentation".to_string())
                })?
            }
        };

        self.apply_style(&mut rendered, &entry.style, false, target.background)?;

        Self::finalize_generated_document(
            &mut rendered.presentation,
            target.final_path,
            target.existing_bytes,
            self.playlist_metadata.application_info(),
        )?;
        validate_rendered_presentation_size(
            &rendered.presentation,
            target.presentation_size,
            &entry.output_key,
        )?;
        write_presentation_file(&rendered.presentation, target.write_path)?;

        Ok((
            PlaylistEntry {
                name: presentation_name,
                slide_type: SlideType::Scripture,
                from_matched_file: false,
                presentation_path: target.final_path.display().to_string(),
                selected_arrangement: None,
                user_music_key: None,
                embedded_data: Some(std::fs::read(target.write_path)?),
            },
            rendered.presentation.cues.len(),
        ))
    }

    async fn resolve_request_identity(
        &self,
        request: &BuildRequest,
    ) -> Result<BuildRequest, BuildServiceError> {
        let mut resolved = request.clone();
        let plan_id = required_identity("plan_id", resolved.plan_id.clone())?;
        let service_name = required_identity(
            "service_name",
            resolved
                .service_name
                .clone()
                .ok_or(BuildServiceError::UnresolvedIdentity {
                    field: "service_name",
                })?,
        )?;
        let playlist_name = if let Some(name) = resolved.playlist_name.clone() {
            name
        } else {
            let date = self.resolve_plan_date(&plan_id).await?;
            format!("{} - {service_name}", date.format("%B %-d, %Y"))
        };
        resolved.plan_id = plan_id;
        resolved.service_name = Some(service_name);
        resolved.playlist_name = Some(playlist_name);
        Ok(resolved)
    }

    async fn resolve_plan_date(
        &self,
        plan_id: &str,
    ) -> Result<chrono::DateTime<chrono::Utc>, BuildServiceError> {
        let (_, plans) = self
            .pco_client
            .get_upcoming_services(60)
            .await
            .map_err(|error| {
                BuildServiceError::message(format!(
                    "could not resolve date for plan {plan_id}: {error}"
                ))
            })?;
        plans
            .into_iter()
            .find(|plan| plan.id == plan_id)
            .map(|plan| plan.date)
            .ok_or_else(|| {
                BuildServiceError::message(format!(
                    "plan {plan_id} was not found while resolving its playlist date"
                ))
            })
    }

    async fn refresh_file_index(&self, output_path: &Path) {
        if let Some(ref mut index) = *self.file_index.lock().await {
            index.add_entry(output_path);
        }
    }

    fn output_presentation_path(&self, presentation_name: &str) -> PathBuf {
        self.generated_presentation_dir
            .unwrap_or_else(|| Path::new("."))
            .join(format!("{presentation_name}.pro"))
    }

    fn presentation_target(&self, entry: &ResolvedItemPlan) -> PathBuf {
        let name = canonical_presentation_name(&entry.playlist_name, entry.slide_type());
        self.output_presentation_path(&name)
    }

    /// Preserve target-owned metadata, then stamp the current producer.
    ///
    /// Keeping these operations in one boundary prevents an old target's
    /// `application_info` from silently replacing the runtime metadata after a
    /// rebuild.
    fn finalize_generated_document(
        presentation: &mut rv_data::Presentation,
        output_path: &Path,
        existing_source_bytes: Option<&[u8]>,
        application_info: &rv_data::ApplicationInfo,
    ) -> Result<(), BuildServiceError> {
        if let Some(source_bytes) = existing_source_bytes {
            let existing = <rv_data::Presentation as prost::Message>::decode(source_bytes)
                .map_err(|error| {
                    BuildServiceError::message(format!(
                        "failed to decode approved existing target '{}': {error}",
                        output_path.display()
                    ))
                })?;
            preserve_presentation_envelope(presentation, &existing);
        }
        apply_application_info(presentation, Some(application_info));
        Ok(())
    }

    fn prepare_existing_presentation(
        entry: &ResolvedItemPlan,
        source_path: &Path,
        source_bytes: &[u8],
        presentation_size: crate::propresenter::PresentationSize,
    ) -> Result<PreparedExistingPresentation, BuildServiceError> {
        let embedded_data = source_bytes.to_vec();
        let presentation = <rv_data::Presentation as prost::Message>::decode(source_bytes)
            .map_err(|error| {
                BuildServiceError::message(format!(
                    "failed to decode existing presentation '{}': {error}",
                    source_path.display()
                ))
            })?;
        validate_rendered_presentation_size(&presentation, presentation_size, &entry.output_key)?;
        let selected_arrangement = if let Some(name) = entry.style.arrangement.as_deref() {
            let arrangement = presentation
                .arrangements
                .iter()
                .find(|arrangement| arrangement.name.eq_ignore_ascii_case(name))
                .ok_or_else(|| BuildServiceError::ArrangementUnavailable {
                    presentation: presentation.name.clone(),
                    arrangement: name.to_string(),
                })?;
            let uuid = arrangement.uuid.as_ref().ok_or_else(|| {
                BuildServiceError::message(format!(
                    "arrangement '{name}' in '{}' has no UUID",
                    source_path.display()
                ))
            })?;
            let uuid = Uuid::parse_str(&uuid.string).map_err(|error| {
                BuildServiceError::message(format!(
                    "arrangement '{name}' in '{}' has invalid UUID: {error}",
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

    fn apply_macros(
        &self,
        rendered: &mut RenderedPresentation,
        style: &super::plan::PresentationStyle,
        all_content_colored: bool,
    ) -> Result<(), BuildServiceError> {
        if let Some(binding) = &style.first_cue_macro {
            let has_title_role = style.title_slide.is_some();
            let macro_name = binding.select(!has_title_role && all_content_colored);
            if has_title_role {
                add_macro_to_cue_entries(
                    &mut rendered.presentation,
                    rendered.cue_roles.title_entries(),
                    macro_name,
                    self.macro_cache,
                )?;
            } else if let Some(first_entry) = rendered.cue_roles.first_entry() {
                add_macro_to_cue_entries(
                    &mut rendered.presentation,
                    &[first_entry],
                    macro_name,
                    self.macro_cache,
                )?;
            }
        }
        if let Some(binding) = &style.first_content_cue_macro {
            let macro_name = binding.select(all_content_colored);
            add_macro_to_cue_entries(
                &mut rendered.presentation,
                rendered.cue_roles.content_entries(),
                macro_name,
                self.macro_cache,
            )?;
        }
        Ok(())
    }

    fn apply_style(
        &self,
        rendered: &mut RenderedPresentation,
        style: &super::plan::PresentationStyle,
        all_content_colored: bool,
        reviewed_background: Option<ReviewedBackgroundAsset<'_>>,
    ) -> Result<(), BuildServiceError> {
        self.apply_macros(rendered, style, all_content_colored)?;
        match (&style.background, reviewed_background) {
            (Some(_), Some(background)) => {
                crate::propresenter::background::add_reviewed_background_to_first_cue(
                    &mut rendered.presentation,
                    background.path,
                    background.data,
                );
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

fn validate_rendered_presentation_size(
    presentation: &rv_data::Presentation,
    expected: crate::propresenter::PresentationSize,
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

fn approved_source_bytes<'a>(
    sources: &'a SourceManifest,
    path: &Path,
) -> Result<&'a [u8], BuildServiceError> {
    sources.bytes(path).ok_or_else(|| {
        BuildServiceError::message(format!(
            "approved source manifest has no bytes for '{}'",
            path.display()
        ))
    })
}

fn bible_source_path(project_data_root: &Path, version: BibleVersion) -> PathBuf {
    project_data_root.join("bibles").join(version.file_name())
}

fn discovered_media_paths(entries: &[PlaylistEntry]) -> Result<Vec<PathBuf>, BuildServiceError> {
    let mut paths = HashSet::new();
    for entry in entries {
        let Some(data) = entry.embedded_data.as_deref() else {
            continue;
        };
        let dependencies = presentation_media_dependencies_from_bytes(data).map_err(|error| {
            BuildServiceError::message(format!(
                "failed to inspect media dependencies for '{}': {error}",
                entry.name
            ))
        })?;
        for dependency in dependencies {
            let path = dependency.path.ok_or_else(|| {
                BuildServiceError::message(format!(
                    "rendered media dependency is not an absolute local file: {}",
                    dependency.source
                ))
            })?;
            paths.insert(canonical_media_source(&path)?);
        }
    }
    Ok(paths.into_iter().collect())
}

fn reviewed_media_source_paths(
    reviewed: &ReviewedServicePlan,
) -> Result<Vec<PathBuf>, BuildServiceError> {
    let mut paths = HashSet::new();
    for plan in reviewed.plans() {
        if !matches!(
            plan.action,
            PlanAction::UseExisting | PlanAction::EditInPlace
        ) {
            continue;
        }
        let Some(source_path) = plan.file_path.as_deref().map(Path::new) else {
            continue;
        };
        let source_bytes = reviewed.source_bytes(source_path).ok_or_else(|| {
            BuildServiceError::message(format!(
                "reviewed plan has no captured bytes for '{}'",
                source_path.display()
            ))
        })?;
        let dependencies =
            presentation_media_dependencies_from_bytes(source_bytes).map_err(|error| {
                BuildServiceError::message(format!(
                    "failed to inspect reviewed media dependencies for '{}': {error}",
                    source_path.display()
                ))
            })?;
        for dependency in dependencies {
            let path = dependency.path.ok_or_else(|| {
                BuildServiceError::message(format!(
                    "reviewed media dependency is not an absolute local file: {}",
                    dependency.source
                ))
            })?;
            paths.insert(canonical_media_source(&path)?);
        }
    }
    Ok(paths.into_iter().collect())
}

fn resolve_reviewed_backgrounds(
    plans: &[ResolvedItemPlan],
    project_data_root: &Path,
) -> Result<Vec<ReviewedBackgroundPath>, BuildServiceError> {
    let mut backgrounds = Vec::new();
    for plan in plans {
        let Some(background) = &plan.style.background else {
            continue;
        };
        let path = crate::propresenter::background::resolve_background_image(
            project_data_root,
            background.file().as_path(),
        )?;
        backgrounds.push(ReviewedBackgroundPath {
            output_key: plan.output_key.clone(),
            path,
        });
    }
    Ok(backgrounds)
}

fn zero_slide_warnings(slides: usize) -> Vec<String> {
    if slides == 0 {
        vec!["presentation has zero slides".to_string()]
    } else {
        Vec::new()
    }
}

fn collect_build_warnings(entries: &[BuildServiceEntry]) -> Vec<String> {
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

#[cfg(test)]
fn presentation_output_dir(library_path: Option<&Path>) -> PathBuf {
    let base = library_path.unwrap_or_else(|| Path::new("."));
    let default_library = base.join("Default");
    if default_library.is_dir() {
        default_library
    } else {
        base.to_path_buf()
    }
}

fn validate_unique_request_keys(
    skip_output_keys: &[String],
    overrides: &[EntryOverride],
) -> Result<(), BuildServiceError> {
    let mut skip_keys = HashSet::new();
    for key in skip_output_keys {
        if !skip_keys.insert(key.as_str()) {
            return Err(BuildServiceError::message(format!(
                "duplicate skip_output_key '{key}'"
            )));
        }
    }

    let mut override_keys = HashSet::new();
    for entry in overrides {
        if !override_keys.insert(entry.output_key.as_str()) {
            return Err(BuildServiceError::message(format!(
                "duplicate override for output_key '{}'",
                entry.output_key
            )));
        }
        if skip_keys.contains(entry.output_key.as_str()) {
            return Err(BuildServiceError::message(format!(
                "output_key '{}' cannot be both skipped and overridden",
                entry.output_key
            )));
        }
    }
    Ok(())
}

fn resolve_requested_plans(
    plans: &[ResolvedItemPlan],
    skip_output_keys: &[String],
    overrides: &[EntryOverride],
) -> Result<Vec<ResolvedItemPlan>, BuildServiceError> {
    validate_unique_request_keys(skip_output_keys, overrides)?;
    let skip_set = skip_output_keys
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let override_map = overrides
        .iter()
        .map(|entry| (entry.output_key.as_str(), entry))
        .collect::<HashMap<_, _>>();
    validate_requested_plan_keys(plans, &skip_set, &override_map)?;

    Ok(plans
        .iter()
        .map(|plan| {
            if skip_set.contains(plan.output_key.as_str()) {
                let mut skipped = plan.clone();
                skipped.action = PlanAction::Skip;
                skipped.reason = "Skipped by reviewed build request".to_string();
                skipped
            } else {
                apply_override(plan, override_map.get(plan.output_key.as_str()).copied())
            }
        })
        .collect())
}

fn validate_requested_plan_keys(
    plans: &[ResolvedItemPlan],
    skip_set: &HashSet<&str>,
    override_map: &HashMap<&str, &EntryOverride>,
) -> Result<(), BuildServiceError> {
    let mut known_keys = HashSet::new();
    let mut duplicate_keys = Vec::new();
    for plan in plans {
        if !known_keys.insert(plan.output_key.as_str()) {
            duplicate_keys.push(plan.output_key.as_str());
        }
    }
    if !duplicate_keys.is_empty() {
        duplicate_keys.sort_unstable();
        duplicate_keys.dedup();
        return Err(BuildServiceError::message(format!(
            "duplicate plan output_keys: {}",
            duplicate_keys.join(", ")
        )));
    }
    let mut unknown_skips = skip_set
        .iter()
        .copied()
        .filter(|key| !known_keys.contains(key))
        .collect::<Vec<_>>();
    unknown_skips.sort_unstable();
    if !unknown_skips.is_empty() {
        return Err(BuildServiceError::message(format!(
            "unknown skip_output_keys: {}",
            unknown_skips.join(", ")
        )));
    }
    let mut unknown_overrides = override_map
        .keys()
        .copied()
        .filter(|key| !known_keys.contains(key))
        .collect::<Vec<_>>();
    unknown_overrides.sort_unstable();
    if !unknown_overrides.is_empty() {
        return Err(BuildServiceError::message(format!(
            "unknown override output_keys: {}",
            unknown_overrides.join(", ")
        )));
    }
    Ok(())
}

fn prepare_build(plans: &[ResolvedItemPlan]) -> Result<Vec<ExecutablePlan>, BuildServiceError> {
    let mut executable = Vec::with_capacity(plans.len());
    for plan in plans {
        let effective = plan.clone();
        let action = match effective.action {
            PlanAction::Skip => ExecutableAction::Skip {
                summary: format!("skipped: {}", effective.reason),
            },
            PlanAction::NeedsReview => {
                return Err(BuildServiceError::message(format!(
                    "build blocked by unresolved item {} ('{}'): {}",
                    effective.output_key, effective.pco_title, effective.reason
                )));
            }
            PlanAction::UseExisting => {
                if has_render_only_style(&effective.style) {
                    return Err(BuildServiceError::message(format!(
                        "existing item '{}' cannot apply background, display, macro, or line-limit overrides",
                        effective.output_key
                    )));
                }
                let path = effective.file_path.as_deref().ok_or_else(|| {
                    BuildServiceError::message(format!(
                        "existing item '{}' has no presentation path",
                        effective.output_key
                    ))
                })?;
                let file_path = PathBuf::from(path);
                if !file_path.is_file() {
                    return Err(BuildServiceError::message(format!(
                        "existing presentation for '{}' is not a file: {path}",
                        effective.output_key
                    )));
                }
                ExecutableAction::UseExisting { file_path }
            }
            PlanAction::EditInPlace => {
                reject_rendered_arrangement(&effective)?;
                let path = effective.file_path.as_deref().ok_or_else(|| {
                    BuildServiceError::message(format!(
                        "edit-in-place item '{}' has no target file",
                        effective.output_key
                    ))
                })?;
                let file_path = PathBuf::from(path);
                if !file_path.is_file() {
                    return Err(BuildServiceError::message(format!(
                        "edit-in-place target for '{}' is not a file: {path}",
                        effective.output_key
                    )));
                }
                validate_generated_content(&effective)?;
                ExecutableAction::EditInPlace { file_path }
            }
            PlanAction::GenerateNew => {
                reject_rendered_arrangement(&effective)?;
                if effective.file_path.is_some() {
                    return Err(BuildServiceError::message(format!(
                        "generate-new item '{}' unexpectedly has an existing target path",
                        effective.output_key
                    )));
                }
                validate_generated_content(&effective)?;
                ExecutableAction::GenerateNew
            }
        };
        executable.push(ExecutablePlan {
            plan: effective,
            action,
            reviewed_background: None,
        });
    }

    Ok(executable)
}

const fn has_render_only_style(style: &super::plan::PresentationStyle) -> bool {
    style.background.is_some()
        || style.content_slide.is_some()
        || style.title_slide.is_some()
        || style.first_cue_macro.is_some()
        || style.first_content_cue_macro.is_some()
        || style.max_lines_per_slide.is_some()
}

fn reject_rendered_arrangement(entry: &ResolvedItemPlan) -> Result<(), BuildServiceError> {
    if let Some(arrangement) = &entry.style.arrangement {
        return Err(BuildServiceError::message(format!(
            "rendered item '{}' cannot select arrangement '{arrangement}' because rendering rebuilds the presentation",
            entry.output_key
        )));
    }
    Ok(())
}

fn validate_generated_content(entry: &ResolvedItemPlan) -> Result<(), BuildServiceError> {
    match &entry.content_source {
        ContentSource::Description { .. } if entry.parsed_content().is_some() => Ok(()),
        ContentSource::Scripture { .. } => Ok(()),
        ContentSource::None if is_title_only_generation(entry) => Ok(()),
        ContentSource::Description { .. } => Err(BuildServiceError::message(format!(
            "description item '{}' has no parsed content",
            entry.output_key
        ))),
        ContentSource::None => Err(BuildServiceError::message(format!(
            "generated item '{}' has no content source",
            entry.output_key
        ))),
    }
}

fn apply_override(
    entry: &ResolvedItemPlan,
    override_entry: Option<&EntryOverride>,
) -> ResolvedItemPlan {
    let mut effective = entry.clone();
    if let Some(override_entry) = override_entry {
        if let Some(ref playlist_name) = override_entry.playlist_name {
            effective.playlist_name.clone_from(playlist_name);
        }
        if let Some(ref file_path) = override_entry.file_path {
            effective.file_path = Some(file_path.clone());
            effective.action = PlanAction::UseExisting;
            effective.reason = "Build override file".to_string();
            retain_read_only_style(&mut effective.style);
        }
        if let Some(slide_type) = override_entry.slide_type {
            effective.item_kind = item_kind_from_override(slide_type);
            effective.item_type = item_type_from_override(slide_type);
        }
        if let Some(action) = override_entry.action {
            effective.action = action;
            effective.reason = "Build override action".to_string();
            if matches!(action, PlanAction::GenerateNew) {
                effective.file_path = None;
            } else if matches!(action, PlanAction::UseExisting) {
                retain_read_only_style(&mut effective.style);
            }
        }
        if let Some(background) = &override_entry.background {
            effective.style.background = Some(background.clone());
        }
        if let Some(ref arrangement) = override_entry.arrangement {
            effective.style.arrangement = Some(arrangement.clone());
        }
    }
    effective
}

fn retain_read_only_style(style: &mut super::plan::PresentationStyle) {
    let arrangement = style.arrangement.take();
    *style = super::plan::PresentationStyle {
        arrangement,
        ..super::plan::PresentationStyle::default()
    };
}

const fn item_kind_from_override(slide_type: OverrideSlideType) -> ItemKind {
    match slide_type {
        OverrideSlideType::Lyrics => ItemKind::Song,
        OverrideSlideType::Scripture => ItemKind::Scripture,
        OverrideSlideType::Title | OverrideSlideType::Nametag => ItemKind::Nametag,
        OverrideSlideType::Graphic => ItemKind::Graphic,
        OverrideSlideType::Text => ItemKind::Other,
    }
}

fn item_type_from_override(slide_type: OverrideSlideType) -> Option<String> {
    match slide_type {
        OverrideSlideType::Lyrics => Some("song".to_string()),
        OverrideSlideType::Scripture => Some("scripture".to_string()),
        OverrideSlideType::Title | OverrideSlideType::Nametag => Some("title".to_string()),
        OverrideSlideType::Text | OverrideSlideType::Graphic => None,
    }
}

fn is_title_only_generation(entry: &ResolvedItemPlan) -> bool {
    matches!(entry.action, PlanAction::GenerateNew)
        && matches!(
            entry.item_type.as_deref(),
            Some("title" | "nametag" | "content_nametag")
        )
}

fn build_description_presentation_with_templates(
    name: &str,
    content_template: &rv_data::PresentationSlide,
    title_template: Option<&rv_data::PresentationSlide>,
    segments: &[StyledSegment],
    title_text: Option<&str>,
    max_lines_override: Option<usize>,
) -> Option<RenderedPresentation> {
    let (wrap_col, max_lines) =
        crate::propresenter::template::extract_slide_metrics(content_template)
            .map_or((45, DEFAULT_MAX_LINES_PER_SLIDE), |metrics| {
                (metrics.chars_per_line, metrics.max_lines)
            });
    let max_lines = max_lines_override.unwrap_or(max_lines);

    let slide_groups = pack_segments_for_slides(segments, wrap_col, max_lines);
    let title_segments = title_text
        .filter(|title| !title.is_empty())
        .map(|title| vec![StyledSegment::unstyled(title)]);

    assemble_presentation_with_title_template_and_roles(
        name,
        content_template,
        title_template,
        title_segments.as_deref(),
        &slide_groups,
    )
}

fn all_content_segments_colored(segments: &[StyledSegment]) -> bool {
    let mut content = segments.iter().filter(|segment| !segment.text.is_empty());
    content.next().is_some_and(|first| {
        first.color.is_some() && content.all(|segment| segment.color.is_some())
    })
}

fn parse_bible_version(name: &str) -> Result<BibleVersion, BuildServiceError> {
    BibleVersion::from_name(name)
        .ok_or_else(|| BuildServiceError::UnsupportedBibleVersion(name.to_string()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

    use super::*;
    use crate::project_config::{BackgroundAssetPath, BackgroundId};
    use crate::workflow::plan::{PresentationStyle, ResolvedItemPlan};
    use prost::Message;

    fn test_background(id: &str) -> ResolvedBackground {
        ResolvedBackground::new(
            BackgroundId::new(id).expect("valid test background id"),
            BackgroundAssetPath::new(format!("backgrounds/{id}.png"))
                .expect("valid test background path"),
        )
    }

    fn presentation_with_size(name: &str, width: f64, height: f64) -> rv_data::Presentation {
        rv_data::Presentation {
            name: name.to_string(),
            cues: vec![rv_data::Cue {
                actions: vec![rv_data::Action {
                    action_type_data: Some(rv_data::action::ActionTypeData::Slide(
                        rv_data::action::SlideType {
                            slide: Some(rv_data::action::slide_type::Slide::Presentation(
                                rv_data::PresentationSlide {
                                    base_slide: Some(rv_data::Slide {
                                        size: Some(rv_data::graphics::Size { width, height }),
                                        ..rv_data::Slide::default()
                                    }),
                                    ..rv_data::PresentationSlide::default()
                                },
                            )),
                        },
                    )),
                    ..rv_data::Action::default()
                }],
                ..rv_data::Cue::default()
            }],
            ..rv_data::Presentation::default()
        }
    }

    #[test]
    fn apply_override_updates_rendered_name_and_background() {
        let entry = ResolvedItemPlan {
            output_key: "3:main".to_string(),
            position: 3,
            pco_title: "Call to Worship".to_string(),
            playlist_name: "Call to Worship".to_string(),
            action: PlanAction::GenerateNew,
            style: PresentationStyle {
                background: Some(test_background("default")),
                ..PresentationStyle::default()
            },
            ..ResolvedItemPlan::default()
        };
        let override_entry = EntryOverride {
            output_key: "3:main".to_string(),
            action: None,
            playlist_name: Some("Weekly Call to Worship".to_string()),
            file_path: None,
            slide_type: None,
            background: Some(test_background("sermon")),
            arrangement: None,
        };

        let effective = apply_override(&entry, Some(&override_entry));

        assert_eq!(effective.playlist_name, "Weekly Call to Worship");
        assert_eq!(effective.style.background, Some(test_background("sermon")));
    }

    #[test]
    fn unresolved_plan_is_rejected_before_execution() {
        let plan = ResolvedItemPlan {
            output_key: "1:main".to_string(),
            pco_title: "Unknown item".to_string(),
            action: PlanAction::NeedsReview,
            reason: "ambiguous classification".to_string(),
            ..ResolvedItemPlan::default()
        };

        let error = prepare_build(&[plan]).expect_err("unresolved plans must block");

        assert!(error.to_string().contains("ambiguous classification"));
    }

    #[test]
    fn duplicate_plan_identities_fail_preflight() {
        let plans = vec![
            ResolvedItemPlan {
                output_key: "pco:item-1:main".to_string(),
                action: PlanAction::Skip,
                ..ResolvedItemPlan::default()
            },
            ResolvedItemPlan {
                output_key: "pco:item-1:main".to_string(),
                action: PlanAction::Skip,
                ..ResolvedItemPlan::default()
            },
        ];

        let error = resolve_requested_plans(&plans, &[], &[])
            .expect_err("duplicate PCO identities must not produce ambiguous decisions");

        assert_eq!(
            error.to_string(),
            "duplicate plan output_keys: pco:item-1:main"
        );
    }

    #[tokio::test]
    async fn changed_reviewed_source_is_rejected_before_output_staging() {
        let root = tempfile::tempdir().expect("temporary root");
        let source = root.path().join("source.pro");
        let output = root.path().join("output");
        std::fs::create_dir(&output).expect("create output directory");
        std::fs::write(&source, b"reviewed presentation").expect("write reviewed source");
        let pco_client = PlanningCenterClient::new(&crate::config::Config::default());
        let bible_service = Arc::new(Mutex::new(BibleService::new(root.path().join("bibles"))));
        let file_index = Arc::new(Mutex::new(None));
        let template_cache = ThemeCache::load(None).expect("empty theme cache");
        let macro_cache = MacroCache::empty();
        let playlist_metadata = PlaylistMetadata::offline_test();
        let executor = ServiceBuildExecutor::new(
            &pco_client,
            &bible_service,
            &file_index,
            &template_cache,
            &macro_cache,
            &playlist_metadata,
            Some(&output),
            Some(&output),
        );
        let unresolved = executor
            .build_service(
                &BuildRequest {
                    plan_id: "plan-with-no-network-client".to_string(),
                    playlist_name: Some("Unresolved".to_string()),
                    ..BuildRequest::default()
                },
                &ProjectConfig::default(),
            )
            .await
            .expect_err("missing service identity must fail before Planning Center access");
        assert!(matches!(
            unresolved,
            BuildServiceError::UnresolvedIdentity {
                field: "service_name"
            }
        ));
        let reviewed = executor
            .review_build_request(
                BuildRequest {
                    plan_id: "plan-1".to_string(),
                    service_name: Some("Sunday Morning".to_string()),
                    playlist_name: Some("Reviewed".to_string()),
                    ..BuildRequest::default()
                },
                &[ResolvedItemPlan {
                    output_key: "pco:item-1:main".to_string(),
                    pco_title: "Existing".to_string(),
                    playlist_name: "Existing".to_string(),
                    file_path: Some(source.display().to_string()),
                    action: PlanAction::UseExisting,
                    ..ResolvedItemPlan::default()
                }],
                crate::propresenter::PresentationSize::FULL_HD,
            )
            .expect("capture reviewed request");
        std::fs::write(&source, b"changed presentation").expect("change source after review");

        let error = executor
            .build_reviewed_request(reviewed)
            .await
            .expect_err("changed bytes must invalidate the reviewed build");

        assert!(matches!(
            error,
            BuildServiceError::SourceReview(SourceReviewError::Changed { .. })
        ));
        assert_eq!(
            std::fs::read_dir(&output)
                .expect("read output directory")
                .count(),
            0
        );
    }

    #[tokio::test]
    async fn reviewed_build_uses_bound_identity_without_planning_center_refetch() {
        let root = tempfile::tempdir().expect("temporary root");
        let output = root.path().join("output");
        std::fs::create_dir(&output).expect("create output directory");
        let pco_client = PlanningCenterClient::new(&crate::config::Config::default());
        let bible_service = Arc::new(Mutex::new(BibleService::new(root.path().join("bibles"))));
        let file_index = Arc::new(Mutex::new(None));
        let template_cache = ThemeCache::load(None).expect("empty theme cache");
        let macro_cache = MacroCache::empty();
        let playlist_metadata = PlaylistMetadata::offline_test();
        let executor = ServiceBuildExecutor::new(
            &pco_client,
            &bible_service,
            &file_index,
            &template_cache,
            &macro_cache,
            &playlist_metadata,
            Some(&output),
            Some(&output),
        );
        let reviewed = executor
            .review_build_request(
                BuildRequest {
                    plan_id: "plan-with-no-network-client".to_string(),
                    service_name: Some("Sunday Morning".to_string()),
                    playlist_name: Some("May 24, 2026 - Sunday Morning".to_string()),
                    skip_output_keys: vec!["pco:item-1:main".to_string()],
                    ..BuildRequest::default()
                },
                &[ResolvedItemPlan {
                    output_key: "pco:item-1:main".to_string(),
                    pco_title: "Manual item".to_string(),
                    action: PlanAction::NeedsReview,
                    reason: "Needs a decision".to_string(),
                    ..ResolvedItemPlan::default()
                }],
                crate::propresenter::PresentationSize::FULL_HD,
            )
            .expect("review request");

        assert_eq!(reviewed.service_name(), "Sunday Morning");
        assert_eq!(reviewed.playlist_name(), "May 24, 2026 - Sunday Morning");
        assert_eq!(reviewed.plans()[0].action, PlanAction::Skip);
        let result = executor
            .build_reviewed_request(reviewed)
            .await
            .expect("bound request should build without Planning Center access");

        assert_eq!(result.skipped_count, 1);
        assert_eq!(
            result.playlist_path,
            output
                .join("May 24, 2026 - Sunday Morning.proplaylist")
                .display()
                .to_string()
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn reviewed_portable_media_uses_canonical_captured_source() {
        let root = tempfile::tempdir().expect("temporary root");
        let output = root.path().join("output");
        std::fs::create_dir(&output).expect("create output directory");
        let media = root.path().join("background.png");
        std::fs::write(&media, b"reviewed media").expect("write reviewed media");
        let alias = root.path().join("media-alias.png");
        std::os::unix::fs::symlink(&media, &alias).expect("create media symlink");
        let pco_client = PlanningCenterClient::new(&crate::config::Config::default());
        let bible_service = Arc::new(Mutex::new(BibleService::new(root.path().join("bibles"))));
        let file_index = Arc::new(Mutex::new(None));
        let template_cache = ThemeCache::load(None).expect("empty theme cache");
        let macro_cache = MacroCache::empty();
        let playlist_metadata = PlaylistMetadata::offline_test();
        let executor = ServiceBuildExecutor::new(
            &pco_client,
            &bible_service,
            &file_index,
            &template_cache,
            &macro_cache,
            &playlist_metadata,
            Some(&output),
            Some(&output),
        );
        let reviewed = executor
            .review_build_request(
                BuildRequest {
                    plan_id: "plan-1".to_string(),
                    service_name: Some("Sunday Morning".to_string()),
                    playlist_name: Some("Portable".to_string()),
                    playlist_package_mode: PlaylistPackageMode::ExportPortable,
                    media_assets: vec![PlaylistMediaAsset::new(&alias)],
                    ..BuildRequest::default()
                },
                &[],
                crate::propresenter::PresentationSize::FULL_HD,
            )
            .expect("review portable request");

        assert_eq!(
            reviewed.media_assets()[0].source_path,
            media.canonicalize().expect("canonical media path")
        );
        std::fs::write(&media, b"changed media").expect("change captured media");
        let error = executor
            .build_reviewed_request(reviewed)
            .await
            .expect_err("changed portable media must invalidate review");

        assert!(matches!(
            error,
            BuildServiceError::SourceReview(SourceReviewError::Changed { .. })
        ));
        assert_eq!(
            std::fs::read_dir(&output)
                .expect("read output directory")
                .count(),
            0
        );
    }

    #[tokio::test]
    async fn portable_review_auto_embeds_managed_background_bytes() {
        let root = tempfile::tempdir().expect("temporary root");
        let output = root.path().join("output");
        let backgrounds = root.path().join("backgrounds");
        std::fs::create_dir(&output).expect("create output directory");
        std::fs::create_dir(&backgrounds).expect("create background directory");
        let background = backgrounds.join("default.png");
        let background_bytes = [137, 80, 78, 71, 13, 10, 26, 10, 1, 2, 3, 4];
        std::fs::write(&background, background_bytes).expect("write background");

        let pco_client = PlanningCenterClient::new(&crate::config::Config::default());
        let bible_service = Arc::new(Mutex::new(BibleService::new(root.path().join("bibles"))));
        let file_index = Arc::new(Mutex::new(None));
        let template_cache = ThemeCache::load(None).expect("empty theme cache");
        let macro_cache = MacroCache::empty();
        let playlist_metadata = PlaylistMetadata::offline_test();
        let mut executor = ServiceBuildExecutor::new(
            &pco_client,
            &bible_service,
            &file_index,
            &template_cache,
            &macro_cache,
            &playlist_metadata,
            Some(&output),
            Some(&output),
        );
        executor.project_data_root = root.path().to_path_buf();
        let reviewed = executor
            .review_build_request(
                BuildRequest {
                    plan_id: "plan-1".to_string(),
                    service_name: Some("Sunday Morning".to_string()),
                    playlist_name: Some("Portable Background".to_string()),
                    playlist_package_mode: PlaylistPackageMode::ExportPortable,
                    ..BuildRequest::default()
                },
                &[ResolvedItemPlan {
                    output_key: "pco:item-1:main".to_string(),
                    pco_title: "Skipped fixture".to_string(),
                    action: PlanAction::Skip,
                    style: PresentationStyle {
                        background: Some(ResolvedBackground::new(
                            BackgroundId::new("default").expect("valid background id"),
                            BackgroundAssetPath::new("backgrounds/default.png")
                                .expect("valid background path"),
                        )),
                        ..PresentationStyle::default()
                    },
                    ..ResolvedItemPlan::default()
                }],
                crate::propresenter::PresentationSize::FULL_HD,
            )
            .expect("capture portable background review");
        assert_eq!(reviewed.media_assets().len(), 1);

        let result = executor
            .build_reviewed_request(reviewed)
            .await
            .expect("build portable package");
        let package = crate::propresenter::package::read_playlist_package(&result.playlist_path)
            .expect("read built package");
        let canonical = background
            .canonicalize()
            .expect("canonical background")
            .display()
            .to_string();

        assert_eq!(result.media_asset_count, 1);
        assert_eq!(
            package
                .embedded_file_data
                .get(&canonical)
                .expect("portable background member"),
            &background_bytes
        );
    }

    #[test]
    fn same_output_key_cannot_be_skipped_and_overridden() {
        let request = BuildRequest {
            skip_output_keys: vec!["1:main".to_string()],
            overrides: vec![EntryOverride {
                output_key: "1:main".to_string(),
                ..EntryOverride::default()
            }],
            ..BuildRequest::default()
        };

        let error = validate_unique_request_keys(&request.skip_output_keys, &request.overrides)
            .expect_err("skip and override must not compete for precedence");

        assert_eq!(
            error.to_string(),
            "output_key '1:main' cannot be both skipped and overridden"
        );
    }

    #[test]
    fn slide_type_override_does_not_invent_display_assets() {
        let entry = ResolvedItemPlan::default();
        let override_entry = EntryOverride {
            slide_type: Some(OverrideSlideType::Scripture),
            ..EntryOverride::default()
        };

        let effective = apply_override(&entry, Some(&override_entry));

        assert_eq!(effective.item_kind, ItemKind::Scripture);
        assert_eq!(effective.item_type.as_deref(), Some("scripture"));
        assert_eq!(effective.style.background, None);
        assert_eq!(effective.style.content_slide, None);
        assert_eq!(effective.style.first_cue_macro, None);
    }

    #[test]
    fn slide_type_override_preserves_title_and_graphic_semantics() {
        assert_eq!(
            item_kind_from_override(OverrideSlideType::Title),
            ItemKind::Nametag
        );
        assert_eq!(
            item_kind_from_override(OverrideSlideType::Graphic),
            ItemKind::Graphic
        );
    }

    #[test]
    fn unsupported_bible_version_is_a_typed_error() {
        assert!(matches!(
            parse_bible_version("ESV"),
            Err(BuildServiceError::UnsupportedBibleVersion(version)) if version == "ESV"
        ));
    }

    #[test]
    fn rendered_plan_cannot_select_an_arrangement() {
        let plan = ResolvedItemPlan {
            output_key: "1:main".to_string(),
            action: PlanAction::GenerateNew,
            content_source: ContentSource::Description {
                parsed_content: Some(crate::workflow::description_parser::ParsedContent {
                    segments: vec![crate::workflow::description_parser::ParsedSegment {
                        text: "content".to_string(),
                        color: None,
                        bold: None,
                        italic: None,
                    }],
                    title_text: None,
                }),
            },
            style: PresentationStyle {
                arrangement: Some("Default".to_string()),
                ..PresentationStyle::default()
            },
            ..ResolvedItemPlan::default()
        };

        let error =
            prepare_build(&[plan]).expect_err("rendered arrangement must fail during preflight");

        assert!(error
            .to_string()
            .contains("cannot select arrangement 'Default'"));
    }

    #[test]
    fn read_only_plan_rejects_background_override() {
        let directory = tempfile::tempdir().expect("tempdir");
        let source = directory.path().join("existing.pro");
        std::fs::write(&source, b"presentation").expect("existing fixture");
        let plan = ResolvedItemPlan {
            output_key: "1:main".to_string(),
            action: PlanAction::UseExisting,
            file_path: Some(source.display().to_string()),
            style: PresentationStyle {
                background: Some(test_background("default")),
                ..PresentationStyle::default()
            },
            ..ResolvedItemPlan::default()
        };

        let error =
            prepare_build(&[plan]).expect_err("read-only background must fail during preflight");

        assert!(error.to_string().contains("cannot apply background"));
    }

    #[test]
    fn use_existing_preparation_uses_approved_bytes_without_rereading_source() {
        let directory = tempfile::tempdir().expect("tempdir");
        let source = directory.path().join("existing.pro");
        let original = presentation_with_size("Approved", 1920.0, 1080.0).encode_to_vec();
        std::fs::write(&source, b"changed on disk").expect("write changed source");
        let plan = ResolvedItemPlan {
            output_key: "1:main".to_string(),
            action: PlanAction::UseExisting,
            ..ResolvedItemPlan::default()
        };

        let prepared = ServiceBuildExecutor::prepare_existing_presentation(
            &plan,
            &source,
            &original,
            crate::propresenter::PresentationSize::FULL_HD,
        )
        .expect("existing file should prepare");

        assert_eq!(prepared.embedded_data, original);
        assert_eq!(
            std::fs::read(source).expect("read source"),
            b"changed on disk"
        );
    }

    #[test]
    fn use_existing_preparation_rechecks_approved_presentation_size() {
        let directory = tempfile::tempdir().expect("tempdir");
        let source = directory.path().join("legacy.pro");
        let source_bytes = presentation_with_size("Legacy", 1280.0, 720.0).encode_to_vec();
        let plan = ResolvedItemPlan {
            output_key: "pco:legacy:main".to_string(),
            action: PlanAction::UseExisting,
            ..ResolvedItemPlan::default()
        };

        let error = ServiceBuildExecutor::prepare_existing_presentation(
            &plan,
            &source,
            &source_bytes,
            crate::propresenter::PresentationSize::FULL_HD,
        )
        .expect_err("legacy dimensions must not pass execution preflight");

        assert!(matches!(
            error,
            BuildServiceError::PresentationSizeInvariant {
                output_key,
                expected,
                actual,
            } if output_key == "pco:legacy:main"
                && expected.width() == 1920
                && actual == "1280x720"
        ));
    }

    #[test]
    fn existing_arrangement_carries_native_uuid_and_exact_name() {
        let directory = tempfile::tempdir().expect("tempdir");
        let source = directory.path().join("existing.pro");
        let arrangement_uuid = Uuid::new_v4();
        let mut presentation = presentation_with_size("Existing Presentation", 1920.0, 1080.0);
        presentation.arrangements = vec![rv_data::presentation::Arrangement {
            uuid: Some(rv_data::Uuid {
                string: arrangement_uuid.to_string(),
            }),
            name: "Default".to_string(),
            group_identifiers: Vec::new(),
        }];
        let source_bytes = prost::Message::encode_to_vec(&presentation);
        let plan = ResolvedItemPlan {
            style: PresentationStyle {
                arrangement: Some("default".to_string()),
                ..PresentationStyle::default()
            },
            ..ResolvedItemPlan::default()
        };

        let prepared = ServiceBuildExecutor::prepare_existing_presentation(
            &plan,
            &source,
            &source_bytes,
            crate::propresenter::PresentationSize::FULL_HD,
        )
        .expect("native arrangement should resolve case-insensitively");
        let selected = prepared.selected_arrangement.expect("selected arrangement");

        assert_eq!(selected.uuid(), &arrangement_uuid);
        assert_eq!(selected.name(), "Default");
    }

    #[test]
    fn missing_existing_arrangement_is_a_typed_error() {
        let directory = tempfile::tempdir().expect("tempdir");
        let source = directory.path().join("existing.pro");
        let presentation = presentation_with_size("Existing Presentation", 1920.0, 1080.0);
        let source_bytes = prost::Message::encode_to_vec(&presentation);
        std::fs::write(&source, &source_bytes).expect("write presentation");
        let plan = ResolvedItemPlan {
            style: PresentationStyle {
                arrangement: Some("Missing".to_string()),
                ..PresentationStyle::default()
            },
            ..ResolvedItemPlan::default()
        };

        let error = ServiceBuildExecutor::prepare_existing_presentation(
            &plan,
            &source,
            &source_bytes,
            crate::propresenter::PresentationSize::FULL_HD,
        )
        .expect_err("missing existing arrangement must fail");

        assert!(matches!(
            error,
            BuildServiceError::ArrangementUnavailable {
                presentation,
                arrangement,
            } if presentation == "Existing Presentation" && arrangement == "Missing"
        ));
    }

    #[test]
    fn malformed_existing_target_blocks_uuid_preservation() {
        let directory = tempfile::tempdir().expect("tempdir");
        let target = directory.path().join("existing.pro");
        let original = b"malformed presentation";
        std::fs::write(&target, original).expect("write target");
        let mut presentation = rv_data::Presentation::default();

        let application_info = rv_data::ApplicationInfo::default();
        let result = ServiceBuildExecutor::finalize_generated_document(
            &mut presentation,
            &target,
            Some(original),
            &application_info,
        );

        assert!(result.is_err());
        assert_eq!(std::fs::read(target).expect("read target"), original);
    }

    #[test]
    fn regenerated_target_preserves_owned_metadata_and_stamps_current_producer() {
        let directory = tempfile::tempdir().expect("tempdir");
        let target = directory.path().join("existing.pro");
        let existing = rv_data::Presentation {
            uuid: Some(rv_data::Uuid {
                string: "existing-id".to_string(),
            }),
            category: "Liturgy".to_string(),
            notes: "Operator note".to_string(),
            application_info: Some(rv_data::ApplicationInfo {
                platform: rv_data::application_info::Platform::Macos as i32,
                ..rv_data::ApplicationInfo::default()
            }),
            ..rv_data::Presentation::default()
        };
        let existing_bytes = prost::Message::encode_to_vec(&existing);
        std::fs::write(&target, &existing_bytes).expect("write existing target");
        let mut regenerated = rv_data::Presentation {
            name: "Regenerated".to_string(),
            ..rv_data::Presentation::default()
        };

        let current_application_info = rv_data::ApplicationInfo {
            application: rv_data::application_info::Application::Propresenter as i32,
            application_version: Some(rv_data::Version {
                major_version: 21,
                minor_version: 3,
                ..rv_data::Version::default()
            }),
            ..rv_data::ApplicationInfo::default()
        };
        ServiceBuildExecutor::finalize_generated_document(
            &mut regenerated,
            &target,
            Some(&existing_bytes),
            &current_application_info,
        )
        .expect("preserve existing envelope");

        assert_eq!(regenerated.uuid, existing.uuid);
        assert_eq!(regenerated.category, "Liturgy");
        assert_eq!(regenerated.notes, "Operator note");
        assert_eq!(
            regenerated.application_info.as_ref(),
            Some(&current_application_info)
        );
    }

    #[test]
    fn generated_presentations_use_default_folder_when_library_root_is_configured() {
        let dir = tempfile::tempdir().expect("tempdir");
        let default_dir = dir.path().join("Default");
        std::fs::create_dir(&default_dir).expect("create Default library folder");

        assert_eq!(
            presentation_output_dir(Some(dir.path())),
            default_dir,
            "library roots should write generated .pro files into Default"
        );
    }

    #[test]
    fn generated_presentations_keep_explicit_library_folder() {
        let dir = tempfile::tempdir().expect("tempdir");

        assert_eq!(
            presentation_output_dir(Some(dir.path())),
            dir.path(),
            "explicit library folders should be used as-is"
        );
    }

    #[test]
    fn styled_generated_cue_orders_slide_macro_then_background_media() {
        let root = tempfile::tempdir().expect("temporary project data root");
        let backgrounds = root.path().join("backgrounds");
        std::fs::create_dir(&backgrounds).expect("create background directory");
        let background_path = backgrounds.join("styled.png");
        std::fs::write(
            &background_path,
            [
                137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0, 0, 1,
                0, 0, 0, 1,
            ],
        )
        .expect("write valid PNG header");

        let macro_path = root.path().join("Macros");
        let macros = rv_data::MacrosDocument {
            application_info: None,
            macros: vec![rv_data::macros_document::Macro {
                uuid: Some(rv_data::Uuid {
                    string: "00000000-0000-0000-0000-000000000001".to_string(),
                }),
                name: "Styled Content".to_string(),
                color: None,
                actions: Vec::new(),
                trigger_on_startup: false,
                image_type: 0,
                image_data: Vec::new(),
            }],
            macro_collections: Vec::new(),
        };
        std::fs::write(&macro_path, prost::Message::encode_to_vec(&macros))
            .expect("write macro document");
        let macro_cache = MacroCache::load_from(&macro_path).expect("load macro cache");

        let pco_client = PlanningCenterClient::new(&crate::config::Config::default());
        let bible_service = Arc::new(Mutex::new(BibleService::new(root.path().join("bibles"))));
        let file_index = Arc::new(Mutex::new(None));
        let template_cache = ThemeCache::load(None).expect("empty theme cache");
        let playlist_metadata = PlaylistMetadata::offline_test();
        let mut executor = ServiceBuildExecutor::new(
            &pco_client,
            &bible_service,
            &file_index,
            &template_cache,
            &macro_cache,
            &playlist_metadata,
            None,
            None,
        );
        executor.project_data_root = root.path().to_path_buf();

        let mut rendered = build_description_presentation_with_templates(
            "Styled",
            &rv_data::PresentationSlide::default(),
            None,
            &[StyledSegment::unstyled("Generated content")],
            None,
            None,
        )
        .expect("render generated presentation");
        let style = PresentationStyle {
            background: Some(test_background("styled")),
            content_slide: Some("Content".to_string()),
            first_cue_macro: Some(crate::workflow::plan::CueMacro::new(
                "Styled Content".to_string(),
                None,
            )),
            ..PresentationStyle::default()
        };

        let background_bytes = std::fs::read(&background_path).expect("read reviewed background");
        executor
            .apply_style(
                &mut rendered,
                &style,
                false,
                Some(ReviewedBackgroundAsset {
                    path: &background_path,
                    data: &background_bytes,
                }),
            )
            .expect("apply generated presentation style");

        let cue = rendered
            .presentation
            .cues
            .first()
            .expect("generated content cue");
        assert_eq!(
            cue.actions
                .iter()
                .map(|action| action.r#type)
                .collect::<Vec<_>>(),
            vec![
                rv_data::action::ActionType::PresentationSlide as i32,
                rv_data::action::ActionType::Macro as i32,
                rv_data::action::ActionType::Media as i32,
            ]
        );
        assert!(matches!(
            cue.actions[0].action_type_data,
            Some(rv_data::action::ActionTypeData::Slide(_))
        ));
        assert_eq!(
            crate::propresenter::macros::macro_action_name(&cue.actions[1]),
            Some("Styled Content")
        );
        assert!(matches!(
            &cue.actions[2].action_type_data,
            Some(rv_data::action::ActionTypeData::Media(media))
                if media.layer_type == rv_data::action::LayerType::Background as i32
        ));
    }

    #[tokio::test]
    async fn reviewed_build_rejects_generated_target_that_appears_after_preview() {
        let root = tempfile::tempdir().expect("temporary root");
        let output = root.path().join("output");
        std::fs::create_dir(&output).expect("create output directory");
        let pco_client = PlanningCenterClient::new(&crate::config::Config::default());
        let bible_service = Arc::new(Mutex::new(BibleService::new(root.path().join("bibles"))));
        let file_index = Arc::new(Mutex::new(None));
        let template_cache = ThemeCache::load(None).expect("empty theme cache");
        let macro_cache = MacroCache::empty();
        let playlist_metadata = PlaylistMetadata::offline_test();
        let executor = ServiceBuildExecutor::new(
            &pco_client,
            &bible_service,
            &file_index,
            &template_cache,
            &macro_cache,
            &playlist_metadata,
            Some(&output),
            Some(&output),
        );
        let plan = ResolvedItemPlan {
            output_key: "pco:item-1:main".to_string(),
            pco_title: "Generated".to_string(),
            playlist_name: "Generated".to_string(),
            action: PlanAction::GenerateNew,
            item_type: Some("title".to_string()),
            ..ResolvedItemPlan::default()
        };
        let target = executor.presentation_target(&plan);
        let reviewed = executor
            .review_build_request(
                BuildRequest {
                    plan_id: "plan-1".to_string(),
                    service_name: Some("Sunday Morning".to_string()),
                    playlist_name: Some("Reviewed".to_string()),
                    ..BuildRequest::default()
                },
                &[plan],
                crate::propresenter::PresentationSize::FULL_HD,
            )
            .expect("review absent generated target");

        std::fs::write(&target, b"external presentation").expect("create target after preview");
        let error = executor
            .build_reviewed_request(reviewed)
            .await
            .expect_err("an output absent during preview must not be overwritten");

        assert!(matches!(
            error,
            BuildServiceError::OutputReview(OutputReviewError::Appeared { path, .. })
                if path == target
        ));
        assert_eq!(
            std::fs::read(&target).expect("read external presentation"),
            b"external presentation"
        );
    }

    #[tokio::test]
    async fn reviewed_build_rejects_playlist_target_changed_after_preview() {
        let root = tempfile::tempdir().expect("temporary root");
        let output = root.path().join("output");
        std::fs::create_dir(&output).expect("create output directory");
        let playlist_target = playlist_output_path(Some(&output), "Reviewed");
        std::fs::write(&playlist_target, b"reviewed playlist").expect("write reviewed playlist");
        let pco_client = PlanningCenterClient::new(&crate::config::Config::default());
        let bible_service = Arc::new(Mutex::new(BibleService::new(root.path().join("bibles"))));
        let file_index = Arc::new(Mutex::new(None));
        let template_cache = ThemeCache::load(None).expect("empty theme cache");
        let macro_cache = MacroCache::empty();
        let playlist_metadata = PlaylistMetadata::offline_test();
        let executor = ServiceBuildExecutor::new(
            &pco_client,
            &bible_service,
            &file_index,
            &template_cache,
            &macro_cache,
            &playlist_metadata,
            Some(&output),
            Some(&output),
        );
        let reviewed = executor
            .review_build_request(
                BuildRequest {
                    plan_id: "plan-1".to_string(),
                    service_name: Some("Sunday Morning".to_string()),
                    playlist_name: Some("Reviewed".to_string()),
                    ..BuildRequest::default()
                },
                &[],
                crate::propresenter::PresentationSize::FULL_HD,
            )
            .expect("review existing playlist target");

        std::fs::write(&playlist_target, b"changed playlist").expect("change reviewed playlist");
        let error = executor
            .build_reviewed_request(reviewed)
            .await
            .expect_err("a changed playlist target must not be overwritten");

        assert!(matches!(
            error,
            BuildServiceError::OutputReview(OutputReviewError::Changed { path, .. })
                if path == playlist_target
        ));
        assert_eq!(
            std::fs::read(&playlist_target).expect("read changed playlist"),
            b"changed playlist"
        );
    }

    #[cfg(unix)]
    #[test]
    fn reviewed_background_keeps_canonical_identity_after_symlink_retarget() {
        let root = tempfile::tempdir().expect("temporary project root");
        let backgrounds = root.path().join("backgrounds");
        std::fs::create_dir(&backgrounds).expect("create backgrounds");
        let first = backgrounds.join("first.png");
        let second = backgrounds.join("second.png");
        let first_bytes = [137, 80, 78, 71, 13, 10, 26, 10, 1];
        std::fs::write(&first, first_bytes).expect("write first background");
        std::fs::write(&second, [137, 80, 78, 71, 13, 10, 26, 10, 2])
            .expect("write second background");
        let selected = backgrounds.join("selected.png");
        std::os::unix::fs::symlink(&first, &selected).expect("link first background");

        let reviewed = ReviewedBuildRequest::capture(
            BoundBuildRequest::try_from(BuildRequest {
                plan_id: "plan".to_string(),
                service_name: Some("service".to_string()),
                playlist_name: Some("playlist".to_string()),
                ..BuildRequest::default()
            })
            .expect("bound request"),
            vec![ResolvedItemPlan {
                output_key: "pco:item:main".to_string(),
                style: PresentationStyle {
                    background: Some(ResolvedBackground::new(
                        BackgroundId::new("selected").expect("background id"),
                        BackgroundAssetPath::new("backgrounds/selected.png")
                            .expect("background path"),
                    )),
                    ..PresentationStyle::default()
                },
                ..ResolvedItemPlan::default()
            }],
            crate::propresenter::PresentationSize::FULL_HD,
            root.path(),
            std::iter::empty(),
            std::iter::empty(),
        )
        .expect("capture reviewed request");

        std::fs::remove_file(&selected).expect("remove original link");
        std::os::unix::fs::symlink(&second, &selected).expect("retarget background link");

        let canonical_first = first.canonicalize().expect("canonical first background");
        assert_eq!(reviewed.backgrounds[0].path, canonical_first);
        assert_eq!(
            reviewed
                .reviewed
                .source_bytes(&canonical_first)
                .expect("reviewed background bytes"),
            first_bytes
        );
    }
}
