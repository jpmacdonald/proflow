//! Background image support for `ProPresenter` slides.
//!
//! Adds a `BackgroundMedia` action to a cue, replicating what happens when
//! you drag an image onto a slide in `ProPresenter`.

use std::io::Read;
use std::path::{Path, PathBuf};

use super::generated::rv_data::{
    self, action, graphics, media, url, AlphaType, FileProperties, Media, Url, Uuid,
};
use action::LayerType;

/// Failure to resolve a configured background image inside the project bundle.
#[derive(Debug, thiserror::Error)]
pub enum BackgroundImageError {
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
    /// The configured file did not have the expected image signature.
    #[error("background image content does not match its extension: {0}")]
    InvalidFormat(PathBuf),
}

/// Resolve and validate one project-relative background image.
///
/// The returned path is canonical, confined beneath `data_root`, non-empty,
/// and begins with the expected PNG, JPEG, or TIFF signature.
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

    let mut file = std::fs::File::open(&image).map_err(|source| BackgroundImageError::Image {
        path: image.clone(),
        source,
    })?;
    let mut header = [0_u8; 8];
    let header_len = file
        .read(&mut header)
        .map_err(|source| BackgroundImageError::Image {
            path: image.clone(),
            source,
        })?;
    let extension = image
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase);
    let valid = match extension.as_deref() {
        Some("png") => header_len >= 8 && header == [137, 80, 78, 71, 13, 10, 26, 10],
        Some("jpg" | "jpeg") => header_len >= 2 && header[..2] == [0xff, 0xd8],
        Some("tif" | "tiff") => {
            header_len >= 4 && matches!(&header[..4], [b'I', b'I', 42, 0] | [b'M', b'M', 0, 42])
        }
        _ => false,
    };
    if !valid {
        return Err(BackgroundImageError::InvalidFormat(image));
    }
    Ok(image)
}

/// Create the background-layer `Media` action `ProPresenter` writes for an image.
///
/// Replicates the protobuf structure that `ProPresenter` generates when
/// you drag an image onto a slide as a background.
pub fn make_background_media_action(image_path: &Path) -> rv_data::Action {
    make_background_media_action_with_dimensions(image_path, image_dimensions(image_path))
}

fn make_background_media_action_with_dimensions(
    image_path: &Path,
    dimensions: Option<(u32, u32)>,
) -> rv_data::Action {
    let abs_string = background_file_url(image_path);
    let relative_file_path = propresenter_relative_file_path(image_path);
    let media_url = Url {
        platform: rv_data::url::Platform::Macos as i32,
        storage: Some(url::Storage::AbsoluteString(abs_string)),
        relative_file_path,
    };
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
    let natural_size = dimensions.map(|(width, height)| graphics::Size {
        width: f64::from(width),
        height: f64::from(height),
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

/// Add a background image action to the first operator cue in a presentation.
pub fn add_background_to_first_cue(presentation: &mut rv_data::Presentation, image_path: &Path) {
    let cue_idx = first_operator_cue_index(presentation).unwrap_or(0);
    let Some(first_cue) = presentation.cues.get_mut(cue_idx) else {
        return;
    };
    ensure_background_on_cue(first_cue, image_path);
}

/// Add a background using the exact image bytes captured during preview.
pub(crate) fn add_reviewed_background_to_first_cue(
    presentation: &mut rv_data::Presentation,
    image_path: &Path,
    image_data: &[u8],
) {
    let cue_idx = first_operator_cue_index(presentation).unwrap_or(0);
    let Some(first_cue) = presentation.cues.get_mut(cue_idx) else {
        return;
    };
    ensure_reviewed_background_on_cue(first_cue, image_path, image_data);
}

/// Return the cue index `ProPresenter` operators see first for the selected/default
/// arrangement, falling back to the first grouped cue.
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

/// Ensure a cue has exactly one background image action for the given image.
///
/// Returns true when the cue was changed.
pub fn ensure_background_on_cue(cue: &mut rv_data::Cue, image_path: &Path) -> bool {
    ensure_background_on_cue_with_dimensions(cue, image_path, image_dimensions(image_path))
}

fn ensure_reviewed_background_on_cue(
    cue: &mut rv_data::Cue,
    image_path: &Path,
    image_data: &[u8],
) -> bool {
    ensure_background_on_cue_with_dimensions(
        cue,
        image_path,
        image_dimensions_from_bytes(image_data),
    )
}

fn ensure_background_on_cue_with_dimensions(
    cue: &mut rv_data::Cue,
    image_path: &Path,
    dimensions: Option<(u32, u32)>,
) -> bool {
    let desired_url = background_file_url(image_path);
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
    let bg_action = make_background_media_action_with_dimensions(image_path, dimensions);
    cue.actions.push(bg_action);
    true
}

fn background_file_url(image_path: &Path) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let abs_path = image_path
        .canonicalize()
        .unwrap_or_else(|_| image_path.to_path_buf());
    let mut encoded = String::with_capacity(abs_path.as_os_str().len() + 16);
    for byte in abs_path.to_string_lossy().bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'/'
            | b'-'
            | b'_'
            | b'.'
            | b'~'
            | b','
            | b'('
            | b')'
            | b'\'' => encoded.push(char::from(byte)),
            _ => {
                encoded.push('%');
                encoded.push(char::from(HEX[usize::from(byte >> 4)]));
                encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
            }
        }
    }
    format!("file://{encoded}")
}

fn propresenter_relative_file_path(image_path: &Path) -> Option<url::RelativeFilePath> {
    let root = std::env::var_os("PROPRESENTER_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join("Documents/ProPresenter")))?;
    let root = root.canonicalize().ok()?;
    let image = image_path.canonicalize().ok()?;
    let relative = image.strip_prefix(root).ok()?;
    Some(url::RelativeFilePath::Local(url::LocalRelativePath {
        root: url::local_relative_path::Root::Show as i32,
        path: relative.to_string_lossy().replace('\\', "/"),
    }))
}

fn image_dimensions(image_path: &Path) -> Option<(u32, u32)> {
    let data = std::fs::read(image_path).ok()?;
    image_dimensions_from_bytes(&data)
}

fn image_dimensions_from_bytes(data: &[u8]) -> Option<(u32, u32)> {
    png_dimensions(data).or_else(|| jpeg_dimensions(data))
}

fn png_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() < 24 || data[..8] != [137, 80, 78, 71, 13, 10, 26, 10] {
        return None;
    }
    Some((
        u32::from_be_bytes(data[16..20].try_into().ok()?),
        u32::from_be_bytes(data[20..24].try_into().ok()?),
    ))
}

fn jpeg_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    if !data.starts_with(&[0xff, 0xd8]) {
        return None;
    }
    let mut offset = 2;
    while offset + 4 <= data.len() {
        while data.get(offset) == Some(&0xff) {
            offset += 1;
        }
        let marker = *data.get(offset)?;
        offset += 1;
        if matches!(marker, 0xd8 | 0xd9) {
            continue;
        }
        let length = usize::from(u16::from_be_bytes(
            data.get(offset..offset + 2)?.try_into().ok()?,
        ));
        if length < 2 || offset + length > data.len() {
            return None;
        }
        if matches!(
            marker,
            0xc0 | 0xc1
                | 0xc2
                | 0xc3
                | 0xc5
                | 0xc6
                | 0xc7
                | 0xc9
                | 0xca
                | 0xcb
                | 0xcd
                | 0xce
                | 0xcf
        ) {
            let height = u32::from(u16::from_be_bytes(
                data.get(offset + 3..offset + 5)?.try_into().ok()?,
            ));
            let width = u32::from(u16::from_be_bytes(
                data.get(offset + 5..offset + 7)?.try_into().ok()?,
            ));
            return Some((width, height));
        }
        offset += length;
    }
    None
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
    use std::path::Path;

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

    fn minimal_png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = vec![
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, b'I', b'H', b'D', b'R',
        ];
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes
    }

    #[test]
    fn reviewed_background_uses_captured_dimensions_not_live_file_bytes() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("background.png");
        let reviewed = minimal_png(1920, 1080);
        std::fs::write(&path, minimal_png(1, 1)).expect("write changed live image");
        let mut cue = cue("cue");

        ensure_reviewed_background_on_cue(&mut cue, &path, &reviewed);

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
        assert_eq!((size.width, size.height), (1920.0, 1080.0));
    }

    #[test]
    fn adds_background_to_arrangement_first_cue_not_raw_first_cue() {
        let mut presentation = rv_data::Presentation {
            selected_arrangement: Some(rv_data::Uuid {
                string: "arr".to_string(),
            }),
            arrangements: vec![rv_data::presentation::Arrangement {
                uuid: Some(rv_data::Uuid {
                    string: "arr".to_string(),
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

        add_background_to_first_cue(&mut presentation, Path::new("/tmp/default.png"));

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
        let mut png = vec![137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13];
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&1920_u32.to_be_bytes());
        png.extend_from_slice(&1080_u32.to_be_bytes());
        std::fs::write(&image, png).expect("write png fixture");

        let action = make_background_media_action(&image);

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
                width: 1920.0,
                height: 1080.0,
            })
        );
        assert_eq!(drawing.alpha_type, AlphaType::Straight as i32);
        assert_eq!(
            image.file.as_ref().and_then(|file| file.local_url.as_ref()),
            element.url.as_ref()
        );
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
        std::fs::write(&image, [137, 80, 78, 71, 13, 10, 26, 10]).expect("png signature fixture");

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
