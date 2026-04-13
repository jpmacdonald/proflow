//! Shared typed workflow plan model.

use serde::Serialize;

use super::description_parser::ParsedContent;
use crate::propresenter::SlideType;

/// Individual scripture reference within a multi-reference item.
#[derive(Debug, Clone, Serialize)]
pub struct ScriptureRefInfo {
    /// Parsed reference string (e.g., "Isaiah 35:1-6").
    pub reference: String,
    /// Bible version (e.g., "`NRSVue`").
    pub version: String,
}

/// Explicit action the workflow should take for an item.
#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanAction {
    /// Reuse an existing library file as-is.
    UseExisting,
    /// Update an existing file from parsed description content.
    EditInPlace,
    /// Generate a new file from the configured source.
    GenerateNew,
    /// Exclude the item from the build.
    #[default]
    Skip,
    /// Surface the item for user review before building.
    NeedsReview,
}

/// Broad item kind used by the shared workflow.
#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Song,
    Scripture,
    PersonNametag,
    #[default]
    Other,
}

/// Structured scripture source data for generated plans.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ScriptureContent {
    /// Single scripture reference for single-reference generation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    /// Requested Bible version for generation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bible_version: Option<String>,
    /// Multi-reference payload for combined scripture generation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<ScriptureRefInfo>,
}

/// Explicit source of content for a planned item.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContentSource {
    /// No generated content source is required.
    #[default]
    None,
    /// Content comes from parsed description text.
    Description {
        /// Parsed description content, when available.
        #[serde(skip_serializing_if = "Option::is_none")]
        parsed_content: Option<ParsedContent>,
    },
    /// Content comes from scripture lookup data.
    Scripture {
        /// Structured scripture request details.
        scripture: ScriptureContent,
    },
}

impl From<crate::project_config::ItemKind> for ItemKind {
    fn from(kind: crate::project_config::ItemKind) -> Self {
        match kind {
            crate::project_config::ItemKind::Song => Self::Song,
            crate::project_config::ItemKind::Scripture => Self::Scripture,
            crate::project_config::ItemKind::Nametag => Self::PersonNametag,
            _ => Self::Other,
        }
    }
}

/// Styling/rendering metadata associated with a plan.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PresentationStyle {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arrangement: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template_name: Option<String>,
    /// Separate title slide template (e.g. for scripture title cards).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_template: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub macro_name: Option<String>,
    /// Macro for content slides (after the title slide).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_macro: Option<String>,
}

/// Shared typed plan used by preview/build workflow stages.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ResolvedItemPlan {
    /// Stable workflow key for a single planned output.
    pub output_key: String,
    pub position: usize,
    pub pco_title: String,
    pub playlist_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    pub action: PlanAction,
    pub reason: String,
    pub item_kind: ItemKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_type: Option<String>,
    pub content_source: ContentSource,
    pub style: PresentationStyle,
}

impl ResolvedItemPlan {
    /// Stable key for the primary output generated from a PCO item position.
    pub fn primary_output_key(position: usize) -> String {
        format!("{position}:main")
    }

    /// Stable key for an expanded output generated from a rule step.
    pub fn expanded_output_key(position: usize, step_index: usize, use_type: &str) -> String {
        format!("{position}:expand:{step_index}:{use_type}")
    }

    /// Return parsed description content when this plan is description-driven.
    pub fn parsed_content(&self) -> Option<&ParsedContent> {
        match &self.content_source {
            ContentSource::Description { parsed_content } => parsed_content.as_ref(),
            _ => None,
        }
    }

    /// Return scripture generation data when this plan is scripture-driven.
    pub fn scripture_content(&self) -> Option<&ScriptureContent> {
        match &self.content_source {
            ContentSource::Scripture { scripture } => Some(scripture),
            _ => None,
        }
    }

    /// Resolve the `ProPresenter` slide type for the plan.
    pub const fn slide_type(&self) -> SlideType {
        match self.item_kind {
            ItemKind::Song => SlideType::Lyrics,
            ItemKind::Scripture => SlideType::Scripture,
            ItemKind::PersonNametag | ItemKind::Other => SlideType::Text,
        }
    }
}
