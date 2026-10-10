//! Checked, renderer-independent presentation specification.
//!
//! Source-specific workflow code produces this small semantic model. Native
//! `ProPresenter` protobuf construction is owned by the renderer, so content
//! planning cannot accidentally create partial cues or dangling group members.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use super::rtf::StyledSegment;

/// Failure to construct a checked presentation specification.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PresentationSpecError {
    /// A semantic identifier is blank or padded with whitespace.
    #[error("{kind} must be non-empty, unpadded, and contain no control characters")]
    InvalidIdentifier {
        /// Kind of identifier being checked.
        kind: &'static str,
    },
    /// A text field occurs more than once in one cue.
    #[error("cue contains duplicate text field '{field}'")]
    DuplicateTextField {
        /// Duplicate semantic field.
        field: String,
    },
    /// A cue label is blank or padded with whitespace.
    #[error("cue label must be non-empty, unpadded, and contain no control characters")]
    InvalidCueLabel,
    /// Two cue groups share one semantic identity.
    #[error("presentation contains duplicate cue group id '{id}'")]
    DuplicateGroupId {
        /// Repeated semantic group identity.
        id: String,
    },
    /// Two arrangements have names that differ only by case.
    #[error("presentation contains ambiguous arrangement names '{first}' and '{duplicate}'")]
    DuplicateArrangementName {
        /// First exact spelling.
        first: String,
        /// Conflicting exact spelling.
        duplicate: String,
    },
    /// An arrangement references a group absent from the presentation.
    #[error("arrangement '{arrangement}' references unknown cue group id '{group}'")]
    UnknownArrangementGroup {
        /// Arrangement containing the dangling reference.
        arrangement: String,
        /// Missing semantic group identity.
        group: String,
    },
    /// The selected arrangement is absent from the presentation.
    #[error("selected arrangement '{name}' is not declared by the presentation")]
    UnknownSelectedArrangement {
        /// Missing arrangement name.
        name: String,
    },
}

/// Stable semantic identity of one cue role.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CueRoleId(String);

impl CueRoleId {
    /// Parse one non-empty role identity.
    pub fn new(value: impl Into<String>) -> Result<Self, PresentationSpecError> {
        checked_identifier(value.into(), "cue role").map(Self)
    }

    /// Return the role identity text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Stable semantic identity of one presentation-local cue group.
///
/// This identity is deliberately separate from the operator-visible group
/// name. Native presentations may contain several distinct groups with the
/// same display name, while arrangements reference their local UUIDs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GroupId(String);

impl GroupId {
    /// Parse one non-empty semantic group identity.
    pub fn new(value: impl Into<String>) -> Result<Self, PresentationSpecError> {
        checked_identifier(value.into(), "cue group id").map(Self)
    }

    /// Conventional identity for presentations with one anonymous root group.
    pub fn root() -> Self {
        Self("__proflow_root".to_string())
    }

    /// Return the semantic group identity text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Checked operator-visible arrangement name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArrangementName(String);

impl ArrangementName {
    /// Parse one non-empty arrangement name.
    pub fn new(value: impl Into<String>) -> Result<Self, PresentationSpecError> {
        checked_identifier(value.into(), "arrangement name").map(Self)
    }

    /// Return the exact native arrangement name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Semantic text field bound to a native template element by a resolved role.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TextField(String);

impl TextField {
    /// Parse one non-empty semantic field name.
    pub fn new(value: impl Into<String>) -> Result<Self, PresentationSpecError> {
        checked_identifier(value.into(), "text field").map(Self)
    }

    /// Conventional field used by single-text-box templates.
    pub fn body() -> Self {
        Self("body".to_string())
    }

    /// Return the semantic field name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Checked native slide-action label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CueLabel(String);

impl CueLabel {
    /// Parse a non-empty label.
    pub fn new(value: impl Into<String>) -> Result<Self, PresentationSpecError> {
        let value = value.into();
        if value.is_empty() || value.trim() != value || value.chars().any(char::is_control) {
            Err(PresentationSpecError::InvalidCueLabel)
        } else {
            Ok(Self(value))
        }
    }

    /// Return the label text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Non-empty text values for one cue, keyed by semantic template field.
#[derive(Debug, Clone)]
pub struct TextBindings {
    values: BTreeMap<TextField, Vec<StyledSegment>>,
}

impl TextBindings {
    /// Bind one field.
    pub fn single(field: TextField, segments: Vec<StyledSegment>) -> Self {
        Self {
            values: BTreeMap::from([(field, segments)]),
        }
    }

    /// Bind several distinct fields.
    pub fn new(
        first: (TextField, Vec<StyledSegment>),
        rest: impl IntoIterator<Item = (TextField, Vec<StyledSegment>)>,
    ) -> Result<Self, PresentationSpecError> {
        let mut values = BTreeMap::from([first]);
        for (field, segments) in rest {
            if values.insert(field.clone(), segments).is_some() {
                return Err(PresentationSpecError::DuplicateTextField {
                    field: field.as_str().to_string(),
                });
            }
        }
        Ok(Self { values })
    }

    /// Iterate through field bindings in deterministic field-name order.
    pub fn iter(&self) -> impl Iterator<Item = (&TextField, &[StyledSegment])> {
        self.values
            .iter()
            .map(|(field, segments)| (field, segments.as_slice()))
    }
}

/// Content applied to one template slide.
#[derive(Debug, Clone)]
pub enum CueContent {
    /// Preserve the template exactly, without changing text fields.
    Static,
    /// Replace one or more explicitly bound text fields.
    Text(TextBindings),
}

/// One checked cue in a presentation.
#[derive(Debug, Clone)]
pub struct CueSpec {
    role: CueRoleId,
    content: CueContent,
    label: Option<CueLabel>,
}

impl CueSpec {
    /// Create one static template cue.
    pub const fn static_slide(role: CueRoleId) -> Self {
        Self {
            role,
            content: CueContent::Static,
            label: None,
        }
    }

    /// Create one cue with explicit text bindings.
    pub const fn text(role: CueRoleId, bindings: TextBindings) -> Self {
        Self {
            role,
            content: CueContent::Text(bindings),
            label: None,
        }
    }

    /// Attach a checked native slide-action label.
    #[must_use]
    pub fn with_label(mut self, label: CueLabel) -> Self {
        self.label = Some(label);
        self
    }

    /// Return the semantic role used by this cue.
    pub const fn role(&self) -> &CueRoleId {
        &self.role
    }

    /// Return the content applied to the role template.
    pub const fn content(&self) -> &CueContent {
        &self.content
    }

    /// Return the optional native slide-action label.
    pub const fn label(&self) -> Option<&CueLabel> {
        self.label.as_ref()
    }
}

/// One non-empty native cue group.
#[derive(Debug, Clone)]
pub struct GroupSpec {
    id: GroupId,
    name: Option<String>,
    first: CueSpec,
    rest: Vec<CueSpec>,
}

impl GroupSpec {
    /// Create one anonymous group containing at least one cue.
    pub fn anonymous(first: CueSpec, rest: Vec<CueSpec>) -> Self {
        Self::anonymous_with_id(GroupId::root(), first, rest)
    }

    /// Create one identified anonymous group containing at least one cue.
    pub const fn anonymous_with_id(id: GroupId, first: CueSpec, rest: Vec<CueSpec>) -> Self {
        Self {
            id,
            name: None,
            first,
            rest,
        }
    }

    /// Create one named group containing at least one cue.
    pub fn named(
        name: impl Into<String>,
        first: CueSpec,
        rest: Vec<CueSpec>,
    ) -> Result<Self, PresentationSpecError> {
        let name = checked_identifier(name.into(), "cue group name")?;
        Self::named_with_id(GroupId(name.clone()), name, first, rest)
    }

    /// Create one identified named group containing at least one cue.
    pub fn named_with_id(
        id: GroupId,
        name: impl Into<String>,
        first: CueSpec,
        rest: Vec<CueSpec>,
    ) -> Result<Self, PresentationSpecError> {
        Ok(Self {
            id,
            name: Some(checked_identifier(name.into(), "cue group name")?),
            first,
            rest,
        })
    }

    /// Return the stable presentation-local group identity.
    pub const fn id(&self) -> &GroupId {
        &self.id
    }

    /// Return the optional group display name.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Iterate through the non-empty cue sequence.
    pub fn cues(&self) -> impl Iterator<Item = &CueSpec> {
        std::iter::once(&self.first).chain(self.rest.iter())
    }
}

/// One non-empty ordered arrangement of presentation-local cue groups.
///
/// Repeated group references are intentional: a chorus may occur several times
/// in one arrangement while remaining one native cue group.
#[derive(Debug, Clone)]
pub struct ArrangementSpec {
    name: ArrangementName,
    first_group: GroupId,
    rest_groups: Vec<GroupId>,
}

impl ArrangementSpec {
    /// Create one named arrangement containing at least one group reference.
    pub fn new(
        name: impl Into<String>,
        first_group: GroupId,
        rest_groups: Vec<GroupId>,
    ) -> Result<Self, PresentationSpecError> {
        Ok(Self {
            name: ArrangementName::new(name)?,
            first_group,
            rest_groups,
        })
    }

    /// Return the exact operator-visible arrangement name.
    pub const fn name(&self) -> &ArrangementName {
        &self.name
    }

    /// Iterate through group references in exact operator traversal order.
    pub fn groups(&self) -> impl Iterator<Item = &GroupId> {
        std::iter::once(&self.first_group).chain(self.rest_groups.iter())
    }
}

/// One non-empty presentation ready for native rendering.
#[derive(Debug, Clone)]
pub struct PresentationSpec {
    name: String,
    first: GroupSpec,
    rest: Vec<GroupSpec>,
    arrangements: Vec<ArrangementSpec>,
    selected_arrangement: Option<ArrangementName>,
}

impl PresentationSpec {
    /// Create one presentation containing at least one non-empty cue group.
    pub fn new(
        name: impl Into<String>,
        first: GroupSpec,
        rest: Vec<GroupSpec>,
    ) -> Result<Self, PresentationSpecError> {
        let spec = Self {
            name: checked_identifier(name.into(), "presentation name")?,
            first,
            rest,
            arrangements: Vec::new(),
            selected_arrangement: None,
        };
        spec.validate_group_identities()?;
        Ok(spec)
    }

    /// Attach checked native arrangements and an optional selected arrangement.
    pub fn with_arrangements(
        mut self,
        arrangements: Vec<ArrangementSpec>,
        selected_arrangement: Option<ArrangementName>,
    ) -> Result<Self, PresentationSpecError> {
        let groups = self
            .groups()
            .map(|group| group.id().clone())
            .collect::<BTreeSet<_>>();
        let mut arrangement_names = BTreeMap::<String, String>::new();
        for arrangement in &arrangements {
            let canonical = arrangement.name().as_str().to_lowercase();
            if let Some(first) =
                arrangement_names.insert(canonical, arrangement.name().as_str().to_string())
            {
                return Err(PresentationSpecError::DuplicateArrangementName {
                    first,
                    duplicate: arrangement.name().as_str().to_string(),
                });
            }
            for group in arrangement.groups() {
                if !groups.contains(group) {
                    return Err(PresentationSpecError::UnknownArrangementGroup {
                        arrangement: arrangement.name().as_str().to_string(),
                        group: group.as_str().to_string(),
                    });
                }
            }
        }

        let selected_arrangement = selected_arrangement
            .map(|selected| {
                arrangements
                    .iter()
                    .find(|arrangement| {
                        arrangement.name().as_str().to_lowercase()
                            == selected.as_str().to_lowercase()
                    })
                    .map(|arrangement| arrangement.name().clone())
                    .ok_or_else(|| PresentationSpecError::UnknownSelectedArrangement {
                        name: selected.as_str().to_string(),
                    })
            })
            .transpose()?;

        self.arrangements = arrangements;
        self.selected_arrangement = selected_arrangement;
        Ok(self)
    }

    /// Return the native presentation name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Iterate through the non-empty group sequence.
    pub fn groups(&self) -> impl Iterator<Item = &GroupSpec> {
        std::iter::once(&self.first).chain(self.rest.iter())
    }

    /// Iterate through native arrangements in exact serialized order.
    pub fn arrangements(&self) -> impl Iterator<Item = &ArrangementSpec> {
        self.arrangements.iter()
    }

    /// Return the exact selected arrangement name, when one was declared.
    pub const fn selected_arrangement(&self) -> Option<&ArrangementName> {
        self.selected_arrangement.as_ref()
    }

    fn validate_group_identities(&self) -> Result<(), PresentationSpecError> {
        let mut identities = BTreeSet::new();
        for group in self.groups() {
            if !identities.insert(group.id()) {
                return Err(PresentationSpecError::DuplicateGroupId {
                    id: group.id().as_str().to_string(),
                });
            }
        }
        Ok(())
    }
}

fn checked_identifier(value: String, kind: &'static str) -> Result<String, PresentationSpecError> {
    if value.is_empty() || value.trim() != value || value.chars().any(char::is_control) {
        Err(PresentationSpecError::InvalidIdentifier { kind })
    } else {
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn checked_collections_are_nonempty_by_construction() {
        let role = CueRoleId::new("content").expect("valid role");
        let cue = CueSpec::static_slide(role);
        let group = GroupSpec::anonymous(cue, Vec::new());
        let presentation =
            PresentationSpec::new("Service", group, Vec::new()).expect("valid presentation");

        assert_eq!(presentation.groups().count(), 1);
        assert_eq!(
            presentation
                .groups()
                .next()
                .map(|group| group.cues().count()),
            Some(1)
        );
    }

    #[test]
    fn duplicate_text_fields_are_rejected() {
        let body = TextField::body();
        let result = TextBindings::new(
            (body.clone(), vec![StyledSegment::unstyled("first")]),
            [(body, vec![StyledSegment::unstyled("second")])],
        );

        assert!(matches!(
            result,
            Err(PresentationSpecError::DuplicateTextField { field }) if field == "body"
        ));
    }

    #[test]
    fn native_identifiers_reject_control_characters() {
        assert!(matches!(
            CueRoleId::new("content\nsecond"),
            Err(PresentationSpecError::InvalidIdentifier { kind: "cue role" })
        ));
        assert!(matches!(
            CueLabel::new("1-3\r"),
            Err(PresentationSpecError::InvalidCueLabel)
        ));
    }

    #[test]
    fn presentation_rejects_duplicate_semantic_group_identities() {
        let role = CueRoleId::new("content").expect("role");
        let group_id = GroupId::new("verse").expect("group id");
        let first = GroupSpec::anonymous_with_id(
            group_id.clone(),
            CueSpec::static_slide(role.clone()),
            Vec::new(),
        );
        let duplicate =
            GroupSpec::anonymous_with_id(group_id, CueSpec::static_slide(role), Vec::new());

        assert!(matches!(
            PresentationSpec::new("Song", first, vec![duplicate]),
            Err(PresentationSpecError::DuplicateGroupId { id }) if id == "verse"
        ));
    }

    #[test]
    fn duplicate_group_display_names_remain_distinct_by_semantic_id() {
        let role = CueRoleId::new("content").expect("role");
        let first = GroupSpec::named_with_id(
            GroupId::new("verse-early").expect("group id"),
            "Verse",
            CueSpec::static_slide(role.clone()),
            Vec::new(),
        )
        .expect("named group");
        let second = GroupSpec::named_with_id(
            GroupId::new("verse-late").expect("group id"),
            "Verse",
            CueSpec::static_slide(role),
            Vec::new(),
        )
        .expect("named group");

        let presentation =
            PresentationSpec::new("Song", first, vec![second]).expect("distinct group ids");

        assert_eq!(
            presentation
                .groups()
                .filter_map(GroupSpec::name)
                .collect::<Vec<_>>(),
            vec!["Verse", "Verse"]
        );
    }

    #[test]
    fn arrangement_references_are_checked_without_removing_repeats() {
        let role = CueRoleId::new("content").expect("role");
        let verse_id = GroupId::new("verse").expect("group id");
        let chorus_id = GroupId::new("chorus").expect("group id");
        let first = GroupSpec::anonymous_with_id(
            verse_id.clone(),
            CueSpec::static_slide(role.clone()),
            Vec::new(),
        );
        let chorus = GroupSpec::anonymous_with_id(
            chorus_id.clone(),
            CueSpec::static_slide(role),
            Vec::new(),
        );
        let arrangement = ArrangementSpec::new(
            "Default",
            verse_id.clone(),
            vec![chorus_id.clone(), verse_id, chorus_id],
        )
        .expect("arrangement");

        let presentation = PresentationSpec::new("Song", first, vec![chorus])
            .expect("presentation")
            .with_arrangements(
                vec![arrangement],
                Some(ArrangementName::new("default").expect("selection")),
            )
            .expect("checked arrangements");
        let arrangement = presentation.arrangements().next().expect("one arrangement");

        assert_eq!(
            arrangement
                .groups()
                .map(GroupId::as_str)
                .collect::<Vec<_>>(),
            vec!["verse", "chorus", "verse", "chorus"]
        );
        assert_eq!(
            presentation
                .selected_arrangement()
                .map(ArrangementName::as_str),
            Some("Default")
        );
    }

    #[test]
    fn arrangements_reject_dangling_groups_ambiguous_names_and_unknown_selection() {
        let make_presentation = || {
            PresentationSpec::new(
                "Song",
                GroupSpec::anonymous_with_id(
                    GroupId::new("verse").expect("group id"),
                    CueSpec::static_slide(CueRoleId::new("content").expect("role")),
                    Vec::new(),
                ),
                Vec::new(),
            )
            .expect("presentation")
        };
        let dangling = ArrangementSpec::new(
            "Default",
            GroupId::new("missing").expect("group id"),
            Vec::new(),
        )
        .expect("arrangement");
        assert!(matches!(
            make_presentation().with_arrangements(vec![dangling], None),
            Err(PresentationSpecError::UnknownArrangementGroup { group, .. })
                if group == "missing"
        ));

        let verse = GroupId::new("verse").expect("group id");
        let first =
            ArrangementSpec::new("Default", verse.clone(), Vec::new()).expect("arrangement");
        let duplicate = ArrangementSpec::new("default", verse, Vec::new()).expect("arrangement");
        assert!(matches!(
            make_presentation().with_arrangements(vec![first, duplicate], None),
            Err(PresentationSpecError::DuplicateArrangementName { .. })
        ));

        assert!(matches!(
            make_presentation().with_arrangements(
                Vec::new(),
                Some(ArrangementName::new("Missing").expect("selection")),
            ),
            Err(PresentationSpecError::UnknownSelectedArrangement { name })
                if name == "Missing"
        ));
    }
}
