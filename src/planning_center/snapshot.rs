//! Immutable normalized Planning Center input reviewed by one service build.

use std::fmt;

use chrono::{DateTime, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::identity::ResolvedPlanIdentity;
use super::types::Item;

/// Exact normalized Planning Center fields that can affect one service build.
///
/// This deliberately fingerprints the checked domain model rather than raw
/// JSON: transport metadata that cannot affect classification or rendering
/// does not invalidate an approval, while every consumed plan/item/song field
/// does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanSnapshot {
    plan_id: String,
    service_id: String,
    service_name: String,
    plan_title: String,
    date: DateTime<Utc>,
    default_playlist_name: String,
    items: Vec<Item>,
}

impl PlanSnapshot {
    pub(crate) fn from_resolved(identity: ResolvedPlanIdentity, items: Vec<Item>) -> Self {
        Self {
            plan_id: identity.plan_id,
            service_id: identity.service_id,
            service_name: identity.service_name,
            plan_title: identity.plan_title,
            date: identity.date,
            default_playlist_name: identity.default_playlist_name,
            items,
        }
    }

    /// Stable Planning Center plan identity.
    pub fn plan_id(&self) -> &str {
        &self.plan_id
    }

    /// Parent service-type identity required for a direct freshness refetch.
    pub fn service_id(&self) -> &str {
        &self.service_id
    }

    /// Authoritative service-type name.
    pub fn service_name(&self) -> &str {
        &self.service_name
    }

    /// Planning Center plan title.
    pub fn plan_title(&self) -> &str {
        &self.plan_title
    }

    /// Scheduled plan date and time.
    pub const fn date(&self) -> DateTime<Utc> {
        self.date
    }

    /// Canonical operator-facing playlist name derived from plan metadata.
    pub fn default_playlist_name(&self) -> &str {
        &self.default_playlist_name
    }

    /// Items in authoritative Planning Center sequence order.
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// Content revision for receipts and drift diagnostics.
    pub fn revision(&self) -> Result<PlanRevision, PlanRevisionError> {
        let bytes = serde_json::to_vec(self).map_err(PlanRevisionError::Serialize)?;
        Ok(PlanRevision(Sha256::digest(bytes).into()))
    }

    /// Prove that a direct refetch still represents the reviewed source.
    pub fn verify_current(&self, current: &Self) -> Result<(), PlanFreshnessError> {
        if self == current {
            return Ok(());
        }
        Err(PlanFreshnessError::Changed {
            plan_id: self.plan_id.clone(),
            expected: self.revision()?,
            actual: current.revision()?,
        })
    }

    #[cfg(test)]
    pub(crate) fn offline(plan_id: &str, service_name: &str) -> Self {
        Self {
            plan_id: plan_id.to_string(),
            service_id: "offline-service".to_string(),
            service_name: service_name.to_string(),
            plan_title: "Offline test plan".to_string(),
            date: DateTime::<Utc>::UNIX_EPOCH,
            default_playlist_name: "Offline test playlist".to_string(),
            items: Vec::new(),
        }
    }
}

/// SHA-256 of one normalized Planning Center plan snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct PlanRevision([u8; 32]);

impl fmt::Display for PlanRevision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// A normalized snapshot could not be serialized for revision evidence.
#[derive(Debug, thiserror::Error)]
pub enum PlanRevisionError {
    /// Serialization failed before hashing.
    #[error("failed to fingerprint normalized Planning Center plan: {0}")]
    Serialize(serde_json::Error),
}

/// Planning Center changed after the operator reviewed and prepared a build.
#[derive(Debug, thiserror::Error)]
pub enum PlanFreshnessError {
    /// Revision evidence could not be produced.
    #[error(transparent)]
    Revision(#[from] PlanRevisionError),
    /// At least one normalized build input changed.
    #[error(
        "Planning Center plan '{plan_id}' changed after preview (expected {expected}, found {actual}); preview again"
    )]
    Changed {
        /// Stable Planning Center plan identity.
        plan_id: String,
        /// Revision approved by preview.
        expected: PlanRevision,
        /// Revision observed immediately before commit.
        actual: PlanRevision,
    },
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use chrono::TimeZone;

    use super::*;
    use crate::planning_center::types::{Category, Song};

    fn snapshot() -> PlanSnapshot {
        PlanSnapshot {
            plan_id: "plan-1".to_string(),
            service_id: "service-1".to_string(),
            service_name: "9:00am contemporary".to_string(),
            plan_title: "July 26".to_string(),
            date: Utc
                .with_ymd_and_hms(2026, 7, 26, 13, 0, 0)
                .single()
                .expect("valid date"),
            default_playlist_name: "July 26, 2026 - 9am Contemporary".to_string(),
            items: vec![Item {
                id: "item-1".to_string(),
                position: 10,
                title: "Song".to_string(),
                description: None,
                category: Category::Song,
                note: None,
                song: Some(Song {
                    title: "Song".to_string(),
                    author: None,
                    copyright: None,
                    ccli: None,
                    themes: None,
                    lyrics: Some("lyrics".to_string()),
                    arrangement: Some("Default".to_string()),
                }),
                scripture: None,
            }],
        }
    }

    #[test]
    fn identical_normalized_plans_have_the_same_revision() {
        let first = snapshot();
        let second = first.clone();

        assert_eq!(
            first.revision().expect("first revision"),
            second.revision().expect("second revision")
        );
        assert!(first.verify_current(&second).is_ok());
    }

    #[test]
    fn consumed_item_changes_invalidate_the_review() {
        let reviewed = snapshot();
        let mut current = reviewed.clone();
        current.items[0].song.as_mut().expect("song").lyrics = Some("changed lyrics".to_string());

        assert!(matches!(
            reviewed.verify_current(&current),
            Err(PlanFreshnessError::Changed { plan_id, .. }) if plan_id == "plan-1"
        ));
    }
}
