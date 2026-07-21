//! Cue-local instantiation of native theme slide identity graphs.

use std::collections::{BTreeMap, BTreeSet};

use super::SlideInstantiationError;
use crate::propresenter::generated::rv_data;

type UuidRemap = BTreeMap<String, rv_data::Uuid>;

#[derive(Debug)]
struct RemintedIdentity {
    source_key: Option<String>,
    uuid: rv_data::Uuid,
}

#[derive(Default)]
struct SourceIdentities {
    kinds: BTreeMap<String, &'static str>,
}

impl SourceIdentities {
    fn remint(
        &mut self,
        field: &mut Option<rv_data::Uuid>,
        kind: &'static str,
    ) -> Result<RemintedIdentity, SlideInstantiationError> {
        let source_key = field.as_ref().and_then(source_identity_key);
        if let Some(key) = &source_key {
            if let Some(first_kind) = self.kinds.insert(key.clone(), kind) {
                return Err(SlideInstantiationError::DuplicateIdentity {
                    uuid: field
                        .as_ref()
                        .map_or_else(String::new, |uuid| uuid.string.clone()),
                    first_kind,
                    duplicate_kind: kind,
                });
            }
        }

        let uuid = new_uuid();
        field.clone_from(&Some(uuid.clone()));
        Ok(RemintedIdentity { source_key, uuid })
    }
}

/// Clone one native theme slide into an independent cue-local identity graph.
///
/// Theme media, timer, screen, transition-effect, and effect-preset UUIDs are
/// references to installed/external objects and remain untouched. Only objects
/// owned by the cloned slide and references between those objects are reminted.
pub(super) fn instantiate_template_slide(
    template: &rv_data::PresentationSlide,
) -> Result<rv_data::PresentationSlide, SlideInstantiationError> {
    let mut instance = template.clone();
    let mut source_identities = SourceIdentities::default();
    let mut element_ids = UuidRemap::new();
    let mut build_order_ids = UuidRemap::new();

    if let Some(slide) = &mut instance.base_slide {
        source_identities.remint(&mut slide.uuid, "slide")?;

        for element in &mut slide.elements {
            if let Some(graphics) = &mut element.element {
                let reminted = source_identities.remint(&mut graphics.uuid, "graphics element")?;
                record_remap(&mut element_ids, &reminted);
                record_remap(&mut build_order_ids, &reminted);
            }
            if let Some(build) = &mut element.build_in {
                let reminted = source_identities.remint(&mut build.uuid, "build in")?;
                record_remap(&mut build_order_ids, &reminted);
            }
            if let Some(build) = &mut element.build_out {
                let reminted = source_identities.remint(&mut build.uuid, "build out")?;
                record_remap(&mut build_order_ids, &reminted);
            }
            for child_build in &mut element.child_builds {
                let reminted = source_identities.remint(&mut child_build.uuid, "child build")?;
                record_remap(&mut build_order_ids, &reminted);
            }
        }

        for guideline in &mut slide.guidelines {
            source_identities.remint(&mut guideline.uuid, "slide guideline")?;
        }
    }

    for guideline in &mut instance.template_guidelines {
        source_identities.remint(&mut guideline.uuid, "template guideline")?;
    }

    if let Some(slide) = &mut instance.base_slide {
        for element in &mut slide.elements {
            let owning_element_uuid = element
                .element
                .as_ref()
                .and_then(|graphics| graphics.uuid.as_ref());
            rewrite_build_target(
                element.build_in.as_mut(),
                owning_element_uuid,
                &element_ids,
                "build-in elementUUID",
                "build in",
            )?;
            rewrite_build_target(
                element.build_out.as_mut(),
                owning_element_uuid,
                &element_ids,
                "build-out elementUUID",
                "build out",
            )?;
            for data_link in &mut element.data_links {
                rewrite_data_link(data_link, &element_ids)?;
            }
        }

        for uuid in &mut slide.element_build_order {
            rewrite_required_reference(uuid, &build_order_ids, "element build order")?;
        }
    }

    validate_slide_identity_graph(&instance)?;
    Ok(instance)
}

/// Prove that every cue-local slide object has one identity and every local
/// reference resolves exactly once inside the same slide.
pub fn validate_slide_identity_graph(
    instance: &rv_data::PresentationSlide,
) -> Result<(), SlideInstantiationError> {
    use rv_data::slide::element::data_link::visibility_link::condition::ConditionType;
    use rv_data::slide::element::data_link::PropertyType;

    let slide = instance
        .base_slide
        .as_ref()
        .ok_or(SlideInstantiationError::MissingBaseSlide)?;
    let mut identities = BTreeMap::<String, &'static str>::new();
    let mut element_ids = BTreeSet::<String>::new();
    let mut build_order_ids = BTreeSet::<String>::new();

    register_identity(&mut identities, slide.uuid.as_ref(), "slide")?;
    for element in &slide.elements {
        if let Some(graphics) = &element.element {
            let key =
                register_identity(&mut identities, graphics.uuid.as_ref(), "graphics element")?;
            element_ids.insert(key.clone());
            build_order_ids.insert(key);
        }
        for (build, kind) in [
            (element.build_in.as_ref(), "build in"),
            (element.build_out.as_ref(), "build out"),
        ] {
            let Some(build) = build else {
                continue;
            };
            if element.element.is_none() {
                return Err(SlideInstantiationError::BuildWithoutElement { build_kind: kind });
            }
            let key = register_identity(&mut identities, build.uuid.as_ref(), kind)?;
            build_order_ids.insert(key);
        }
        for build in &element.child_builds {
            let key = register_identity(&mut identities, build.uuid.as_ref(), "child build")?;
            build_order_ids.insert(key);
        }
    }
    for element in &slide.elements {
        for (build, relation) in [
            (element.build_in.as_ref(), "build-in elementUUID"),
            (element.build_out.as_ref(), "build-out elementUUID"),
        ] {
            if let Some(build) = build {
                require_local_reference(build.element_uuid.as_ref(), &element_ids, relation)?;
            }
        }
        for data_link in &element.data_links {
            match &data_link.property_type {
                Some(PropertyType::AlternateText(alternate)) => optional_local_reference(
                    alternate.other_element_uuid.as_ref(),
                    &element_ids,
                    "alternate-text link",
                )?,
                Some(PropertyType::AlternateFill(alternate)) => optional_local_reference(
                    alternate.other_element_uuid.as_ref(),
                    &element_ids,
                    "alternate-fill link",
                )?,
                Some(PropertyType::VisibilityLink(link)) => {
                    for condition in &link.conditions {
                        if let Some(ConditionType::ElementVisibility(visibility)) =
                            &condition.condition_type
                        {
                            optional_local_reference(
                                visibility.other_element_uuid.as_ref(),
                                &element_ids,
                                "element-visibility link",
                            )?;
                        }
                    }
                }
                _ => {}
            }
        }
    }
    for guideline in &slide.guidelines {
        register_identity(&mut identities, guideline.uuid.as_ref(), "slide guideline")?;
    }
    for guideline in &instance.template_guidelines {
        register_identity(
            &mut identities,
            guideline.uuid.as_ref(),
            "template guideline",
        )?;
    }
    for reference in &slide.element_build_order {
        require_local_reference(Some(reference), &build_order_ids, "element build order")?;
    }
    Ok(())
}

fn register_identity(
    identities: &mut BTreeMap<String, &'static str>,
    identity: Option<&rv_data::Uuid>,
    kind: &'static str,
) -> Result<String, SlideInstantiationError> {
    let identity = identity
        .and_then(validated_identity_key)
        .ok_or(SlideInstantiationError::MissingIdentity { kind })?;
    if let Some(first_kind) = identities.insert(identity.clone(), kind) {
        return Err(SlideInstantiationError::DuplicateIdentity {
            uuid: identity,
            first_kind,
            duplicate_kind: kind,
        });
    }
    Ok(identity)
}

fn optional_local_reference(
    reference: Option<&rv_data::Uuid>,
    identities: &BTreeSet<String>,
    relation: &'static str,
) -> Result<(), SlideInstantiationError> {
    reference.map_or(Ok(()), |reference| {
        require_local_reference(Some(reference), identities, relation)
    })
}

fn require_local_reference(
    reference: Option<&rv_data::Uuid>,
    identities: &BTreeSet<String>,
    relation: &'static str,
) -> Result<(), SlideInstantiationError> {
    let source = reference.map_or_else(String::new, |uuid| uuid.string.clone());
    if reference
        .and_then(validated_identity_key)
        .is_some_and(|identity| identities.contains(&identity))
    {
        Ok(())
    } else {
        Err(SlideInstantiationError::DanglingReference {
            relation,
            uuid: source,
        })
    }
}

fn record_remap(remap: &mut UuidRemap, identity: &RemintedIdentity) {
    if let Some(key) = &identity.source_key {
        remap.insert(key.clone(), identity.uuid.clone());
    }
}

fn rewrite_build_target(
    build: Option<&mut rv_data::slide::element::Build>,
    owning_element_uuid: Option<&rv_data::Uuid>,
    element_ids: &UuidRemap,
    relation: &'static str,
    build_kind: &'static str,
) -> Result<(), SlideInstantiationError> {
    let Some(build) = build else {
        return Ok(());
    };
    let Some(owner) = owning_element_uuid else {
        return Err(SlideInstantiationError::BuildWithoutElement { build_kind });
    };

    if let Some(target) = &mut build.element_uuid {
        rewrite_required_reference(target, element_ids, relation)?;
    } else {
        build.element_uuid = Some(owner.clone());
    }
    Ok(())
}

fn rewrite_data_link(
    data_link: &mut rv_data::slide::element::DataLink,
    element_ids: &UuidRemap,
) -> Result<(), SlideInstantiationError> {
    use rv_data::slide::element::data_link::PropertyType;

    match &mut data_link.property_type {
        Some(PropertyType::AlternateText(alternate)) => rewrite_optional_reference(
            &mut alternate.other_element_uuid,
            element_ids,
            "alternate-text link",
        ),
        Some(PropertyType::AlternateFill(alternate)) => rewrite_optional_reference(
            &mut alternate.other_element_uuid,
            element_ids,
            "alternate-fill link",
        ),
        Some(PropertyType::VisibilityLink(link)) => {
            use rv_data::slide::element::data_link::visibility_link::condition::ConditionType;

            for condition in &mut link.conditions {
                if let Some(ConditionType::ElementVisibility(visibility)) =
                    &mut condition.condition_type
                {
                    rewrite_optional_reference(
                        &mut visibility.other_element_uuid,
                        element_ids,
                        "element-visibility link",
                    )?;
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn rewrite_optional_reference(
    reference: &mut Option<rv_data::Uuid>,
    remap: &UuidRemap,
    relation: &'static str,
) -> Result<(), SlideInstantiationError> {
    if let Some(uuid) = reference {
        rewrite_required_reference(uuid, remap, relation)?;
    }
    Ok(())
}

fn rewrite_required_reference(
    reference: &mut rv_data::Uuid,
    remap: &UuidRemap,
    relation: &'static str,
) -> Result<(), SlideInstantiationError> {
    let source = reference.string.clone();
    let reminted = source_identity_key(reference)
        .and_then(|key| remap.get(&key))
        .ok_or(SlideInstantiationError::DanglingReference {
            relation,
            uuid: source,
        })?;
    reference.clone_from(reminted);
    Ok(())
}

/// Return the opaque identity key carried by a source template.
///
/// Native source documents are reminted before they cross the generated-output
/// boundary, so legacy non-UUID identity text remains usable for matching its
/// source-local references.
fn source_identity_key(uuid: &rv_data::Uuid) -> Option<String> {
    let value = uuid.string.trim();
    (!value.is_empty()).then(|| value.to_ascii_lowercase())
}

/// Return the canonical key permitted in a finalized generated slide graph.
fn validated_identity_key(uuid: &rv_data::Uuid) -> Option<String> {
    let value = uuid.string.trim();
    if value != uuid.string {
        return None;
    }
    uuid::Uuid::parse_str(value)
        .ok()
        .map(|identity| identity.to_string())
}

fn new_uuid() -> rv_data::Uuid {
    rv_data::Uuid {
        string: uuid::Uuid::new_v4().to_string(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic)]

    use std::collections::BTreeSet;

    use prost::Message;

    use super::*;

    fn native_uuid(value: &str) -> rv_data::Uuid {
        rv_data::Uuid {
            string: value.to_string(),
        }
    }

    fn data_link(
        property_type: rv_data::slide::element::data_link::PropertyType,
    ) -> rv_data::slide::element::DataLink {
        rv_data::slide::element::DataLink {
            property_type: Some(property_type),
        }
    }

    #[test]
    fn every_committed_native_template_instantiates_a_closed_local_graph() {
        let fixture_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/propresenter/native/templates");
        let mut fixtures = std::fs::read_dir(&fixture_root)
            .unwrap_or_else(|error| panic!("read {}: {error}", fixture_root.display()))
            .map(|entry| {
                entry.unwrap_or_else(|error| {
                    panic!(
                        "read fixture entry under {}: {error}",
                        fixture_root.display()
                    )
                })
            })
            .filter(|entry| {
                entry
                    .file_type()
                    .unwrap_or_else(|error| {
                        panic!("inspect fixture {}: {error}", entry.path().display())
                    })
                    .is_file()
                    && entry
                        .path()
                        .extension()
                        .and_then(std::ffi::OsStr::to_str)
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("pro"))
            })
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        fixtures.sort();
        assert!(
            !fixtures.is_empty(),
            "{} contains no native template fixtures",
            fixture_root.display()
        );

        for path in fixtures {
            let presentation = rv_data::Presentation::decode(
                std::fs::read(&path)
                    .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
                    .as_slice(),
            )
            .unwrap_or_else(|error| panic!("decode {}: {error}", path.display()));
            let mut slide_count = 0;
            for action in presentation.cues.iter().flat_map(|cue| &cue.actions) {
                let Some(rv_data::action::ActionTypeData::Slide(slide_action)) =
                    &action.action_type_data
                else {
                    continue;
                };
                let Some(rv_data::action::slide_type::Slide::Presentation(slide)) =
                    &slide_action.slide
                else {
                    continue;
                };
                let instance = instantiate_template_slide(slide)
                    .unwrap_or_else(|error| panic!("instantiate {}: {error}", path.display()));
                validate_slide_identity_graph(&instance)
                    .unwrap_or_else(|error| panic!("validate {}: {error}", path.display()));
                slide_count += 1;
            }
            assert!(slide_count > 0, "{} has no template slides", path.display());
        }
    }

    #[test]
    fn remints_opaque_source_identities_before_strict_final_validation() {
        let source_element = native_uuid("legacy-element-id");
        let source = rv_data::PresentationSlide {
            base_slide: Some(rv_data::Slide {
                uuid: Some(native_uuid("legacy-slide-id")),
                elements: vec![rv_data::slide::Element {
                    element: Some(rv_data::graphics::Element {
                        uuid: Some(source_element.clone()),
                        ..rv_data::graphics::Element::default()
                    }),
                    build_in: Some(rv_data::slide::element::Build {
                        uuid: Some(native_uuid("legacy-build-id")),
                        element_uuid: Some(source_element),
                        ..rv_data::slide::element::Build::default()
                    }),
                    ..rv_data::slide::Element::default()
                }],
                ..rv_data::Slide::default()
            }),
            ..rv_data::PresentationSlide::default()
        };

        let instance = instantiate_template_slide(&source)
            .expect("opaque source identities are reminted before validation");
        validate_slide_identity_graph(&instance).expect("reminted graph is canonical");
        let slide = instance.base_slide.as_ref().expect("base slide");
        assert!(
            uuid::Uuid::parse_str(&slide.uuid.as_ref().expect("slide identity").string).is_ok()
        );
        assert!(uuid::Uuid::parse_str(
            &slide.elements[0]
                .element
                .as_ref()
                .and_then(|element| element.uuid.as_ref())
                .expect("element identity")
                .string
        )
        .is_ok());
    }

    #[test]
    fn final_validation_rejects_non_uuid_local_identity() {
        let instance = rv_data::PresentationSlide {
            base_slide: Some(rv_data::Slide {
                uuid: Some(native_uuid("not-a-uuid")),
                ..rv_data::Slide::default()
            }),
            ..rv_data::PresentationSlide::default()
        };

        assert!(matches!(
            validate_slide_identity_graph(&instance),
            Err(SlideInstantiationError::MissingIdentity { kind: "slide" })
        ));
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn remints_local_graph_and_preserves_external_references() {
        use rv_data::slide::element::data_link::visibility_link::condition::ConditionType;
        use rv_data::slide::element::data_link::PropertyType;

        let source_element_a = native_uuid("10000000-0000-0000-0000-000000000001");
        let source_element_b = native_uuid("10000000-0000-0000-0000-000000000002");
        let source_build_in = native_uuid("10000000-0000-0000-0000-000000000003");
        let source_build_out = native_uuid("10000000-0000-0000-0000-000000000004");
        let source_child_build = native_uuid("10000000-0000-0000-0000-000000000005");
        let external_media = native_uuid("20000000-0000-0000-0000-000000000001");
        let external_timer = native_uuid("20000000-0000-0000-0000-000000000002");
        let external_screen = native_uuid("20000000-0000-0000-0000-000000000003");
        let external_effect = native_uuid("20000000-0000-0000-0000-000000000004");

        let source = rv_data::PresentationSlide {
            base_slide: Some(rv_data::Slide {
                elements: vec![
                    rv_data::slide::Element {
                        element: Some(rv_data::graphics::Element {
                            uuid: Some(source_element_a.clone()),
                            fill: Some(rv_data::graphics::Fill {
                                enable: true,
                                fill_type: Some(rv_data::graphics::fill::FillType::Media(
                                    rv_data::Media {
                                        uuid: Some(external_media),
                                        ..rv_data::Media::default()
                                    },
                                )),
                            }),
                            ..rv_data::graphics::Element::default()
                        }),
                        build_in: Some(rv_data::slide::element::Build {
                            uuid: Some(source_build_in.clone()),
                            element_uuid: Some(source_element_a.clone()),
                            transition: Some(rv_data::Transition {
                                favorite_uuid: Some(external_effect),
                                ..rv_data::Transition::default()
                            }),
                            ..rv_data::slide::element::Build::default()
                        }),
                        build_out: Some(rv_data::slide::element::Build {
                            uuid: Some(source_build_out),
                            element_uuid: Some(source_element_a.clone()),
                            ..rv_data::slide::element::Build::default()
                        }),
                        child_builds: vec![rv_data::slide::element::ChildBuild {
                            uuid: Some(source_child_build),
                            ..rv_data::slide::element::ChildBuild::default()
                        }],
                        data_links: vec![
                            data_link(PropertyType::AlternateText(
                                rv_data::slide::element::data_link::AlternateElementText {
                                    other_element_uuid: Some(source_element_b.clone()),
                                    ..rv_data::slide::element::data_link::AlternateElementText::default()
                                },
                            )),
                            data_link(PropertyType::AlternateFill(
                                rv_data::slide::element::data_link::AlternateElementFill {
                                    other_element_uuid: Some(source_element_b.clone()),
                                    ..rv_data::slide::element::data_link::AlternateElementFill::default()
                                },
                            )),
                            data_link(PropertyType::VisibilityLink(
                                rv_data::slide::element::data_link::VisibilityLink {
                                    conditions: vec![
                                        rv_data::slide::element::data_link::visibility_link::Condition {
                                            condition_type: Some(ConditionType::ElementVisibility(
                                                rv_data::slide::element::data_link::visibility_link::condition::ElementVisibility {
                                                    other_element_uuid: Some(source_element_b.clone()),
                                                    ..rv_data::slide::element::data_link::visibility_link::condition::ElementVisibility::default()
                                                },
                                            )),
                                        },
                                        rv_data::slide::element::data_link::visibility_link::Condition {
                                            condition_type: Some(ConditionType::TimerVisibility(
                                                rv_data::slide::element::data_link::visibility_link::condition::TimerVisibility {
                                                    timer_uuid: Some(external_timer.clone()),
                                                    ..rv_data::slide::element::data_link::visibility_link::condition::TimerVisibility::default()
                                                },
                                            )),
                                        },
                                    ],
                                    ..rv_data::slide::element::data_link::VisibilityLink::default()
                                },
                            )),
                            data_link(PropertyType::TimerText(
                                rv_data::slide::element::data_link::TimerText {
                                    timer_uuid: Some(external_timer.clone()),
                                    ..rv_data::slide::element::data_link::TimerText::default()
                                },
                            )),
                            data_link(PropertyType::OutputScreen(
                                rv_data::slide::element::data_link::OutputScreen {
                                    screen_id: Some(external_screen.clone()),
                                    ..rv_data::slide::element::data_link::OutputScreen::default()
                                },
                            )),
                        ],
                        ..rv_data::slide::Element::default()
                    },
                    rv_data::slide::Element {
                        element: Some(rv_data::graphics::Element {
                            uuid: Some(source_element_b.clone()),
                            ..rv_data::graphics::Element::default()
                        }),
                        ..rv_data::slide::Element::default()
                    },
                ],
                element_build_order: vec![source_element_a.clone(), source_build_in],
                guidelines: vec![rv_data::AlignmentGuide {
                    uuid: Some(native_uuid("10000000-0000-0000-0000-000000000006")),
                    ..rv_data::AlignmentGuide::default()
                }],
                uuid: Some(native_uuid("10000000-0000-0000-0000-000000000007")),
                ..rv_data::Slide::default()
            }),
            template_guidelines: vec![rv_data::AlignmentGuide {
                uuid: Some(native_uuid("10000000-0000-0000-0000-000000000008")),
                ..rv_data::AlignmentGuide::default()
            }],
            ..rv_data::PresentationSlide::default()
        };

        let instance = instantiate_template_slide(&source).expect("valid local graph");
        let source_slide = source.base_slide.as_ref().expect("source slide");
        let slide = instance.base_slide.as_ref().expect("instantiated slide");
        let first = &slide.elements[0];
        let second = &slide.elements[1];
        let first_uuid = first
            .element
            .as_ref()
            .and_then(|element| element.uuid.as_ref())
            .expect("first element identity");
        let second_uuid = second
            .element
            .as_ref()
            .and_then(|element| element.uuid.as_ref())
            .expect("second element identity");
        let build_in = first.build_in.as_ref().expect("build in");
        let build_out = first.build_out.as_ref().expect("build out");

        let local_ids = [
            slide.uuid.as_ref().expect("slide identity"),
            first_uuid,
            second_uuid,
            build_in.uuid.as_ref().expect("build-in identity"),
            build_out.uuid.as_ref().expect("build-out identity"),
            first.child_builds[0]
                .uuid
                .as_ref()
                .expect("child-build identity"),
            slide.guidelines[0]
                .uuid
                .as_ref()
                .expect("guideline identity"),
            instance.template_guidelines[0]
                .uuid
                .as_ref()
                .expect("template-guideline identity"),
        ];
        let unique = local_ids
            .iter()
            .map(|uuid| uuid.string.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(unique.len(), local_ids.len());
        assert!(local_ids
            .iter()
            .all(|uuid| uuid::Uuid::parse_str(&uuid.string).is_ok()));
        assert_ne!(slide.uuid, source_slide.uuid);
        assert_ne!(first_uuid, &source_element_a);
        assert_ne!(second_uuid, &source_element_b);

        assert_eq!(build_in.element_uuid.as_ref(), Some(first_uuid));
        assert_eq!(build_out.element_uuid.as_ref(), Some(first_uuid));
        assert_eq!(&slide.element_build_order[0], first_uuid);
        assert_eq!(
            &slide.element_build_order[1],
            build_in.uuid.as_ref().expect("build-in identity")
        );

        let PropertyType::AlternateText(alternate) = first.data_links[0]
            .property_type
            .as_ref()
            .expect("alternate text")
        else {
            panic!("alternate-text link");
        };
        assert_eq!(alternate.other_element_uuid.as_ref(), Some(second_uuid));
        let PropertyType::AlternateFill(alternate) = first.data_links[1]
            .property_type
            .as_ref()
            .expect("alternate fill")
        else {
            panic!("alternate-fill link");
        };
        assert_eq!(alternate.other_element_uuid.as_ref(), Some(second_uuid));
        let PropertyType::VisibilityLink(visibility) = first.data_links[2]
            .property_type
            .as_ref()
            .expect("visibility link")
        else {
            panic!("visibility link");
        };
        let Some(ConditionType::ElementVisibility(element_visibility)) =
            &visibility.conditions[0].condition_type
        else {
            panic!("element visibility");
        };
        assert_eq!(
            element_visibility.other_element_uuid.as_ref(),
            Some(second_uuid)
        );
        let Some(ConditionType::TimerVisibility(timer_visibility)) =
            &visibility.conditions[1].condition_type
        else {
            panic!("timer visibility");
        };
        assert_eq!(timer_visibility.timer_uuid.as_ref(), Some(&external_timer));

        assert_eq!(
            first
                .element
                .as_ref()
                .and_then(|element| element.fill.as_ref()),
            source_slide.elements[0]
                .element
                .as_ref()
                .and_then(|element| element.fill.as_ref())
        );
        assert_eq!(
            build_in.transition.as_ref(),
            source_slide.elements[0]
                .build_in
                .as_ref()
                .and_then(|build| build.transition.as_ref())
        );
        let PropertyType::TimerText(timer) = first.data_links[3]
            .property_type
            .as_ref()
            .expect("timer text")
        else {
            panic!("timer text");
        };
        assert_eq!(timer.timer_uuid.as_ref(), Some(&external_timer));
        let PropertyType::OutputScreen(screen) = first.data_links[4]
            .property_type
            .as_ref()
            .expect("output screen")
        else {
            panic!("output screen");
        };
        assert_eq!(screen.screen_id.as_ref(), Some(&external_screen));
    }

    #[test]
    fn rejects_ambiguous_or_dangling_local_identity_graphs() {
        use rv_data::slide::element::data_link::PropertyType;

        let duplicate = native_uuid("30000000-0000-0000-0000-000000000001");
        let duplicate_source = rv_data::PresentationSlide {
            base_slide: Some(rv_data::Slide {
                elements: vec![
                    rv_data::slide::Element {
                        element: Some(rv_data::graphics::Element {
                            uuid: Some(duplicate.clone()),
                            ..rv_data::graphics::Element::default()
                        }),
                        ..rv_data::slide::Element::default()
                    },
                    rv_data::slide::Element {
                        element: Some(rv_data::graphics::Element {
                            uuid: Some(duplicate),
                            ..rv_data::graphics::Element::default()
                        }),
                        ..rv_data::slide::Element::default()
                    },
                ],
                ..rv_data::Slide::default()
            }),
            ..rv_data::PresentationSlide::default()
        };

        assert!(matches!(
            instantiate_template_slide(&duplicate_source),
            Err(SlideInstantiationError::DuplicateIdentity {
                first_kind: "graphics element",
                duplicate_kind: "graphics element",
                ..
            })
        ));

        let dangling_source = rv_data::PresentationSlide {
            base_slide: Some(rv_data::Slide {
                elements: vec![rv_data::slide::Element {
                    element: Some(rv_data::graphics::Element {
                        uuid: Some(native_uuid("30000000-0000-0000-0000-000000000002")),
                        ..rv_data::graphics::Element::default()
                    }),
                    data_links: vec![data_link(PropertyType::AlternateText(
                        rv_data::slide::element::data_link::AlternateElementText {
                            other_element_uuid: Some(native_uuid(
                                "30000000-0000-0000-0000-000000000003",
                            )),
                            ..rv_data::slide::element::data_link::AlternateElementText::default()
                        },
                    ))],
                    ..rv_data::slide::Element::default()
                }],
                ..rv_data::Slide::default()
            }),
            ..rv_data::PresentationSlide::default()
        };

        assert!(matches!(
            instantiate_template_slide(&dangling_source),
            Err(SlideInstantiationError::DanglingReference {
                relation: "alternate-text link",
                ..
            })
        ));
    }
}
