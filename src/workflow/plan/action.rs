//! Executable actions and complete proposals held for review.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use propresenter::SlideType;
use serde::Serialize;

use super::scripture::ScriptureContent;
use super::{ExistingTransform, RenderStyle, ResolvedBackground};
use crate::project_config::{BackgroundAssetPath, BackgroundId};
use crate::workflow::description_parser::ParsedContent;

/// One complete, executable presentation operation.
///
/// Every variant owns exactly the inputs its operation requires. Paths,
/// generated content, render styles, and arrangements therefore cannot drift
/// independently into contradictory combinations.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ReadyAction {
    /// Reuse one existing library presentation without rendering it.
    UseExisting {
        /// Exact presentation selected from the indexed library.
        file_path: PathBuf,
        /// Optional native arrangement selected from that presentation.
        #[serde(skip_serializing_if = "Option::is_none")]
        arrangement: Option<String>,
    },
    /// Apply one checked transform to an existing presentation in the selected
    /// staging library.
    RestyleExisting {
        /// Presentation selected from the configured staging library.
        file_path: PathBuf,
        /// Optional native arrangement selected from that presentation.
        #[serde(skip_serializing_if = "Option::is_none")]
        arrangement: Option<String>,
        /// Complete non-empty transform applied to the native presentation.
        transform: ExistingTransform,
    },
    /// Rebuild one existing presentation from parsed description content.
    EditDescription {
        /// Existing presentation whose owned envelope is preserved.
        file_path: PathBuf,
        /// Checked description content rendered into the presentation.
        parsed_content: ParsedContent,
        /// Rendering policy for the rebuilt presentation.
        style: RenderStyle,
    },
    /// Generate one new presentation from parsed description content.
    GenerateDescription {
        /// Checked description content rendered into the presentation.
        parsed_content: ParsedContent,
        /// Rendering policy for the new presentation.
        style: RenderStyle,
    },
    /// Generate one new presentation from local scripture data.
    GenerateScripture {
        /// Checked scripture source rendered into the presentation.
        scripture: ScriptureContent,
        /// Rendering policy for the new presentation.
        style: RenderStyle,
    },
    /// Generate one title-only presentation.
    GenerateTitle {
        /// Operator-visible title text.
        text: String,
        /// Rendering policy for the new presentation.
        style: RenderStyle,
    },
}

impl ReadyAction {
    pub(super) const fn required_slide_type(&self) -> Option<SlideType> {
        match self {
            Self::GenerateScripture { .. } => Some(SlideType::Scripture),
            Self::GenerateTitle { .. } => Some(SlideType::Title),
            Self::UseExisting { .. }
            | Self::RestyleExisting { .. }
            | Self::EditDescription { .. }
            | Self::GenerateDescription { .. } => None,
        }
    }

    pub(super) const fn operation_name(&self) -> &'static str {
        match self {
            Self::UseExisting { .. } => "use-existing",
            Self::RestyleExisting { .. } => "restyle-existing",
            Self::EditDescription { .. } => "edit-description",
            Self::GenerateDescription { .. } => "generated-description",
            Self::GenerateScripture { .. } => "generated-scripture",
            Self::GenerateTitle { .. } => "generated-title",
        }
    }

    /// Return the existing presentation read by this action, when any.
    pub fn file_path(&self) -> Option<&Path> {
        match self {
            Self::UseExisting { file_path, .. }
            | Self::RestyleExisting { file_path, .. }
            | Self::EditDescription { file_path, .. } => Some(file_path),
            Self::GenerateDescription { .. }
            | Self::GenerateScripture { .. }
            | Self::GenerateTitle { .. } => None,
        }
    }

    /// Return the rendering policy carried by rendered actions.
    pub const fn render_style(&self) -> Option<&RenderStyle> {
        match self {
            Self::EditDescription { style, .. }
            | Self::GenerateDescription { style, .. }
            | Self::GenerateScripture { style, .. }
            | Self::GenerateTitle { style, .. } => Some(style),
            Self::UseExisting { .. } | Self::RestyleExisting { .. } => None,
        }
    }

    /// Return the exact presentation background required by this action.
    pub const fn background(&self) -> Option<&ResolvedBackground> {
        match self {
            Self::RestyleExisting { transform, .. } => transform.replacement_background(),
            Self::EditDescription { style, .. }
            | Self::GenerateDescription { style, .. }
            | Self::GenerateScripture { style, .. }
            | Self::GenerateTitle { style, .. } => style.background(),
            Self::UseExisting { .. } => None,
        }
    }

    /// Use `file` for this action's background, keeping the background's
    /// identity. Does nothing for an action that has no background.
    pub(super) fn use_background_file(
        &mut self,
        files: &BTreeMap<BackgroundId, BackgroundAssetPath>,
    ) {
        let Some(replacement) = self.background().and_then(|background| {
            let file = files.get(background.id())?;
            (file != background.file())
                .then(|| ResolvedBackground::new(background.id().clone(), file.clone()))
        }) else {
            return;
        };
        match self {
            Self::RestyleExisting { transform, .. } => {
                *transform = transform.clone().with_replacement_background(replacement);
            }
            Self::EditDescription { style, .. }
            | Self::GenerateDescription { style, .. }
            | Self::GenerateScripture { style, .. }
            | Self::GenerateTitle { style, .. } => {
                *style = style.clone().with_background(replacement);
            }
            Self::UseExisting { .. } => {}
        }
    }

    /// Return the native arrangement selected by a read-only action.
    pub fn arrangement(&self) -> Option<&str> {
        match self {
            Self::UseExisting { arrangement, .. } | Self::RestyleExisting { arrangement, .. } => {
                arrangement.as_deref()
            }
            Self::EditDescription { .. }
            | Self::GenerateDescription { .. }
            | Self::GenerateScripture { .. }
            | Self::GenerateTitle { .. } => None,
        }
    }

    /// Return checked description content carried by this action.
    pub const fn parsed_content(&self) -> Option<&ParsedContent> {
        match self {
            Self::EditDescription { parsed_content, .. }
            | Self::GenerateDescription { parsed_content, .. } => Some(parsed_content),
            Self::UseExisting { .. }
            | Self::RestyleExisting { .. }
            | Self::GenerateScripture { .. }
            | Self::GenerateTitle { .. } => None,
        }
    }

    /// Return checked scripture content carried by this action.
    pub const fn scripture_content(&self) -> Option<&ScriptureContent> {
        match self {
            Self::GenerateScripture { scripture, .. } => Some(scripture),
            Self::UseExisting { .. }
            | Self::RestyleExisting { .. }
            | Self::EditDescription { .. }
            | Self::GenerateDescription { .. }
            | Self::GenerateTitle { .. } => None,
        }
    }

    /// Return title-only content carried by this action.
    pub fn title_text(&self) -> Option<&str> {
        match self {
            Self::GenerateTitle { text, .. } => Some(text),
            Self::UseExisting { .. }
            | Self::RestyleExisting { .. }
            | Self::EditDescription { .. }
            | Self::GenerateDescription { .. }
            | Self::GenerateScripture { .. } => None,
        }
    }
}

/// Incomplete or policy-blocked state shown for explicit human review.
///
/// A proposal is retained only when classification had already assembled one
/// complete action. This lets a collision or audit route that action to review
/// without dissolving it back into optional fields.
#[derive(Debug, Clone, Serialize)]
pub struct ReviewContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    proposal: Option<ReviewProposal>,
    /// Library files the review found and could not choose between.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    candidates: Vec<PathBuf>,
}

/// Song source and arrangement decisions require their own explicit resolution.
/// Styling decisions cannot satisfy either obligation.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "resolution", content = "action", rename_all = "snake_case")]
enum ReviewProposal {
    Resolved(ReadyAction),
    SongCandidate(ReadyAction),
    SongArrangement(ReadyAction),
}

impl ReviewContext {
    /// Create review state with an optional complete proposed action.
    pub fn new(proposed_action: Option<ReadyAction>) -> Self {
        Self {
            proposal: proposed_action.map(ReviewProposal::Resolved),
            candidates: Vec::new(),
        }
    }

    /// Review state for an item that several library files could be.
    pub(crate) fn file_choices(paths: &[String]) -> Self {
        Self {
            proposal: None,
            candidates: paths.iter().map(PathBuf::from).collect(),
        }
    }

    /// Library files the review could not choose between.
    pub fn candidates(&self) -> &[PathBuf] {
        &self.candidates
    }

    /// Retain a song suggestion whose identity needs an explicit file selection.
    pub(crate) const fn song_candidate(action: ReadyAction) -> Self {
        Self {
            proposal: Some(ReviewProposal::SongCandidate(action)),
            candidates: Vec::new(),
        }
    }

    /// Retain a resolved song source whose arrangement needs an explicit choice.
    pub(crate) const fn song_arrangement(action: ReadyAction) -> Self {
        Self {
            proposal: Some(ReviewProposal::SongArrangement(action)),
            candidates: Vec::new(),
        }
    }

    /// Return the complete proposed action, when classification found one.
    pub const fn proposed_action(&self) -> Option<&ReadyAction> {
        match &self.proposal {
            Some(
                ReviewProposal::Resolved(action)
                | ReviewProposal::SongCandidate(action)
                | ReviewProposal::SongArrangement(action),
            ) => Some(action),
            None => None,
        }
    }

    pub(super) const fn proposed_action_mut(&mut self) -> Option<&mut ReadyAction> {
        match &mut self.proposal {
            Some(
                ReviewProposal::Resolved(action)
                | ReviewProposal::SongCandidate(action)
                | ReviewProposal::SongArrangement(action),
            ) => Some(action),
            None => None,
        }
    }

    pub(super) const fn requires_song_selection(&self) -> bool {
        matches!(self.proposal, Some(ReviewProposal::SongCandidate(_)))
    }

    pub(super) const fn requires_song_arrangement(&self) -> bool {
        matches!(self.proposal, Some(ReviewProposal::SongArrangement(_)))
    }
}
