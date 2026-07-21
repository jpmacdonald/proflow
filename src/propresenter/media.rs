//! Media dependency discovery for `ProPresenter` presentations.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::propresenter::deserialize::{decode_presentation_bytes, ProPresenterError};
use crate::propresenter::generated::rv_data::{
    self, action, graphics, media, presentation, presentation_slide, slide,
};
use crate::propresenter::native_url;

/// A checked media file reference found inside a presentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaDependency {
    locator: native_url::NativeFileLocator,
}

impl serde::Serialize for MediaDependency {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;

        let mut fields = 1;
        fields += usize::from(self.stored_absolute_path().is_some());
        fields += usize::from(self.basename().is_some());
        let mut dependency = serializer.serialize_struct("MediaDependency", fields)?;
        dependency.serialize_field("source", self.source())?;
        if let Some(path) = self.stored_absolute_path() {
            dependency.serialize_field("path", path)?;
        }
        if let Some(basename) = self.basename() {
            dependency.serialize_field("basename", basename)?;
        }
        dependency.end()
    }
}

/// Checked filesystem state of a native media dependency.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaDependencyResolution {
    /// The preferred native candidate exists as a regular file.
    Available(PathBuf),
    /// The locator is local, but none of its candidates currently exists.
    Missing(PathBuf),
    /// The native reference has no safe local candidate.
    Unresolved,
}

impl MediaDependency {
    /// Original URL/path string stored in the presentation.
    #[must_use]
    pub fn source(&self) -> &str {
        self.locator.source()
    }

    /// Stored absolute path retained for diagnostics.
    ///
    /// Use [`Self::resolve`] when selecting bytes: the active show-relative
    /// locator intentionally has precedence over this value.
    #[must_use]
    pub fn stored_absolute_path(&self) -> Option<&Path> {
        self.locator.stored_absolute_path()
    }

    /// Final decoded path component, when one is available.
    #[must_use]
    pub fn basename(&self) -> Option<&str> {
        self.locator.basename()
    }

    /// Resolve this dependency without collapsing missing and non-local states.
    #[must_use]
    pub fn resolve(&self, show_root: Option<&Path>) -> MediaDependencyResolution {
        match self.locator.resolve(show_root) {
            native_url::NativeFileResolution::Available(path) => {
                MediaDependencyResolution::Available(path)
            }
            native_url::NativeFileResolution::Missing(path) => {
                MediaDependencyResolution::Missing(path)
            }
            native_url::NativeFileResolution::Unresolved => MediaDependencyResolution::Unresolved,
        }
    }

    const fn has_local_locator(&self) -> bool {
        self.locator.has_local_candidate()
    }
}

/// Decode a `.pro` presentation and return referenced media dependencies.
pub fn presentation_media_dependencies_from_bytes(
    data: &[u8],
) -> Result<Vec<MediaDependency>, ProPresenterError> {
    let presentation = decode_presentation_bytes(data, "in-memory presentation")?;
    Ok(presentation_media_dependencies(&presentation))
}

/// Return all unique media dependencies referenced by a presentation.
#[must_use]
pub fn presentation_media_dependencies(
    presentation: &rv_data::Presentation,
) -> Vec<MediaDependency> {
    let mut dependencies = Vec::new();
    let mut seen = HashSet::new();

    if let Some(chord_chart) = &presentation.chord_chart {
        push_url_dependency(chord_chart, &mut dependencies, &mut seen);
    }

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

/// Return unique media dependencies inherited from one native theme slide.
#[must_use]
pub(crate) fn presentation_slide_media_dependencies(
    slide: &rv_data::PresentationSlide,
) -> Vec<MediaDependency> {
    let mut dependencies = Vec::new();
    let mut seen = HashSet::new();
    collect_presentation_slide_dependencies(slide, &mut dependencies, &mut seen);
    dependencies
}

fn collect_timeline_dependencies(
    timeline: &presentation::Timeline,
    dependencies: &mut Vec<MediaDependency>,
    seen: &mut HashSet<native_url::NativeFileLocator>,
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
    seen: &mut HashSet<native_url::NativeFileLocator>,
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
        Some(action::ActionTypeData::ExternalPresentation(external_presentation)) => {
            if let Some(url) = &external_presentation.url {
                push_url_dependency(url, dependencies, seen);
            }
        }
        _ => {}
    }
}

fn collect_presentation_slide_dependencies(
    slide: &rv_data::PresentationSlide,
    dependencies: &mut Vec<MediaDependency>,
    seen: &mut HashSet<native_url::NativeFileLocator>,
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
    seen: &mut HashSet<native_url::NativeFileLocator>,
) {
    for element in &slide.elements {
        collect_slide_element_dependencies(element, dependencies, seen);
    }
}

fn collect_slide_element_dependencies(
    element: &slide::Element,
    dependencies: &mut Vec<MediaDependency>,
    seen: &mut HashSet<native_url::NativeFileLocator>,
) {
    if let Some(graphics_element) = &element.element {
        if let Some(fill) = &graphics_element.fill {
            collect_graphics_fill_dependencies(fill, dependencies, seen);
        }

        if let Some(text) = &graphics_element.text {
            if let Some(attributes) = &text.attributes {
                collect_text_attribute_dependencies(attributes, dependencies, seen);
            }
        }
    }

    for data_link in &element.data_links {
        collect_data_link_dependencies(data_link, dependencies, seen);
    }
}

fn collect_data_link_dependencies(
    data_link: &slide::element::DataLink,
    dependencies: &mut Vec<MediaDependency>,
    seen: &mut HashSet<native_url::NativeFileLocator>,
) {
    use slide::element::data_link::{ticker::SourceType, PropertyType};

    match &data_link.property_type {
        Some(PropertyType::FileFeed(file_feed)) => {
            if let Some(url) = &file_feed.url {
                push_local_url_dependency(url, dependencies, seen);
            }
        }
        Some(PropertyType::RssFeed(feed)) => {
            if let Some(url) = &feed.url {
                push_local_url_dependency(url, dependencies, seen);
            }
        }
        Some(PropertyType::Ticker(ticker)) => match &ticker.source_type {
            Some(SourceType::FileType(file)) => {
                if let Some(url) = &file.url {
                    push_local_url_dependency(url, dependencies, seen);
                }
            }
            Some(SourceType::RssType(feed)) => {
                if let Some(url) = &feed.url {
                    push_local_url_dependency(url, dependencies, seen);
                }
            }
            Some(SourceType::TextType(_)) | None => {}
        },
        _ => {}
    }
}

fn collect_graphics_fill_dependencies(
    fill: &graphics::Fill,
    dependencies: &mut Vec<MediaDependency>,
    seen: &mut HashSet<native_url::NativeFileLocator>,
) {
    if let Some(graphics::fill::FillType::Media(media)) = &fill.fill_type {
        push_media_dependency(media, dependencies, seen);
    }
}

fn collect_text_attribute_dependencies(
    attributes: &graphics::text::Attributes,
    dependencies: &mut Vec<MediaDependency>,
    seen: &mut HashSet<native_url::NativeFileLocator>,
) {
    if let Some(graphics::text::attributes::Fill::MediaFill(media_fill)) = &attributes.fill {
        if let Some(media) = &media_fill.media {
            push_media_dependency(media, dependencies, seen);
        }
    }

    for custom_attribute in &attributes.custom_attributes {
        if let Some(graphics::text::attributes::custom_attribute::Attribute::MediaFill(
            media_fill,
        )) = &custom_attribute.attribute
        {
            if let Some(media) = &media_fill.media {
                push_media_dependency(media, dependencies, seen);
            }
        }
    }
}

fn push_media_dependency(
    media: &rv_data::Media,
    dependencies: &mut Vec<MediaDependency>,
    seen: &mut HashSet<native_url::NativeFileLocator>,
) {
    if let Some(url) = &media.url {
        push_url_dependency(url, dependencies, seen);
    }

    let file = match &media.type_properties {
        Some(media::TypeProperties::Audio(properties)) => properties.file.as_ref(),
        Some(media::TypeProperties::Image(properties)) => properties.file.as_ref(),
        Some(media::TypeProperties::Video(properties)) => properties.file.as_ref(),
        Some(media::TypeProperties::WebContent(properties)) => {
            if let Some(url) = &properties.url {
                push_local_url_dependency(url, dependencies, seen);
            }
            None
        }
        Some(media::TypeProperties::LiveVideo(_)) | None => None,
    };
    if let Some(local_url) = file.and_then(|file| file.local_url.as_ref()) {
        push_url_dependency(local_url, dependencies, seen);
    }
}

fn push_local_url_dependency(
    url: &rv_data::Url,
    dependencies: &mut Vec<MediaDependency>,
    seen: &mut HashSet<native_url::NativeFileLocator>,
) {
    push_dependency(
        dependency_from_url(url).filter(MediaDependency::has_local_locator),
        dependencies,
        seen,
    );
}

fn push_url_dependency(
    url: &rv_data::Url,
    dependencies: &mut Vec<MediaDependency>,
    seen: &mut HashSet<native_url::NativeFileLocator>,
) {
    push_dependency(dependency_from_url(url), dependencies, seen);
}

fn push_dependency(
    dependency: Option<MediaDependency>,
    dependencies: &mut Vec<MediaDependency>,
    seen: &mut HashSet<native_url::NativeFileLocator>,
) {
    let Some(dependency) = dependency else {
        return;
    };
    if seen.insert(dependency.locator.clone()) {
        dependencies.push(dependency);
    }
}

fn dependency_from_url(url: &rv_data::Url) -> Option<MediaDependency> {
    let locator = native_url::NativeFileLocator::from_url(url)?;
    Some(MediaDependency { locator })
}

#[cfg(test)]
fn dependency_from_source(source: &str) -> Option<MediaDependency> {
    if source.trim().is_empty() {
        return None;
    }
    dependency_from_url(&rv_data::Url {
        storage: Some(
            crate::propresenter::generated::rv_data::url::Storage::AbsoluteString(
                source.to_string(),
            ),
        ),
        ..rv_data::Url::default()
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use crate::propresenter::background::make_background_media_action_for_test;
    use crate::propresenter::generated::rv_data::url;
    use prost::Message;

    #[test]
    fn byte_dependency_scan_requires_native_presentation_identity() {
        let error = presentation_media_dependencies_from_bytes(&[])
            .expect_err("empty protobuf is not a native presentation");

        assert!(matches!(error, ProPresenterError::UnsupportedFormat { .. }));
    }

    #[test]
    fn byte_dependency_scan_accepts_identified_native_presentation() {
        let presentation = rv_data::Presentation {
            name: "Identified".to_string(),
            uuid: Some(rv_data::Uuid {
                string: "identified-id".to_string(),
            }),
            ..rv_data::Presentation::default()
        };

        let dependencies =
            presentation_media_dependencies_from_bytes(&presentation.encode_to_vec())
                .expect("identified presentation");

        assert!(dependencies.is_empty());
    }

    #[test]
    fn decodes_file_url_dependency() {
        let dependency = dependency_from_source(
            "file:///Users/jimmy/Media/Home.%20lyrics%20slide%20background.png",
        )
        .expect("dependency");

        assert_eq!(
            dependency.stored_absolute_path(),
            Some(Path::new(
                "/Users/jimmy/Media/Home. lyrics slide background.png"
            ))
        );
        assert_eq!(
            dependency.basename(),
            Some("Home. lyrics slide background.png")
        );
        assert_eq!(
            serde_json::to_value(&dependency).expect("serialize dependency"),
            serde_json::json!({
                "source": "file:///Users/jimmy/Media/Home.%20lyrics%20slide%20background.png",
                "path": "/Users/jimmy/Media/Home. lyrics slide background.png",
                "basename": "Home. lyrics slide background.png",
            })
        );
    }

    #[test]
    fn finds_action_media_dependencies() {
        let media_path = Path::new("/tmp/proflow-media-test/default.jpg");
        let presentation = rv_data::Presentation {
            cues: vec![rv_data::Cue {
                actions: vec![make_background_media_action_for_test(
                    media_path,
                    (1, 1),
                    Path::new("/tmp"),
                )],
                ..rv_data::Cue::default()
            }],
            ..rv_data::Presentation::default()
        };

        let dependencies = presentation_media_dependencies(&presentation);

        assert_eq!(dependencies.len(), 1);
        assert_eq!(dependencies[0].stored_absolute_path(), Some(media_path));
        assert_eq!(dependencies[0].basename(), Some("default.jpg"));
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
            .map(MediaDependency::stored_absolute_path)
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

    #[test]
    fn finds_local_dependencies_across_native_file_carriers() {
        let presentation = rv_data::Presentation {
            chord_chart: Some(file_url("/show/charts/presentation.prochord")),
            cues: vec![rv_data::Cue {
                actions: vec![
                    media_action("/show/media/action.jpg"),
                    media_action_for(media_with_type_properties(media::TypeProperties::Audio(
                        media::AudioTypeProperties {
                            file: Some(file_properties("/show/audio/cue.mp3")),
                            ..media::AudioTypeProperties::default()
                        },
                    ))),
                    external_presentation_action("/show/presentations/external.key"),
                    media_action_for(media_with_type_properties(
                        media::TypeProperties::WebContent(media::WebContentTypeProperties {
                            url: Some(file_url("/show/web/local.html")),
                            ..media::WebContentTypeProperties::default()
                        }),
                    )),
                    presentation_slide_action(rv_data::PresentationSlide {
                        chord_chart: Some(file_url("/show/charts/slide.prochord")),
                        base_slide: Some(rv_data::Slide {
                            elements: vec![
                                graphics_media_element(media_with_type_properties(
                                    media::TypeProperties::Image(media::ImageTypeProperties {
                                        file: Some(file_properties("/show/media/fill.png")),
                                        ..media::ImageTypeProperties::default()
                                    }),
                                )),
                                file_feed_element(file_url("/show/data/feed.txt")),
                                ticker_file_element("/show/data/ticker.txt"),
                                rss_feed_element(file_url("/show/data/feed.xml")),
                                ticker_rss_element("/show/data/ticker.xml"),
                                custom_attribute_media_element(media(
                                    "/show/media/custom-attribute.png",
                                )),
                            ],
                            ..rv_data::Slide::default()
                        }),
                        ..rv_data::PresentationSlide::default()
                    }),
                ],
                ..rv_data::Cue::default()
            }],
            timeline: Some(presentation::Timeline {
                audio_action: Some(media_action_for(media_with_type_properties(
                    media::TypeProperties::Video(media::VideoTypeProperties {
                        file: Some(file_properties("/show/video/timeline.mov")),
                        ..media::VideoTypeProperties::default()
                    }),
                ))),
                ..presentation::Timeline::default()
            }),
            ..rv_data::Presentation::default()
        };

        let paths = presentation_media_dependencies(&presentation)
            .iter()
            .map(|dependency| dependency.stored_absolute_path().map(Path::to_path_buf))
            .collect::<Vec<_>>();

        assert_eq!(
            paths,
            [
                "/show/charts/presentation.prochord",
                "/show/media/action.jpg",
                "/show/audio/cue.mp3",
                "/show/presentations/external.key",
                "/show/web/local.html",
                "/show/charts/slide.prochord",
                "/show/media/fill.png",
                "/show/data/feed.txt",
                "/show/data/ticker.txt",
                "/show/data/feed.xml",
                "/show/data/ticker.xml",
                "/show/media/custom-attribute.png",
                "/show/video/timeline.mov",
            ]
            .map(|path| Some(PathBuf::from(path)))
        );
    }

    #[test]
    fn stale_absolute_media_falls_back_to_show_relative_locator() {
        let directory = tempfile::tempdir().expect("tempdir");
        let relative = Path::new("Media/current.png");
        let actual = directory.path().join(relative);
        std::fs::create_dir_all(actual.parent().expect("media parent")).expect("create media");
        std::fs::write(&actual, b"image").expect("write media");
        let url = rv_data::Url {
            storage: Some(url::Storage::AbsoluteString(
                "file:///stale-machine/Media/current.png".to_string(),
            )),
            relative_file_path: Some(url::RelativeFilePath::Local(url::LocalRelativePath {
                root: url::local_relative_path::Root::Show as i32,
                path: relative.display().to_string(),
            })),
            ..rv_data::Url::default()
        };

        let dependency = dependency_from_url(&url).expect("dependency");

        assert_eq!(
            dependency.resolve(Some(directory.path())),
            MediaDependencyResolution::Available(actual)
        );
    }

    #[test]
    fn active_show_media_wins_when_stale_absolute_media_still_exists() {
        let directory = tempfile::tempdir().expect("tempdir");
        let show_root = directory.path().join("show");
        let stale_root = directory.path().join("stale");
        let relative = Path::new("Media/current.png");
        let actual = show_root.join(relative);
        let stale = stale_root.join("current.png");
        std::fs::create_dir_all(actual.parent().expect("show media parent"))
            .expect("create show media");
        std::fs::create_dir_all(&stale_root).expect("create stale media");
        std::fs::write(&actual, b"current").expect("write current media");
        std::fs::write(&stale, b"stale").expect("write stale media");
        let url = rv_data::Url {
            storage: Some(url::Storage::AbsoluteString(native_url::file_url(
                &stale.to_string_lossy(),
            ))),
            relative_file_path: Some(url::RelativeFilePath::Local(url::LocalRelativePath {
                root: url::local_relative_path::Root::Show as i32,
                path: relative.display().to_string(),
            })),
            ..rv_data::Url::default()
        };

        let dependency = dependency_from_url(&url).expect("dependency");

        assert_eq!(dependency.stored_absolute_path(), Some(stale.as_path()));
        assert_eq!(
            dependency.resolve(Some(&show_root)),
            MediaDependencyResolution::Available(actual)
        );
    }

    #[test]
    fn local_missing_and_remote_unresolved_dependencies_remain_distinct() {
        let local =
            dependency_from_source("file:///missing/background.png").expect("local dependency");
        let remote = dependency_from_source("https://example.com/background.png")
            .expect("remote dependency");

        assert_eq!(
            local.resolve(None),
            MediaDependencyResolution::Missing(PathBuf::from("/missing/background.png"))
        );
        assert_eq!(remote.resolve(None), MediaDependencyResolution::Unresolved);
    }

    #[test]
    fn preserves_remote_file_references_as_unresolved_dependencies() {
        let presentation = rv_data::Presentation {
            chord_chart: Some(remote_url("https://example.com/chart.prochord")),
            cues: vec![rv_data::Cue {
                actions: vec![
                    media_action_for(media_with_type_properties(
                        media::TypeProperties::WebContent(media::WebContentTypeProperties {
                            url: Some(remote_url("https://example.com/live")),
                            ..media::WebContentTypeProperties::default()
                        }),
                    )),
                    presentation_slide_action(rv_data::PresentationSlide {
                        base_slide: Some(rv_data::Slide {
                            elements: vec![file_feed_element(remote_url(
                                "https://example.com/feed.txt",
                            ))],
                            ..rv_data::Slide::default()
                        }),
                        ..rv_data::PresentationSlide::default()
                    }),
                ],
                ..rv_data::Cue::default()
            }],
            ..rv_data::Presentation::default()
        };

        let dependencies = presentation_media_dependencies(&presentation);

        assert_eq!(dependencies.len(), 1);
        assert_eq!(
            dependencies[0].source(),
            "https://example.com/chart.prochord"
        );
        assert_eq!(dependencies[0].stored_absolute_path(), None);
        assert_eq!(dependencies[0].basename(), Some("chart.prochord"));
    }

    fn media_action(path: &str) -> rv_data::Action {
        media_action_for(media(path))
    }

    fn media_action_for(media: rv_data::Media) -> rv_data::Action {
        rv_data::Action {
            action_type_data: Some(action::ActionTypeData::Media(action::MediaType {
                element: Some(media),
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

    fn remote_url(source: &str) -> rv_data::Url {
        rv_data::Url {
            storage: Some(url::Storage::AbsoluteString(source.to_string())),
            ..rv_data::Url::default()
        }
    }

    fn file_properties(path: &str) -> rv_data::FileProperties {
        rv_data::FileProperties {
            local_url: Some(file_url(path)),
            ..rv_data::FileProperties::default()
        }
    }

    fn media_with_type_properties(type_properties: media::TypeProperties) -> rv_data::Media {
        rv_data::Media {
            type_properties: Some(type_properties),
            ..rv_data::Media::default()
        }
    }

    fn graphics_media_element(media: rv_data::Media) -> slide::Element {
        slide::Element {
            element: Some(graphics::Element {
                fill: Some(graphics::Fill {
                    enable: true,
                    fill_type: Some(graphics::fill::FillType::Media(media)),
                }),
                ..graphics::Element::default()
            }),
            ..slide::Element::default()
        }
    }

    fn custom_attribute_media_element(media: rv_data::Media) -> slide::Element {
        use graphics::text::attributes::{custom_attribute::Attribute, CustomAttribute};

        slide::Element {
            element: Some(graphics::Element {
                text: Some(graphics::Text {
                    attributes: Some(graphics::text::Attributes {
                        custom_attributes: vec![CustomAttribute {
                            attribute: Some(Attribute::MediaFill(graphics::text::MediaFill {
                                media: Some(media),
                            })),
                            ..CustomAttribute::default()
                        }],
                        ..graphics::text::Attributes::default()
                    }),
                    ..graphics::Text::default()
                }),
                ..graphics::Element::default()
            }),
            ..slide::Element::default()
        }
    }

    fn file_feed_element(url: rv_data::Url) -> slide::Element {
        use slide::element::data_link::{FileFeed, PropertyType};

        slide::Element {
            data_links: vec![slide::element::DataLink {
                property_type: Some(PropertyType::FileFeed(FileFeed { url: Some(url) })),
            }],
            ..slide::Element::default()
        }
    }

    fn ticker_file_element(path: &str) -> slide::Element {
        use slide::element::data_link::{ticker, PropertyType, Ticker};

        slide::Element {
            data_links: vec![slide::element::DataLink {
                property_type: Some(PropertyType::Ticker(Ticker {
                    source_type: Some(ticker::SourceType::FileType(ticker::FileType {
                        url: Some(file_url(path)),
                    })),
                    ..Ticker::default()
                })),
            }],
            ..slide::Element::default()
        }
    }

    fn rss_feed_element(url: rv_data::Url) -> slide::Element {
        use slide::element::data_link::{PropertyType, RssFeed};

        slide::Element {
            data_links: vec![slide::element::DataLink {
                property_type: Some(PropertyType::RssFeed(RssFeed {
                    url: Some(url),
                    ..RssFeed::default()
                })),
            }],
            ..slide::Element::default()
        }
    }

    fn ticker_rss_element(path: &str) -> slide::Element {
        use slide::element::data_link::{ticker, PropertyType, Ticker};

        slide::Element {
            data_links: vec![slide::element::DataLink {
                property_type: Some(PropertyType::Ticker(Ticker {
                    source_type: Some(ticker::SourceType::RssType(ticker::RssType {
                        url: Some(file_url(path)),
                        ..ticker::RssType::default()
                    })),
                    ..Ticker::default()
                })),
            }],
            ..slide::Element::default()
        }
    }

    fn presentation_slide_action(slide: rv_data::PresentationSlide) -> rv_data::Action {
        rv_data::Action {
            action_type_data: Some(action::ActionTypeData::Slide(action::SlideType {
                slide: Some(action::slide_type::Slide::Presentation(slide)),
            })),
            ..rv_data::Action::default()
        }
    }

    fn external_presentation_action(path: &str) -> rv_data::Action {
        rv_data::Action {
            action_type_data: Some(action::ActionTypeData::ExternalPresentation(
                action::ExternalPresentationType {
                    url: Some(file_url(path)),
                },
            )),
            ..rv_data::Action::default()
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
