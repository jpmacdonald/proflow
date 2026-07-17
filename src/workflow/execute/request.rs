//! Build-request state transitions and identity resolution.

use std::path::{Path, PathBuf};

use crate::planning_center::types::{Plan, Service};
use crate::propresenter::package::PlaylistPackageMode;
use crate::propresenter::playlist::PlaylistMediaAsset;

use super::overrides::{validate_request_edits, EntryOverride};
use super::{BuildServiceError, ServiceBuildExecutor};

const PLAN_METADATA_LOOKAHEAD_DAYS: i64 = 60;

#[derive(Debug, PartialEq, Eq)]
struct ResolvedPlanIdentity {
    service_name: String,
    default_playlist_name: String,
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
    /// Whether the playlist references library files or embeds portable assets.
    pub playlist_package_mode: PlaylistPackageMode,
    /// Explicit media sources to embed in portable exports.
    pub media_assets: Vec<PlaylistMediaAsset>,
}

/// Fully resolved request identity accepted by the review phase.
#[derive(Debug)]
pub(super) struct BoundBuildRequest {
    pub(super) plan_id: String,
    pub(super) service_name: String,
    pub(super) playlist_name: String,
    pub(super) playlist_package_mode: PlaylistPackageMode,
    pub(super) media_assets: Vec<PlaylistMediaAsset>,
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

/// Resolve a presentation's media locator for a portable export.
///
/// Copied native libraries retain absolute paths from their source workstation.
/// When that path is unavailable, the active workspace's canonical Media/Assets
/// file with the exact same filename supplies the bytes embedded in the package.
pub(super) fn portable_media_source(
    path: &Path,
    propresenter_root: &Path,
) -> Result<PathBuf, BuildServiceError> {
    match canonical_media_source(path) {
        Ok(path) => return Ok(path),
        Err(BuildServiceError::MediaSource { .. }) => {}
        Err(error) => return Err(error),
    }
    let Some(file_name) = path.file_name() else {
        return canonical_media_source(path);
    };
    let workspace_asset = propresenter_root.join("Media/Assets").join(file_name);
    canonical_media_source(&workspace_asset).or_else(|_| canonical_media_source(path))
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
    pub(super) async fn resolve_request_identity(
        &self,
        request: &BuildRequest,
    ) -> Result<BuildRequest, BuildServiceError> {
        validate_request_edits(&request.skip_output_keys, &request.overrides)?;
        let mut resolved = request.clone();
        let plan_id = required_identity("plan_id", resolved.plan_id.clone())?;
        let (services, plans) = self
            .pco_client
            .get_upcoming_services(PLAN_METADATA_LOOKAHEAD_DAYS)
            .await
            .map_err(|error| {
                BuildServiceError::message(format!(
                    "could not resolve metadata for plan {plan_id}: {error}"
                ))
            })?;
        let identity = resolve_plan_identity(
            &services,
            &plans,
            &plan_id,
            resolved.service_name.as_deref(),
        )?;
        let playlist_name = if let Some(name) = resolved.playlist_name.clone() {
            required_identity("playlist_name", name)?
        } else {
            identity.default_playlist_name
        };
        resolved.plan_id = plan_id;
        resolved.service_name = Some(identity.service_name);
        resolved.playlist_name = Some(playlist_name);
        Ok(resolved)
    }
}

fn resolve_plan_identity(
    services: &[Service],
    plans: &[Plan],
    plan_id: &str,
    supplied_service_name: Option<&str>,
) -> Result<ResolvedPlanIdentity, BuildServiceError> {
    let plan = plans
        .iter()
        .find(|plan| plan.id == plan_id)
        .ok_or_else(|| BuildServiceError::PlanNotFound {
            plan_id: plan_id.to_string(),
            days_ahead: PLAN_METADATA_LOOKAHEAD_DAYS,
        })?;
    let service_name = services
        .iter()
        .find(|service| service.id == plan.service_id)
        .map_or_else(|| plan.service_name.clone(), |service| service.name.clone());
    if let Some(supplied) = supplied_service_name {
        required_identity("service_name", supplied.to_string())?;
        if supplied != service_name {
            return Err(BuildServiceError::ServiceNameMismatch {
                plan_id: plan_id.to_string(),
                supplied: supplied.to_string(),
                actual: service_name,
            });
        }
    }
    Ok(ResolvedPlanIdentity {
        default_playlist_name: format!(
            "{} - {}",
            plan.date.format("%B %-d, %Y"),
            playlist_service_label(&service_name)
        ),
        service_name,
    })
}

fn playlist_service_label(service_name: &str) -> String {
    let Some((time, description)) = service_name.split_once(char::is_whitespace) else {
        return service_name.to_string();
    };
    let Some((clock, period)) = time
        .strip_suffix("am")
        .map(|clock| (clock, "am"))
        .or_else(|| time.strip_suffix("pm").map(|clock| (clock, "pm")))
    else {
        return service_name.to_string();
    };
    let Some((hour, minute)) = clock.split_once(':') else {
        return service_name.to_string();
    };
    let Ok(hour) = hour.parse::<u8>() else {
        return service_name.to_string();
    };
    if !(1..=12).contains(&hour)
        || minute.len() != 2
        || !minute.bytes().all(|byte| byte.is_ascii_digit())
    {
        return service_name.to_string();
    }
    let compact_time = if minute == "00" {
        format!("{hour}{period}")
    } else {
        format!("{hour}{minute}{period}")
    };
    let description = description.trim();
    if description.is_empty() {
        return compact_time;
    }
    let mut characters = description.chars();
    let Some(first) = characters.next() else {
        return compact_time;
    };
    format!(
        "{compact_time} {}{}",
        first.to_uppercase(),
        characters.as_str()
    )
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;
    use tempfile::tempdir;

    fn plan() -> Plan {
        Plan {
            id: "plan-1".to_string(),
            service_id: "service-1".to_string(),
            service_name: "stale fallback".to_string(),
            date: Utc
                .with_ymd_and_hms(2026, 7, 19, 14, 0, 0)
                .single()
                .expect("valid date"),
            title: "July 19".to_string(),
            items: Vec::new(),
        }
    }

    #[test]
    fn plan_identity_comes_from_planning_center_catalog() {
        let services = [Service {
            id: "service-1".to_string(),
            name: "Sunday Morning".to_string(),
        }];
        let identity =
            resolve_plan_identity(&services, &[plan()], "plan-1", None).expect("catalog identity");

        assert_eq!(identity.service_name, "Sunday Morning");
        assert_eq!(
            identity.default_playlist_name,
            "July 19, 2026 - Sunday Morning"
        );
    }

    #[test]
    fn caller_service_name_is_only_an_exact_assertion() {
        let services = [Service {
            id: "service-1".to_string(),
            name: "Sunday Morning".to_string(),
        }];
        let error = resolve_plan_identity(&services, &[plan()], "plan-1", Some("Sunday Mornng"))
            .expect_err("typo cannot select service-group policy");

        assert!(matches!(
            error,
            BuildServiceError::ServiceNameMismatch { actual, .. }
                if actual == "Sunday Morning"
        ));
    }

    #[test]
    fn playlist_service_labels_match_the_operator_naming_convention() {
        assert_eq!(
            playlist_service_label("9:00am contemporary"),
            "9am Contemporary"
        );
        assert_eq!(
            playlist_service_label("10:30am traditional"),
            "1030am Traditional"
        );
        assert_eq!(playlist_service_label("Sunday Morning"), "Sunday Morning");
    }

    #[test]
    fn portable_media_rebases_a_stale_workstation_path_to_the_active_workspace() {
        let root = tempdir().expect("workspace root");
        let assets = root.path().join("Media/Assets");
        std::fs::create_dir_all(&assets).expect("media assets directory");
        let local = assets.join("Announcement.jpg");
        std::fs::write(&local, b"image").expect("local media");

        let resolved = portable_media_source(
            Path::new("/Users/another/ProPresenter/Media/Assets/Announcement.jpg"),
            root.path(),
        )
        .expect("exact active-workspace asset should resolve");

        assert_eq!(
            resolved,
            local.canonicalize().expect("canonical local media")
        );
    }
}
