//! Media dependency discovery for `ProPresenter` presentations.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use prost::Message;

use crate::propresenter::generated::rv_data::{
    self, action, graphics, presentation, presentation_slide, slide, url,
};

/// A media file reference found inside a presentation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MediaDependency {
    /// Original URL/path string stored in the presentation.
    pub source: String,
    /// Filesystem path when the source is a local `file://` URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// Filename portion, decoded when possible.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub basename: Option<String>,
}

/// Decode a `.pro` presentation and return referenced media dependencies.
pub fn presentation_media_dependencies_from_bytes(
    data: &[u8],
) -> Result<Vec<MediaDependency>, prost::DecodeError> {
    let presentation = rv_data::Presentation::decode(data)?;
    Ok(presentation_media_dependencies(&presentation))
}

/// Return all unique media dependencies referenced by a presentation.
#[must_use]
pub fn presentation_media_dependencies(
    presentation: &rv_data::Presentation,
) -> Vec<MediaDependency> {
    let mut dependencies = Vec::new();
    let mut seen = HashSet::new();

    for cue in &presentation.cues {
        for action in &cue.actions {
            collect_action_dependencies(action, &mut dependencies, &mut seen);
        }
    }

    if let Some(timeline) = &presentation.timeline {
        collect_timeline_dependencies(timeline, &mut dependencies, &mut seen);
    }

    dependencies
}

fn collect_timeline_dependencies(
    timeline: &presentation::Timeline,
    dependencies: &mut Vec<MediaDependency>,
    seen: &mut HashSet<String>,
) {
    if let Some(action) = &timeline.audio_action {
        collect_action_dependencies(action, dependencies, seen);
    }
    for cue in timeline.cues.iter().chain(&timeline.cues_v2) {
        if let Some(presentation::timeline::cue::TriggerInfo::Action(action)) = &cue.trigger_info {
            collect_action_dependencies(action, dependencies, seen);
        }
    }
}

fn collect_action_dependencies(
    action: &rv_data::Action,
    dependencies: &mut Vec<MediaDependency>,
    seen: &mut HashSet<String>,
) {
    match &action.action_type_data {
        Some(action::ActionTypeData::Media(media_type)) => {
            if let Some(media) = &media_type.element {
                push_media_dependency(media, dependencies, seen);
            }
            for marker in &media_type.markers {
                for action in &marker.actions {
                    collect_action_dependencies(action, dependencies, seen);
                }
            }
        }
        Some(action::ActionTypeData::Slide(slide_type)) => match &slide_type.slide {
            Some(action::slide_type::Slide::Presentation(slide)) => {
                collect_presentation_slide_dependencies(slide, dependencies, seen);
            }
            Some(action::slide_type::Slide::Prop(slide)) => {
                if let Some(base_slide) = &slide.base_slide {
                    collect_slide_dependencies(base_slide, dependencies, seen);
                }
            }
            None => {}
        },
        _ => {}
    }
}

fn collect_presentation_slide_dependencies(
    slide: &rv_data::PresentationSlide,
    dependencies: &mut Vec<MediaDependency>,
    seen: &mut HashSet<String>,
) {
    if let Some(url) = &slide.chord_chart {
        push_url_dependency(url, dependencies, seen);
    }

    if let Some(base_slide) = &slide.base_slide {
        collect_slide_dependencies(base_slide, dependencies, seen);
    }

    if let Some(presentation_slide::Notes {
        attributes: Some(attributes),
        ..
    }) = &slide.notes
    {
        collect_text_attribute_dependencies(attributes, dependencies, seen);
    }
}

fn collect_slide_dependencies(
    slide: &rv_data::Slide,
    dependencies: &mut Vec<MediaDependency>,
    seen: &mut HashSet<String>,
) {
    for element in &slide.elements {
        collect_slide_element_dependencies(element, dependencies, seen);
    }
}

fn collect_slide_element_dependencies(
    element: &slide::Element,
    dependencies: &mut Vec<MediaDependency>,
    seen: &mut HashSet<String>,
) {
    let Some(graphics_element) = &element.element else {
        return;
    };

    if let Some(fill) = &graphics_element.fill {
        collect_graphics_fill_dependencies(fill, dependencies, seen);
    }

    if let Some(text) = &graphics_element.text {
        if let Some(attributes) = &text.attributes {
            collect_text_attribute_dependencies(attributes, dependencies, seen);
        }
    }
}

fn collect_graphics_fill_dependencies(
    fill: &graphics::Fill,
    dependencies: &mut Vec<MediaDependency>,
    seen: &mut HashSet<String>,
) {
    if let Some(graphics::fill::FillType::Media(media)) = &fill.fill_type {
        push_media_dependency(media, dependencies, seen);
    }
}

fn collect_text_attribute_dependencies(
    attributes: &graphics::text::Attributes,
    dependencies: &mut Vec<MediaDependency>,
    seen: &mut HashSet<String>,
) {
    if let Some(graphics::text::attributes::Fill::MediaFill(media_fill)) = &attributes.fill {
        if let Some(media) = &media_fill.media {
            push_media_dependency(media, dependencies, seen);
        }
    }
}

fn push_media_dependency(
    media: &rv_data::Media,
    dependencies: &mut Vec<MediaDependency>,
    seen: &mut HashSet<String>,
) {
    if let Some(url) = &media.url {
        push_url_dependency(url, dependencies, seen);
    }
}

fn push_url_dependency(
    url: &rv_data::Url,
    dependencies: &mut Vec<MediaDependency>,
    seen: &mut HashSet<String>,
) {
    let Some(dependency) = dependency_from_url(url) else {
        return;
    };
    if seen.insert(dependency.source.clone()) {
        dependencies.push(dependency);
    }
}

fn dependency_from_url(url: &rv_data::Url) -> Option<MediaDependency> {
    match &url.storage {
        Some(url::Storage::AbsoluteString(value) | url::Storage::RelativePath(value)) => {
            dependency_from_source(value)
        }
        None => match &url.relative_file_path {
            Some(url::RelativeFilePath::Local(local)) => dependency_from_source(&local.path),
            Some(url::RelativeFilePath::External(external)) => {
                dependency_from_source(&external.path)
            }
            None => None,
        },
    }
}

fn dependency_from_source(source: &str) -> Option<MediaDependency> {
    if source.trim().is_empty() {
        return None;
    }

    let decoded = decode_file_url_or_path(source);
    let path = decoded
        .as_deref()
        .and_then(|value| value.starts_with('/').then(|| PathBuf::from(value)));
    let basename = path
        .as_deref()
        .and_then(Path::file_name)
        .and_then(|value| value.to_str())
        .map(ToOwned::to_owned)
        .or_else(|| {
            decoded
                .as_deref()
                .and_then(|value| value.rsplit('/').next())
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        });

    Some(MediaDependency {
        source: source.to_string(),
        path,
        basename,
    })
}

fn decode_file_url_or_path(source: &str) -> Option<String> {
    let path = source.strip_prefix("file://").map_or(source, |value| {
        value.strip_prefix("localhost").unwrap_or(value)
    });
    percent_decode(path)
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hi = bytes.get(index + 1).and_then(|byte| hex_value(*byte))?;
            let lo = bytes.get(index + 2).and_then(|byte| hex_value(*byte))?;
            decoded.push((hi << 4) | lo);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use crate::propresenter::background::make_background_media_action;

    #[test]
    fn decodes_file_url_dependency() {
        let dependency = dependency_from_source(
            "file:///Users/jimmy/Media/Home.%20lyrics%20slide%20background.png",
        )
        .expect("dependency");

        assert_eq!(
            dependency.path.as_deref(),
            Some(Path::new(
                "/Users/jimmy/Media/Home. lyrics slide background.png"
            ))
        );
        assert_eq!(
            dependency.basename.as_deref(),
            Some("Home. lyrics slide background.png")
        );
    }

    #[test]
    fn finds_action_media_dependencies() {
        let media_path = Path::new("/tmp/proflow-media-test/default.jpg");
        let presentation = rv_data::Presentation {
            cues: vec![rv_data::Cue {
                actions: vec![make_background_media_action(media_path)],
                ..rv_data::Cue::default()
            }],
            ..rv_data::Presentation::default()
        };

        let dependencies = presentation_media_dependencies(&presentation);

        assert_eq!(dependencies.len(), 1);
        assert_eq!(dependencies[0].path.as_deref(), Some(media_path));
        assert_eq!(dependencies[0].basename.as_deref(), Some("default.jpg"));
    }

    #[test]
    fn finds_timeline_marker_and_prop_slide_media_dependencies() {
        // This is the native semantic shape: an audio action belongs directly
        // to the timeline, both timeline cue generations can hold actions, and
        // a media action's playback marker can itself trigger another action.
        // `Prelude Slides.pro` stores the same audio action as `audio_action`
        // and as a v2 cue, so this also fixes the expected source de-duplication.
        let timeline_audio = media_action_with_marker(
            "/show/audio/timeline.mp3",
            media_action("/show/media/marker.png"),
        );
        let presentation = rv_data::Presentation {
            timeline: Some(presentation::Timeline {
                audio_action: Some(timeline_audio.clone()),
                cues: vec![timeline_action_cue(media_action(
                    "/show/media/legacy-timeline.mov",
                ))],
                cues_v2: vec![
                    timeline_action_cue(timeline_audio),
                    timeline_action_cue(prop_slide_action("/show/media/prop-fill.jpg")),
                ],
                ..presentation::Timeline::default()
            }),
            ..rv_data::Presentation::default()
        };

        let dependencies = presentation_media_dependencies(&presentation);
        let paths = dependencies
            .iter()
            .map(|dependency| dependency.path.as_deref())
            .collect::<Vec<_>>();

        assert_eq!(
            paths,
            vec![
                Some(Path::new("/show/audio/timeline.mp3")),
                Some(Path::new("/show/media/marker.png")),
                Some(Path::new("/show/media/legacy-timeline.mov")),
                Some(Path::new("/show/media/prop-fill.jpg")),
            ]
        );
    }

    fn media_action(path: &str) -> rv_data::Action {
        rv_data::Action {
            action_type_data: Some(action::ActionTypeData::Media(action::MediaType {
                element: Some(media(path)),
                ..action::MediaType::default()
            })),
            ..rv_data::Action::default()
        }
    }

    fn media_action_with_marker(path: &str, marker_action: rv_data::Action) -> rv_data::Action {
        rv_data::Action {
            action_type_data: Some(action::ActionTypeData::Media(action::MediaType {
                element: Some(media(path)),
                markers: vec![action::media_type::PlaybackMarker {
                    actions: vec![marker_action],
                    ..action::media_type::PlaybackMarker::default()
                }],
                ..action::MediaType::default()
            })),
            ..rv_data::Action::default()
        }
    }

    fn media(path: &str) -> rv_data::Media {
        rv_data::Media {
            url: Some(file_url(path)),
            ..rv_data::Media::default()
        }
    }

    fn file_url(path: &str) -> rv_data::Url {
        rv_data::Url {
            storage: Some(url::Storage::AbsoluteString(format!("file://{path}"))),
            ..rv_data::Url::default()
        }
    }

    fn timeline_action_cue(action: rv_data::Action) -> presentation::timeline::Cue {
        presentation::timeline::Cue {
            trigger_info: Some(presentation::timeline::cue::TriggerInfo::Action(action)),
            ..presentation::timeline::Cue::default()
        }
    }

    fn prop_slide_action(path: &str) -> rv_data::Action {
        let slide = rv_data::Slide {
            elements: vec![slide::Element {
                element: Some(graphics::Element {
                    fill: Some(graphics::Fill {
                        enable: true,
                        fill_type: Some(graphics::fill::FillType::Media(media(path))),
                    }),
                    ..graphics::Element::default()
                }),
                ..slide::Element::default()
            }],
            ..rv_data::Slide::default()
        };
        rv_data::Action {
            action_type_data: Some(action::ActionTypeData::Slide(action::SlideType {
                slide: Some(action::slide_type::Slide::Prop(rv_data::PropSlide {
                    base_slide: Some(slide),
                    ..rv_data::PropSlide::default()
                })),
            })),
            ..rv_data::Action::default()
        }
    }
}
