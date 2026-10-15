//! Typed failures while normalizing Planning Center resources.

/// Malformed or internally inconsistent Planning Center source data.
///
/// HTTP success is not sufficient for semantic success. These errors keep an
/// incomplete JSON:API resource from being normalized into a different domain
/// concept, such as treating a declared song as a plain text item when its
/// included `Song` resource is missing.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PlanningCenterSourceError {
    /// A resource lacks a field the plan model requires.
    #[error("{resource} is missing required field '{field}'")]
    MissingField {
        /// Resource description, such as `plan 'plan-1'`.
        resource: String,
        /// Missing JSON field.
        field: &'static str,
    },
    /// A field is present with the wrong JSON type.
    #[error("{resource} field '{field}' must be {expected}")]
    InvalidFieldType {
        /// Resource description.
        resource: String,
        /// Offending JSON field.
        field: &'static str,
        /// Expected JSON type.
        expected: &'static str,
    },
    /// An identity field is blank, padded, or contains control characters.
    #[error(
        "{resource} field '{field}' must be non-empty, unpadded, and contain no control characters"
    )]
    InvalidIdentity {
        /// Resource description.
        resource: String,
        /// Offending JSON field.
        field: &'static str,
    },
    /// A plan has nothing to display as a title.
    #[error("{resource} has neither a usable 'title' nor 'dates' field")]
    MissingPlanTitle {
        /// Resource description.
        resource: String,
    },
    /// A plan's `sort_date` is not RFC 3339.
    #[error("{resource} has invalid RFC 3339 sort_date '{value}': {message}")]
    InvalidSortDate {
        /// Resource description.
        resource: String,
        /// Raw field value.
        value: String,
        /// Parser diagnostic.
        message: String,
    },
    /// The same included resource appears twice with different data.
    #[error("included {resource_type} id '{id}' has conflicting resource data")]
    ConflictingIncluded {
        /// JSON:API resource type.
        resource_type: &'static str,
        /// Resource identity.
        id: String,
    },
    /// A relationship is not object-or-null data with an id.
    #[error(
        "{resource} relationship '{relationship}' must contain object-or-null data with a valid id"
    )]
    MalformedRelationship {
        /// Resource description.
        resource: String,
        /// Relationship name.
        relationship: &'static str,
    },
    /// A relationship points at an included resource that was not provided.
    #[error(
        "{resource} declares {relationship} relationship '{target_id}', but included {target_type} '{target_id}' is missing"
    )]
    MissingIncludedRelationship {
        /// Resource description.
        resource: String,
        /// Relationship name.
        relationship: &'static str,
        /// Expected included resource type.
        target_type: &'static str,
        /// Referenced identity.
        target_id: String,
    },
    /// An item names an arrangement but no song.
    #[error(
        "{resource} declares arrangement relationship '{arrangement_id}' without a song relationship"
    )]
    ArrangementWithoutSong {
        /// Resource description.
        resource: String,
        /// Arrangement identity.
        arrangement_id: String,
    },
    /// Two items in one plan claim the same sequence position.
    #[error(
        "plan '{plan_id}' items '{first_item_id}' and '{duplicate_item_id}' both declare sequence {sequence}"
    )]
    DuplicateItemSequence {
        /// Plan identity.
        plan_id: String,
        /// Duplicated sequence number.
        sequence: usize,
        /// First item carrying the sequence.
        first_item_id: String,
        /// Second item carrying the sequence.
        duplicate_item_id: String,
    },
}
