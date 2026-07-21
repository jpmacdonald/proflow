//! Background image support for `ProPresenter` slides.
//!
//! Adds a `BackgroundMedia` action to a cue, replicating what happens when
//! you drag an image onto a slide in `ProPresenter`.

use std::{
    io::Cursor,
    path::{Path, PathBuf},
};

use super::generated::rv_data::{
    self, action, graphics, media, url, AlphaType, FileProperties, Media, Uuid,
};
use action::LayerType;
use image::ImageFormat;
use sha2::{Digest, Sha256};

/// Failure to resolve a configured background image inside the project bundle.
#[derive(Debug, thiserror::Error)]
pub enum BackgroundImageError {
    /// The selected/default operator traversal was unsafe for mutation.
    #[error(transparent)]
    OperatorTraversal(#[from] crate::propresenter::arrangement::OperatorTraversalError),
    /// The project data root could not be canonicalized.
    #[error("failed to resolve project data root {path}: {source}")]
    DataRoot {
        /// Configured project data root.
        path: PathBuf,
        /// Filesystem failure.
        source: std::io::Error,
    },
    /// The configured image path could not be canonicalized.
    #[error("failed to resolve background image {path}: {source}")]
    Image {
        /// Configured image path.
        path: PathBuf,
        /// Filesystem failure.
        source: std::io::Error,
    },
    /// A symlink escaped the selected project data root.
    #[error("background image {path} escapes project data root {root}")]
    OutsideDataRoot {
        /// Canonical image path.
        path: PathBuf,
        /// Canonical project data root.
        root: PathBuf,
    },
    /// The configured path did not identify a regular file.
    #[error("background image is not a regular file: {0}")]
    NotFile(PathBuf),
    /// The configured image was empty.
    #[error("background image is empty: {0}")]
    Empty(PathBuf),
    /// The configured file was unsupported, malformed, or did not match its extension.
    #[error("background image is not a supported image matching its extension: {0}")]
    InvalidFormat(PathBuf),
    /// The configured image declared an unusable natural size.
    #[error("background image has a zero width or height: {0}")]
    InvalidDimensions(PathBuf),
}

/// Failure to restyle the entry cues of a checked native arrangement set.
#[derive(Debug, thiserror::Error)]
pub enum ArrangementBackgroundError {
    /// The reviewed image bytes were not a usable background.
    #[error(transparent)]
    Image(#[from] BackgroundImageError),
    /// The requested UUID and exact native name did not identify a complete arrangement.
    #[error("arrangement {name:?} ({uuid}) is unavailable or incomplete")]
    UnavailableArrangement {
        /// Requested native arrangement UUID.
        uuid: uuid::Uuid,
        /// Requested exact native arrangement name.
        name: String,
    },
    /// More than one native arrangement used the same UUID.
    #[error("arrangement UUID {uuid} is ambiguous (including {name:?})")]
    AmbiguousArrangement {
        /// Duplicated native arrangement UUID.
        uuid: uuid::Uuid,
        /// Name of one arrangement carrying the duplicated UUID.
        name: String,
    },
    /// An alternate arrangement did not have one completely resolvable entry cue.
    #[error("arrangement #{index} {name:?} has no safe entry cue")]
    UnresolvedArrangementEntry {
        /// Zero-based index in native arrangement order.
        index: usize,
        /// Native arrangement name, which may itself be malformed.
        name: String,
    },
}

/// Failure to restyle the operator entry cue of an arrangement-less presentation.
#[derive(Debug, thiserror::Error)]
pub enum OperatorEntryBackgroundError {
    /// The reviewed image bytes were not a usable background.
    #[error(transparent)]
    Image(#[from] BackgroundImageError),
    /// The arrangement-less cue-group traversal was unsafe for mutation.
    #[error(transparent)]
    OperatorTraversal(#[from] crate::propresenter::arrangement::OperatorTraversalError),
    /// The presentation had native arrangements and requires exact selection.
    #[error("arrangement-less background update received {count} native arrangements")]
    HasArrangements {
        /// Number of native arrangements found.
        count: usize,
    },
    /// The presentation contained no operator-visible cue.
    #[error("arrangement-less presentation has no operator-visible cue")]
    MissingOperatorCue,
    /// The operator entry cue had no stable native identity.
    #[error("operator entry cue #{index} has no native UUID")]
    MissingOperatorCueUuid {
        /// Zero-based cue index in native cue order.
        index: usize,
    },
}

/// Resolve and validate one project-relative background image.
///
/// The returned path is canonical, confined beneath `data_root`, and contains
/// a decodable PNG, JPEG, or TIFF image with nonzero dimensions.
pub fn resolve_background_image(
    data_root: &Path,
    relative_path: &Path,
) -> Result<PathBuf, BackgroundImageError> {
    let root = data_root
        .canonicalize()
        .map_err(|source| BackgroundImageError::DataRoot {
            path: data_root.to_path_buf(),
            source,
        })?;
    let requested = data_root.join(relative_path);
    let image = requested
        .canonicalize()
        .map_err(|source| BackgroundImageError::Image {
            path: requested.clone(),
            source,
        })?;
    if !image.starts_with(&root) {
        return Err(BackgroundImageError::OutsideDataRoot { path: image, root });
    }
    let metadata = image
        .metadata()
        .map_err(|source| BackgroundImageError::Image {
            path: image.clone(),
            source,
        })?;
    if !metadata.is_file() {
        return Err(BackgroundImageError::NotFile(image));
    }
    if metadata.len() == 0 {
        return Err(BackgroundImageError::Empty(image));
    }

    let bytes = std::fs::read(&image).map_err(|source| BackgroundImageError::Image {
        path: image.clone(),
        source,
    })?;
    validate_background_image_bytes(&image, &bytes)?;
    Ok(image)
}

/// Validate the exact bytes used by a reviewed build and return their natural
/// size. This function is pure so resolution, review capture, and rendering all
/// enforce one format predicate.
pub(crate) fn validate_background_image_bytes(
    image_path: &Path,
    bytes: &[u8],
) -> Result<(u32, u32), BackgroundImageError> {
    if bytes.is_empty() {
        return Err(BackgroundImageError::Empty(image_path.to_path_buf()));
    }
    let extension = image_path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase);
    let format = match extension.as_deref() {
        Some("png") => ImageFormat::Png,
        Some("jpg" | "jpeg") => ImageFormat::Jpeg,
        Some("tif" | "tiff") => ImageFormat::Tiff,
        _ => {
            return Err(BackgroundImageError::InvalidFormat(
                image_path.to_path_buf(),
            ));
        }
    };
    // Supplying the format selected from the extension both enforces agreement
    // and makes the decoder consume the complete image rather than trusting a
    // signature or dimension header alone.
    let decoded = image::ImageReader::with_format(Cursor::new(bytes), format)
        .decode()
        .map_err(|_| BackgroundImageError::InvalidFormat(image_path.to_path_buf()))?;
    let (width, height) = (decoded.width(), decoded.height());
    if width == 0 || height == 0 {
        return Err(BackgroundImageError::InvalidDimensions(
            image_path.to_path_buf(),
        ));
    }
    Ok((width, height))
}

#[cfg(test)]
pub(crate) fn make_background_media_action_for_test(
    image_path: &Path,
    dimensions: (u32, u32),
    propresenter_root: &Path,
) -> rv_data::Action {
    make_background_media_action_with_dimensions(image_path, dimensions, propresenter_root)
}

fn make_background_media_action_with_dimensions(
    image_path: &Path,
    dimensions: (u32, u32),
    propresenter_root: &Path,
) -> rv_data::Action {
    let media_url = super::native_url::local_file_url(image_path, propresenter_root);
    let filename = image_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string();
    let format = image_path
        .extension()
        .and_then(|extension| extension.to_str())
        .map_or_else(String::new, |extension| {
            match extension.to_ascii_lowercase().as_str() {
                "jpeg" => "JPG".to_string(),
                other => other.to_ascii_uppercase(),
            }
        });
    let natural_size = Some(graphics::Size {
        width: f64::from(dimensions.0),
        height: f64::from(dimensions.1),
    });

    let media = Media {
        uuid: Some(Uuid {
            string: uuid::Uuid::new_v4().to_string(),
        }),
        url: Some(media_url.clone()),
        metadata: Some(media::Metadata {
            manufacture_name: String::new(),
            manufacture_url: None,
            information: String::new(),
            artist: String::new(),
            format,
            color_format: media::metadata::ColorFormat::Sdr as i32,
        }),
        type_properties: Some(media::TypeProperties::Image(media::ImageTypeProperties {
            drawing: Some(media::DrawingProperties {
                scale_behavior: media::ScaleBehavior::Fit as i32,
                is_blurred: false,
                scale_alignment: media::ScaleAlignment::MiddleCenter as i32,
                flipped_horizontally: false,
                flipped_vertically: false,
                natural_size,
                custom_image_rotation: 0.0,
                custom_image_bounds: None,
                custom_image_aspect_locked: false,
                alpha_inverted: false,
                native_rotation: media::drawing_properties::NativeRotationType::RotateStandard
                    as i32,
                selected_effect_preset_uuid: Some(Uuid {
                    string: "00000000-0000-0000-0000-000000000000".to_string(),
                }),
                effects: Vec::new(),
                crop_enable: false,
                crop_insets: Some(graphics::EdgeInsets {
                    left: 0.0,
                    right: 0.0,
                    top: 0.0,
                    bottom: 0.0,
                }),
                alpha_type: AlphaType::Straight as i32,
            }),
            file: Some(FileProperties {
                local_url: Some(media_url),
                remote_properties: None,
            }),
        })),
    };

    rv_data::Action {
        uuid: Some(Uuid {
            string: uuid::Uuid::new_v4().to_string(),
        }),
        name: filename,
        label: None,
        delay_time: 0.0,
        old_type: None,
        is_enabled: true,
        layer_identification: None,
        duration: 0.0,
        r#type: action::ActionType::Media as i32,
        action_type_data: Some(action::ActionTypeData::Media(action::MediaType {
            transition_duration: 0.0,
            selected_effect_preset_uuid: None,
            transition: None,
            effects: Vec::new(),
            element: Some(media),
            layer_type: LayerType::Background as i32,
            always_retrigger: false,
            markers: Vec::new(),
            media_type: Some(action::media_type::MediaType::Image(
                action::media_type::Image {},
            )),
        })),
    }
}

/// Add a background using the exact image bytes captured during preview.
pub(crate) fn add_reviewed_background_to_first_cue(
    presentation: &mut rv_data::Presentation,
    image_path: &Path,
    image_data: &[u8],
    propresenter_root: &Path,
) -> Result<(), BackgroundImageError> {
    let dimensions = validate_background_image_bytes(image_path, image_data)?;
    let cue_idx = crate::propresenter::arrangement::checked_operator_cue_indices(presentation)?
        .first()
        .copied()
        .ok_or(crate::propresenter::arrangement::OperatorTraversalError::EmptyTraversal)?;
    let first_cue = &mut presentation.cues[cue_idx];
    ensure_background_on_cue_with_dimensions(first_cue, image_path, dimensions, propresenter_root);
    Ok(())
}

/// Replace the background at every native arrangement entry cue and select one
/// exact arrangement.
///
/// The operation is copy-on-write and all-or-nothing: every arrangement must
/// have a unique, complete traversal before any protobuf field changes. Existing
/// background actions retain their wrapper identity and position; only their
/// name, action kind, and media payload are canonicalized. When an entry cue has
/// no background, one canonical action with deterministic identities is appended.
/// Entry cues shared by multiple arrangements are updated once.
///
/// Returns whether the presentation changed.
pub(crate) fn replace_arrangement_entry_backgrounds(
    presentation: &mut rv_data::Presentation,
    image_path: &Path,
    image_data: &[u8],
    propresenter_root: &Path,
    selected_arrangement_uuid: &uuid::Uuid,
    selected_arrangement_name: &str,
) -> Result<bool, ArrangementBackgroundError> {
    let dimensions = validate_background_image_bytes(image_path, image_data)?;
    let (selected_native_uuid, entries) = checked_arrangement_entries(
        presentation,
        selected_arrangement_uuid,
        selected_arrangement_name,
    )?;

    // Work on a complete value so even an internal structural mismatch cannot
    // expose a partially restyled presentation to the caller.
    let mut transformed = presentation.clone();
    transformed.selected_arrangement = Some(selected_native_uuid);
    for entry in entries {
        let Some(cue) = transformed.cues.get_mut(entry.cue_index) else {
            return Err(ArrangementBackgroundError::UnresolvedArrangementEntry {
                index: entry.arrangement_index,
                name: entry.arrangement_name,
            });
        };
        replace_backgrounds_on_cue(
            cue,
            &entry.cue_uuid,
            image_path,
            dimensions,
            propresenter_root,
        );
    }

    if transformed == *presentation {
        Ok(false)
    } else {
        *presentation = transformed;
        Ok(true)
    }
}

/// Replace the first operator-visible background when a native presentation has
/// no arrangements.
///
/// This is deliberately separate from [`replace_arrangement_entry_backgrounds`]
/// so callers cannot silently discard an available native arrangement. A stale
/// selected-arrangement reference is rejected rather than interpreted as raw
/// cue order.
/// Existing background wrappers retain their identity and position; insertion
/// is deterministic and the operation is copy-on-write.
pub(crate) fn replace_operator_entry_background(
    presentation: &mut rv_data::Presentation,
    image_path: &Path,
    image_data: &[u8],
    propresenter_root: &Path,
) -> Result<bool, OperatorEntryBackgroundError> {
    if !presentation.arrangements.is_empty() {
        return Err(OperatorEntryBackgroundError::HasArrangements {
            count: presentation.arrangements.len(),
        });
    }
    let dimensions = validate_background_image_bytes(image_path, image_data)?;
    let cue_index = crate::propresenter::arrangement::checked_operator_cue_indices(presentation)?
        .first()
        .copied()
        .ok_or(crate::propresenter::arrangement::OperatorTraversalError::EmptyTraversal)?;
    let cue_uuid = presentation
        .cues
        .get(cue_index)
        .and_then(|cue| cue.uuid.as_ref())
        .map(|uuid| uuid.string.clone())
        .ok_or(OperatorEntryBackgroundError::MissingOperatorCueUuid { index: cue_index })?;

    let mut transformed = presentation.clone();
    transformed.selected_arrangement = None;
    let Some(cue) = transformed.cues.get_mut(cue_index) else {
        return Err(OperatorEntryBackgroundError::MissingOperatorCue);
    };
    replace_backgrounds_on_cue(cue, &cue_uuid, image_path, dimensions, propresenter_root);

    if transformed == *presentation {
        Ok(false)
    } else {
        *presentation = transformed;
        Ok(true)
    }
}

struct ArrangementEntry {
    arrangement_index: usize,
    arrangement_name: String,
    cue_index: usize,
    cue_uuid: String,
}

fn checked_arrangement_entries(
    presentation: &rv_data::Presentation,
    selected_uuid: &uuid::Uuid,
    selected_name: &str,
) -> Result<(rv_data::Uuid, Vec<ArrangementEntry>), ArrangementBackgroundError> {
    let selected_native_uuid =
        checked_selected_native_uuid(presentation, selected_uuid, selected_name)?;

    let mut entries = Vec::new();
    for (arrangement_index, arrangement) in presentation.arrangements.iter().enumerate() {
        let resolved = match crate::propresenter::arrangement::selectable_arrangement(
            presentation,
            arrangement,
        ) {
            Ok(resolved) => resolved,
            Err(crate::propresenter::arrangement::ArrangementSelectionError::Ambiguous {
                ..
            }) => {
                let Some(uuid) = arrangement
                    .uuid
                    .as_ref()
                    .and_then(|native| uuid::Uuid::parse_str(&native.string).ok())
                else {
                    return Err(ArrangementBackgroundError::UnresolvedArrangementEntry {
                        index: arrangement_index,
                        name: arrangement.name.clone(),
                    });
                };
                return Err(ArrangementBackgroundError::AmbiguousArrangement {
                    uuid,
                    name: arrangement.name.clone(),
                });
            }
            Err(
                crate::propresenter::arrangement::ArrangementSelectionError::Unavailable
                | crate::propresenter::arrangement::ArrangementSelectionError::Incomplete,
            ) => {
                return Err(ArrangementBackgroundError::UnresolvedArrangementEntry {
                    index: arrangement_index,
                    name: arrangement.name.clone(),
                });
            }
        };
        let cue_index = resolved.entry_cue_index();
        let Some(cue_uuid) = presentation
            .cues
            .get(cue_index)
            .and_then(|cue| cue.uuid.as_ref())
            .map(|uuid| uuid.string.clone())
        else {
            return Err(ArrangementBackgroundError::UnresolvedArrangementEntry {
                index: arrangement_index,
                name: arrangement.name.clone(),
            });
        };

        if !entries
            .iter()
            .any(|entry: &ArrangementEntry| entry.cue_index == cue_index)
        {
            entries.push(ArrangementEntry {
                arrangement_index,
                arrangement_name: arrangement.name.clone(),
                cue_index,
                cue_uuid,
            });
        }
    }

    Ok((selected_native_uuid, entries))
}

fn checked_selected_native_uuid(
    presentation: &rv_data::Presentation,
    selected_uuid: &uuid::Uuid,
    selected_name: &str,
) -> Result<rv_data::Uuid, ArrangementBackgroundError> {
    use crate::propresenter::arrangement::{
        selectable_arrangement_by_identity, ArrangementSelectionError,
    };

    let resolved = selectable_arrangement_by_identity(presentation, selected_uuid, selected_name)
        .map_err(|error| match error {
        ArrangementSelectionError::Ambiguous { .. } => {
            ArrangementBackgroundError::AmbiguousArrangement {
                uuid: *selected_uuid,
                name: selected_name.to_string(),
            }
        }
        ArrangementSelectionError::Unavailable | ArrangementSelectionError::Incomplete => {
            ArrangementBackgroundError::UnavailableArrangement {
                uuid: *selected_uuid,
                name: selected_name.to_string(),
            }
        }
    })?;
    resolved.native_uuid().cloned().ok_or_else(|| {
        ArrangementBackgroundError::UnavailableArrangement {
            uuid: *selected_uuid,
            name: selected_name.to_string(),
        }
    })
}

fn replace_backgrounds_on_cue(
    cue: &mut rv_data::Cue,
    cue_uuid: &str,
    image_path: &Path,
    dimensions: (u32, u32),
    propresenter_root: &Path,
) {
    let existing_background = cue
        .actions
        .iter()
        .find(|action| is_background_media_action(action));
    let replacement = canonical_replacement_action(
        existing_background,
        cue_uuid,
        image_path,
        dimensions,
        propresenter_root,
    );
    let mut replacement = Some(replacement);
    let mut actions = Vec::with_capacity(cue.actions.len().saturating_add(1));
    for action in &cue.actions {
        if is_background_media_action(action) {
            if let Some(replacement) = replacement.take() {
                actions.push(replacement);
            }
        } else {
            actions.push(action.clone());
        }
    }
    if let Some(replacement) = replacement {
        actions.push(replacement);
    }
    cue.actions = actions;
}

fn canonical_replacement_action(
    existing: Option<&rv_data::Action>,
    cue_uuid: &str,
    image_path: &Path,
    dimensions: (u32, u32),
    propresenter_root: &Path,
) -> rv_data::Action {
    let desired_url = super::native_url::canonical_file_url(image_path);
    let mut canonical =
        make_background_media_action_with_dimensions(image_path, dimensions, propresenter_root);
    let action_uuid = deterministic_background_uuid(cue_uuid, &desired_url, b"action");
    let media_uuid = deterministic_background_uuid(cue_uuid, &desired_url, b"media");
    canonical.uuid = Some(action_uuid);
    if let Some(action::ActionTypeData::Media(media_action)) = canonical.action_type_data.as_mut() {
        if let Some(media) = media_action.element.as_mut() {
            media.uuid = Some(media_uuid);
        }
    }

    if let Some(existing) = existing {
        let canonical_uuid = canonical.uuid.clone();
        let canonical_name = canonical.name;
        let canonical_type = canonical.r#type;
        let canonical_payload = canonical.action_type_data;
        canonical = existing.clone();
        // A changed URL must receive fresh action/media identity so
        // ProPresenter does not reuse the prior media object's cached render.
        canonical.uuid = canonical_uuid;
        canonical.name = canonical_name;
        canonical.r#type = canonical_type;
        canonical.action_type_data = canonical_payload;
    }
    canonical
}

#[cfg(test)]
fn background_media_uuid(action: &rv_data::Action) -> Option<&rv_data::Uuid> {
    let Some(action::ActionTypeData::Media(media_action)) = &action.action_type_data else {
        return None;
    };
    media_action.element.as_ref()?.uuid.as_ref()
}

fn deterministic_background_uuid(cue_uuid: &str, image_url: &str, role: &[u8]) -> rv_data::Uuid {
    let mut hasher = Sha256::new();
    hasher.update(b"proflow:arrangement-entry-background:v1\0");
    hasher.update(role);
    hasher.update([0]);
    hasher.update(cue_uuid.as_bytes());
    hasher.update([0]);
    hasher.update(image_url.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    // RFC 9562 UUIDv8 reserves this version for application-defined hashes.
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    rv_data::Uuid {
        string: uuid::Uuid::from_bytes(bytes).to_string(),
    }
}

/// Return the cue index `ProPresenter` operators appear to see first.
///
/// This is best-effort inspection. Native mutation must use
/// `arrangement::checked_operator_cue_indices` instead.
pub fn first_operator_cue_index(presentation: &rv_data::Presentation) -> Option<usize> {
    crate::propresenter::arrangement::operator_cue_indices(presentation)
        .into_iter()
        .next()
}

/// Return whether an action is a background media action.
#[must_use]
pub const fn is_background_media_action(action: &rv_data::Action) -> bool {
    action.r#type == action::ActionType::BackgroundMedia as i32
        || matches!(
            &action.action_type_data,
            Some(action::ActionTypeData::Media(media_type))
                if media_type.layer_type == LayerType::Background as i32
        )
}

fn ensure_background_on_cue_with_dimensions(
    cue: &mut rv_data::Cue,
    image_path: &Path,
    dimensions: (u32, u32),
    propresenter_root: &Path,
) -> bool {
    let desired_url = super::native_url::canonical_file_url(image_path);
    let existing: Vec<_> = cue
        .actions
        .iter()
        .filter(|action| is_background_media_action(action))
        .collect();
    if existing.len() == 1
        && existing
            .first()
            .and_then(|action| background_media_url(action))
            .is_some_and(|url| url == desired_url)
    {
        return false;
    }

    cue.actions
        .retain(|action| !is_background_media_action(action));
    let bg_action =
        make_background_media_action_with_dimensions(image_path, dimensions, propresenter_root);
    cue.actions.push(bg_action);
    true
}

fn background_media_url(action: &rv_data::Action) -> Option<&str> {
    let Some(action::ActionTypeData::Media(media_type)) = &action.action_type_data else {
        return None;
    };
    media_type
        .element
        .as_ref()?
        .url
        .as_ref()?
        .storage
        .as_ref()
        .and_then(|storage| match storage {
            url::Storage::AbsoluteString(value) => Some(value.as_str()),
            url::Storage::RelativePath(_) => None,
        })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic)]

    use std::{io::Cursor, path::Path};

    use super::*;

    fn cue(id: &str) -> rv_data::Cue {
        rv_data::Cue {
            uuid: Some(rv_data::Uuid {
                string: id.to_string(),
            }),
            ..rv_data::Cue::default()
        }
    }

    fn group(name: &str, id: &str, cue_id: &str) -> rv_data::presentation::CueGroup {
        rv_data::presentation::CueGroup {
            group: Some(rv_data::Group {
                uuid: Some(rv_data::Uuid {
                    string: id.to_string(),
                }),
                name: name.to_string(),
                ..rv_data::Group::default()
            }),
            cue_identifiers: vec![rv_data::Uuid {
                string: cue_id.to_string(),
            }],
        }
    }

    fn encoded_image(format: ImageFormat, width: u32, height: u32) -> Vec<u8> {
        let image = image::DynamicImage::new_rgb8(width, height);
        let mut bytes = Cursor::new(Vec::new());
        image
            .write_to(&mut bytes, format)
            .expect("encode valid image fixture");
        bytes.into_inner()
    }

    fn native_uuid(value: &str) -> rv_data::Uuid {
        rv_data::Uuid {
            string: value.to_string(),
        }
    }

    fn marker_action(id: &str, name: &str, action_type: action::ActionType) -> rv_data::Action {
        rv_data::Action {
            uuid: Some(native_uuid(id)),
            name: name.to_string(),
            is_enabled: true,
            r#type: action_type as i32,
            ..rv_data::Action::default()
        }
    }

    fn old_background_action(
        path: &Path,
        root: &Path,
        action_id: &str,
        media_id: &str,
    ) -> rv_data::Action {
        let mut action = make_background_media_action_for_test(path, (1, 1), root);
        action.uuid = Some(native_uuid(action_id));
        action.delay_time = 1.25;
        action.duration = 2.5;
        let Some(action::ActionTypeData::Media(media_action)) = action.action_type_data.as_mut()
        else {
            panic!("background fixture must be media");
        };
        let Some(media) = media_action.element.as_mut() else {
            panic!("background fixture must carry media");
        };
        media.uuid = Some(native_uuid(media_id));
        action
    }

    fn arrangement(uuid: &str, name: &str, group_id: &str) -> rv_data::presentation::Arrangement {
        rv_data::presentation::Arrangement {
            uuid: Some(native_uuid(uuid)),
            name: name.to_string(),
            group_identifiers: vec![native_uuid(group_id)],
        }
    }

    fn restyle_fixture(root: &Path) -> rv_data::Presentation {
        let mut default_entry = cue("default-entry");
        default_entry.name = "Default entry".to_string();
        default_entry.actions = vec![
            marker_action(
                "slide-default",
                "Default slide",
                action::ActionType::PresentationSlide,
            ),
            old_background_action(
                &root.join("old default.png"),
                root,
                "background-default",
                "media-default",
            ),
            marker_action("macro-default", "Song macro", action::ActionType::Macro),
        ];

        let mut youth_entry = cue("youth-entry");
        youth_entry.name = "Youth entry".to_string();
        youth_entry.actions = vec![
            old_background_action(
                &root.join("old youth.png"),
                root,
                "background-youth",
                "media-youth",
            ),
            marker_action(
                "slide-youth",
                "Youth slide",
                action::ActionType::PresentationSlide,
            ),
        ];

        let mut sparse_entry = cue("sparse-entry");
        sparse_entry.name = "Sparse entry".to_string();
        sparse_entry.actions = vec![
            marker_action(
                "slide-sparse",
                "Sparse slide",
                action::ActionType::PresentationSlide,
            ),
            marker_action("macro-sparse", "Sparse macro", action::ActionType::Macro),
        ];

        let mut unrelated = cue("unrelated");
        unrelated.name = "Unrelated cue".to_string();
        unrelated.actions = vec![old_background_action(
            &root.join("leave me alone.png"),
            root,
            "background-unrelated",
            "media-unrelated",
        )];

        rv_data::Presentation {
            uuid: Some(native_uuid("presentation-identity")),
            name: "Preserve this song".to_string(),
            notes: "operator notes".to_string(),
            selected_arrangement: Some(native_uuid("99999999-9999-4999-8999-999999999999")),
            arrangements: vec![
                arrangement(
                    "11111111-1111-4111-8111-111111111111",
                    "Default",
                    "group-default",
                ),
                arrangement(
                    "22222222-2222-4222-8222-222222222222",
                    "Youth",
                    "group-youth",
                ),
                arrangement(
                    "33333333-3333-4333-8333-333333333333",
                    "Sparse",
                    "group-sparse",
                ),
            ],
            cue_groups: vec![
                group("Default", "group-default", "default-entry"),
                group("Youth", "group-youth", "youth-entry"),
                group("Sparse", "group-sparse", "sparse-entry"),
                group("Unrelated", "group-unrelated", "unrelated"),
            ],
            cues: vec![default_entry, youth_entry, sparse_entry, unrelated],
            ..rv_data::Presentation::default()
        }
    }

    fn background_actions(cue: &rv_data::Cue) -> Vec<&rv_data::Action> {
        cue.actions
            .iter()
            .filter(|action| is_background_media_action(action))
            .collect()
    }

    fn non_background_actions(cue: &rv_data::Cue) -> Vec<&rv_data::Action> {
        cue.actions
            .iter()
            .filter(|action| !is_background_media_action(action))
            .collect()
    }

    #[test]
    fn reviewed_background_uses_captured_dimensions_not_live_file_bytes() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("background.png");
        let reviewed = encoded_image(ImageFormat::Png, 3, 2);
        std::fs::write(&path, encoded_image(ImageFormat::Png, 1, 1))
            .expect("write changed live image");

        let mut presentation = rv_data::Presentation {
            cues: vec![cue("cue")],
            ..rv_data::Presentation::default()
        };
        add_reviewed_background_to_first_cue(&mut presentation, &path, &reviewed, directory.path())
            .expect("valid reviewed background");
        let cue = &presentation.cues[0];

        let Some(action::ActionTypeData::Media(media_action)) = &cue.actions[0].action_type_data
        else {
            panic!("expected background media action");
        };
        let Some(media::TypeProperties::Image(image)) = media_action
            .element
            .as_ref()
            .and_then(|element| element.type_properties.as_ref())
        else {
            panic!("expected image properties");
        };
        let size = image
            .drawing
            .as_ref()
            .and_then(|drawing| drawing.natural_size.as_ref())
            .expect("reviewed natural size");
        assert_eq!((size.width, size.height), (3.0, 2.0));
    }

    #[test]
    fn reviewed_background_rejects_malformed_selected_arrangement_atomically() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("background.png");
        let reviewed = encoded_image(ImageFormat::Png, 3, 2);
        std::fs::write(&path, &reviewed).expect("write image");
        let selected = "11111111-1111-4111-8111-111111111111";
        let mut presentation = rv_data::Presentation {
            selected_arrangement: Some(native_uuid(selected)),
            arrangements: vec![arrangement(selected, "Default", "missing-group")],
            cues: vec![cue("cue")],
            ..rv_data::Presentation::default()
        };
        let original = presentation.clone();

        let error = add_reviewed_background_to_first_cue(
            &mut presentation,
            &path,
            &reviewed,
            directory.path(),
        )
        .expect_err("background mutation must not use inspection fallback order");

        assert!(matches!(
            error,
            BackgroundImageError::OperatorTraversal(
                crate::propresenter::arrangement::OperatorTraversalError::SelectedArrangementIncomplete { ref name }
            ) if name == "Default"
        ));
        assert_eq!(presentation, original);
    }

    #[test]
    fn fully_decodable_backgrounds_report_nonzero_dimensions() {
        for (path, bytes, expected) in [
            (
                Path::new("background.png"),
                encoded_image(ImageFormat::Png, 3, 2),
                (3, 2),
            ),
            (
                Path::new("background.jpg"),
                encoded_image(ImageFormat::Jpeg, 4, 3),
                (4, 3),
            ),
            (
                Path::new("background.tif"),
                encoded_image(ImageFormat::Tiff, 5, 4),
                (5, 4),
            ),
            (
                Path::new("background.tiff"),
                encoded_image(ImageFormat::Tiff, 2, 1),
                (2, 1),
            ),
        ] {
            assert_eq!(
                validate_background_image_bytes(path, &bytes).expect("supported image"),
                expected
            );
        }
    }

    #[test]
    fn header_only_and_truncated_backgrounds_are_rejected() {
        let png = encoded_image(ImageFormat::Png, 3, 2);
        let jpeg = encoded_image(ImageFormat::Jpeg, 3, 2);
        let tiff = encoded_image(ImageFormat::Tiff, 3, 2);
        for (path, bytes) in [
            (Path::new("header-only.png"), png[..24].to_vec()),
            (Path::new("truncated.png"), png[..png.len() / 2].to_vec()),
            (Path::new("truncated.jpg"), jpeg[..jpeg.len() / 2].to_vec()),
            (
                Path::new("header-only.tiff"),
                tiff[..16.min(tiff.len())].to_vec(),
            ),
            (Path::new("truncated.tiff"), tiff[..tiff.len() / 2].to_vec()),
            (Path::new("unsupported.gif"), b"GIF89a".to_vec()),
        ] {
            assert!(matches!(
                validate_background_image_bytes(path, &bytes),
                Err(BackgroundImageError::InvalidFormat(error_path)) if error_path == path
            ));
        }
    }

    #[test]
    fn extension_must_match_decoded_format() {
        let png = encoded_image(ImageFormat::Png, 3, 2);
        let path = Path::new("png-with-jpeg-extension.jpg");
        assert!(matches!(
            validate_background_image_bytes(path, &png),
            Err(BackgroundImageError::InvalidFormat(error_path)) if error_path == path
        ));
    }

    #[test]
    fn adds_background_to_arrangement_first_cue_not_raw_first_cue() {
        const ARRANGEMENT_UUID: &str = "11111111-1111-4111-8111-111111111111";
        let mut presentation = rv_data::Presentation {
            selected_arrangement: Some(rv_data::Uuid {
                string: ARRANGEMENT_UUID.to_string(),
            }),
            arrangements: vec![rv_data::presentation::Arrangement {
                uuid: Some(rv_data::Uuid {
                    string: ARRANGEMENT_UUID.to_string(),
                }),
                name: "Default".to_string(),
                group_identifiers: vec![
                    rv_data::Uuid {
                        string: "g-title".to_string(),
                    },
                    rv_data::Uuid {
                        string: "g-body".to_string(),
                    },
                ],
            }],
            cues: vec![cue("body"), cue("title")],
            cue_groups: vec![
                group("Title", "g-title", "title"),
                group("Verse", "g-body", "body"),
            ],
            ..rv_data::Presentation::default()
        };

        add_reviewed_background_to_first_cue(
            &mut presentation,
            Path::new("/tmp/default.png"),
            &encoded_image(ImageFormat::Png, 3, 2),
            Path::new("/tmp"),
        )
        .expect("valid reviewed background");

        assert!(!presentation.cues[0]
            .actions
            .iter()
            .any(is_background_media_action));
        assert!(presentation.cues[1]
            .actions
            .iter()
            .any(is_background_media_action));
    }

    #[test]
    fn background_action_matches_native_media_envelope() {
        let directory = tempfile::tempdir().expect("tempdir");
        let image = directory.path().join("sermon background.png");
        std::fs::write(&image, encoded_image(ImageFormat::Png, 3, 2)).expect("write png fixture");

        let action = make_background_media_action_for_test(&image, (3, 2), directory.path());

        assert_eq!(action.r#type, action::ActionType::Media as i32);
        assert_eq!(action.name, "sermon background.png");
        assert!(matches!(
            &action.action_type_data,
            Some(action::ActionTypeData::Media(_))
        ));
        let Some(action::ActionTypeData::Media(media_type)) = &action.action_type_data else {
            return;
        };
        assert_eq!(media_type.layer_type, LayerType::Background as i32);
        let element = media_type.element.as_ref().expect("media element");
        assert_eq!(
            element
                .metadata
                .as_ref()
                .map(|metadata| metadata.format.as_str()),
            Some("PNG")
        );
        let url = element.url.as_ref().expect("media URL");
        assert!(matches!(
            &url.storage,
            Some(url::Storage::AbsoluteString(_))
        ));
        assert!(matches!(
            &url.relative_file_path,
            Some(url::RelativeFilePath::Local(local))
                if local.root == url::local_relative_path::Root::Show as i32
                    && local.path == "sermon background.png"
        ));
        let Some(url::Storage::AbsoluteString(url)) = &url.storage else {
            return;
        };
        assert!(url.ends_with("/sermon%20background.png"));
        assert!(matches!(
            &element.type_properties,
            Some(media::TypeProperties::Image(_))
        ));
        let Some(media::TypeProperties::Image(image)) = &element.type_properties else {
            return;
        };
        let drawing = image.drawing.as_ref().expect("image drawing metadata");
        assert_eq!(
            drawing.natural_size,
            Some(graphics::Size {
                width: 3.0,
                height: 2.0,
            })
        );
        assert_eq!(drawing.alpha_type, AlphaType::Straight as i32);
        assert_eq!(
            image.file.as_ref().and_then(|file| file.local_url.as_ref()),
            element.url.as_ref()
        );
    }

    #[test]
    fn arrangement_restyle_is_atomic_faithful_deterministic_and_idempotent() {
        let directory = tempfile::tempdir().expect("tempdir");
        let image = directory.path().join("new lyrics.png");
        std::fs::write(&image, encoded_image(ImageFormat::Png, 1, 1))
            .expect("changed live background");
        let reviewed = encoded_image(ImageFormat::Png, 3, 2);
        let selected =
            uuid::Uuid::parse_str("11111111-1111-4111-8111-111111111111").expect("selected UUID");
        let before = restyle_fixture(directory.path());
        let mut presentation = before.clone();

        assert!(replace_arrangement_entry_backgrounds(
            &mut presentation,
            &image,
            &reviewed,
            directory.path(),
            &selected,
            "Default",
        )
        .expect("complete arrangements can be restyled"));

        assert_eq!(
            presentation.selected_arrangement,
            Some(native_uuid("11111111-1111-4111-8111-111111111111"))
        );
        assert_eq!(presentation.uuid, before.uuid);
        assert_eq!(presentation.name, before.name);
        assert_eq!(presentation.notes, before.notes);
        assert_eq!(presentation.arrangements, before.arrangements);
        assert_eq!(presentation.cue_groups, before.cue_groups);
        assert_eq!(presentation.cues[3], before.cues[3]);

        for cue_index in 0..3 {
            assert_eq!(
                non_background_actions(&presentation.cues[cue_index]),
                non_background_actions(&before.cues[cue_index])
            );
            let backgrounds = background_actions(&presentation.cues[cue_index]);
            assert_eq!(backgrounds.len(), 1);
            assert_eq!(
                background_media_url(backgrounds[0]),
                Some(crate::propresenter::native_url::canonical_file_url(&image).as_str())
            );
        }

        let default_before = background_actions(&before.cues[0])[0];
        let default_after = background_actions(&presentation.cues[0])[0];
        assert_ne!(default_after.uuid, default_before.uuid);
        assert_eq!(default_after.label, default_before.label);
        assert_eq!(
            default_after.delay_time.to_bits(),
            default_before.delay_time.to_bits()
        );
        assert_eq!(default_after.old_type, default_before.old_type);
        assert_eq!(default_after.is_enabled, default_before.is_enabled);
        assert_eq!(
            default_after.layer_identification,
            default_before.layer_identification
        );
        assert_eq!(
            default_after.duration.to_bits(),
            default_before.duration.to_bits()
        );
        assert_ne!(
            background_media_uuid(default_after),
            background_media_uuid(default_before)
        );
        let action_ids = presentation.cues[0]
            .actions
            .iter()
            .filter_map(|action| action.uuid.as_ref())
            .map(|uuid| uuid.string.as_str())
            .collect::<Vec<_>>();
        assert_eq!(action_ids[0], "slide-default");
        assert_ne!(action_ids[1], "background-default");
        assert_eq!(action_ids[2], "macro-default");
        assert_eq!(
            presentation.cues[2]
                .actions
                .last()
                .map(|action| action.name.as_str()),
            Some("new lyrics.png")
        );

        let once = presentation.clone();
        assert!(!replace_arrangement_entry_backgrounds(
            &mut presentation,
            &image,
            &reviewed,
            directory.path(),
            &selected,
            "Default",
        )
        .expect("reapplying is valid"));
        assert_eq!(presentation, once);

        let mut independently_restyled = before;
        replace_arrangement_entry_backgrounds(
            &mut independently_restyled,
            &image,
            &reviewed,
            directory.path(),
            &selected,
            "Default",
        )
        .expect("same source can be restyled again");
        assert_eq!(independently_restyled, once);
    }

    #[test]
    fn arrangement_restyle_rejects_unavailable_or_ambiguous_selection_without_mutation() {
        let directory = tempfile::tempdir().expect("tempdir");
        let image = directory.path().join("new.png");
        let reviewed = encoded_image(ImageFormat::Png, 3, 2);
        let missing =
            uuid::Uuid::parse_str("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").expect("missing UUID");
        let mut unavailable = restyle_fixture(directory.path());
        let unavailable_before = unavailable.clone();

        assert!(matches!(
            replace_arrangement_entry_backgrounds(
                &mut unavailable,
                &image,
                &reviewed,
                directory.path(),
                &missing,
                "Default",
            ),
            Err(ArrangementBackgroundError::UnavailableArrangement { uuid, name })
                if uuid == missing && name == "Default"
        ));
        assert_eq!(unavailable, unavailable_before);

        let selected =
            uuid::Uuid::parse_str("11111111-1111-4111-8111-111111111111").expect("selected UUID");
        let mut ambiguous = restyle_fixture(directory.path());
        ambiguous.arrangements[1].uuid = Some(native_uuid(&selected.to_string()));
        let ambiguous_before = ambiguous.clone();
        assert!(matches!(
            replace_arrangement_entry_backgrounds(
                &mut ambiguous,
                &image,
                &reviewed,
                directory.path(),
                &selected,
                "Default",
            ),
            Err(ArrangementBackgroundError::AmbiguousArrangement { uuid, .. })
                if uuid == selected
        ));
        assert_eq!(ambiguous, ambiguous_before);
    }

    #[test]
    fn arrangement_restyle_rejects_an_unresolved_alternate_before_changing_any_cue() {
        let directory = tempfile::tempdir().expect("tempdir");
        let image = directory.path().join("new.png");
        let reviewed = encoded_image(ImageFormat::Png, 3, 2);
        let selected =
            uuid::Uuid::parse_str("11111111-1111-4111-8111-111111111111").expect("selected UUID");
        let mut presentation = restyle_fixture(directory.path());
        presentation.arrangements[2].group_identifiers = vec![native_uuid("missing-group")];
        let before = presentation.clone();

        assert!(matches!(
            replace_arrangement_entry_backgrounds(
                &mut presentation,
                &image,
                &reviewed,
                directory.path(),
                &selected,
                "Default",
            ),
            Err(ArrangementBackgroundError::UnresolvedArrangementEntry {
                index: 2,
                name,
            }) if name == "Sparse"
        ));
        assert_eq!(presentation, before);
    }

    #[test]
    fn arrangementless_restyle_targets_operator_entry_and_is_idempotent() {
        let directory = tempfile::tempdir().expect("tempdir");
        let image = directory.path().join("new.png");
        let reviewed = encoded_image(ImageFormat::Png, 3, 2);
        let mut unrelated = cue("raw-first");
        unrelated.actions = vec![old_background_action(
            &directory.path().join("unrelated.png"),
            directory.path(),
            "unrelated-background",
            "unrelated-media",
        )];
        let mut operator_entry = cue("operator-entry");
        operator_entry.actions = vec![
            marker_action(
                "operator-slide",
                "Operator slide",
                action::ActionType::PresentationSlide,
            ),
            old_background_action(
                &directory.path().join("old.png"),
                directory.path(),
                "operator-background",
                "operator-media",
            ),
            marker_action(
                "operator-macro",
                "Operator macro",
                action::ActionType::Macro,
            ),
        ];
        let mut presentation = rv_data::Presentation {
            cue_groups: vec![
                group("Entry", "entry-group", "operator-entry"),
                group("Other", "other-group", "raw-first"),
            ],
            cues: vec![unrelated, operator_entry],
            ..rv_data::Presentation::default()
        };
        let before = presentation.clone();

        assert!(replace_operator_entry_background(
            &mut presentation,
            &image,
            &reviewed,
            directory.path(),
        )
        .expect("arrangement-less presentation can be restyled"));
        assert_eq!(presentation.selected_arrangement, None);
        assert_eq!(presentation.cues[0], before.cues[0]);
        let action_ids = presentation.cues[1]
            .actions
            .iter()
            .filter_map(|action| action.uuid.as_ref())
            .map(|uuid| uuid.string.as_str())
            .collect::<Vec<_>>();
        assert_eq!(action_ids[0], "operator-slide");
        assert_ne!(action_ids[1], "operator-background");
        assert_eq!(action_ids[2], "operator-macro");
        assert_eq!(
            background_media_url(background_actions(&presentation.cues[1])[0]),
            Some(crate::propresenter::native_url::canonical_file_url(&image).as_str())
        );

        let once = presentation.clone();
        assert!(!replace_operator_entry_background(
            &mut presentation,
            &image,
            &reviewed,
            directory.path(),
        )
        .expect("reapplying is valid"));
        assert_eq!(presentation, once);
    }

    #[test]
    fn arrangementless_restyle_rejects_a_stale_selection_atomically() {
        let directory = tempfile::tempdir().expect("tempdir");
        let mut presentation = rv_data::Presentation {
            selected_arrangement: Some(native_uuid("stale-selection")),
            cues: vec![cue("operator-entry")],
            ..rv_data::Presentation::default()
        };
        let before = presentation.clone();

        let error = replace_operator_entry_background(
            &mut presentation,
            &directory.path().join("new.png"),
            &encoded_image(ImageFormat::Png, 3, 2),
            directory.path(),
        )
        .expect_err("stale selected identity must not fall through to raw cue order");

        assert!(matches!(
            error,
            OperatorEntryBackgroundError::OperatorTraversal(
                crate::propresenter::arrangement::OperatorTraversalError::SelectedArrangementUnavailable { ref identifier }
            ) if identifier == "stale-selection"
        ));
        assert_eq!(presentation, before);
    }

    #[test]
    fn arrangementless_restyle_rejects_native_arrangements_without_mutation() {
        let directory = tempfile::tempdir().expect("tempdir");
        let mut presentation = restyle_fixture(directory.path());
        let before = presentation.clone();

        assert!(matches!(
            replace_operator_entry_background(
                &mut presentation,
                &directory.path().join("new.png"),
                &encoded_image(ImageFormat::Png, 3, 2),
                directory.path(),
            ),
            Err(OperatorEntryBackgroundError::HasArrangements { count: 3 })
        ));
        assert_eq!(presentation, before);
    }

    #[test]
    fn configured_background_must_be_nonempty_image_data() {
        let root = tempfile::tempdir().expect("data root");
        let backgrounds = root.path().join("backgrounds");
        std::fs::create_dir(&backgrounds).expect("background directory");
        let image = backgrounds.join("empty.png");
        std::fs::write(&image, []).expect("empty image fixture");
        let canonical_image = image.canonicalize().expect("canonical empty image path");

        assert!(matches!(
            resolve_background_image(root.path(), Path::new("backgrounds/empty.png")),
            Err(BackgroundImageError::Empty(path)) if path == canonical_image
        ));
    }

    #[test]
    fn configured_background_is_resolved_from_explicit_relative_path() {
        let root = tempfile::tempdir().expect("data root");
        let backgrounds = root.path().join("backgrounds");
        std::fs::create_dir(&backgrounds).expect("background directory");
        let image = backgrounds.join("seasonal.png");
        std::fs::write(&image, encoded_image(ImageFormat::Png, 3, 2)).expect("png fixture");

        let resolved = resolve_background_image(root.path(), Path::new("backgrounds/seasonal.png"))
            .expect("valid image should resolve");

        assert_eq!(
            resolved,
            image.canonicalize().expect("canonical image path")
        );
    }

    #[cfg(unix)]
    #[test]
    fn configured_background_cannot_escape_through_symlink() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().expect("data root");
        let outside = tempfile::tempdir().expect("outside root");
        let outside_image = outside.path().join("outside.png");
        std::fs::write(&outside_image, [137, 80, 78, 71, 13, 10, 26, 10])
            .expect("png signature fixture");
        symlink(outside.path(), root.path().join("linked")).expect("asset symlink");

        assert!(matches!(
            resolve_background_image(root.path(), Path::new("linked/outside.png")),
            Err(BackgroundImageError::OutsideDataRoot { .. })
        ));
    }
}
