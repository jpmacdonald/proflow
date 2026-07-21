//! Build-request state transitions and identity resolution.

use std::path::{Path, PathBuf};

use crate::planning_center::PlanSnapshot;
use crate::propresenter::playlist::PlaylistExportIntent;

use super::overrides::{validate_request_edits, EntryOverride};
use super::{BuildServiceError, ServiceBuildExecutor};

/// Input arguments for the shared service build workflow.
///
/// `service_name` and `playlist_name` may be unresolved at the transport
/// boundary. [`ServiceBuildExecutor`] resolves them before this value can enter
/// review; `BoundBuildRequest` is the private checked state used afterward.
#[derive(Debug, Clone, Default)]
pub struct BuildRequest {
    /// Stable Planning Center plan identity.
    pub plan_id: String,
    /// Planning Center service type name, when already resolved by the caller.
    pub service_name: Option<String>,
    /// Playlist display and file name, or `None` to derive it from the plan date.
    pub playlist_name: Option<String>,
    /// Exact plan output identities the operator chose to omit.
    pub skip_output_keys: Vec<String>,
    /// Reviewed per-entry decisions keyed by stable plan output identity.
    pub overrides: Vec<EntryOverride>,
    /// Complete linked-library or portable-import intent.
    pub playlist_export: PlaylistExportIntent,
}

/// Fully resolved request identity accepted by the review phase.
#[derive(Debug)]
pub(super) struct BoundBuildRequest {
    pub(super) plan_id: String,
    pub(super) service_name: String,
    pub(super) playlist_name: String,
    pub(super) playlist_export: PlaylistExportIntent,
}

impl TryFrom<BuildRequest> for BoundBuildRequest {
    type Error = BuildServiceError;

    fn try_from(request: BuildRequest) -> Result<Self, Self::Error> {
        validate_request_edits(&request.skip_output_keys, &request.overrides)?;
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
        let playlist_export = match request.playlist_export {
            PlaylistExportIntent::LibraryLinks => PlaylistExportIntent::LibraryLinks,
            PlaylistExportIntent::PortableImport {
                additional_media_assets,
            } => PlaylistExportIntent::portable_import(
                additional_media_assets
                    .into_iter()
                    .map(|mut asset| {
                        asset.source_path = canonical_media_source(&asset.source_path)?;
                        Ok(asset)
                    })
                    .collect::<Result<Vec<_>, BuildServiceError>>()?,
            ),
        };
        Ok(Self {
            plan_id,
            service_name,
            playlist_name,
            playlist_export,
        })
    }
}

pub(super) fn required_identity(
    field: &'static str,
    value: String,
) -> Result<String, BuildServiceError> {
    if value.is_empty() {
        return Err(BuildServiceError::UnresolvedIdentity { field });
    }
    validate_identity(field, &value)?;
    Ok(value)
}

pub(super) fn validate_identity(field: &'static str, value: &str) -> Result<(), BuildServiceError> {
    if value.is_empty() {
        return Err(BuildServiceError::UnresolvedIdentity { field });
    }
    if value.trim() != value || value.chars().any(char::is_control) {
        return Err(BuildServiceError::InvalidIdentity { field });
    }
    Ok(())
}

pub(super) fn validate_path_identity(
    field: &'static str,
    path: &Path,
) -> Result<(), BuildServiceError> {
    if path.as_os_str().is_empty() {
        return Err(BuildServiceError::InvalidIdentity { field });
    }
    let displayed = path.to_string_lossy();
    if displayed.trim() != displayed || displayed.chars().any(char::is_control) {
        return Err(BuildServiceError::InvalidIdentity { field });
    }
    Ok(())
}

pub(super) fn canonical_media_source(path: &Path) -> Result<PathBuf, BuildServiceError> {
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

pub(super) fn canonical_presentation_source(path: &Path) -> Result<PathBuf, BuildServiceError> {
    let canonical =
        path.canonicalize()
            .map_err(|source| BuildServiceError::PresentationSource {
                path: path.to_path_buf(),
                source,
            })?;
    if !canonical.is_file() {
        return Err(BuildServiceError::PresentationSourceNotFile { path: canonical });
    }
    let is_native_suffix = canonical
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pro"));
    if !is_native_suffix {
        return Err(BuildServiceError::PresentationSourceExtension { path: canonical });
    }
    Ok(canonical)
}

impl ServiceBuildExecutor<'_> {
    pub(super) fn bind_request_identity(
        request: BuildRequest,
        source: &PlanSnapshot,
    ) -> Result<BuildRequest, BuildServiceError> {
        validate_request_edits(&request.skip_output_keys, &request.overrides)?;
        let mut resolved = request;
        let plan_id = required_identity("plan_id", resolved.plan_id)?;
        if plan_id != source.plan_id() {
            return Err(BuildServiceError::PlanningCenterSnapshotIdentity {
                requested: plan_id,
                captured: source.plan_id().to_string(),
            });
        }
        if let Some(supplied) = resolved.service_name.as_deref() {
            validate_identity("service_name", supplied)?;
            if supplied != source.service_name() {
                return Err(BuildServiceError::ServiceNameMismatch {
                    plan_id: source.plan_id().to_string(),
                    supplied: supplied.to_string(),
                    actual: source.service_name().to_string(),
                });
            }
        }
        let playlist_name = if let Some(name) = resolved.playlist_name.take() {
            required_identity("playlist_name", name)?
        } else {
            source.default_playlist_name().to_string()
        };
        resolved.plan_id = source.plan_id().to_string();
        resolved.service_name = Some(source.service_name().to_string());
        resolved.playlist_name = Some(playlist_name);
        Ok(resolved)
    }
}
