//! Background image support for `ProPresenter` slides.
//!
//! Adds a `BackgroundMedia` action to a cue, replicating what happens when
//! you drag an image onto a slide in `ProPresenter`.

use std::path::{Path, PathBuf};

use super::generated::rv_data::{self, action, media, url, Media, Url, Uuid};
use action::LayerType;

/// Background image category — determines which image file to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundCategory {
    /// Default slide background (most generated slides).
    Default,
    /// Sermon-related background.
    Sermon,
}

/// Resolve the background image path for a category.
///
/// Looks in `data/backgrounds/` for `default.(jpg|png)` or `sermon.(jpg|png)`.
/// Returns `None` if no matching file exists.
pub fn resolve_background_image(
    data_dir: &Path,
    category: BackgroundCategory,
) -> Option<PathBuf> {
    let base_name = match category {
        BackgroundCategory::Default => "default",
        BackgroundCategory::Sermon => "sermon",
    };
    let bg_dir = data_dir.join("backgrounds");
    for ext in &["jpg", "jpeg", "png", "tiff", "tif"] {
        let path = bg_dir.join(format!("{base_name}.{ext}"));
        if path.exists() {
            return Some(path);
        }
    }
    None
}

/// Create a `BackgroundMedia` action for an image file.
///
/// Replicates the protobuf structure that `ProPresenter` generates when
/// you drag an image onto a slide as a background.
pub fn make_background_media_action(image_path: &Path) -> rv_data::Action {
    let abs_path = image_path
        .canonicalize()
        .unwrap_or_else(|_| image_path.to_path_buf());
    let abs_string = format!("file://{}", abs_path.display());

    let media = Media {
        uuid: Some(Uuid {
            string: uuid::Uuid::new_v4().to_string(),
        }),
        url: Some(Url {
            platform: rv_data::url::Platform::Macos as i32,
            storage: Some(url::Storage::AbsoluteString(abs_string)),
            relative_file_path: None,
        }),
        metadata: None,
        type_properties: Some(media::TypeProperties::Image(
            media::ImageTypeProperties {
                drawing: None,
                file: None,
            },
        )),
    };

    rv_data::Action {
        uuid: Some(Uuid {
            string: uuid::Uuid::new_v4().to_string(),
        }),
        name: String::new(),
        label: None,
        delay_time: 0.0,
        old_type: None,
        is_enabled: true,
        layer_identification: None,
        duration: 0.0,
        r#type: action::ActionType::BackgroundMedia as i32,
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

/// Add a background image action to the first cue in a presentation.
///
/// Skips presentations that have no cues or are pre-service/final slide type.
pub fn add_background_to_first_cue(
    presentation: &mut rv_data::Presentation,
    image_path: &Path,
) {
    let Some(first_cue) = presentation.cues.first_mut() else {
        return;
    };
    let bg_action = make_background_media_action(image_path);
    first_cue.actions.push(bg_action);
}
