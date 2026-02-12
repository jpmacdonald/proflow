//! Builder pattern for creating `ProPresenter` presentations.
//!
//! Provides a fluent API for constructing presentations with valid
//! reference chains between cues, groups, and arrangements.

#![allow(dead_code)]

use uuid::Uuid;
use crate::propresenter::data_model as dm;

/// Builder for creating `ProPresenter` presentations with valid reference chains
pub struct PresentationBuilder {
    presentation: dm::Presentation,
    cues: Vec<dm::Cue>,
    cue_groups: Vec<dm::CueGroup>,
    arrangements: Vec<dm::Arrangement>,
    selected_arrangement: Option<Uuid>,
}

impl PresentationBuilder {
    /// Create a new presentation builder
    pub fn new(name: &str) -> Self {
        Self {
            presentation: dm::Presentation {
                name: name.to_string(),
                uuid: Uuid::new_v4(),
                category: String::new(),
                notes: String::new(),
                ccli: None,
                bible_reference: None,
                cues: Vec::new(),
                cue_groups: Vec::new(),
                arrangements: Vec::new(),
                timeline: None,
                application_info: None,
                music_key: String::new(),
                music: None,
                slide_show: None,
                path: None,
                last_used: None,
                last_modified: None,
            },
            cues: Vec::new(),
            cue_groups: Vec::new(),
            arrangements: Vec::new(),
            selected_arrangement: None,
        }
    }

    /// Set the category
    #[must_use]
    pub fn with_category(mut self, category: &str) -> Self {
        self.presentation.category = category.to_string();
        self
    }

    /// Set the UUID
    #[must_use]
    pub const fn with_uuid(mut self, uuid: Uuid) -> Self {
        self.presentation.uuid = uuid;
        self
    }

    /// Add cues, ensuring they have valid UUIDs
    #[must_use]
    pub fn with_cues(mut self, cues: Vec<dm::Cue>) -> Self {
        // Ensure each cue has a UUID
        let cues = cues.into_iter().map(|mut cue| {
            if cue.uuid == Uuid::nil() {
                cue.uuid = Uuid::new_v4();
            }
            cue
        }).collect();
        self.cues = cues;
        self
    }

    /// Add cue groups, ensuring they reference valid cues
    #[must_use]
    pub fn with_cue_groups(mut self, groups: Vec<dm::CueGroup>) -> Self {
        // Validate and store cue groups
        let cue_uuids: Vec<Uuid> = self.cues.iter().map(|c| c.uuid).collect();
        
        let groups = groups.into_iter().map(|mut group| {
            // Ensure group has UUID
            if group.group.uuid == Uuid::nil() {
                group.group.uuid = Uuid::new_v4();
            }
            
            // Ensure application group identifier
            if group.group.application_group_identifier.is_empty() {
                group.group.application_group_identifier = Uuid::new_v4().to_string();
            }

            // Validate cue references
            group.cue_identifiers.retain(|cue_id| {
                cue_uuids.contains(cue_id)
            });

            group
        }).collect();

        self.cue_groups = groups;
        self
    }

    /// Add arrangements, ensuring they reference valid groups
    #[must_use]
    pub fn with_arrangements(mut self, arrangements: Vec<dm::Arrangement>) -> Self {
        // Get valid group UUIDs
        let group_uuids: Vec<Uuid> = self.cue_groups.iter()
            .map(|g| g.group.uuid)
            .collect();

        let arrangements = arrangements.into_iter().map(|mut arr| {
            // Ensure arrangement has UUID
            if arr.uuid == Uuid::nil() {
                arr.uuid = Uuid::new_v4();
            }

            // Validate group references
            arr.group_identifiers.retain(|group_id| {
                group_uuids.contains(group_id)
            });

            arr
        }).collect();

        self.arrangements = arrangements;
        self
    }

    /// Set the selected arrangement, must be a valid arrangement
    #[must_use]
    pub fn with_selected_arrangement(mut self, uuid: Uuid) -> Self {
        if self.arrangements.iter().any(|a| a.uuid == uuid) {
            self.selected_arrangement = Some(uuid);
        }
        self
    }

    /// Build the presentation, ensuring all references are valid
    pub fn build(mut self) -> Result<dm::Presentation, String> {
        // Validate we have at least one arrangement
        if self.arrangements.is_empty() {
            return Err("Presentation must have at least one arrangement".to_string());
        }

        // Set selected arrangement if not set
        if self.selected_arrangement.is_none() {
            self.selected_arrangement = Some(self.arrangements[0].uuid);
        }

        // Set up completion chain for cues
        let cue_uuids: Vec<Uuid> = self.cues.iter().map(|c| c.uuid).collect();
        for i in 0..self.cues.len() {
            if i < self.cues.len() - 1 {
                // Point to next cue
                self.cues[i].completion_target_type = dm::CompletionTargetType::Next;
                self.cues[i].completion_target_uuid = Some(cue_uuids[i + 1]);
            } else {
                // Last cue points to nothing
                self.cues[i].completion_target_type = dm::CompletionTargetType::None;
                self.cues[i].completion_target_uuid = None;
            }
            self.cues[i].completion_action_type = dm::CompletionActionType::First;
        }

        // Build final presentation
        self.presentation.cues = self.cues;
        self.presentation.cue_groups = self.cue_groups;
        self.presentation.arrangements = self.arrangements;
        
        Ok(self.presentation)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn test_builder_enforces_valid_references() {
        // Create a simple presentation
        let cue = dm::Cue {
            uuid: Uuid::new_v4(),
            name: "Test Cue".to_string(),
            actions: vec![],
            enabled: true,
            hot_key: None,
            completion_target_type: dm::CompletionTargetType::None,
            completion_target_uuid: None,
            completion_action_type: dm::CompletionActionType::First,
            completion_action_uuid: None,
            completion_time: 0.0,
        };

        let group = dm::CueGroup {
            group: dm::Group {
                uuid: Uuid::new_v4(),
                name: "Test Group".to_string(),
                color: dm::Color::default(),
                hot_key: None,
                application_group_identifier: String::new(),
            },
            cue_identifiers: vec![cue.uuid],
        };

        let arrangement = dm::Arrangement {
            uuid: Uuid::new_v4(),
            name: "Default".to_string(),
            group_identifiers: vec![group.group.uuid],
        };

        // Build presentation
        let result = PresentationBuilder::new("Test")
            .with_cues(vec![cue])
            .with_cue_groups(vec![group])
            .with_arrangements(vec![arrangement.clone()])
            .with_selected_arrangement(arrangement.uuid)
            .build();

        assert!(result.is_ok());
        let presentation = result.unwrap();

        // Verify references
        assert_eq!(presentation.arrangements.len(), 1);
        assert_eq!(presentation.cue_groups.len(), 1);
        assert_eq!(presentation.cues.len(), 1);

        // Verify chain
        let arr = &presentation.arrangements[0];
        let group_id = arr.group_identifiers[0];
        let group = presentation.cue_groups.iter().find(|g| g.group.uuid == group_id).unwrap();
        let cue_id = group.cue_identifiers[0];
        let _cue = presentation.cues.iter().find(|c| c.uuid == cue_id).unwrap();
    }
} 