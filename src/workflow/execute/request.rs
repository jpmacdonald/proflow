//! Build-request state transitions and identity resolution.

use std::path::{Path, PathBuf};

use crate::planning_center::PlanSnapshot;
use proflow_propresenter::playlist::PlaylistExportIntent;

use super::overrides::{validate_request_edits, EntryOverride};
use thiserror::Error;

use super::ServiceBuildExecutor;
use crate::workflow::plan::PlanSemanticsError;

/// A build request that is malformed on its own terms or inconsistent with
/// the plan it names. These failures never involve `ProPresenter` state and are
/// reported to callers as invalid parameters.
#[derive(Debug, Error)]
pub enum RequestError {
    /// A required request identity was absent at the checked transition.
    #[error("build request {field} must be a non-empty resolved value before review")]
    UnresolvedIdentity {
        /// Missing or empty request field.
        field: &'static str,
    },
    /// A request identity could not be preserved as an exact lookup key.
    #[error("{field} must be non-empty, unpadded, and contain no control characters")]
    InvalidIdentity {
        /// Invalid request, override, path, arrangement, or title field.
        field: &'static str,
    },
    /// Planning Center did not return the requested plan in the supported
    /// service-build lookahead window.
    #[error("plan '{plan_id}' was not found in the next {days_ahead} days")]
    PlanNotFound {
        /// Stable Planning Center plan identity.
        plan_id: String,
        /// Lookahead window searched for authoritative metadata.
        days_ahead: i64,
    },
    /// No plan is scheduled in the lookahead window.
    #[error("no upcoming plan was found in the next {days_ahead} days")]
    NoUpcomingPlan {
        /// Lookahead window searched.
        days_ahead: i64,
    },
    /// More than one plan is scheduled at the soonest time.
    #[error(
        "more than one plan is next: {}; name the service type or the plan id",
        candidates.join(", ")
    )]
    AmbiguousNextPlan {
        /// Each candidate as `id (service type)`.
        candidates: Vec<String>,
    },
    /// A caller-supplied service type disagreed with Planning Center metadata.
    #[error(
        "service_name mismatch for plan '{plan_id}': caller supplied '{supplied}', Planning Center reports '{actual}'"
    )]
    ServiceNameMismatch {
        /// Stable Planning Center plan identity.
        plan_id: String,
        /// Caller-supplied assertion.
        supplied: String,
        /// Authoritative Planning Center service type.
        actual: String,
    },
    /// A captured source was accidentally paired with a different request.
    #[error(
        "Planning Center snapshot identity mismatch: request named '{requested}', captured '{captured}'"
    )]
    PlanningCenterSnapshotIdentity {
        /// Plan identity supplied by the request.
        requested: String,
        /// Plan identity captured from Planning Center.
        captured: String,
    },
    /// An entry override did not change any semantic property.
    #[error("override for output_key '{output_key}' contains no decision")]
    EmptyOverride {
        /// Stable plan identity targeted by the empty override.
        output_key: String,
    },
    /// The same request key was supplied more than once for one edit kind.
    #[error("duplicate {kind} '{key}'")]
    DuplicateRequestEdit {
        /// Operator edit collection containing the duplicate.
        kind: &'static str,
        /// Repeated stable output key.
        key: String,
    },
    /// One output was requested in mutually exclusive edit collections.
    #[error("output_key '{output_key}' cannot be both skipped and overridden")]
    ConflictingRequestEdits {
        /// Stable output identity with contradictory decisions.
        output_key: String,
    },
    /// The classifier emitted duplicate stable output identities.
    #[error("duplicate plan output_keys: {}", keys.join(", "))]
    DuplicatePlanOutputKeys {
        /// Sorted duplicate identities.
        keys: Vec<String>,
    },
    /// A request attempted to skip outputs absent from the reviewed plan.
    #[error("unknown skip_output_keys: {}", keys.join(", "))]
    UnknownSkipOutputKeys {
        /// Sorted unknown identities.
        keys: Vec<String>,
    },
    /// A request attempted to override outputs absent from the reviewed plan.
    #[error("unknown override output_keys: {}", keys.join(", "))]
    UnknownOverrideOutputKeys {
        /// Sorted unknown identities.
        keys: Vec<String>,
    },
    /// An override action is incompatible with the planned semantic content.
    #[error("override for '{output_key}' cannot {intent}")]
    UnsupportedOverride {
        /// Stable plan identity being overridden.
        output_key: String,
        /// Specific incompatible transition.
        intent: &'static str,
    },
    /// An operator decision contradicted the content carried by its planned
    /// presentation action.
    #[error(transparent)]
    PlanSemantics(#[from] PlanSemanticsError),
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
    /// A reviewed presentation source could not be resolved to one stable file.
    #[error("failed to resolve reviewed presentation source '{}': {source}", path.display())]
    PresentationSource {
        /// Requested presentation path.
        path: PathBuf,
        /// Filesystem resolution failure.
        #[source]
        source: std::io::Error,
    },
    /// A reviewed presentation source resolved to a non-file path.
    #[error("reviewed presentation source is not a regular file: {}", path.display())]
    PresentationSourceNotFile {
        /// Canonical presentation path.
        path: PathBuf,
    },
    /// A reviewed presentation source did not use the native `.pro` suffix.
    #[error("reviewed presentation source must be a .pro file: {}", path.display())]
    PresentationSourceExtension {
        /// Canonical presentation path.
        path: PathBuf,
    },
}

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
    type Error = RequestError;

    fn try_from(request: BuildRequest) -> Result<Self, Self::Error> {
        validate_request_edits(&request.skip_output_keys, &request.overrides)?;
        let plan_id = required_identity("plan_id", request.plan_id)?;
        let service_name = required_identity(
            "service_name",
            request
                .service_name
                .ok_or(RequestError::UnresolvedIdentity {
                    field: "service_name",
                })?,
        )?;
        let playlist_name = required_identity(
            "playlist_name",
            request
                .playlist_name
                .ok_or(RequestError::UnresolvedIdentity {
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
                    .collect::<Result<Vec<_>, RequestError>>()?,
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
) -> Result<String, RequestError> {
    if value.is_empty() {
        return Err(RequestError::UnresolvedIdentity { field });
    }
    validate_identity(field, &value)?;
    Ok(value)
}

pub(super) fn validate_identity(field: &'static str, value: &str) -> Result<(), RequestError> {
    if value.is_empty() {
        return Err(RequestError::UnresolvedIdentity { field });
    }
    if value.trim() != value || value.chars().any(char::is_control) {
        return Err(RequestError::InvalidIdentity { field });
    }
    Ok(())
}

pub(super) fn validate_path_identity(field: &'static str, path: &Path) -> Result<(), RequestError> {
    if path.as_os_str().is_empty() {
        return Err(RequestError::InvalidIdentity { field });
    }
    let displayed = path.to_string_lossy();
    if displayed.trim() != displayed || displayed.chars().any(char::is_control) {
        return Err(RequestError::InvalidIdentity { field });
    }
    Ok(())
}

pub(super) fn canonical_media_source(path: &Path) -> Result<PathBuf, RequestError> {
    let canonical = path
        .canonicalize()
        .map_err(|source| RequestError::MediaSource {
            path: path.to_path_buf(),
            source,
        })?;
    if canonical.is_file() {
        Ok(canonical)
    } else {
        Err(RequestError::MediaSourceNotFile { path: canonical })
    }
}

pub(super) fn canonical_presentation_source(path: &Path) -> Result<PathBuf, RequestError> {
    let canonical = path
        .canonicalize()
        .map_err(|source| RequestError::PresentationSource {
            path: path.to_path_buf(),
            source,
        })?;
    if !canonical.is_file() {
        return Err(RequestError::PresentationSourceNotFile { path: canonical });
    }
    let is_native_suffix = canonical
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pro"));
    if !is_native_suffix {
        return Err(RequestError::PresentationSourceExtension { path: canonical });
    }
    Ok(canonical)
}

impl ServiceBuildExecutor<'_> {
    pub(super) fn bind_request_identity(
        request: BuildRequest,
        source: &PlanSnapshot,
    ) -> Result<BuildRequest, RequestError> {
        validate_request_edits(&request.skip_output_keys, &request.overrides)?;
        let mut resolved = request;
        let plan_id = required_identity("plan_id", resolved.plan_id)?;
        if plan_id != source.plan_id() {
            return Err(RequestError::PlanningCenterSnapshotIdentity {
                requested: plan_id,
                captured: source.plan_id().to_string(),
            });
        }
        if let Some(supplied) = resolved.service_name.as_deref() {
            validate_identity("service_name", supplied)?;
            if supplied != source.service_name() {
                return Err(RequestError::ServiceNameMismatch {
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
