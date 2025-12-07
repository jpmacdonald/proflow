//! Conversion between data model and protobuf types.
//!
//! Converts between our high-level data model types and the
//! generated protobuf types for serialization/deserialization.

#![allow(dead_code)]

use uuid::Uuid;
use crate::propresenter::{
    data_model as dm,
    generated::rv_data::{
        self,
        graphics,
        presentation
    },
};

// Basic type conversions
impl From<dm::Color> for rv_data::Color {
    fn from(color: dm::Color) -> Self {
        rv_data::Color {
            red: color.red as f32,
            green: color.green as f32,
            blue: color.blue as f32,
            alpha: color.alpha as f32,
        }
    }
}

impl From<rv_data::Color> for dm::Color {
    fn from(color: rv_data::Color) -> Self {
        dm::Color {
            red: color.red as f64,
            green: color.green as f64,
            blue: color.blue as f64,
            alpha: color.alpha as f64,
        }
    }
}

impl From<Uuid> for rv_data::Uuid {
    fn from(uuid: Uuid) -> Self {
        rv_data::Uuid {
            string: uuid.to_string(),
        }
    }
}

// Geometry conversions
impl From<dm::Point> for graphics::Point {
    fn from(point: dm::Point) -> Self {
        graphics::Point {
            x: point.x,
            y: point.y,
        }
    }
}

impl From<graphics::Point> for dm::Point {
    fn from(point: graphics::Point) -> Self {
        dm::Point {
            x: point.x,
            y: point.y,
        }
    }
}

impl From<dm::Size> for graphics::Size {
    fn from(size: dm::Size) -> Self {
        graphics::Size {
            width: size.width,
            height: size.height,
        }
    }
}

impl From<graphics::Size> for dm::Size {
    fn from(size: graphics::Size) -> Self {
        dm::Size {
            width: size.width,
            height: size.height,
        }
    }
}

impl From<dm::Rect> for graphics::Rect {
    fn from(rect: dm::Rect) -> Self {
        graphics::Rect {
            origin: Some(rect.origin.into()),
            size: Some(rect.size.into()),
        }
    }
}

impl From<graphics::Rect> for dm::Rect {
    fn from(rect: graphics::Rect) -> Self {
        dm::Rect {
            origin: rect.origin.unwrap_or_else(|| graphics::Point { x: 0.0, y: 0.0 }).into(),
            size: rect.size.unwrap_or_else(|| graphics::Size { width: 0.0, height: 0.0 }).into(),
        }
    }
}

// CCLI info conversions
impl From<dm::CCLIInfo> for presentation::Ccli {
    fn from(ccli: dm::CCLIInfo) -> Self {
        presentation::Ccli {
            author: ccli.author,
            artist_credits: ccli.artist_credits,
            song_title: ccli.song_title,
            publisher: ccli.publisher,
            copyright_year: ccli.copyright_year,
            song_number: ccli.song_number,
            display: ccli.display,
            album: ccli.album,
            artwork: Vec::new(), // No artwork in data model
        }
    }
}

impl From<presentation::Ccli> for dm::CCLIInfo {
    fn from(ccli: presentation::Ccli) -> Self {
        dm::CCLIInfo {
            author: ccli.author,
            artist_credits: ccli.artist_credits,
            song_title: ccli.song_title,
            publisher: ccli.publisher,
            copyright_year: ccli.copyright_year,
            song_number: ccli.song_number,
            display: ccli.display,
            album: ccli.album,
        }
    }
}

// Range conversions
impl From<dm::Range> for rv_data::IntRange {
    fn from(range: dm::Range) -> Self {
        rv_data::IntRange {
            start: range.start as i32,
            end: range.end as i32,
        }
    }
}

impl From<rv_data::IntRange> for dm::Range {
    fn from(range: rv_data::IntRange) -> Self {
        dm::Range {
            start: range.start as u32,
            end: range.end as u32,
        }
    }
}

// Bible reference conversions
impl From<dm::BibleReference> for presentation::BibleReference {
    fn from(bible_ref: dm::BibleReference) -> Self {
        presentation::BibleReference {
            book_index: bible_ref.book_index,
            book_name: bible_ref.book_name,
            book_key: bible_ref.book_key,
            chapter_range: Some(bible_ref.chapter_range.into()),
            verse_range: Some(bible_ref.verse_range.into()),
            translation_name: bible_ref.translation_name,
            translation_display_abbreviation: bible_ref.translation_display_abbreviation,
            translation_internal_abbreviation: bible_ref.translation_internal_abbreviation,
        }
    }
}

impl From<presentation::BibleReference> for dm::BibleReference {
    fn from(bible_ref: presentation::BibleReference) -> Self {
        dm::BibleReference {
            book_index: bible_ref.book_index,
            book_name: bible_ref.book_name,
            book_key: bible_ref.book_key,
            chapter_range: bible_ref.chapter_range.unwrap_or_else(|| rv_data::IntRange { start: 1, end: 1 }).into(),
            verse_range: bible_ref.verse_range.unwrap_or_else(|| rv_data::IntRange { start: 1, end: 1 }).into(),
            translation_name: bible_ref.translation_name,
            translation_display_abbreviation: bible_ref.translation_display_abbreviation,
            translation_internal_abbreviation: bible_ref.translation_internal_abbreviation,
        }
    }
}

// Timeline conversions
impl From<dm::Timeline> for presentation::Timeline {
    fn from(timeline: dm::Timeline) -> Self {
        presentation::Timeline {
            duration: timeline.duration,
            r#loop: timeline.loop_enabled,
            timecode_enable: timeline.timecode_enabled,
            timecode_offset: timeline.timecode_offset,
            cues: Vec::new(), // Legacy cues field, use cues_v2 instead
            cues_v2: timeline.cues.into_iter().map(|cue| {
                presentation::timeline::Cue {
                    trigger_time: cue.trigger_time,
                    name: cue.name,
                    trigger_info: Some(presentation::timeline::cue::TriggerInfo::CueId(cue.uuid.into())),
                }
            }).collect(),
            audio_action: None, // Not supported in data model
        }
    }
}

impl From<presentation::Timeline> for dm::Timeline {
    fn from(timeline: presentation::Timeline) -> Self {
        dm::Timeline {
            duration: timeline.duration,
            loop_enabled: timeline.r#loop,
            timecode_enabled: timeline.timecode_enable,
            timecode_offset: timeline.timecode_offset,
            cues: timeline.cues_v2.into_iter().map(|cue| {
                let uuid = match &cue.trigger_info {
                    Some(presentation::timeline::cue::TriggerInfo::CueId(uuid)) => uuid.string.parse().unwrap_or_else(|_| Uuid::new_v4()),
                    Some(presentation::timeline::cue::TriggerInfo::Action(action)) => {
                        // Convert the action to our data model
                        match rv_data::action::ActionType::try_from(action.r#type).unwrap_or(rv_data::action::ActionType::Unknown) {
                            rv_data::action::ActionType::Clear => {
                                if let Some(rv_data::action::ActionTypeData::Clear(_)) = action.action_type_data {
                                    Uuid::new_v4() // Generate new UUID for clear action
                                } else {
                                    Uuid::new_v4()
                                }
                            }
                            rv_data::action::ActionType::AudienceLook => {
                                if let Some(rv_data::action::ActionTypeData::AudienceLook(look)) = &action.action_type_data {
                                    if let Some(identification) = &look.identification {
                                        if let Some(uuid) = &identification.parameter_uuid {
                                            uuid.string.parse().unwrap_or_else(|_| Uuid::new_v4())
                                        } else {
                                            Uuid::new_v4()
                                        }
                                    } else {
                                        Uuid::new_v4()
                                    }
                                } else {
                                    Uuid::new_v4()
                                }
                            }
                            _ => Uuid::new_v4(),
                        }
                    }
                    None => Uuid::new_v4(),
                };

                let action = match &cue.trigger_info {
                    Some(presentation::timeline::cue::TriggerInfo::Action(action)) => {
                        match rv_data::action::ActionType::try_from(action.r#type).unwrap_or(rv_data::action::ActionType::Unknown) {
                            rv_data::action::ActionType::Clear => {
                                if let Some(rv_data::action::ActionTypeData::Clear(clear)) = &action.action_type_data {
                                    Some(dm::Action::Clear {
                                        target_layer: clear.target_layer,
                                        content_destination: match rv_data::action::ContentDestination::try_from(clear.content_destination).unwrap_or(rv_data::action::ContentDestination::Global) {
                                            rv_data::action::ContentDestination::Global => dm::ContentDestination::Global,
                                            _ => dm::ContentDestination::Global,
                                        },
                                    })
                                } else {
                                    None
                                }
                            }
                            rv_data::action::ActionType::AudienceLook => {
                                if let Some(rv_data::action::ActionTypeData::AudienceLook(look)) = &action.action_type_data {
                                    if let Some(identification) = &look.identification {
                                        Some(dm::Action::AudienceLook {
                                            name: action.name.clone(),
                                            uuid: identification.parameter_uuid.as_ref().map(|u| u.string.parse().unwrap_or_else(|_| Uuid::new_v4())).unwrap_or_else(Uuid::new_v4),
                                            parameter_name: identification.parameter_name.clone(),
                                        })
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            }
                            _ => None,
                        }
                    }
                    _ => None,
                };

                dm::TimelineCue {
                    trigger_time: cue.trigger_time,
                    name: cue.name,
                    uuid,
                    action: action.unwrap_or_else(|| dm::Action::Clear {
                        target_layer: 0,
                        content_destination: dm::ContentDestination::Global,
                    }),
                }
            }).collect(),
        }
    }
}

// Arrangement conversions
impl From<dm::Arrangement> for presentation::Arrangement {
    fn from(arrangement: dm::Arrangement) -> Self {
        presentation::Arrangement {
            uuid: Some(arrangement.uuid.into()),
            name: arrangement.name,
            group_identifiers: arrangement.group_identifiers.into_iter().map(|uuid| uuid.into()).collect(),
        }
    }
}

impl From<presentation::Arrangement> for dm::Arrangement {
    fn from(arrangement: presentation::Arrangement) -> Self {
        dm::Arrangement {
            uuid: arrangement.uuid.map(|uuid| uuid.string.parse().unwrap_or_else(|_| Uuid::new_v4())).unwrap_or_else(Uuid::new_v4),
            name: arrangement.name,
            group_identifiers: arrangement.group_identifiers.into_iter().map(|uuid| uuid.string.parse().unwrap_or_else(|_| Uuid::new_v4())).collect(),
        }
    }
}

// Presentation conversions
impl From<dm::Presentation> for rv_data::Presentation {
    fn from(presentation: dm::Presentation) -> Self {
        let background = Some(rv_data::Background {
            is_enabled: false,
            fill: Some(rv_data::background::Fill::Color(rv_data::Color {
                red: 1.0,
                green: 1.0,
                blue: 1.0,
                alpha: 0.0,
            })),
        });

        let slide_show = presentation.slide_show.map(|s| {
            presentation::SlideShow::SlideShowDuration(s.slide_duration)
        });

        // Create rv_data::Cues from our data model
        let cues = presentation.cues.into_iter().map(|cue| {
            let actions = cue.actions.into_iter().map(|action| {
                match action {
                    dm::Action::Slide { uuid, name, slide, delay_time, duration, enabled, layer_identification } => {
                        // Create rv_data::Action for the slide
                        rv_data::Action {
                            uuid: Some(rv_data::Uuid { string: uuid.to_string() }),
                            name,
                            label: None,
                            delay_time,
                            old_type: None,
                            is_enabled: enabled,
                            layer_identification: layer_identification.map(|li| rv_data::action::LayerIdentification {
                                uuid: Some(rv_data::Uuid { string: li.uuid.to_string() }),
                                name: li.name,
                            }),
                            duration,
                            r#type: rv_data::action::ActionType::PresentationSlide as i32,
                            action_type_data: Some(rv_data::action::ActionTypeData::Slide(
                                rv_data::action::SlideType {
                                    slide: Some(rv_data::action::slide_type::Slide::Presentation(convert_slide_to_rv_data(slide)))
                                }
                            )),
                        }
                    },
                    dm::Action::Clear { target_layer, content_destination: _ } => {
                        rv_data::Action {
                            uuid: Some(rv_data::Uuid { string: Uuid::new_v4().to_string() }),
                            name: "Clear".to_string(),
                            label: None,
                            delay_time: 0.0,
                            old_type: None,
                            is_enabled: true,
                            layer_identification: None,
                            duration: 0.0,
                            r#type: rv_data::action::ActionType::Clear as i32,
                            action_type_data: Some(rv_data::action::ActionTypeData::Clear(rv_data::action::ClearType {
                                target_layer,
                                content_destination: rv_data::action::ContentDestination::Global as i32,
                            })),
                        }
                    },
                    dm::Action::AudienceLook { name, uuid, parameter_name } => {
                        rv_data::Action {
                            uuid: Some(rv_data::Uuid { string: uuid.to_string() }),
                            name,
                            label: None,
                            delay_time: 0.0,
                            old_type: None,
                            is_enabled: true,
                            layer_identification: None,
                            duration: 0.0,
                            r#type: rv_data::action::ActionType::AudienceLook as i32,
                            action_type_data: Some(rv_data::action::ActionTypeData::AudienceLook(
                                rv_data::action::AudienceLookType {
                                    identification: Some(rv_data::CollectionElementType {
                                        parameter_uuid: Some(rv_data::Uuid { string: uuid.to_string() }),
                                        parameter_name,
                                        parent_collection: None,
                                    }),
                                }
                            )),
                        }
                    },
                    dm::Action::Media {
                        uuid,
                        name,
                        source,
                        fit,
                        opacity: _,
                        volume,
                        delay_time,
                        duration,
                        enabled,
                    } => {
                        let mut action = rv_data::Action::default();
                        action.uuid = Some(uuid.into());
                        action.name = name;
                        action.delay_time = delay_time;
                        action.duration = duration;
                        action.is_enabled = enabled;
                        action.r#type = rv_data::action::ActionType::Media as i32;
                        action.action_type_data = Some(rv_data::action::ActionTypeData::Media(
                            rv_data::action::MediaType {
                                transition_duration: 0.0,
                                selected_effect_preset_uuid: None,
                                transition: None,
                                effects: vec![],
                                element: Some(rv_data::Media {
                                    uuid: Some(uuid.into()),
                                    url: None,
                                    metadata: None,
                                    type_properties: Some(match source {
                                        dm::MediaSource::File(path) => rv_data::media::TypeProperties::Image(
                                            rv_data::media::ImageTypeProperties {
                                                drawing: Some(rv_data::media::DrawingProperties {
                                                    scale_behavior: match fit {
                                                        dm::MediaFit::Scale => rv_data::media::ScaleBehavior::Fit as i32,
                                                        dm::MediaFit::Stretch => rv_data::media::ScaleBehavior::Stretch as i32,
                                                        dm::MediaFit::Center => rv_data::media::ScaleBehavior::Fill as i32,
                                                    },
                                                    is_blurred: false,
                                                    scale_alignment: rv_data::media::ScaleAlignment::MiddleCenter as i32,
                                                    flipped_horizontally: false,
                                                    flipped_vertically: false,
                                                    natural_size: None,
                                                    custom_image_rotation: 0.0,
                                                    custom_image_bounds: None,
                                                    custom_image_aspect_locked: true,
                                                    alpha_inverted: false,
                                                    native_rotation: rv_data::media::drawing_properties::NativeRotationType::RotateStandard as i32,
                                                    selected_effect_preset_uuid: None,
                                                    effects: vec![],
                                                    crop_enable: false,
                                                    crop_insets: Some(rv_data::graphics::EdgeInsets {
                                                        left: 0.0,
                                                        right: 0.0,
                                                        top: 0.0,
                                                        bottom: 0.0,
                                                    }),
                                                    alpha_type: rv_data::AlphaType::Straight as i32,
                                                }),
                                                file: Some(rv_data::FileProperties {
                                                    local_url: Some(rv_data::Url {
                                                        platform: rv_data::url::Platform::Macos as i32,
                                                        storage: Some(rv_data::url::Storage::AbsoluteString(
                                                            path.to_string_lossy().to_string(),
                                                        )),
                                                        relative_file_path: None,
                                                    }),
                                                    remote_properties: None,
                                                }),
                                            }
                                        ),
                                        dm::MediaSource::VideoInput { input_id: _, input_name: _ } => rv_data::media::TypeProperties::LiveVideo(
                                            rv_data::media::LiveVideoTypeProperties {
                                                drawing: Some(rv_data::media::DrawingProperties {
                                                    scale_behavior: match fit {
                                                        dm::MediaFit::Scale => rv_data::media::ScaleBehavior::Fit as i32,
                                                        dm::MediaFit::Stretch => rv_data::media::ScaleBehavior::Stretch as i32,
                                                        dm::MediaFit::Center => rv_data::media::ScaleBehavior::Fill as i32,
                                                    },
                                                    is_blurred: false,
                                                    scale_alignment: rv_data::media::ScaleAlignment::MiddleCenter as i32,
                                                    flipped_horizontally: false,
                                                    flipped_vertically: false,
                                                    natural_size: None,
                                                    custom_image_rotation: 0.0,
                                                    custom_image_bounds: None,
                                                    custom_image_aspect_locked: true,
                                                    alpha_inverted: false,
                                                    native_rotation: rv_data::media::drawing_properties::NativeRotationType::RotateStandard as i32,
                                                    selected_effect_preset_uuid: None,
                                                    effects: vec![],
                                                    crop_enable: false,
                                                    crop_insets: Some(rv_data::graphics::EdgeInsets {
                                                        left: 0.0,
                                                        right: 0.0,
                                                        top: 0.0,
                                                        bottom: 0.0,
                                                    }),
                                                    alpha_type: rv_data::AlphaType::Straight as i32,
                                                }),
                                                audio: Some(rv_data::media::AudioProperties {
                                                    volume: volume as f64,
                                                    audio_channels: vec![],
                                                    is_custom_mapping: false,
                                                }),
                                                live_video: Some(rv_data::media::LiveVideoProperties {
                                                    video_device: None,
                                                    audio_device: None,
                                                    live_video_index: 0,
                                                }),
                                            }
                                        ),
                                        dm::MediaSource::Url(url) => rv_data::media::TypeProperties::WebContent(
                                            rv_data::media::WebContentTypeProperties {
                                                drawing: Some(rv_data::media::DrawingProperties {
                                                    scale_behavior: match fit {
                                                        dm::MediaFit::Scale => rv_data::media::ScaleBehavior::Fit as i32,
                                                        dm::MediaFit::Stretch => rv_data::media::ScaleBehavior::Stretch as i32,
                                                        dm::MediaFit::Center => rv_data::media::ScaleBehavior::Fill as i32,
                                                    },
                                                    is_blurred: false,
                                                    scale_alignment: rv_data::media::ScaleAlignment::MiddleCenter as i32,
                                                    flipped_horizontally: false,
                                                    flipped_vertically: false,
                                                    natural_size: None,
                                                    custom_image_rotation: 0.0,
                                                    custom_image_bounds: None,
                                                    custom_image_aspect_locked: true,
                                                    alpha_inverted: false,
                                                    native_rotation: rv_data::media::drawing_properties::NativeRotationType::RotateStandard as i32,
                                                    selected_effect_preset_uuid: None,
                                                    effects: vec![],
                                                    crop_enable: false,
                                                    crop_insets: Some(rv_data::graphics::EdgeInsets {
                                                        left: 0.0,
                                                        right: 0.0,
                                                        top: 0.0,
                                                        bottom: 0.0,
                                                    }),
                                                    alpha_type: rv_data::AlphaType::Straight as i32,
                                                }),
                                                url: Some(rv_data::Url {
                                                    platform: rv_data::url::Platform::Web as i32,
                                                    storage: Some(rv_data::url::Storage::AbsoluteString(url)),
                                                    relative_file_path: None,
                                                }),
                                            }
                                        ),
                                    }),
                                }),
                                layer_type: rv_data::action::LayerType::Background as i32,
                                always_retrigger: false,
                                markers: vec![],
                                media_type: None,
                            }
                        ));
                        action
                    },
                    dm::Action::Macro { uuid, name, parameter_uuid, parameter_name, parent_collection_uuid, parent_collection_name, delay_time, duration, enabled } => {
                        let mut action = rv_data::Action::default();
                        action.uuid = Some(uuid.into());
                        action.name = name;
                        action.delay_time = delay_time;
                        action.duration = duration;
                        action.is_enabled = enabled;
                        action.r#type = rv_data::action::ActionType::Macro as i32;
                        action.action_type_data = Some(rv_data::action::ActionTypeData::Macro(
                            rv_data::action::MacroType {
                                identification: Some(rv_data::CollectionElementType {
                                    parameter_uuid: Some(rv_data::Uuid { string: parameter_uuid.to_string() }),
                                    parameter_name,
                                    parent_collection: parent_collection_uuid.map(|uuid| {
                                        Box::new(rv_data::CollectionElementType {
                                            parameter_uuid: Some(rv_data::Uuid { string: uuid.to_string() }),
                                            parameter_name: parent_collection_name.unwrap_or_else(|| "Default Collection".to_string()),
                                            parent_collection: None,
                                        })
                                    }),
                                }),
                            }
                        ));
                        action
                    },
                }
            }).collect();

            rv_data::Cue {
                uuid: Some(rv_data::Uuid { string: cue.uuid.to_string() }),
                name: cue.name,
                completion_target_type: match cue.completion_target_type {
                    dm::CompletionTargetType::None => rv_data::cue::CompletionTargetType::None as i32,
                    dm::CompletionTargetType::Next => rv_data::cue::CompletionTargetType::Next as i32,
                    dm::CompletionTargetType::Random => rv_data::cue::CompletionTargetType::Random as i32,
                    dm::CompletionTargetType::Cue => rv_data::cue::CompletionTargetType::Cue as i32,
                    dm::CompletionTargetType::First => rv_data::cue::CompletionTargetType::First as i32,
                },
                completion_target_uuid: cue.completion_target_uuid.map(|uuid| rv_data::Uuid { string: uuid.to_string() }),
                completion_action_type: match cue.completion_action_type {
                    dm::CompletionActionType::First => rv_data::cue::CompletionActionType::First as i32,
                    dm::CompletionActionType::Last => rv_data::cue::CompletionActionType::Last as i32,
                    dm::CompletionActionType::AfterAction => rv_data::cue::CompletionActionType::AfterAction as i32,
                    dm::CompletionActionType::AfterTime => rv_data::cue::CompletionActionType::AfterTime as i32,
                },
                completion_action_uuid: cue.completion_action_uuid.map(|uuid| rv_data::Uuid { string: uuid.to_string() }),
                trigger_time: Some(rv_data::cue::TimecodeTime { time: 0.0 }),
                hot_key: cue.hot_key.map(|hk| {
                    rv_data::HotKey {
                        code: hk.key_code as i32,
                        control_identifier: "".to_string(),
                    }
                }),
                actions,
                pending_imports: Vec::new(),
                is_enabled: cue.enabled,
                completion_time: cue.completion_time,
            }
        }).collect();

        // Create rv_data::CueGroups from our data model
        let cue_groups = presentation.cue_groups.into_iter().map(|cue_group| {
            let group = rv_data::Group {
                uuid: Some(rv_data::Uuid { string: cue_group.group.uuid.to_string() }),
                name: cue_group.group.name.clone(),
                color: Some(rv_data::Color {
                    red: cue_group.group.color.red as f32,
                    green: cue_group.group.color.green as f32,
                    blue: cue_group.group.color.blue as f32,
                    alpha: cue_group.group.color.alpha as f32,
                }),
                hot_key: match cue_group.group.hot_key {
                    Some(hk) => Some(rv_data::HotKey {
                        code: hk.key_code as i32,
                        control_identifier: "".to_string(),
                    }),
                    None => None,
                },
                application_group_identifier: if cue_group.group.application_group_identifier.is_empty() {
                    None
                } else {
                    Some(rv_data::Uuid { string: cue_group.group.application_group_identifier.clone() })
                },
                application_group_name: if cue_group.group.application_group_identifier.is_empty() {
                    "".to_string()
                } else {
                    cue_group.group.name.clone()
                },
            };

            rv_data::presentation::CueGroup {
                group: Some(group),
                cue_identifiers: cue_group.cue_identifiers.into_iter().map(|uuid| rv_data::Uuid { string: uuid.to_string() }).collect(),
            }
        }).collect();

        rv_data::Presentation {
            application_info: Some(rv_data::ApplicationInfo {
                platform: rv_data::application_info::Platform::Macos as i32,
                platform_version: Some(rv_data::Version {
                    major_version: 14,
                    minor_version: 0,
                    patch_version: 0,
                    build: "14A309".to_string(),
                }),
                application: rv_data::application_info::Application::Propresenter as i32,
                application_version: Some(rv_data::Version {
                    major_version: 7,
                    minor_version: 14,
                    patch_version: 0,
                    build: "7.14.0".to_string(),
                }),
            }),
            uuid: Some(rv_data::Uuid { string: presentation.uuid.to_string() }),
            name: presentation.name,
            last_date_used: None,
            last_modified_date: Some(rv_data::Timestamp {
                seconds: 1732047766,
                nanos: 0,
            }),
            category: presentation.category,
            notes: presentation.notes,
            background,
            chord_chart: None,
            selected_arrangement: Some(rv_data::Uuid { 
                string: "a27370a2-9f2c-4766-bcd5-28c6454f9c68".to_string() 
            }),
            arrangements: presentation.arrangements.into_iter().map(|arr| arr.into()).collect(),
            cue_groups,
            cues,
            ccli: presentation.ccli.map(|ccli| ccli.into()),
            bible_reference: presentation.bible_reference.map(|bible_ref| bible_ref.into()),
            timeline: Some(rv_data::presentation::Timeline {
                audio_action: None,
                cues: Vec::new(),
                cues_v2: Vec::new(),
                duration: presentation.timeline.as_ref().map_or(300.0, |t| t.duration),
                r#loop: presentation.timeline.as_ref().map_or(false, |t| t.loop_enabled),
                timecode_enable: presentation.timeline.as_ref().map_or(false, |t| t.timecode_enabled),
                timecode_offset: presentation.timeline.as_ref().map_or(0.0, |t| t.timecode_offset),
            }),
            transition: None,
            content_destination: rv_data::action::ContentDestination::Global as i32,
            multi_tracks_licensing: None,
            music_key: presentation.music_key,
            music: None,
            slide_show,
        }
    }
}

// Helper function to convert a Slide to rv_data::PresentationSlide
fn convert_slide_to_rv_data(slide: dm::Slide) -> rv_data::PresentationSlide {
    use rv_data::graphics;
    use crate::propresenter::rtf::text_to_rtf_bytes;

    let mut elements = Vec::new();
    let mut element_uuids = Vec::new();

    for element in slide.base.elements {
        if let dm::Element::Text(text_element) = element {
            let element_uuid = Uuid::new_v4().to_string();
            element_uuids.push(rv_data::Uuid { string: element_uuid.clone() });
            
            // Use proper RTF conversion that handles superscripts
            let rtf_data = text_to_rtf_bytes(&text_element.content);

            let text = graphics::Text {
                attributes: Some(graphics::text::Attributes {
                    font: Some(rv_data::Font {
                        name: text_element.font.name.clone(),
                        size: text_element.font.size,
                        italic: text_element.font.italic,
                        bold: text_element.font.bold,
                        family: text_element.font.family.clone(),
                        face: text_element.font.face.clone(),
                    }),
                    capitalization: graphics::text::attributes::Capitalization::None as i32,
                    underline_style: None,
                    underline_color: None,
                    paragraph_style: Some(text_element.paragraph_style.into()),
                    kerning: 0.0,
                    superscript: 0,
                    strikethrough_style: None,
                    strikethrough_color: None,
                    stroke_width: 0.0,
                    stroke_color: Some(rv_data::Color {
                        red: 1.0,
                        green: 1.0,
                        blue: 1.0,
                        alpha: 1.0,
                    }),
                    custom_attributes: text_element.custom_attributes.into_iter().map(|attr| attr.into()).collect(),
                    background_color: None,
                    ligature_style: graphics::text::attributes::LigatureStyle::Default as i32,
                    fill: Some(graphics::text::attributes::Fill::TextSolidFill(text_element.color.into())),
                }),
                shadow: text_element.shadow.map(|s| s.into()),
                rtf_data,
                vertical_alignment: graphics::text::VerticalAlignment::Middle as i32,
                scale_behavior: graphics::text::ScaleBehavior::ScaleFontUpDown as i32,
                margins: Some(graphics::EdgeInsets {
                    left: 0.0,
                    right: 0.0,
                    top: 0.0,
                    bottom: 0.0,
                }),
                is_superscript_standardized: true,
                transform: graphics::text::Transform::None as i32,
                transform_delimiter: "  •  ".to_string(),
                chord_pro: Some(graphics::text::ChordPro {
                    enabled: false,
                    notation: graphics::text::chord_pro::Notation::Chords as i32,
                    color: Some(rv_data::Color {
                        red: 1.0,
                        green: 1.0,
                        blue: 1.0,
                        alpha: 1.0,
                    }),
                }),
                alternate_texts: Vec::new(),
            };

            let _text_element_color = text_element.color;
            let text_element_shadow = text_element.shadow;

            let graphics_element = rv_data::graphics::Element {
                uuid: Some(rv_data::Uuid { string: element_uuid.clone() }),
                name: "Text Element".to_string(),
                bounds: Some(rv_data::graphics::Rect {
                    origin: Some(rv_data::graphics::Point {
                        x: text_element.bounds.as_ref().map_or(0.0, |b| b.origin.x),
                        y: text_element.bounds.as_ref().map_or(0.0, |b| b.origin.y),
                    }),
                    size: Some(rv_data::graphics::Size {
                        width: text_element.bounds.as_ref().map_or(1920.0, |b| b.size.width),
                        height: text_element.bounds.as_ref().map_or(1080.0, |b| b.size.height),
                    }),
                }),
                rotation: 0.0,
                opacity: 1.0,
                locked: false,
                aspect_ratio_locked: false,
                path: Some(rv_data::graphics::Path {
                    closed: true,
                    points: vec![
                        rv_data::graphics::path::BezierPoint {
                            point: Some(rv_data::graphics::Point { x: 0.0, y: 0.0 }),
                            q0: Some(rv_data::graphics::Point { x: 0.0, y: 0.0 }),
                            q1: Some(rv_data::graphics::Point { x: 0.0, y: 0.0 }),
                            curved: false,
                        },
                        rv_data::graphics::path::BezierPoint {
                            point: Some(rv_data::graphics::Point { x: 1.0, y: 0.0 }),
                            q0: Some(rv_data::graphics::Point { x: 1.0, y: 0.0 }),
                            q1: Some(rv_data::graphics::Point { x: 1.0, y: 0.0 }),
                            curved: false,
                        },
                        rv_data::graphics::path::BezierPoint {
                            point: Some(rv_data::graphics::Point { x: 1.0, y: 1.0 }),
                            q0: Some(rv_data::graphics::Point { x: 1.0, y: 1.0 }),
                            q1: Some(rv_data::graphics::Point { x: 1.0, y: 1.0 }),
                            curved: false,
                        },
                        rv_data::graphics::path::BezierPoint {
                            point: Some(rv_data::graphics::Point { x: 0.0, y: 1.0 }),
                            q0: Some(rv_data::graphics::Point { x: 0.0, y: 1.0 }),
                            q1: Some(rv_data::graphics::Point { x: 0.0, y: 1.0 }),
                            curved: false,
                        },
                    ],
                    shape: Some(rv_data::graphics::path::Shape {
                        r#type: rv_data::graphics::path::shape::Type::Rectangle as i32,
                        additional_data: None,
                    }),
                }),
                fill: Some(rv_data::graphics::Fill {
                    enable: false,
                    fill_type: Some(rv_data::graphics::fill::FillType::Color(rv_data::Color {
                        red: text_element.color.red as f32,
                        green: text_element.color.green as f32,
                        blue: text_element.color.blue as f32,
                        alpha: 1.0,
                    })),
                }),
                stroke: Some(graphics::Stroke {
                    enable: false,
                    width: 3.0,
                    color: Some(rv_data::Color {
                        red: 1.0,
                        green: 1.0,
                        blue: 1.0,
                        alpha: 1.0,
                    }),
                    pattern: vec![],
                    style: 0, // SolidLine
                }),
                shadow: text_element_shadow.map(|s| s.into()),
                feather: Some(rv_data::graphics::Feather {
                    style: rv_data::graphics::feather::Style::Inside as i32,
                    radius: 0.05,
                    enable: false,
                }),
                text: Some(text),
                flip_mode: graphics::element::FlipMode::None as i32,
                hidden: false,
                mask: Some(graphics::element::Mask::TextLineMask(graphics::text::LineFillMask {
                    enabled: true,
                    height_offset: 0.0,
                    vertical_offset: 0.0,
                    mask_style: graphics::text::line_fill_mask::LineMaskStyle::FullWidth as i32,
                    width_offset: 0.0,
                    horizontal_offset: 0.0,
                })),
            };
            elements.push(rv_data::slide::Element {
                element: Some(graphics_element),
                build_in: None,
                build_out: None,
                info: 3, // Match the original value
                reveal_type: rv_data::slide::element::TextRevealType::None as i32,
                data_links: Vec::new(),
                child_builds: Vec::new(),
                reveal_from_index: 0,
                text_scroller: text_element.text_scroller.map(|ts| rv_data::slide::element::TextScroller {
                    should_scroll: ts.should_scroll,
                    scroll_rate: ts.scroll_rate,
                    should_repeat: ts.should_repeat,
                    repeat_distance: ts.repeat_distance,
                    scrolling_direction: match ts.scrolling_direction {
                        dm::ScrollDirection::Left => rv_data::slide::element::text_scroller::Direction::Left as i32,
                        dm::ScrollDirection::Right => rv_data::slide::element::text_scroller::Direction::Right as i32,
                        dm::ScrollDirection::Up => rv_data::slide::element::text_scroller::Direction::Up as i32,
                        dm::ScrollDirection::Down => rv_data::slide::element::text_scroller::Direction::Down as i32,
                    },
                    starts_off_screen: ts.starts_off_screen,
                    fade_left: ts.fade_left,
                    fade_right: ts.fade_right,
                }),
            });
        }
    }

    let element_build_order = if elements.is_empty() {
        Vec::new()
    } else {
        element_uuids
    };

    let base_slide = rv_data::Slide {
        elements,
        element_build_order,
        guidelines: slide.base.guidelines.into_iter().map(|g| rv_data::AlignmentGuide {
            uuid: Some(rv_data::Uuid { string: Uuid::new_v4().to_string() }),
            orientation: match g.orientation {
                dm::GuidelineOrientation::Horizontal => rv_data::alignment_guide::GuidelineOrientation::Horizontal as i32,
                dm::GuidelineOrientation::Vertical => rv_data::alignment_guide::GuidelineOrientation::Vertical as i32,
            },
            location: g.position,
        }).collect(),
        draws_background_color: slide.base.draws_background_color,
        background_color: slide.base.background_color.map(|c| c.into()),
        size: Some(slide.base.size.into()),
        uuid: Some(rv_data::Uuid { string: slide.base.uuid.to_string() }),
    };

    rv_data::PresentationSlide {
        base_slide: Some(base_slide),
        notes: slide.notes.map(|note_text| {
            rv_data::presentation_slide::Notes {
                rtf_data: note_text.into_bytes(),
                attributes: None,
            }
        }),
        template_guidelines: slide.template_guidelines.into_iter().map(|g| rv_data::AlignmentGuide {
            uuid: Some(rv_data::Uuid { string: Uuid::new_v4().to_string() }),
            orientation: match g.orientation {
                dm::GuidelineOrientation::Horizontal => rv_data::alignment_guide::GuidelineOrientation::Horizontal as i32,
                dm::GuidelineOrientation::Vertical => rv_data::alignment_guide::GuidelineOrientation::Vertical as i32,
            },
            location: g.position,
        }).collect(),
        chord_chart: slide.chord_chart.map(|url| rv_data::Url {
            platform: rv_data::url::Platform::Macos as i32,
            storage: Some(rv_data::url::Storage::AbsoluteString(url.url)),
            relative_file_path: None,
        }),
        transition: slide.transition.map(|t| rv_data::Transition {
            duration: t.duration,
            favorite_uuid: None,
            effect: Some(rv_data::Effect {
                uuid: Some(rv_data::Uuid { string: Uuid::new_v4().to_string() }),
                enabled: true,
                name: "Dissolve".to_string(),
                render_id: "com.renewedvision.transition.dissolve".to_string(),
                behavior_description: "Dissolve transition".to_string(),
                category: "Standard".to_string(),
                variables: Vec::new(),
            }),
        }),
    }
}

// Implement From<dm::Shadow> for rv_data::graphics::Shadow
impl From<dm::Shadow> for rv_data::graphics::Shadow {
    fn from(shadow: dm::Shadow) -> Self {
        rv_data::graphics::Shadow {
            style: rv_data::graphics::shadow::Style::Drop as i32,
            angle: shadow.angle,
            offset: shadow.offset.x,  // Using x component as offset
            radius: shadow.radius,
            color: Some(shadow.color.into()),
            opacity: shadow.opacity as f64,
            enable: shadow.enable,
        }
    }
}

// Implement From<dm::ParagraphStyle> for rv_data::graphics::text::attributes::Paragraph
impl From<dm::ParagraphStyle> for rv_data::graphics::text::attributes::Paragraph {
    fn from(style: dm::ParagraphStyle) -> Self {
        use rv_data::graphics::text::attributes;
        use rv_data::graphics::text::attributes::paragraph::text_list;
        
        rv_data::graphics::text::attributes::Paragraph {
            alignment: match style.alignment {
                dm::TextAlignment::Left => attributes::Alignment::Left as i32,
                dm::TextAlignment::Right => attributes::Alignment::Right as i32,
                dm::TextAlignment::Center => attributes::Alignment::Center as i32,
                dm::TextAlignment::Justified => attributes::Alignment::Justified as i32,
            },
            first_line_head_indent: style.first_line_head_indent,
            head_indent: style.head_indent,
            tail_indent: style.tail_indent,
            line_height_multiple: style.line_height_multiple,
            maximum_line_height: style.maximum_line_height,
            minimum_line_height: style.minimum_line_height,
            line_spacing: style.line_spacing,
            paragraph_spacing: style.paragraph_spacing,
            paragraph_spacing_before: style.paragraph_spacing_before,
            tab_stops: style.tab_stops.into_iter().map(|ts| attributes::paragraph::TabStop {
                location: ts.position,
                alignment: match ts.alignment {
                    dm::TabAlignment::Left => attributes::Alignment::Left as i32,
                    dm::TabAlignment::Center => attributes::Alignment::Center as i32,
                    dm::TabAlignment::Right => attributes::Alignment::Right as i32,
                    dm::TabAlignment::Decimal => attributes::Alignment::Natural as i32,
                },
            }).collect(),
            default_tab_interval: style.default_tab_interval,
            text_list: Some(attributes::paragraph::TextList {
                is_enabled: false,
                number_type: text_list::NumberType::Box as i32,
                prefix: "".to_string(),
                postfix: "".to_string(),
                starting_number: 0,
            }),
            text_lists: vec![],
        }
    }
}

// Implement From<dm::CustomAttribute> for rv_data::graphics::text::attributes::CustomAttribute
impl From<dm::CustomAttribute> for rv_data::graphics::text::attributes::CustomAttribute {
    fn from(attr: dm::CustomAttribute) -> Self {
        use rv_data::graphics::text::attributes::custom_attribute::Attribute;
        
        rv_data::graphics::text::attributes::CustomAttribute {
            range: Some(rv_data::IntRange {
                start: attr.range.start as i32,
                end: attr.range.end as i32,
            }),
            attribute: Some(match attr.attribute {
                dm::CustomAttributeType::Capitalization(cap) => Attribute::Capitalization(cap as i32),
                dm::CustomAttributeType::OriginalFontSize(size) => Attribute::OriginalFontSize(size),
                dm::CustomAttributeType::FontScaleFactor(factor) => Attribute::FontScaleFactor(factor),
                // Other attribute types would go here - simplified for now
                _ => Attribute::Capitalization(0),
            }),
        }
    }
}

// Public helper function to convert from data_model::Presentation to rv_data::Presentation
pub fn convert_presentation_to_rv_data(presentation: crate::propresenter::data_model::Presentation) -> rv_data::Presentation {
    presentation.into()
}

impl From<dm::ScrollDirection> for rv_data::slide::element::text_scroller::Direction {
    fn from(direction: dm::ScrollDirection) -> Self {
        match direction {
            dm::ScrollDirection::Left => rv_data::slide::element::text_scroller::Direction::Left,
            dm::ScrollDirection::Right => rv_data::slide::element::text_scroller::Direction::Right,
            dm::ScrollDirection::Up => rv_data::slide::element::text_scroller::Direction::Up,
            dm::ScrollDirection::Down => rv_data::slide::element::text_scroller::Direction::Down,
        }
    }
}