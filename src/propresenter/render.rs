//! Pure translation from checked presentation specifications to native data.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use super::generated::rv_data;
use super::groups::GroupCatalog;
use super::presentation_spec::{CueContent, CueRoleId, GroupId, PresentationSpec, TextField};
use super::rtf::{extract_text_options, rtf_to_text, segments_to_rtf_bytes, StyledSegment};
use super::text_fit::CueTextFitSummary;

pub(crate) mod slide_instance;

use slide_instance::instantiate_template_slide;

/// Failure to inspect or bind native template text elements.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TemplateSlotError {
    /// A native template contains the same named text element more than once.
    #[error("template contains duplicate named text slot '{name}'")]
    DuplicateNativeSlot {
        /// Duplicate native graphics-element name.
        name: String,
    },
    /// A native text element referenced by a role does not exist.
    #[error("template has no named text slot '{name}'")]
    UnknownNativeSlot {
        /// Missing native graphics-element name.
        name: String,
    },
    /// A single-field role cannot identify exactly one editable text element.
    #[error("template has {count} possible default text fields; exactly one is required")]
    AmbiguousDefaultSlot {
        /// Number of possible editable fields.
        count: usize,
    },
    /// A requested semantic field is not declared by its resolved role.
    #[error("role '{role}' has no text field '{field}'")]
    UnknownSemanticField {
        /// Semantic cue role.
        role: String,
        /// Missing semantic field.
        field: String,
    },
    /// Two semantic fields target the same native text element.
    #[error("role '{role}' maps more than one field to native slot '{slot}'")]
    DuplicateNativeBinding {
        /// Semantic cue role.
        role: String,
        /// Reused native graphics-element name.
        slot: String,
    },
    /// One semantic field is declared more than once for a role.
    #[error("role '{role}' maps semantic field '{field}' more than once")]
    DuplicateSemanticBinding {
        /// Semantic cue role.
        role: String,
        /// Repeated semantic field.
        field: String,
    },
    /// A previously inspected native text element was not present while
    /// rendering the cloned slide.
    #[error("inspected native text slot at element index {index} is no longer a text element")]
    InvalidNativeSlot {
        /// Element index captured during template inspection.
        index: usize,
    },
}

/// Failure to clone a native theme slide into an independent cue-local graph.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SlideInstantiationError {
    /// A rendered presentation slide has no local slide graph.
    #[error("rendered presentation slide has no base slide")]
    MissingBaseSlide,
    /// A rendered local object has no stable native UUID.
    #[error("rendered {kind} has no native UUID")]
    MissingIdentity {
        /// Local object missing its identity.
        kind: &'static str,
    },
    /// One source UUID identifies more than one local slide object, making
    /// reference rewriting ambiguous.
    #[error("local slide reuses UUID '{uuid}' for both {first_kind} and {duplicate_kind}")]
    DuplicateIdentity {
        /// Reused source UUID.
        uuid: String,
        /// First local object carrying the UUID.
        first_kind: &'static str,
        /// Later local object carrying the same UUID.
        duplicate_kind: &'static str,
    },
    /// A local reference does not resolve inside the source slide graph.
    #[error("local slide {relation} references missing UUID '{uuid}'")]
    DanglingReference {
        /// Native relationship containing the unresolved UUID.
        relation: &'static str,
        /// Unresolved source UUID.
        uuid: String,
    },
    /// A build is present without a graphics element that can own it.
    #[error("local slide {build_kind} has no graphics element to target")]
    BuildWithoutElement {
        /// Native build field missing its owner.
        build_kind: &'static str,
    },
}

/// Failure to render a checked presentation specification.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RenderError {
    /// Two resolved roles share one semantic identity.
    #[error("render assets contain duplicate cue role '{role}'")]
    DuplicateRole {
        /// Duplicate role identity.
        role: String,
    },
    /// A cue references a role absent from the reviewed render assets.
    #[error("presentation references unresolved cue role '{role}'")]
    MissingRole {
        /// Missing role identity.
        role: String,
    },
    /// A named cue group is absent from the installed group catalog.
    #[error("presentation references unavailable installed cue group '{group}'")]
    MissingGroup {
        /// Missing exact installed group name.
        group: String,
    },
    /// A checked arrangement group was not emitted by the renderer.
    #[error("arrangement '{arrangement}' references unrendered cue group id '{group}'")]
    MissingRenderedGroup {
        /// Arrangement containing the reference.
        arrangement: String,
        /// Missing semantic group identity.
        group: String,
    },
    /// The checked selected arrangement was not emitted by the renderer.
    #[error("selected arrangement '{arrangement}' was not rendered")]
    MissingRenderedArrangement {
        /// Missing exact arrangement name.
        arrangement: String,
    },
    /// A checked group identity was emitted more than once.
    #[error("cue group id '{group}' was rendered more than once")]
    DuplicateRenderedGroup {
        /// Repeated semantic group identity.
        group: String,
    },
    /// Canonical operator traversal referenced a cue without rendered role metadata.
    #[error(
        "operator traversal referenced cue {index}, but role metadata contains {cue_count} cues"
    )]
    MissingRenderedCueRole {
        /// Cue index in canonical operator order.
        index: usize,
        /// Number of rendered cues with semantic role metadata.
        cue_count: usize,
    },
    /// A post-render transform changed cue identity, count, or storage order
    /// while semantic role indexes were still live.
    #[error("post-render transform changed the cue sequence bound to semantic roles")]
    RoleCueSequenceChanged,
    /// A post-render transform changed the selected/default operator traversal
    /// while semantic role indexes were still live.
    #[error("post-render transform changed operator traversal bound to semantic roles")]
    RoleOperatorTraversalChanged,
    /// A post-render transform changed text whose native fit evidence is live.
    #[error("post-render transform changed measured text at cue {cue_index}")]
    MeasuredCueTextChanged {
        /// Cue whose retained native measurement became stale.
        cue_index: usize,
    },
    /// A post-render transform changed native geometry or styling that formed
    /// part of a retained text-fit request.
    #[error("post-render transform changed measured text layout at cue {cue_index}")]
    MeasuredCueLayoutChanged {
        /// Cue whose retained native measurement became stale.
        cue_index: usize,
    },
    /// A post-measurement transform changed the macro selecting an audience
    /// destination, invalidating the retained output-screen proof.
    #[error("post-render transform changed measured audience routing at cue {cue_index}")]
    MeasuredCueDestinationChanged {
        /// Cue whose retained destination evidence became stale.
        cue_index: usize,
    },
    /// Native fit evidence referenced a cue absent from the rendered document.
    #[error("native text-fit evidence references unavailable cue {cue_index}")]
    TextFitEvidenceCueUnavailable {
        /// Invalid rendered cue index.
        cue_index: usize,
    },
    /// Native layout evidence repeats or reorders a cue identity.
    #[error("native text-fit summaries must have unique ascending cue indexes")]
    InvalidTextFitEvidenceOrder,
    /// A measured text cue has no rendering destination evidence.
    #[error("native text-fit summary for cue {cue_index} has no destinations")]
    MissingTextFitDestinationEvidence {
        /// Cue missing its source or output destination proof.
        cue_index: usize,
    },
    /// The rendered selected/default arrangement could not be traversed safely.
    #[error(transparent)]
    OperatorTraversal(#[from] super::arrangement::OperatorTraversalError),
    /// A template field could not be inspected or bound.
    #[error(transparent)]
    Template(#[from] TemplateSlotError),
    /// A theme slide could not be cloned into an independent native graph.
    #[error(transparent)]
    SlideInstantiation(#[from] SlideInstantiationError),
}

#[derive(Debug, Clone, Copy)]
struct NativeTextSlot<'a> {
    name: &'a str,
    index: usize,
    has_visible_text: bool,
}

/// Inspected native slide template with deterministic named text fields.
#[derive(Debug, Clone)]
pub struct SlideTemplate<'a> {
    slide: &'a rv_data::PresentationSlide,
    named: BTreeMap<String, usize>,
    text_slots: Vec<NativeTextSlot<'a>>,
}

impl<'a> SlideTemplate<'a> {
    /// Inspect one native slide without choosing which fields a role may edit.
    pub fn inspect(slide: &'a rv_data::PresentationSlide) -> Result<Self, TemplateSlotError> {
        let text_slots = native_text_slots(slide);
        let mut named = BTreeMap::new();
        for slot in &text_slots {
            if slot.name.is_empty() {
                continue;
            }
            if named.insert(slot.name.to_string(), slot.index).is_some() {
                return Err(TemplateSlotError::DuplicateNativeSlot {
                    name: slot.name.to_string(),
                });
            }
        }
        Ok(Self {
            slide,
            named,
            text_slots,
        })
    }

    /// Return the original native slide.
    pub const fn slide(&self) -> &'a rv_data::PresentationSlide {
        self.slide
    }

    /// Return all uniquely addressable native text slot names.
    pub fn named_slots(&self) -> impl Iterator<Item = &str> {
        self.named.keys().map(String::as_str)
    }

    /// Return the number of meaningful candidates for an implicit body field.
    pub fn default_candidate_count(&self) -> usize {
        self.default_candidates().len()
    }

    /// Prove that an audience theme can bind a configured role on this exact
    /// slide.
    ///
    /// `ProPresenter` theme application does not require a single source text
    /// element and its destination element to share a name. A zero- or
    /// one-field role therefore binds the one meaningful destination text
    /// element. Multi-field roles retain exact native-name binding because
    /// choosing their destination order would otherwise be ambiguous.
    pub(crate) fn validate_native_bindings<'slot>(
        &self,
        native_slots: impl IntoIterator<Item = &'slot str>,
    ) -> Result<(), TemplateSlotError> {
        let native_slots = native_slots.into_iter().collect::<Vec<_>>();
        if native_slots.len() <= 1 {
            self.default_slot()?;
            return Ok(());
        }
        for native_slot in native_slots {
            self.named_index(native_slot)?;
        }
        Ok(())
    }

    fn default_slot(&self) -> Result<usize, TemplateSlotError> {
        let candidates = self.default_candidates();
        if let [index] = candidates.as_slice() {
            Ok(*index)
        } else {
            Err(TemplateSlotError::AmbiguousDefaultSlot {
                count: candidates.len(),
            })
        }
    }

    fn default_candidates(&self) -> Vec<usize> {
        let named = self
            .text_slots
            .iter()
            .filter(|slot| !slot.name.is_empty())
            .map(|slot| slot.index)
            .collect::<Vec<_>>();
        let visible_unnamed = self
            .text_slots
            .iter()
            .filter(|slot| slot.name.is_empty() && slot.has_visible_text)
            .map(|slot| slot.index)
            .collect::<Vec<_>>();

        if !named.is_empty() || !visible_unnamed.is_empty() {
            named.into_iter().chain(visible_unnamed).collect()
        } else if self.text_slots.len() == 1 {
            vec![self.text_slots[0].index]
        } else {
            Vec::new()
        }
    }

    fn named_index(&self, name: &str) -> Result<usize, TemplateSlotError> {
        self.named
            .get(name)
            .copied()
            .ok_or_else(|| TemplateSlotError::UnknownNativeSlot {
                name: name.to_string(),
            })
    }
}

/// One semantic cue role resolved to a native slide and editable fields.
#[derive(Debug, Clone)]
pub struct ResolvedCueRole<'a> {
    id: CueRoleId,
    template: SlideTemplate<'a>,
    fields: BTreeMap<TextField, usize>,
}

impl<'a> ResolvedCueRole<'a> {
    /// Resolve a conventional `body` role from exactly one meaningful field.
    /// Empty unnamed helper elements do not count as meaningful fields.
    pub fn body(
        id: CueRoleId,
        slide: &'a rv_data::PresentationSlide,
    ) -> Result<Self, TemplateSlotError> {
        let template = SlideTemplate::inspect(slide)?;
        let index = template.default_slot()?;
        Ok(Self {
            id,
            template,
            fields: BTreeMap::from([(TextField::body(), index)]),
        })
    }

    /// Resolve explicit semantic fields to exact native graphics-element names.
    pub fn with_slots<'slot>(
        id: CueRoleId,
        slide: &'a rv_data::PresentationSlide,
        first: (TextField, &'slot str),
        rest: impl IntoIterator<Item = (TextField, &'slot str)>,
    ) -> Result<Self, TemplateSlotError> {
        let template = SlideTemplate::inspect(slide)?;
        let role_name = id.as_str().to_string();
        let mut native_names = BTreeSet::new();
        let mut fields = BTreeMap::new();
        for (field, native_name) in std::iter::once(first).chain(rest) {
            if fields.contains_key(&field) {
                return Err(TemplateSlotError::DuplicateSemanticBinding {
                    role: role_name,
                    field: field.as_str().to_string(),
                });
            }
            if !native_names.insert(native_name.to_string()) {
                return Err(TemplateSlotError::DuplicateNativeBinding {
                    role: role_name,
                    slot: native_name.to_string(),
                });
            }
            let index = template.named_index(native_name)?;
            fields.insert(field, index);
        }
        Ok(Self {
            id,
            template,
            fields,
        })
    }

    /// Resolve a role that preserves a template without editable text fields.
    pub fn static_slide(
        id: CueRoleId,
        slide: &'a rv_data::PresentationSlide,
    ) -> Result<Self, TemplateSlotError> {
        Ok(Self {
            id,
            template: SlideTemplate::inspect(slide)?,
            fields: BTreeMap::new(),
        })
    }

    /// Return the semantic role identity.
    pub const fn id(&self) -> &CueRoleId {
        &self.id
    }

    /// Return the immutable native slide used by this role.
    pub const fn slide(&self) -> &'a rv_data::PresentationSlide {
        self.template.slide()
    }

    pub(crate) fn field_index(&self, field: &TextField) -> Result<usize, TemplateSlotError> {
        self.fields
            .get(field)
            .copied()
            .ok_or_else(|| TemplateSlotError::UnknownSemanticField {
                role: self.id.as_str().to_string(),
                field: field.as_str().to_string(),
            })
    }
}

/// Immutable native assets used by one pure render.
#[derive(Debug)]
pub struct RenderAssets<'a> {
    roles: Vec<ResolvedCueRole<'a>>,
    groups: Option<&'a GroupCatalog>,
}

impl<'a> RenderAssets<'a> {
    /// Build an asset set with unique semantic role identities.
    pub fn new(
        first: ResolvedCueRole<'a>,
        rest: Vec<ResolvedCueRole<'a>>,
    ) -> Result<Self, RenderError> {
        let mut identities = BTreeSet::from([first.id().clone()]);
        for role in &rest {
            if !identities.insert(role.id().clone()) {
                return Err(RenderError::DuplicateRole {
                    role: role.id().as_str().to_string(),
                });
            }
        }
        let mut roles = Vec::with_capacity(rest.len() + 1);
        roles.push(first);
        roles.extend(rest);
        Ok(Self {
            roles,
            groups: None,
        })
    }

    /// Bind the exact installed cue-group metadata used by named groups.
    #[must_use]
    pub const fn with_group_catalog(mut self, groups: &'a GroupCatalog) -> Self {
        self.groups = Some(groups);
        self
    }

    pub(crate) fn role(&self, id: &CueRoleId) -> Result<&ResolvedCueRole<'a>, RenderError> {
        self.roles
            .iter()
            .find(|role| role.id() == id)
            .ok_or_else(|| RenderError::MissingRole {
                role: id.as_str().to_string(),
            })
    }

    fn group(&self, name: &str) -> Result<rv_data::Group, RenderError> {
        self.groups
            .and_then(|groups| groups.instantiate(name))
            .ok_or_else(|| RenderError::MissingGroup {
                group: name.to_string(),
            })
    }
}

/// One semantic role transition emitted at a concrete cue index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleTransition {
    cue_index: usize,
    role: CueRoleId,
}

impl RoleTransition {
    /// Return the zero-based cue index where the role begins.
    pub const fn cue_index(&self) -> usize {
        self.cue_index
    }

    /// Return the role entered at this cue.
    pub const fn role(&self) -> &CueRoleId {
        &self.role
    }
}

/// Cue indexes where each rendered semantic role begins.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RenderedCueRoles {
    entries: BTreeMap<CueRoleId, Vec<usize>>,
    transitions: Vec<RoleTransition>,
}

impl RenderedCueRoles {
    /// Return all role transitions in canonical operator traversal order.
    pub fn transitions(&self) -> &[RoleTransition] {
        &self.transitions
    }

    /// Return every cue index where the requested role begins.
    pub fn entries(&self, role: &CueRoleId) -> &[usize] {
        self.entries.get(role).map_or(&[], Vec::as_slice)
    }

    fn record(&mut self, cue_index: usize, role: &CueRoleId) {
        if self.transitions.last().map(RoleTransition::role) == Some(role) {
            return;
        }
        let entries = self.entries.entry(role.clone()).or_default();
        if !entries.contains(&cue_index) {
            entries.push(cue_index);
        }
        self.transitions.push(RoleTransition {
            cue_index,
            role: role.clone(),
        });
    }

    fn from_operator_order(
        presentation: &rv_data::Presentation,
        cue_roles: &[CueRoleId],
    ) -> Result<Self, RenderError> {
        let mut rendered = Self::default();
        for cue_index in super::arrangement::checked_operator_cue_indices(presentation)? {
            let role = cue_roles
                .get(cue_index)
                .ok_or(RenderError::MissingRenderedCueRole {
                    index: cue_index,
                    cue_count: cue_roles.len(),
                })?;
            rendered.record(cue_index, role);
        }
        Ok(rendered)
    }
}

/// Rendered native presentation and its observed semantic role transitions.
///
/// The presentation is producer-neutral: runtime application metadata and
/// producer-specific empty URL fields are intentionally absent. The export
/// boundary may add those fields once it knows which producer it represents.
pub struct RenderedPresentation {
    /// Valid, producer-neutral `ProPresenter` document.
    presentation: rv_data::Presentation,
    /// Actual role-entry cue indexes derived during rendering.
    cue_roles: RenderedCueRoles,
    /// Stable source and macro-selected output-screen layout evidence.
    text_fit_summary: Vec<CueTextFitSummary>,
}

impl RenderedPresentation {
    /// Producer-neutral native document produced by the semantic renderer.
    #[must_use]
    pub const fn presentation(&self) -> &rv_data::Presentation {
        &self.presentation
    }

    /// Semantic role transitions bound to the current cue order.
    #[must_use]
    pub const fn cue_roles(&self) -> &RenderedCueRoles {
        &self.cue_roles
    }

    /// Stable receipt-ready native layout evidence for every rendered text cue.
    #[must_use]
    pub fn text_fit_summary(&self) -> &[CueTextFitSummary] {
        &self.text_fit_summary
    }

    /// Retain complete native summaries after every rendered text cue has
    /// passed its source and output-destination postconditions.
    pub(crate) fn retain_text_fit_summary(
        &mut self,
        summaries: Vec<CueTextFitSummary>,
    ) -> Result<(), RenderError> {
        if summaries
            .windows(2)
            .any(|pair| pair[0].cue_index() >= pair[1].cue_index())
        {
            return Err(RenderError::InvalidTextFitEvidenceOrder);
        }
        if let Some(summary) = summaries
            .iter()
            .find(|summary| summary.cue_index() >= self.presentation.cues.len())
        {
            return Err(RenderError::TextFitEvidenceCueUnavailable {
                cue_index: summary.cue_index(),
            });
        }
        if let Some(summary) = summaries
            .iter()
            .find(|summary| summary.destination_count() == 0)
        {
            return Err(RenderError::MissingTextFitDestinationEvidence {
                cue_index: summary.cue_index(),
            });
        }
        self.text_fit_summary = summaries;
        Ok(())
    }

    /// Atomically replace the native document only when its semantic role
    /// mapping remains valid.
    ///
    /// A detached candidate lets fallible transforms complete before this
    /// boundary. Cue identity/order and the checked operator traversal must
    /// remain exact while [`RenderedCueRoles`] is live.
    pub(crate) fn replace_preserving_role_mapping(
        &mut self,
        candidate: rv_data::Presentation,
    ) -> Result<(), RenderError> {
        let cue_sequence = |presentation: &rv_data::Presentation| {
            presentation
                .cues
                .iter()
                .map(|cue| cue.uuid.as_ref().map(|uuid| uuid.string.clone()))
                .collect::<Vec<_>>()
        };
        if cue_sequence(&self.presentation) != cue_sequence(&candidate) {
            return Err(RenderError::RoleCueSequenceChanged);
        }
        let current_traversal =
            super::arrangement::checked_operator_cue_indices(&self.presentation)?;
        let candidate_traversal = super::arrangement::checked_operator_cue_indices(&candidate)?;
        if current_traversal != candidate_traversal {
            return Err(RenderError::RoleOperatorTraversalChanged);
        }
        for cue_index in self
            .text_fit_summary
            .iter()
            .map(CueTextFitSummary::cue_index)
        {
            let current = self
                .presentation
                .cues
                .get(cue_index)
                .ok_or(RenderError::TextFitEvidenceCueUnavailable { cue_index })?;
            let replacement = candidate
                .cues
                .get(cue_index)
                .ok_or(RenderError::TextFitEvidenceCueUnavailable { cue_index })?;
            if cue_text_payloads(current) != cue_text_payloads(replacement) {
                return Err(RenderError::MeasuredCueTextChanged { cue_index });
            }
            if cue_text_elements(current) != cue_text_elements(replacement) {
                return Err(RenderError::MeasuredCueLayoutChanged { cue_index });
            }
            if cue_macro_payloads(current) != cue_macro_payloads(replacement) {
                return Err(RenderError::MeasuredCueDestinationChanged { cue_index });
            }
        }
        self.presentation = candidate;
        Ok(())
    }

    /// Consume the role-indexed render after all role transitions are applied.
    pub(crate) fn into_presentation(self) -> rv_data::Presentation {
        self.presentation
    }
}

/// Render one checked specification from immutable reviewed assets.
///
/// This semantic phase does not depend on the current `ProPresenter` producer.
pub fn render_presentation(
    spec: &PresentationSpec,
    assets: &RenderAssets<'_>,
) -> Result<RenderedPresentation, RenderError> {
    let mut presentation = producer_neutral_presentation(spec.name());
    let mut cue_roles_by_index = Vec::new();
    let mut native_group_ids = BTreeMap::<GroupId, rv_data::Uuid>::new();

    for group in spec.groups() {
        let mut cue_uuids = Vec::new();
        for cue_spec in group.cues() {
            let role = assets.role(cue_spec.role())?;
            let slide = render_slide(role, cue_spec.content())?;
            let cue_uuid = push_presentation_cue(
                &mut presentation,
                slide,
                cue_spec
                    .label()
                    .map(super::presentation_spec::CueLabel::as_str),
            );
            cue_uuids.push(cue_uuid);
            cue_roles_by_index.push(role.id().clone());
        }
        let installed_group = group.name().map(|name| assets.group(name)).transpose()?;
        let native_id = push_cue_group(&mut presentation, installed_group, &cue_uuids);
        if native_group_ids
            .insert(group.id().clone(), native_id)
            .is_some()
        {
            return Err(RenderError::DuplicateRenderedGroup {
                group: group.id().as_str().to_string(),
            });
        }
    }

    let mut selected_native_id = None;
    for arrangement in spec.arrangements() {
        let native_id = rv_data::Uuid {
            string: uuid::Uuid::new_v4().to_string(),
        };
        let group_identifiers = arrangement
            .groups()
            .map(|group| {
                native_group_ids.get(group).cloned().ok_or_else(|| {
                    RenderError::MissingRenderedGroup {
                        arrangement: arrangement.name().as_str().to_string(),
                        group: group.as_str().to_string(),
                    }
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        presentation
            .arrangements
            .push(rv_data::presentation::Arrangement {
                uuid: Some(native_id.clone()),
                name: arrangement.name().as_str().to_string(),
                group_identifiers,
            });
        if spec.selected_arrangement() == Some(arrangement.name()) {
            selected_native_id = Some(native_id);
        }
    }
    if let Some(selected) = spec.selected_arrangement() {
        presentation.selected_arrangement =
            Some(
                selected_native_id.ok_or_else(|| RenderError::MissingRenderedArrangement {
                    arrangement: selected.as_str().to_string(),
                })?,
            );
    }

    let cue_roles = RenderedCueRoles::from_operator_order(&presentation, &cue_roles_by_index)?;

    Ok(RenderedPresentation {
        presentation,
        cue_roles,
        text_fit_summary: Vec::new(),
    })
}

fn cue_text_payloads(cue: &rv_data::Cue) -> Vec<Vec<u8>> {
    cue_text_elements(cue)
        .into_iter()
        .filter_map(|element| element.text.as_ref())
        .map(|text| text.rtf_data.clone())
        .collect()
}

fn cue_text_elements(cue: &rv_data::Cue) -> Vec<&rv_data::graphics::Element> {
    cue.actions
        .iter()
        .filter_map(|action| match action.action_type_data.as_ref() {
            Some(rv_data::action::ActionTypeData::Slide(slide)) => match slide.slide.as_ref() {
                Some(rv_data::action::slide_type::Slide::Presentation(slide)) => Some(slide),
                _ => None,
            },
            _ => None,
        })
        .flat_map(|slide| {
            slide
                .base_slide
                .iter()
                .flat_map(|base| base.elements.iter())
                .filter_map(|element| element.element.as_ref())
                .filter(|element| element.text.is_some())
        })
        .collect()
}

fn cue_macro_payloads(cue: &rv_data::Cue) -> Vec<Option<rv_data::CollectionElementType>> {
    cue.actions
        .iter()
        .filter_map(|action| match action.action_type_data.as_ref() {
            Some(rv_data::action::ActionTypeData::Macro(macro_action)) => {
                Some(macro_action.identification.clone())
            }
            _ => None,
        })
        .collect()
}

fn native_empty_timeline() -> rv_data::presentation::Timeline {
    rv_data::presentation::Timeline {
        duration: 300.0,
        ..rv_data::presentation::Timeline::default()
    }
}

/// Apply producer metadata captured for the current runtime.
pub(crate) fn apply_application_info(
    presentation: &mut rv_data::Presentation,
    application_info: Option<&rv_data::ApplicationInfo>,
) {
    presentation.application_info = application_info.cloned();

    let producer_platform = application_info.and_then(chord_chart_platform);
    let Some(chord_chart) = presentation.chord_chart.as_mut() else {
        presentation.chord_chart = producer_platform.map(|platform| rv_data::Url {
            platform,
            ..rv_data::Url::default()
        });
        return;
    };

    if chord_chart.platform != rv_data::url::Platform::Unknown as i32 {
        return;
    }

    if let Some(platform) = producer_platform {
        chord_chart.platform = platform;
    } else {
        presentation.chord_chart = None;
    }
}

fn chord_chart_platform(application_info: &rv_data::ApplicationInfo) -> Option<i32> {
    match rv_data::application_info::Platform::try_from(application_info.platform).ok()? {
        rv_data::application_info::Platform::Macos => Some(rv_data::url::Platform::Macos as i32),
        rv_data::application_info::Platform::Windows => Some(rv_data::url::Platform::Win32 as i32),
        rv_data::application_info::Platform::Undefined => None,
    }
}

/// Preserve the broad native metadata of a document edited in place.
///
/// Song, scripture, and music semantics remain attached to the same document.
/// Timeline cue references are discarded because rendered cues were replaced.
pub(crate) fn preserve_edited_document_metadata(
    presentation: &mut rv_data::Presentation,
    existing: &rv_data::Presentation,
) {
    preserve_generated_target_metadata(presentation, existing);
    presentation.chord_chart.clone_from(&existing.chord_chart);
    presentation.ccli.clone_from(&existing.ccli);
    presentation
        .bible_reference
        .clone_from(&existing.bible_reference);
    presentation
        .multi_tracks_licensing
        .clone_from(&existing.multi_tracks_licensing);
    presentation.music_key.clone_from(&existing.music_key);
    presentation.music.clone_from(&existing.music);
}

/// Preserve only stable identity and operator-owned metadata when replacing a
/// generated target with semantically new content.
///
/// CCLI, scripture, chord, music, and `MultiTracks` fields intentionally remain
/// those of the new render. Reusing them from an older target would attach stale
/// semantic identity to unrelated generated content.
pub(crate) fn preserve_generated_target_metadata(
    presentation: &mut rv_data::Presentation,
    existing: &rv_data::Presentation,
) {
    presentation.uuid.clone_from(&existing.uuid);
    presentation
        .last_date_used
        .clone_from(&existing.last_date_used);
    presentation
        .last_modified_date
        .clone_from(&existing.last_modified_date);
    presentation.category.clone_from(&existing.category);
    presentation.notes.clone_from(&existing.notes);
    presentation.background.clone_from(&existing.background);
    presentation.transition.clone_from(&existing.transition);
    presentation.content_destination = existing.content_destination;
    presentation.slide_show.clone_from(&existing.slide_show);
    if let Some(existing_timeline) = &existing.timeline {
        let mut timeline = existing_timeline.clone();
        timeline.cues.clear();
        timeline.cues_v2.clear();
        presentation.timeline = Some(timeline);
    }
}

fn render_slide(
    role: &ResolvedCueRole<'_>,
    content: &CueContent,
) -> Result<rv_data::PresentationSlide, RenderError> {
    let mut slide = instantiate_template_slide(role.template.slide())?;
    if let CueContent::Text(bindings) = content {
        for (field, segments) in bindings.iter() {
            let index = role.field_index(field)?;
            replace_text_at_index(&mut slide, index, segments)?;
        }
    }
    Ok(slide)
}

fn replace_text_at_index(
    slide: &mut rv_data::PresentationSlide,
    index: usize,
    segments: &[StyledSegment],
) -> Result<(), TemplateSlotError> {
    let Some(graphics) = slide
        .base_slide
        .as_mut()
        .and_then(|base| base.elements.get_mut(index))
        .and_then(|element| element.element.as_mut())
    else {
        return Err(TemplateSlotError::InvalidNativeSlot { index });
    };
    let Some(text) = graphics.text.as_mut() else {
        return Err(TemplateSlotError::InvalidNativeSlot { index });
    };
    let options = extract_text_options(text);
    text.rtf_data = segments_to_rtf_bytes(segments, &options);
    Ok(())
}

fn native_text_slots(slide: &rv_data::PresentationSlide) -> Vec<NativeTextSlot<'_>> {
    slide
        .base_slide
        .as_ref()
        .into_iter()
        .flat_map(|base| base.elements.iter().enumerate())
        .filter_map(|(index, element)| {
            let graphics = element.element.as_ref()?;
            let text = graphics.text.as_ref()?;
            let visible = rtf_to_text(&String::from_utf8_lossy(&text.rtf_data))
                .is_some_and(|value| !value.trim().is_empty());
            Some(NativeTextSlot {
                name: graphics.name.trim(),
                index,
                has_visible_text: visible,
            })
        })
        .collect()
}

fn producer_neutral_presentation(name: &str) -> rv_data::Presentation {
    rv_data::Presentation {
        uuid: Some(rv_data::Uuid {
            string: uuid::Uuid::new_v4().to_string(),
        }),
        name: name.to_string(),
        background: Some(rv_data::Background {
            is_enabled: false,
            fill: None,
        }),
        ccli: Some(rv_data::presentation::Ccli::default()),
        timeline: Some(native_empty_timeline()),
        content_destination: rv_data::action::ContentDestination::Global as i32,
        ..rv_data::Presentation::default()
    }
}

fn push_presentation_cue(
    presentation: &mut rv_data::Presentation,
    slide: rv_data::PresentationSlide,
    label: Option<&str>,
) -> uuid::Uuid {
    let cue_uuid = uuid::Uuid::new_v4();
    presentation.cues.push(rv_data::Cue {
        uuid: Some(rv_data::Uuid {
            string: cue_uuid.to_string(),
        }),
        name: String::new(),
        actions: vec![rv_data::Action {
            uuid: Some(rv_data::Uuid {
                string: uuid::Uuid::new_v4().to_string(),
            }),
            name: String::new(),
            label: label.map(|text| rv_data::action::Label {
                text: text.to_string(),
                color: None,
            }),
            delay_time: 0.0,
            old_type: None,
            is_enabled: true,
            layer_identification: None,
            duration: 0.0,
            r#type: rv_data::action::ActionType::PresentationSlide as i32,
            action_type_data: Some(rv_data::action::ActionTypeData::Slide(
                rv_data::action::SlideType {
                    slide: Some(rv_data::action::slide_type::Slide::Presentation(slide)),
                },
            )),
        }],
        completion_target_type: rv_data::cue::CompletionTargetType::None as i32,
        completion_target_uuid: None,
        completion_action_type: rv_data::cue::CompletionActionType::Last as i32,
        completion_action_uuid: None,
        trigger_time: None,
        hot_key: Some(rv_data::HotKey {
            code: 0,
            control_identifier: String::new(),
        }),
        pending_imports: Vec::new(),
        is_enabled: true,
        completion_time: 0.0,
    });
    cue_uuid
}

fn push_cue_group(
    presentation: &mut rv_data::Presentation,
    group: Option<rv_data::Group>,
    cue_uuids: &[uuid::Uuid],
) -> rv_data::Uuid {
    let native_id = rv_data::Uuid {
        string: uuid::Uuid::new_v4().to_string(),
    };
    let group = group.map_or_else(
        || rv_data::Group {
            uuid: Some(native_id.clone()),
            name: String::new(),
            color: None,
            hot_key: Some(rv_data::HotKey::default()),
            application_group_identifier: None,
            application_group_name: String::new(),
        },
        |mut group| {
            group.uuid = Some(native_id.clone());
            group
        },
    );
    presentation
        .cue_groups
        .push(rv_data::presentation::CueGroup {
            group: Some(group),
            cue_identifiers: cue_uuids
                .iter()
                .map(|uuid| rv_data::Uuid {
                    string: uuid.to_string(),
                })
                .collect(),
        });
    native_id
}

#[cfg(test)]
mod tests;
