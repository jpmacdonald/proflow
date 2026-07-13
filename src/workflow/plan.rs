//! Shared typed workflow plan model.

use serde::Serialize;

use super::description_parser::ParsedContent;
use crate::project_config::{BackgroundAssetPath, BackgroundId};
use crate::propresenter::SlideType;

pub use crate::project_config::ItemKind;

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

/// One valid scripture source form for generated plans.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct ScriptureContent(ScriptureSource);

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
enum ScriptureSource {
    Single {
        reference: String,
        bible_version: String,
    },
    Combined {
        references: Vec<ScriptureRefInfo>,
    },
}

/// Borrowed view of a checked scripture source.
#[derive(Debug, Clone, Copy)]
pub enum ScriptureRequest<'a> {
    /// One passage using one explicit Bible version.
    Single {
        reference: &'a str,
        bible_version: &'a str,
    },
    /// Two or more explicitly versioned passages.
    Combined(&'a [ScriptureRefInfo]),
}

impl ScriptureContent {
    /// Create one passage with its required version.
    pub const fn single(reference: String, bible_version: String) -> Self {
        Self(ScriptureSource::Single {
            reference,
            bible_version,
        })
    }

    /// Create a combined source only when at least two passages are present.
    pub fn combined(references: Vec<ScriptureRefInfo>) -> Option<Self> {
        (references.len() >= 2).then_some(Self(ScriptureSource::Combined { references }))
    }

    /// Inspect the single valid source form carried by this value.
    pub fn request(&self) -> ScriptureRequest<'_> {
        match &self.0 {
            ScriptureSource::Single {
                reference,
                bible_version,
            } => ScriptureRequest::Single {
                reference,
                bible_version,
            },
            ScriptureSource::Combined { references } => {
                ScriptureRequest::Combined(references.as_slice())
            }
        }
    }
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

/// A background identifier resolved to the exact project asset used by a plan.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ResolvedBackground {
    id: BackgroundId,
    file: BackgroundAssetPath,
}

impl ResolvedBackground {
    /// Resolve a configured identifier/path pair into reviewed plan state.
    pub const fn new(id: BackgroundId, file: BackgroundAssetPath) -> Self {
        Self { id, file }
    }

    /// Return the project background identifier shown in previews.
    pub const fn id(&self) -> &BackgroundId {
        &self.id
    }

    /// Return the validated project-relative image path used during execution.
    pub const fn file(&self) -> &BackgroundAssetPath {
        &self.file
    }
}

/// Macro triggered when the operator enters a cue region.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CueMacro {
    enter: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    all_content_colored: Option<String>,
}

impl CueMacro {
    /// Build an entry macro and its optional all-colored-content variant.
    pub const fn new(enter: String, all_content_colored: Option<String>) -> Self {
        Self {
            enter,
            all_content_colored,
        }
    }

    /// Return the ordinary region-entry macro.
    pub fn enter(&self) -> &str {
        &self.enter
    }

    /// Select the explicit colored-content variant when applicable.
    pub fn select(&self, all_content_colored: bool) -> &str {
        if all_content_colored {
            self.all_content_colored.as_deref().unwrap_or(&self.enter)
        } else {
            &self.enter
        }
    }

    /// Return the optional all-colored-content variant.
    pub fn all_content_colored(&self) -> Option<&str> {
        self.all_content_colored.as_deref()
    }
}

/// Styling/rendering metadata associated with a rendered plan.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PresentationStyle {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<ResolvedBackground>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arrangement: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_slide: Option<String>,
    /// Separate cue-role slide used for a leading title cue.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_slide: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_cue_macro: Option<CueMacro>,
    /// Macro triggered on the first content cue after a title cue.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_content_cue_macro: Option<CueMacro>,
    /// Max logical lines per generated content slide.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_lines_per_slide: Option<usize>,
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
    /// Stable key for the primary output generated from a Planning Center item.
    pub fn primary_output_key(pco_item_id: &str) -> String {
        format!("pco:{pco_item_id}:main")
    }

    /// Stable key for an expanded output generated from a rule step.
    pub fn expanded_output_key(pco_item_id: &str, step_index: usize, use_type: &str) -> String {
        format!("pco:{pco_item_id}:expand:{step_index}:{use_type}")
    }

    /// Stable key for a project-required presentation inserted when absent.
    pub fn required_output_key(required_item_id: &str) -> String {
        format!("required:{required_item_id}")
    }

    /// Return parsed description content when this plan is description-driven.
    pub const fn parsed_content(&self) -> Option<&ParsedContent> {
        match &self.content_source {
            ContentSource::Description { parsed_content } => parsed_content.as_ref(),
            _ => None,
        }
    }

    /// Return scripture generation data when this plan is scripture-driven.
    pub const fn scripture_content(&self) -> Option<&ScriptureContent> {
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
            ItemKind::Nametag => SlideType::Title,
            ItemKind::Announcement | ItemKind::Graphic => SlideType::Graphic,
            ItemKind::Liturgy | ItemKind::Other => SlideType::Text,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn scripture_content_has_one_checked_source_form() {
        let single = ScriptureContent::single("John 3:16".to_string(), "NRSVue".to_string());
        assert!(matches!(
            single.request(),
            ScriptureRequest::Single {
                reference: "John 3:16",
                bible_version: "NRSVue"
            }
        ));

        assert!(ScriptureContent::combined(Vec::new()).is_none());
        assert!(ScriptureContent::combined(vec![ScriptureRefInfo {
            reference: "John 3:16".to_string(),
            version: "NRSVue".to_string(),
        }])
        .is_none());

        let combined = ScriptureContent::combined(vec![
            ScriptureRefInfo {
                reference: "Psalm 23:1-2".to_string(),
                version: "NRSVue".to_string(),
            },
            ScriptureRefInfo {
                reference: "John 3:16".to_string(),
                version: "NIV".to_string(),
            },
        ])
        .expect("two references form a valid combined source");
        assert!(matches!(
            combined.request(),
            ScriptureRequest::Combined(references) if references.len() == 2
        ));
    }

    #[test]
    fn scripture_content_serialization_preserves_preview_shape() {
        let value = serde_json::to_value(ScriptureContent::single(
            "Ephesians 4:4-6".to_string(),
            "NRSVue".to_string(),
        ))
        .expect("serialize scripture source");

        assert_eq!(
            value,
            serde_json::json!({
                "reference": "Ephesians 4:4-6",
                "bible_version": "NRSVue"
            })
        );
    }

    #[test]
    fn canonical_item_kinds_preserve_propresenter_slide_semantics() {
        for (item_kind, expected) in [
            (ItemKind::Song, SlideType::Lyrics),
            (ItemKind::Scripture, SlideType::Scripture),
            (ItemKind::Nametag, SlideType::Title),
            (ItemKind::Announcement, SlideType::Graphic),
            (ItemKind::Graphic, SlideType::Graphic),
            (ItemKind::Liturgy, SlideType::Text),
            (ItemKind::Other, SlideType::Text),
        ] {
            let plan = ResolvedItemPlan {
                item_kind,
                ..ResolvedItemPlan::default()
            };
            assert_eq!(plan.slide_type(), expected);
        }
    }
}
