//! Immutable source, media, background, and output capture for one review.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::propresenter::media::{
    presentation_media_dependencies_from_bytes, MediaDependencyResolution,
};
use crate::propresenter::playlist::PlaylistExportMode;
use crate::propresenter::PresentationSize;
use crate::workflow::approval::{OutputManifest, PhysicalPath, ReviewedServicePlan};
use crate::workflow::plan::{ReadyAction, ResolvedItemPlan};
use crate::workflow::presentation_render::PresentationRenderError;

use super::{ReviewedRequest, ServiceBuildExecutor};
use crate::workflow::execute::request::canonical_media_source;
use crate::workflow::execute::BuildServiceError;

#[derive(Debug)]
pub(in crate::workflow::execute) struct ReviewedBuildInputs {
    pub(in crate::workflow::execute) request: ReviewedRequest,
    pub(in crate::workflow::execute) reviewed: ReviewedServicePlan,
    pub(in crate::workflow::execute) presentation_size: PresentationSize,
    pub(in crate::workflow::execute) backgrounds: Vec<ReviewedBackgroundPath>,
    pub(in crate::workflow::execute) outputs: OutputManifest,
}

#[derive(Debug)]
pub(in crate::workflow::execute) struct ReviewedBackgroundPath {
    pub(in crate::workflow::execute) output_key: String,
    pub(in crate::workflow::execute) path: PathBuf,
}

impl ReviewedBuildInputs {
    pub(in crate::workflow::execute) fn capture(
        request: ReviewedRequest,
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
        if matches!(
            request.bound().playlist_export.mode(),
            PlaylistExportMode::PortableImport
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

    pub(super) fn plans(&self) -> &[ResolvedItemPlan] {
        self.reviewed.plans()
    }
}

pub(super) fn validate_reviewed_background_bindings(
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

pub(super) fn reviewed_theme_media_paths(
    executor: &ServiceBuildExecutor<'_>,
    plans: &[ResolvedItemPlan],
) -> Result<Vec<PathBuf>, BuildServiceError> {
    let propresenter_root = executor.render_assets.locations().propresenter_root();
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
            .map_err(PresentationRenderError::from)?;
        for dependency in dependencies {
            let path = match dependency.resolve(Some(propresenter_root)) {
                MediaDependencyResolution::Available(path)
                | MediaDependencyResolution::Missing(path) => path,
                MediaDependencyResolution::Unresolved => {
                    return Err(BuildServiceError::ThemeMediaLocatorUnavailable {
                        slide: slide_name.to_string(),
                        locator: dependency.source().to_string(),
                    });
                }
            };
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
            BuildServiceError::MissingReviewedSource {
                path: source_path.to_path_buf(),
            }
        })?;
        let dependencies =
            presentation_media_dependencies_from_bytes(source_bytes).map_err(|error| {
                BuildServiceError::ReviewedMediaInspection {
                    path: source_path.to_path_buf(),
                    source: error,
                }
            })?;
        for dependency in dependencies {
            match dependency.resolve(Some(propresenter_root)) {
                MediaDependencyResolution::Available(path) => {
                    paths.insert(canonical_media_source(&path)?);
                }
                // Portable packages preserve unresolved external references.
                // Available workspace media is embedded; absent media is
                // reported after packaging for operator review.
                MediaDependencyResolution::Missing(path) if !path.exists() => {}
                // A restyle may also remove a stale non-file entry background.
                MediaDependencyResolution::Missing(_) if replaces_entry_background => {}
                MediaDependencyResolution::Missing(path) => {
                    canonical_media_source(&path)?;
                }
                MediaDependencyResolution::Unresolved => {}
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
#[allow(clippy::expect_used)]
mod tests {
    use image::{codecs::png::PngEncoder, ColorType, ImageEncoder};

    use super::*;
    use crate::propresenter::background::{resolve_background_image, BackgroundImageError};

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
}
