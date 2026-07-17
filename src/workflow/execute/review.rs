//! Review capture and approval state transitions.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use prost::Message;

use crate::propresenter::media::presentation_media_dependencies_from_bytes;
use crate::propresenter::package::PlaylistPackageMode;
use crate::propresenter::playlist::{playlist_output_path, PlaylistMediaAsset};
use crate::propresenter::PresentationSize;

use super::overrides::resolve_requested_plans;
use super::request::{
    canonical_media_source, portable_media_source, BoundBuildRequest, BuildRequest,
};
use super::run::PreparedService;
use super::{BuildServiceError, ServiceBuildExecutor};
use crate::workflow::approval::{
    OutputManifest, PhysicalPath, ReviewedServicePlan, SourceManifest,
};
use crate::workflow::plan::{
    PlanDisposition, ReadyAction, ResolvedItemPlan, ReviewContext, ScriptureRequest,
};

mod path_ownership;

use path_ownership::{validate_reviewed_path_ownership, PlannedOutputTarget, ReviewedOutputOwner};

#[derive(Debug)]
pub(super) struct ReviewedBuildInputs {
    pub(super) request: BoundBuildRequest,
    pub(super) reviewed: ReviewedServicePlan,
    pub(super) presentation_size: PresentationSize,
    pub(super) backgrounds: Vec<ReviewedBackgroundPath>,
    pub(super) outputs: OutputManifest,
}

/// Result of resolving and reviewing one build request.
///
/// Only [`Self::Prepared`] carries a value accepted by the build boundary.
/// Unresolved decisions therefore cannot compile as executable input.
#[derive(Debug)]
pub enum BuildReview {
    /// At least one semantic item still requires an operator decision.
    NeedsReview(NeedsReviewBuildRequest),
    /// Every native presentation and the playlist package were materialized.
    Prepared(PreparedBuildRequest),
}

/// A non-executable review retained only for operator-facing diagnostics.
#[derive(Debug)]
pub struct NeedsReviewBuildRequest {
    request: BoundBuildRequest,
    plans: Vec<ResolvedItemPlan>,
}

/// Exact native artifacts prepared from one fully resolved review.
///
/// Fields are private so callers cannot manufacture an executable request from
/// plans that did not pass capture, rendering, packaging, and transaction seal.
/// Reviewed source and portable-media payloads have been consumed; only their
/// fingerprints remain for the final drift check.
#[derive(Debug)]
pub struct PreparedBuildRequest {
    request: BoundBuildRequest,
    plans: Vec<ResolvedItemPlan>,
    sources: SourceManifest,
    prepared: PreparedService,
}

impl ReviewedBuildInputs {
    pub(super) fn capture(
        request: BoundBuildRequest,
        plans: Vec<ResolvedItemPlan>,
        presentation_size: PresentationSize,
        project_data_root: &Path,
        propresenter_root: &Path,
        additional_sources: impl IntoIterator<Item = PathBuf>,
        output_targets: impl IntoIterator<Item = PhysicalPath>,
    ) -> Result<Self, BuildServiceError> {
        let backgrounds = resolve_reviewed_backgrounds(&plans, project_data_root)?;
        let mut additional_sources = additional_sources.into_iter().collect::<Vec<_>>();
        additional_sources.extend(backgrounds.iter().map(|background| background.path.clone()));
        let mut reviewed = ReviewedServicePlan::capture_with_additional_sources(
            plans,
            project_data_root,
            additional_sources,
        )?;
        validate_captured_backgrounds(&backgrounds, &reviewed)?;
        preflight_reviewed_presentations(&reviewed, presentation_size)?;
        if matches!(
            request.playlist_package_mode,
            PlaylistPackageMode::ExportPortable
        ) {
            let media_paths = reviewed_media_source_paths(&reviewed, propresenter_root)?;
            reviewed.extend_sources(media_paths)?;
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

    fn plans(&self) -> &[ResolvedItemPlan] {
        self.reviewed.plans()
    }
}

impl BuildReview {
    const fn request(&self) -> &BoundBuildRequest {
        match self {
            Self::NeedsReview(review) => &review.request,
            Self::Prepared(prepared) => &prepared.request,
        }
    }

    /// Return the exact effective plans rendered in the operator preview.
    pub fn plans(&self) -> &[ResolvedItemPlan] {
        match self {
            Self::NeedsReview(review) => &review.plans,
            Self::Prepared(prepared) => &prepared.plans,
        }
    }

    /// Stable Planning Center plan identity bound to this request.
    pub fn plan_id(&self) -> &str {
        &self.request().plan_id
    }

    /// Planning Center service type resolved before preview approval.
    pub fn service_name(&self) -> &str {
        &self.request().service_name
    }

    /// Final playlist display/file identity resolved before preview approval.
    pub fn playlist_name(&self) -> &str {
        &self.request().playlist_name
    }

    /// Playlist package mode included in the reviewed request.
    pub const fn playlist_package_mode(&self) -> PlaylistPackageMode {
        match self {
            Self::NeedsReview(review) => review.request.playlist_package_mode,
            Self::Prepared(prepared) => prepared.request.playlist_package_mode,
        }
    }

    /// Explicit portable-media sources included in the reviewed request.
    pub fn media_assets(&self) -> &[PlaylistMediaAsset] {
        &self.request().media_assets
    }

    /// Consume a resolved review, or return the first unresolved semantic item.
    pub fn into_prepared(self) -> Result<PreparedBuildRequest, BuildServiceError> {
        match self {
            Self::Prepared(prepared) => Ok(prepared),
            Self::NeedsReview(review) => {
                let plan = review
                    .plans
                    .iter()
                    .find(|plan| plan.needs_review())
                    .ok_or_else(|| {
                        BuildServiceError::message(
                            "non-executable review contains no unresolved plan",
                        )
                    })?;
                Err(super::unresolved_plan_error(plan))
            }
        }
    }
}

impl PreparedBuildRequest {
    pub(super) const fn from_materialized(
        request: BoundBuildRequest,
        plans: Vec<ResolvedItemPlan>,
        sources: SourceManifest,
        prepared: PreparedService,
    ) -> Self {
        Self {
            request,
            plans,
            sources,
            prepared,
        }
    }

    pub(super) fn into_commit_parts(self) -> (SourceManifest, PreparedService) {
        (self.sources, self.prepared)
    }

    /// Exact effective plans from which the artifacts were prepared.
    pub fn plans(&self) -> &[ResolvedItemPlan] {
        &self.plans
    }

    /// Stable Planning Center plan identity bound to these artifacts.
    pub fn plan_id(&self) -> &str {
        &self.request.plan_id
    }

    /// Planning Center service type bound before preparation.
    pub fn service_name(&self) -> &str {
        &self.request.service_name
    }

    /// Final playlist display/file identity bound before preparation.
    pub fn playlist_name(&self) -> &str {
        &self.request.playlist_name
    }

    /// Playlist package mode bound before preparation.
    pub const fn playlist_package_mode(&self) -> PlaylistPackageMode {
        self.request.playlist_package_mode
    }

    /// Reviewed portable-media sources embedded during preparation.
    pub fn media_assets(&self) -> &[PlaylistMediaAsset] {
        &self.request.media_assets
    }

    #[cfg(test)]
    pub(crate) fn prepared_artifact_bytes(
        &self,
        target: &Path,
    ) -> Result<Option<Vec<u8>>, std::io::Error> {
        self.prepared.artifact_bytes(target)
    }

    #[cfg(test)]
    pub(crate) fn has_reviewed_source(&self, source: &Path) -> bool {
        self.sources.contains(source)
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
        let inputs = ReviewedBuildInputs::capture(
            request,
            Vec::new(),
            PresentationSize::FULL_HD,
            Path::new("."),
            Path::new("."),
            std::iter::empty(),
            std::iter::empty(),
        )?;
        let ReviewedBuildInputs {
            request,
            reviewed,
            outputs,
            ..
        } = inputs;
        let transaction =
            crate::workflow::transaction::BuildFileTransaction::from_reviewed(outputs).seal()?;
        let (plans, sources) = reviewed.into_verified_parts()?;
        Ok(Self::from_materialized(
            request,
            plans,
            sources,
            PreparedService::offline_test(transaction),
        ))
    }
}

fn preflight_reviewed_presentations(
    reviewed: &ReviewedServicePlan,
    presentation_size: PresentationSize,
) -> Result<(), BuildServiceError> {
    for plan in reviewed.plans() {
        let (source_path, arrangement, resize) = match plan.ready_action() {
            Some(ReadyAction::UseExisting {
                file_path,
                arrangement,
            }) => (file_path.as_path(), arrangement.as_deref(), false),
            Some(ReadyAction::RestyleExisting {
                file_path,
                arrangement,
                ..
            }) => (file_path.as_path(), arrangement.as_deref(), true),
            _ => continue,
        };
        let source_bytes = reviewed.source_bytes(source_path).ok_or_else(|| {
            BuildServiceError::message(format!(
                "reviewed plan has no captured bytes for '{}'",
                source_path.display()
            ))
        })?;
        let normalized;
        let source_bytes = if resize {
            let mut presentation = crate::propresenter::deserialize::decode_presentation_bytes(
                source_bytes,
                &source_path.display().to_string(),
            )?;
            crate::propresenter::resolution::resize_presentation_canvas(
                &mut presentation,
                presentation_size,
            )?;
            normalized = presentation.encode_to_vec();
            normalized.as_slice()
        } else {
            source_bytes
        };
        ServiceBuildExecutor::prepare_existing_presentation(
            plan.output_key.as_str(),
            source_path,
            arrangement,
            source_bytes,
            presentation_size,
        )?;
    }
    Ok(())
}

#[derive(Debug)]
pub(super) struct ReviewedBackgroundPath {
    pub(super) output_key: String,
    pub(super) path: PathBuf,
}

impl ServiceBuildExecutor<'_> {
    /// Bind a fully resolved request to the effective plans, source bytes, and
    /// output target states shown during preview.
    pub(crate) async fn review_build_request(
        &self,
        mut request: BuildRequest,
        plans: &[ResolvedItemPlan],
        presentation_size: PresentationSize,
    ) -> Result<BuildReview, BuildServiceError> {
        let skip_output_keys = std::mem::take(&mut request.skip_output_keys);
        let overrides = std::mem::take(&mut request.overrides);
        let request = BoundBuildRequest::try_from(request)?;
        let mut plans = resolve_requested_plans(plans, &skip_output_keys, &overrides)?;
        {
            let mut bible = self.bible_service.lock().await;
            reconcile_description_scripture_excerpts(&mut plans, &mut bible);
        }
        if plans.iter().any(ResolvedItemPlan::needs_review) {
            return Ok(BuildReview::NeedsReview(NeedsReviewBuildRequest {
                request,
                plans,
            }));
        }
        let mut additional_sources = request
            .media_assets
            .iter()
            .map(|asset| asset.source_path.clone())
            .collect::<Vec<_>>();
        if matches!(
            request.playlist_package_mode,
            PlaylistPackageMode::ExportPortable
        ) {
            for path in reviewed_theme_media_paths(self, &plans)? {
                additional_sources.push(path);
            }
        }
        let mut output_targets = vec![PlannedOutputTarget::resolve(
            ReviewedOutputOwner::Playlist,
            &playlist_output_path(
                self.render_assets.locations().playlist_output(),
                &request.playlist_name,
            ),
        )?];
        let mut existing_generated_targets = Vec::new();
        for plan in &plans {
            match plan.ready_action() {
                Some(
                    ReadyAction::RestyleExisting { file_path, .. }
                    | ReadyAction::EditDescription { file_path, .. },
                ) => {
                    output_targets.push(PlannedOutputTarget::resolve(
                        ReviewedOutputOwner::Plan(plan.output_key.to_string()),
                        file_path,
                    )?);
                }
                Some(
                    ReadyAction::GenerateDescription { .. }
                    | ReadyAction::GenerateScripture { .. }
                    | ReadyAction::GenerateTitle { .. },
                ) => {
                    let target = self.presentation_target(plan)?;
                    if target.is_file() {
                        existing_generated_targets.push(target.clone());
                    }
                    output_targets.push(PlannedOutputTarget::resolve(
                        ReviewedOutputOwner::Plan(plan.output_key.to_string()),
                        &target,
                    )?);
                }
                Some(ReadyAction::UseExisting { .. }) | None => {}
            }
        }
        validate_reviewed_path_ownership(
            &plans,
            self.render_assets.locations().project_data_root(),
            &additional_sources,
            &output_targets,
        )?;
        additional_sources.extend(existing_generated_targets);
        let reviewed = ReviewedBuildInputs::capture(
            request,
            plans,
            presentation_size,
            self.render_assets.locations().project_data_root(),
            self.render_assets.locations().propresenter_root(),
            additional_sources,
            output_targets.into_iter().map(|target| target.physical),
        )?;
        validate_reviewed_background_bindings(&reviewed)?;
        let prepared = self.prepare_reviewed_service(reviewed).await?;
        Ok(BuildReview::Prepared(prepared))
    }
}

fn reconcile_description_scripture_excerpts(
    plans: &mut [ResolvedItemPlan],
    bible: &mut crate::bible::BibleService,
) {
    for plan in plans {
        if !plan.needs_review() {
            continue;
        }
        let Some((reference, display_reference, bible_version, excerpt_text)) =
            plan.preview_action().and_then(|action| match action {
                ReadyAction::GenerateScripture { scripture, .. } => match scripture.request() {
                    ScriptureRequest::PrefixExcerpt {
                        reference,
                        display_reference,
                        bible_version,
                        excerpt_text,
                    } => Some((
                        reference.to_string(),
                        display_reference.to_string(),
                        bible_version.to_string(),
                        excerpt_text.to_string(),
                    )),
                    ScriptureRequest::Single { .. } | ScriptureRequest::Combined(_) => None,
                },
                _ => None,
            })
        else {
            continue;
        };
        let result = validate_description_scripture_excerpt(
            bible,
            &reference,
            &bible_version,
            &excerpt_text,
        );
        if let Err(error) = result {
            plan.reason =
                format!("Partial scripture '{display_reference}' requires review: {error}");
            continue;
        }

        let previous = std::mem::replace(
            &mut plan.disposition,
            PlanDisposition::NeedsReview(ReviewContext::new(None)),
        );
        let PlanDisposition::NeedsReview(context) = previous else {
            plan.disposition = previous;
            continue;
        };
        let Some(action) = context.into_proposed_action() else {
            continue;
        };
        plan.disposition = PlanDisposition::Ready(action);
        plan.reason = format!(
            "Generate description-bounded scripture slides ({display_reference} {bible_version})"
        );
    }
}

fn validate_description_scripture_excerpt(
    bible: &mut crate::bible::BibleService,
    reference_text: &str,
    bible_version: &str,
    excerpt_text: &str,
) -> Result<(), String> {
    let reference = crate::bible::parse_scripture_ref(reference_text)
        .ok_or_else(|| format!("cannot parse whole-verse lookup '{reference_text}'"))?;
    let version = crate::bible::BibleVersion::from_name(bible_version)
        .ok_or_else(|| format!("unsupported Bible version '{bible_version}'"))?;
    let (header, verses) = bible
        .lookup_verses(&reference, version)
        .map_err(|error| error.to_string())?;
    if !header.missing_verses.is_empty() {
        return Err(format!(
            "local Bible data is missing verses {:?}",
            header.missing_verses
        ));
    }
    crate::bible::reconcile_prefix_excerpt(&verses, excerpt_text)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn validate_reviewed_background_bindings(
    reviewed: &ReviewedBuildInputs,
) -> Result<(), BuildServiceError> {
    for plan in reviewed.plans() {
        let reviewed_background = reviewed
            .backgrounds
            .iter()
            .find(|background| background.output_key == plan.output_key.as_str());
        let expects_background = plan
            .ready_action()
            .and_then(ReadyAction::background)
            .is_some();
        if expects_background != reviewed_background.is_some() {
            return Err(BuildServiceError::ReviewedBackgroundInvariant {
                output_key: plan.output_key.to_string(),
            });
        }
    }
    Ok(())
}

fn reviewed_theme_media_paths(
    executor: &ServiceBuildExecutor<'_>,
    plans: &[ResolvedItemPlan],
) -> Result<Vec<PathBuf>, BuildServiceError> {
    let mut paths = HashSet::new();
    let mut slide_names = HashSet::new();
    for style in plans.iter().filter_map(ResolvedItemPlan::render_style) {
        slide_names.insert(style.content().slide());
        if let Some(title) = style.title() {
            slide_names.insert(title.slide());
        }
    }
    for slide_name in slide_names {
        let dependencies = executor
            .render_assets
            .themes()
            .slide_media_dependencies(slide_name)
            .map_err(|error| BuildServiceError::PresentationRender(error.to_string()))?;
        for dependency in dependencies {
            let path = dependency.path.ok_or_else(|| {
                BuildServiceError::message(format!(
                    "theme slide '{slide_name}' media dependency is not an absolute local file: {}",
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
    propresenter_root: &Path,
) -> Result<Vec<PathBuf>, BuildServiceError> {
    let mut paths = HashSet::new();
    for plan in reviewed.plans() {
        let replaces_entry_background = matches!(
            plan.ready_action(),
            Some(ReadyAction::RestyleExisting { transform, .. })
                if transform.replacement_background().is_some()
        );
        if !matches!(
            plan.ready_action(),
            Some(
                ReadyAction::UseExisting { .. }
                    | ReadyAction::RestyleExisting { .. }
                    | ReadyAction::EditDescription { .. }
            )
        ) {
            continue;
        }
        let Some(source_path) = plan.file_path() else {
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
            match portable_media_source(&path, propresenter_root) {
                Ok(path) => {
                    paths.insert(path);
                }
                // Portable packages preserve unresolved external references.
                // Available workspace media is embedded; absent media is
                // reported after packaging for operator review.
                Err(BuildServiceError::MediaSource { .. }) => {}
                // A restyle may also remove a stale non-file entry background.
                Err(_) if replaces_entry_background => {}
                Err(error) => return Err(error),
            }
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
        let Some(background) = plan.background() else {
            continue;
        };
        let path = crate::propresenter::background::resolve_background_image(
            project_data_root,
            background.file().as_path(),
        )?;
        backgrounds.push(ReviewedBackgroundPath {
            output_key: plan.output_key.to_string(),
            path,
        });
    }
    Ok(backgrounds)
}

fn validate_captured_backgrounds(
    backgrounds: &[ReviewedBackgroundPath],
    reviewed: &ReviewedServicePlan,
) -> Result<(), BuildServiceError> {
    for background in backgrounds {
        let bytes = reviewed.source_bytes(&background.path).ok_or_else(|| {
            BuildServiceError::ReviewedBackgroundInvariant {
                output_key: background.output_key.clone(),
            }
        })?;
        crate::propresenter::background::validate_background_image_bytes(&background.path, bytes)?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use image::{codecs::png::PngEncoder, ColorType, ImageEncoder};

    use super::*;
    use crate::planning_center::types::{Category, Item};
    use crate::project_config::parse_project_config_str;
    use crate::propresenter::background::{resolve_background_image, BackgroundImageError};

    const EXODUS_DESCRIPTION: &str = "1 The whole congregation of the Israelites set out from Elim and came to the wilderness of Sin, which is between Elim and Sinai, on the fifteenth day of the second month after they had departed from the land of Egypt.\n\
         2 The whole congregation of the Israelites complained against Moses and Aaron in the wilderness.\n\
         3 The Israelites said to them, ‘If only we had died by the hand of the Lord in the land of Egypt, when we sat by the pots of meat and ate our fill of bread, for you have brought us out into this wilderness to kill this whole assembly with hunger.’\n\
         4 Then the Lord said to Moses, ‘I am going to rain bread from heaven for you, and each day the people shall go out and gather enough for that day.’";

    fn minimal_png(width: u32, height: u32) -> Vec<u8> {
        let pixel_count = usize::try_from(width).expect("width fits usize")
            * usize::try_from(height).expect("height fits usize");
        let pixels = vec![0u8; pixel_count * 4];
        let mut bytes = Vec::new();
        PngEncoder::new(&mut bytes)
            .write_image(&pixels, width, height, ColorType::Rgba8.into())
            .expect("encode PNG fixture");
        bytes
    }

    #[test]
    fn captured_background_bytes_are_revalidated_after_path_resolution() {
        let root = tempfile::tempdir().expect("project data root");
        let directory = root.path().join("backgrounds");
        std::fs::create_dir(&directory).expect("background directory");
        let image = directory.join("default.png");
        std::fs::write(&image, minimal_png(1920, 1080)).expect("valid initial background");
        let canonical = resolve_background_image(root.path(), Path::new("backgrounds/default.png"))
            .expect("resolved initial background");

        // This models a change in the narrow interval between path resolution
        // and CapturedSources capture. The captured bytes, not the earlier read,
        // are authoritative.
        std::fs::write(&image, [137, 80, 78, 71, 13, 10, 26, 10])
            .expect("replace with truncated background");
        let reviewed = ReviewedServicePlan::capture_with_additional_sources(
            Vec::new(),
            root.path(),
            [canonical.clone()],
        )
        .expect("capture exact replacement bytes");
        let backgrounds = [ReviewedBackgroundPath {
            output_key: "pco:item:main".to_string(),
            path: canonical.clone(),
        }];

        assert!(matches!(
            validate_captured_backgrounds(&backgrounds, &reviewed),
            Err(BuildServiceError::Background(BackgroundImageError::InvalidFormat(path)))
                if path == canonical
        ));
    }

    #[test]
    fn exodus_partial_description_is_proved_against_local_nrsvue_text() {
        let mut bible = crate::bible::BibleService::new(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("data/bibles"),
        );

        assert_eq!(
            validate_description_scripture_excerpt(
                &mut bible,
                "Exodus 16:1-4",
                "NRSVue",
                EXODUS_DESCRIPTION,
            ),
            Ok(())
        );
        assert!(validate_description_scripture_excerpt(
            &mut bible,
            "Exodus 16:1-4",
            "NRSVue",
            &EXODUS_DESCRIPTION.replace("gather enough", "gather too much"),
        )
        .is_err());
    }

    #[test]
    fn validated_partial_description_crosses_from_review_to_ready() {
        let config = parse_project_config_str(
            r#"{
              "version": 4,
              "defaults": { "bible_version": "NRSVue" },
              "cue_roles": { "scripture": { "slide": "Scripture" } },
              "presentation_types": {
                "scripture": {
                  "kind": "scripture",
                  "content_source": "scripture",
                  "output_strategy": "generate_new",
                  "display": { "kind": "single", "role": "scripture" }
                }
              },
              "item_rules": [{
                "id": "scripture",
                "match": { "title_prefix": ["scripture"] },
                "use_type": "scripture"
              }]
            }"#,
        )
        .expect("partial scripture test config");
        let item = Item {
            id: "partial-scripture".to_string(),
            position: 1,
            title: "Scripture - Exodus 16:1-4a (Robert)".to_string(),
            description: Some(EXODUS_DESCRIPTION.to_string()),
            category: Category::Title,
            note: None,
            song: None,
            scripture: None,
        };
        let mut plans = crate::workflow::classify::build_plan(&[item], &config, None, None);
        assert!(plans[0].needs_review());

        let mut bible = crate::bible::BibleService::new(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("data/bibles"),
        );
        reconcile_description_scripture_excerpts(&mut plans, &mut bible);

        assert!(matches!(
            plans[0].ready_action(),
            Some(ReadyAction::GenerateScripture { scripture, .. })
                if matches!(scripture.request(), ScriptureRequest::PrefixExcerpt { .. })
        ));
        assert_eq!(
            plans[0].reason,
            "Generate description-bounded scripture slides (Exodus 16:1-4a NRSVue)"
        );
    }
}
