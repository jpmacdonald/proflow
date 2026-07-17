//! Checked translation from Planning Center JSON:API resources into domain data.

use std::collections::{btree_map::Entry, BTreeMap, HashMap};

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::types::{Category, Item, Plan, Scripture, Service, Song};
use crate::error::Error;

type SourceResult<T> = std::result::Result<T, PlanningCenterSourceError>;

/// Malformed or internally inconsistent Planning Center source data.
///
/// HTTP success is not sufficient for semantic success. These errors keep an
/// incomplete JSON:API resource from being normalized into a different domain
/// concept, such as treating a declared song as a plain text item when its
/// included `Song` resource is missing.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(super) enum PlanningCenterSourceError {
    #[error("{resource} is missing required field '{field}'")]
    MissingField {
        resource: String,
        field: &'static str,
    },
    #[error("{resource} field '{field}' must be {expected}")]
    InvalidFieldType {
        resource: String,
        field: &'static str,
        expected: &'static str,
    },
    #[error(
        "{resource} field '{field}' must be non-empty, unpadded, and contain no control characters"
    )]
    InvalidIdentity {
        resource: String,
        field: &'static str,
    },
    #[error("{resource} has neither a usable 'title' nor 'dates' field")]
    MissingPlanTitle { resource: String },
    #[error("{resource} has invalid RFC 3339 sort_date '{value}': {message}")]
    InvalidSortDate {
        resource: String,
        value: String,
        message: String,
    },
    #[error("included {resource_type} id '{id}' has conflicting resource data")]
    ConflictingIncluded {
        resource_type: &'static str,
        id: String,
    },
    #[error(
        "{resource} relationship '{relationship}' must contain object-or-null data with a valid id"
    )]
    MalformedRelationship {
        resource: String,
        relationship: &'static str,
    },
    #[error(
        "{resource} declares {relationship} relationship '{target_id}', but included {target_type} '{target_id}' is missing"
    )]
    MissingIncludedRelationship {
        resource: String,
        relationship: &'static str,
        target_type: &'static str,
        target_id: String,
    },
    #[error(
        "{resource} declares arrangement relationship '{arrangement_id}' without a song relationship"
    )]
    ArrangementWithoutSong {
        resource: String,
        arrangement_id: String,
    },
    #[error(
        "plan '{plan_id}' items '{first_item_id}' and '{duplicate_item_id}' both declare sequence {sequence}"
    )]
    DuplicateItemSequence {
        plan_id: String,
        sequence: usize,
        first_item_id: String,
        duplicate_item_id: String,
    },
}

#[derive(Debug, Default)]
struct IncludedCatalog<'a> {
    songs: HashMap<&'a str, &'a Value>,
    arrangements: HashMap<&'a str, &'a Value>,
}

impl From<PlanningCenterSourceError> for Error {
    fn from(error: PlanningCenterSourceError) -> Self {
        Self::pco(format!("invalid Planning Center source data: {error}"))
    }
}

pub(super) fn parse_service_types(entries: &[Value]) -> SourceResult<Vec<Service>> {
    entries
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let resource = format!("service type at response index {index}");
            let id = required_identity(value, "id", &resource)?.to_string();
            let name = required_identity(&value["attributes"], "name", &resource)?.to_string();
            Ok(Service { id, name })
        })
        .collect()
}

pub(super) fn parse_plan(
    value: &Value,
    index: usize,
    service_id: &str,
    service_name: &str,
) -> SourceResult<Plan> {
    let resource = format!("service type '{service_id}' plan at response index {index}");
    let id = required_identity(value, "id", &resource)?.to_string();
    let attributes = &value["attributes"];
    let sort_date = required_identity(attributes, "sort_date", &resource)?;
    let date = DateTime::parse_from_rfc3339(sort_date)
        .map_err(|error| PlanningCenterSourceError::InvalidSortDate {
            resource: resource.clone(),
            value: sort_date.to_string(),
            message: error.to_string(),
        })?
        .with_timezone(&Utc);
    let title = optional_identity(attributes, "title", &resource)?
        .or(optional_identity(attributes, "dates", &resource)?)
        .ok_or_else(|| PlanningCenterSourceError::MissingPlanTitle {
            resource: resource.clone(),
        })?
        .to_string();

    Ok(Plan {
        id,
        service_id: service_id.to_string(),
        service_name: service_name.to_string(),
        date,
        title,
        items: Vec::new(),
    })
}

pub(super) fn parse_items(
    entries: &[Value],
    included: &[Value],
    plan_id: &str,
) -> SourceResult<Vec<Item>> {
    let included = build_included_catalog(included)?;
    let mut items_by_sequence = BTreeMap::new();
    for (response_index, value) in entries.iter().enumerate() {
        let item = parse_item(value, response_index, plan_id, &included)?;
        match items_by_sequence.entry(item.position) {
            Entry::Vacant(entry) => {
                entry.insert(item);
            }
            Entry::Occupied(entry) => {
                return Err(PlanningCenterSourceError::DuplicateItemSequence {
                    plan_id: plan_id.to_string(),
                    sequence: item.position,
                    first_item_id: entry.get().id.clone(),
                    duplicate_item_id: item.id,
                });
            }
        }
    }

    Ok(items_by_sequence.into_values().collect())
}

fn build_included_catalog(included: &[Value]) -> SourceResult<IncludedCatalog<'_>> {
    let mut catalog = IncludedCatalog::default();
    for (index, value) in included.iter().enumerate() {
        let resource = format!("included resource at response index {index}");
        let resource_type = required_identity(value, "type", &resource)?;
        let (resource_type, resources) = match resource_type {
            "Song" => ("Song", &mut catalog.songs),
            "Arrangement" => ("Arrangement", &mut catalog.arrangements),
            _ => continue,
        };
        let id = required_identity(value, "id", &resource)?;
        if let Some(first) = resources.get(id) {
            if *first != value {
                return Err(PlanningCenterSourceError::ConflictingIncluded {
                    resource_type,
                    id: id.to_string(),
                });
            }
        } else {
            resources.insert(id, value);
        }
    }
    Ok(catalog)
}

fn parse_item(
    value: &Value,
    index: usize,
    plan_id: &str,
    included: &IncludedCatalog<'_>,
) -> SourceResult<Item> {
    let resource = format!("plan '{plan_id}' item at response index {index}");
    let id = required_identity(value, "id", &resource)?.to_string();
    let attributes = &value["attributes"];
    let title = required_identity(attributes, "title", &resource)?.to_string();
    let sequence = required_sequence(attributes, &resource)?;
    let description = optional_text(attributes, "description", &resource)?;
    let note = optional_text(attributes, "notes", &resource)?;
    let song = parse_song(value.get("relationships"), included, &resource)?;
    let category = classify_item(&title, song.is_some());

    let scripture = if category == Category::Title && title.to_lowercase().contains("scripture") {
        crate::bible::parse_scripture_ref(&title).map(|reference| {
            let reference = if let Some(end) = reference.end_verse {
                format!(
                    "{} {}:{}-{}",
                    reference.book, reference.chapter, reference.start_verse, end
                )
            } else {
                format!(
                    "{} {}:{}",
                    reference.book, reference.chapter, reference.start_verse
                )
            };
            Scripture {
                reference,
                text: description.clone(),
                translation: None,
            }
        })
    } else {
        None
    };

    Ok(Item {
        id,
        position: sequence,
        title,
        description,
        category,
        note,
        song,
        scripture,
    })
}

fn required_sequence(attributes: &Value, resource: &str) -> SourceResult<usize> {
    let Some(value) = attributes.get("sequence") else {
        return Err(PlanningCenterSourceError::MissingField {
            resource: resource.to_string(),
            field: "sequence",
        });
    };
    let Some(sequence) = value.as_u64() else {
        return Err(PlanningCenterSourceError::InvalidFieldType {
            resource: resource.to_string(),
            field: "sequence",
            expected: "a non-negative integer representable as an item position",
        });
    };
    usize::try_from(sequence).map_err(|_| PlanningCenterSourceError::InvalidFieldType {
        resource: resource.to_string(),
        field: "sequence",
        expected: "a non-negative integer representable as an item position",
    })
}

fn parse_song(
    relationships: Option<&Value>,
    included: &IncludedCatalog<'_>,
    item_resource: &str,
) -> SourceResult<Option<Song>> {
    let song_id = relationship_id(relationships, "song", item_resource)?;
    let arrangement_id = relationship_id(relationships, "arrangement", item_resource)?;
    let Some(song_id) = song_id else {
        if let Some(arrangement_id) = arrangement_id {
            return Err(PlanningCenterSourceError::ArrangementWithoutSong {
                resource: item_resource.to_string(),
                arrangement_id: arrangement_id.to_string(),
            });
        }
        return Ok(None);
    };

    let song = included.songs.get(song_id).ok_or_else(|| {
        PlanningCenterSourceError::MissingIncludedRelationship {
            resource: item_resource.to_string(),
            relationship: "song",
            target_type: "Song",
            target_id: song_id.to_string(),
        }
    })?;
    let song_resource = format!("included Song '{song_id}' referenced by {item_resource}");
    let attributes = &song["attributes"];
    let title = required_identity(attributes, "title", &song_resource)?.to_string();
    let author = optional_text(attributes, "author", &song_resource)?;
    let copyright = optional_text(attributes, "copyright", &song_resource)?;
    let ccli = optional_string_or_number(attributes, "ccli_number", &song_resource)?;

    let (lyrics, arrangement) = if let Some(arrangement_id) = arrangement_id {
        let arrangement = included.arrangements.get(arrangement_id).ok_or_else(|| {
            PlanningCenterSourceError::MissingIncludedRelationship {
                resource: item_resource.to_string(),
                relationship: "arrangement",
                target_type: "Arrangement",
                target_id: arrangement_id.to_string(),
            }
        })?;
        let arrangement_resource =
            format!("included Arrangement '{arrangement_id}' referenced by {item_resource}");
        let attributes = &arrangement["attributes"];
        (
            optional_text(attributes, "lyrics", &arrangement_resource)?,
            Some(required_identity(attributes, "name", &arrangement_resource)?.to_string()),
        )
    } else {
        (None, None)
    };

    Ok(Some(Song {
        title,
        author,
        copyright,
        ccli,
        themes: None,
        lyrics,
        arrangement,
    }))
}

fn relationship_id<'a>(
    relationships: Option<&'a Value>,
    relationship: &'static str,
    resource: &str,
) -> SourceResult<Option<&'a str>> {
    let Some(relationships) = relationships else {
        return Ok(None);
    };
    let Some(relationships) = relationships.as_object() else {
        return Err(PlanningCenterSourceError::InvalidFieldType {
            resource: resource.to_string(),
            field: "relationships",
            expected: "an object",
        });
    };
    let Some(relationship_value) = relationships.get(relationship) else {
        return Ok(None);
    };
    let Some(data) = relationship_value.get("data") else {
        return Err(PlanningCenterSourceError::MalformedRelationship {
            resource: resource.to_string(),
            relationship,
        });
    };
    if data.is_null() {
        return Ok(None);
    }
    let Some(data) = data.as_object() else {
        return Err(PlanningCenterSourceError::MalformedRelationship {
            resource: resource.to_string(),
            relationship,
        });
    };
    let Some(id) = data.get("id").and_then(Value::as_str) else {
        return Err(PlanningCenterSourceError::MalformedRelationship {
            resource: resource.to_string(),
            relationship,
        });
    };
    validate_identity(id, relationship, resource)?;
    Ok(Some(id))
}

fn required_identity<'a>(
    object: &'a Value,
    field: &'static str,
    resource: &str,
) -> SourceResult<&'a str> {
    optional_identity(object, field, resource)?.ok_or_else(|| {
        PlanningCenterSourceError::MissingField {
            resource: resource.to_string(),
            field,
        }
    })
}

fn optional_identity<'a>(
    object: &'a Value,
    field: &'static str,
    resource: &str,
) -> SourceResult<Option<&'a str>> {
    let Some(value) = object.get(field) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let value = value
        .as_str()
        .ok_or_else(|| PlanningCenterSourceError::InvalidFieldType {
            resource: resource.to_string(),
            field,
            expected: "a string",
        })?;
    validate_identity(value, field, resource)?;
    Ok(Some(value))
}

fn validate_identity(value: &str, field: &'static str, resource: &str) -> SourceResult<()> {
    if value.is_empty() || value.trim() != value || value.chars().any(char::is_control) {
        Err(PlanningCenterSourceError::InvalidIdentity {
            resource: resource.to_string(),
            field,
        })
    } else {
        Ok(())
    }
}

fn optional_text(
    object: &Value,
    field: &'static str,
    resource: &str,
) -> SourceResult<Option<String>> {
    let Some(value) = object.get(field) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    value
        .as_str()
        .map(|value| Some(value.to_string()))
        .ok_or_else(|| PlanningCenterSourceError::InvalidFieldType {
            resource: resource.to_string(),
            field,
            expected: "a string or null",
        })
}

fn optional_string_or_number(
    object: &Value,
    field: &'static str,
    resource: &str,
) -> SourceResult<Option<String>> {
    let Some(value) = object.get(field) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    if let Some(value) = value.as_str() {
        return Ok(Some(value.to_string()));
    }
    if let Some(value) = value.as_i64() {
        return Ok(Some(value.to_string()));
    }
    Err(PlanningCenterSourceError::InvalidFieldType {
        resource: resource.to_string(),
        field,
        expected: "a string, integer, or null",
    })
}

fn classify_item(title: &str, has_song: bool) -> Category {
    if has_song {
        return Category::Song;
    }

    let lowercase_title = title.to_lowercase();
    if ["scripture", "reading", "sermon", "message"]
        .iter()
        .any(|category| lowercase_title.contains(category))
    {
        Category::Title
    } else if ["announcements", "welcome"]
        .iter()
        .any(|category| lowercase_title.contains(category))
    {
        Category::Graphic
    } else if [
        "PRE-SERVICE",
        "SERVICE",
        "POST-SERVICE",
        "PRAISE",
        "OFFERING",
        "GIVING",
        "PRAYER",
        "LORD'S PRAYER",
        "GREETING",
    ]
    .iter()
    .any(|heading| title.to_uppercase().contains(heading))
    {
        Category::Other
    } else {
        Category::Text
    }
}

#[cfg(test)]
#[path = "normalize/tests.rs"]
mod tests;
