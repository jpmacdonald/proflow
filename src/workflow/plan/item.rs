//! Scripture sources and executable service-item decisions.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{ExistingTransform, RenderStyle, ResolvedBackground};
use crate::propresenter::SlideType;
use crate::workflow::description_parser::ParsedContent;

pub use crate::project_config::ItemKind;

/// Stable identity for one planned workflow output.
///
/// Keys are checked at ad-hoc boundaries and generated through the three
/// canonical workflow forms. Dynamic key segments use collision-free percent
/// encoding for structural delimiters and characters that cannot appear in a
/// checked key.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct OutputKey(String);

/// Invalid ad-hoc output identity rejected before it enters a plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum OutputKeyError {
    /// No output identity was provided.
    #[error("output key cannot be blank")]
    Blank,
    /// The key carries semantically irrelevant surrounding whitespace.
    #[error("output key cannot contain surrounding whitespace")]
    Padded,
    /// The key contains a control character.
    #[error("output key cannot contain control characters")]
    ControlCharacter,
}

impl OutputKey {
    /// Check one ad-hoc output identity.
    pub fn new(value: String) -> Result<Self, OutputKeyError> {
        if value.trim().is_empty() {
            return Err(OutputKeyError::Blank);
        }
        if value.trim() != value {
            return Err(OutputKeyError::Padded);
        }
        if value.chars().any(char::is_control) {
            return Err(OutputKeyError::ControlCharacter);
        }
        Ok(Self(value))
    }

    /// Generate the primary output identity for a Planning Center item.
    pub fn primary(pco_item_id: &str) -> Self {
        Self(format!("pco:{}:main", encode_key_segment(pco_item_id)))
    }

    /// Generate one expanded output identity for a Planning Center rule step.
    pub fn expanded(pco_item_id: &str, step_index: usize, use_type: &str) -> Self {
        Self(format!(
            "pco:{}:expand:{step_index}:{}",
            encode_key_segment(pco_item_id),
            encode_key_segment(use_type)
        ))
    }

    /// Generate the identity for a project-required presentation.
    pub fn required(required_item_id: &str) -> Self {
        Self(format!("required:{}", encode_key_segment(required_item_id)))
    }

    /// Borrow the checked key text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OutputKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl TryFrom<String> for OutputKey {
    type Error = OutputKeyError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl PartialEq<str> for OutputKey {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for OutputKey {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

fn encode_key_segment(segment: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";

    let mut encoded = String::with_capacity(segment.len());
    for character in segment.chars() {
        if character == '%'
            || character == ':'
            || character.is_control()
            || character.is_whitespace()
        {
            let mut bytes = [0; 4];
            for byte in character.encode_utf8(&mut bytes).bytes() {
                encoded.push('%');
                encoded.push(char::from(HEX[usize::from(byte >> 4)]));
                encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
            }
        } else {
            encoded.push(character);
        }
    }
    encoded
}

/// Individual scripture reference within a multi-reference item.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ScriptureRefInfo {
    /// Parsed reference string (e.g., "Isaiah 35:1-6").
    reference: String,
    /// Bible version (e.g., "`NRSVue`").
    version: String,
}

impl ScriptureRefInfo {
    /// Create one explicitly versioned scripture reference.
    pub fn new(reference: String, version: String) -> Result<Self, ScripturePlanError> {
        validate_reference(&reference)?;
        validate_version(&version)?;
        Ok(Self { reference, version })
    }

    /// Return the parsed scripture reference.
    pub fn reference(&self) -> &str {
        &self.reference
    }

    /// Return the explicit Bible version.
    pub fn version(&self) -> &str {
        &self.version
    }
}

/// Invalid scripture identity data rejected before it enters an executable plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ScripturePlanError {
    /// No scripture reference was provided.
    #[error("scripture reference cannot be blank")]
    BlankReference,
    /// The reference carries semantically irrelevant surrounding whitespace.
    #[error("scripture reference cannot contain surrounding whitespace")]
    PaddedReference,
    /// The reference contains a control character.
    #[error("scripture reference cannot contain control characters")]
    ControlCharacterInReference,
    /// No Bible version was provided.
    #[error("Bible version cannot be blank")]
    BlankVersion,
    /// The version carries semantically irrelevant surrounding whitespace.
    #[error("Bible version cannot contain surrounding whitespace")]
    PaddedVersion,
    /// The version contains a control character.
    #[error("Bible version cannot contain control characters")]
    ControlCharacterInVersion,
    /// A partial scripture reference had no authoritative passage text.
    #[error("partial scripture description cannot be blank")]
    BlankExcerpt,
    /// The visible partial reference did not identify the lookup range's prefix.
    #[error(
        "partial scripture display reference must be the whole lookup reference followed by 'a'"
    )]
    MismatchedExcerptReference,
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
    PrefixExcerpt {
        reference: String,
        display_reference: String,
        bible_version: String,
        excerpt_text: String,
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
    /// One passage whose final verse is bounded by validated Planning Center text.
    PrefixExcerpt {
        /// Whole-verse local Bible range used for lookup and labels.
        reference: &'a str,
        /// Operator-visible reference retaining its partial-verse suffix.
        display_reference: &'a str,
        /// Explicit local Bible version.
        bible_version: &'a str,
        /// Authoritative Planning Center wording and cutoff.
        excerpt_text: &'a str,
    },
    /// Two or more explicitly versioned passages.
    Combined(&'a [ScriptureRefInfo]),
}

impl ScriptureContent {
    /// Create one passage with its required version.
    pub fn single(reference: String, bible_version: String) -> Result<Self, ScripturePlanError> {
        validate_reference(&reference)?;
        validate_version(&bible_version)?;
        Ok(Self(ScriptureSource::Single {
            reference,
            bible_version,
        }))
    }

    /// Create one description-bounded prefix excerpt over a whole-verse lookup.
    pub fn prefix_excerpt(
        reference: String,
        display_reference: String,
        bible_version: String,
        excerpt_text: String,
    ) -> Result<Self, ScripturePlanError> {
        validate_reference(&reference)?;
        validate_reference(&display_reference)?;
        validate_version(&bible_version)?;
        if display_reference != format!("{reference}a") {
            return Err(ScripturePlanError::MismatchedExcerptReference);
        }
        if excerpt_text.trim().is_empty() {
            return Err(ScripturePlanError::BlankExcerpt);
        }
        Ok(Self(ScriptureSource::PrefixExcerpt {
            reference,
            display_reference,
            bible_version,
            excerpt_text,
        }))
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
            ScriptureSource::PrefixExcerpt {
                reference,
                display_reference,
                bible_version,
                excerpt_text,
            } => ScriptureRequest::PrefixExcerpt {
                reference,
                display_reference,
                bible_version,
                excerpt_text,
            },
            ScriptureSource::Combined { references } => {
                ScriptureRequest::Combined(references.as_slice())
            }
        }
    }
}

fn validate_reference(reference: &str) -> Result<(), ScripturePlanError> {
    if reference.trim().is_empty() {
        return Err(ScripturePlanError::BlankReference);
    }
    if reference.trim() != reference {
        return Err(ScripturePlanError::PaddedReference);
    }
    if reference.chars().any(char::is_control) {
        return Err(ScripturePlanError::ControlCharacterInReference);
    }
    Ok(())
}

fn validate_version(version: &str) -> Result<(), ScripturePlanError> {
    if version.trim().is_empty() {
        return Err(ScripturePlanError::BlankVersion);
    }
    if version.trim() != version {
        return Err(ScripturePlanError::PaddedVersion);
    }
    if version.chars().any(char::is_control) {
        return Err(ScripturePlanError::ControlCharacterInVersion);
    }
    Ok(())
}

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
    proposed_action: Option<ReadyAction>,
}

impl ReviewContext {
    /// Create review state with an optional complete proposed action.
    pub const fn new(proposed_action: Option<ReadyAction>) -> Self {
        Self { proposed_action }
    }

    /// Return the complete proposed action, when classification found one.
    pub const fn proposed_action(&self) -> Option<&ReadyAction> {
        self.proposed_action.as_ref()
    }

    /// Consume the review state and return its proposed action.
    pub fn into_proposed_action(self) -> Option<ReadyAction> {
        self.proposed_action
    }
}

/// Readiness state for one planned playlist output.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", content = "details", rename_all = "snake_case")]
pub enum PlanDisposition {
    /// The action is complete and may cross the reviewed execution boundary.
    Ready(ReadyAction),
    /// The output is intentionally excluded from the playlist.
    Skip,
    /// The output cannot execute until a human resolves the stated reason.
    NeedsReview(ReviewContext),
}

/// Shared typed plan used by preview/build workflow stages.
#[derive(Debug, Clone, Serialize)]
pub struct ResolvedItemPlan {
    /// Stable workflow key for a single planned output.
    pub(crate) output_key: OutputKey,
    pub position: usize,
    pub pco_title: String,
    pub playlist_name: String,
    pub reason: String,
    pub item_kind: ItemKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_type: Option<String>,
    pub disposition: PlanDisposition,
}

impl ResolvedItemPlan {
    /// Return the stable key for this planned output.
    pub fn output_key(&self) -> &str {
        self.output_key.as_str()
    }

    /// Stable key for the primary output generated from a Planning Center item.
    pub fn primary_output_key(pco_item_id: &str) -> String {
        OutputKey::primary(pco_item_id).to_string()
    }

    /// Stable key for an expanded output generated from a rule step.
    pub fn expanded_output_key(pco_item_id: &str, step_index: usize, use_type: &str) -> String {
        OutputKey::expanded(pco_item_id, step_index, use_type).to_string()
    }

    /// Stable key for a project-required presentation inserted when absent.
    pub fn required_output_key(required_item_id: &str) -> String {
        OutputKey::required(required_item_id).to_string()
    }

    /// Return the action that can execute without further review.
    pub const fn ready_action(&self) -> Option<&ReadyAction> {
        match &self.disposition {
            PlanDisposition::Ready(action) => Some(action),
            PlanDisposition::Skip | PlanDisposition::NeedsReview(_) => None,
        }
    }

    /// Return the action used to populate preview details.
    ///
    /// Review items retain a complete proposal after collision and size audits,
    /// so previews can keep showing the exact file/content/style being reviewed.
    pub const fn preview_action(&self) -> Option<&ReadyAction> {
        match &self.disposition {
            PlanDisposition::Ready(action) => Some(action),
            PlanDisposition::NeedsReview(context) => context.proposed_action(),
            PlanDisposition::Skip => None,
        }
    }

    /// Return whether this plan is intentionally excluded from the playlist.
    pub const fn is_skipped(&self) -> bool {
        matches!(self.disposition, PlanDisposition::Skip)
    }

    /// Return whether this plan still requires an explicit human decision.
    pub const fn needs_review(&self) -> bool {
        matches!(self.disposition, PlanDisposition::NeedsReview(_))
    }

    /// Return the existing presentation shown by this plan, when any.
    pub fn file_path(&self) -> Option<&Path> {
        self.preview_action().and_then(ReadyAction::file_path)
    }

    /// Return rendering metadata shown by this plan, when any.
    pub const fn render_style(&self) -> Option<&RenderStyle> {
        match self.preview_action() {
            Some(action) => action.render_style(),
            None => None,
        }
    }

    /// Return the reviewed presentation background, including transforms that
    /// deliberately preserve slide content rather than carry a render style.
    pub const fn background(&self) -> Option<&ResolvedBackground> {
        match self.preview_action() {
            Some(action) => action.background(),
            None => None,
        }
    }

    /// Return the read-only arrangement shown by this plan, when any.
    pub fn arrangement(&self) -> Option<&str> {
        self.preview_action().and_then(ReadyAction::arrangement)
    }

    /// Return parsed description content when this plan is description-driven.
    pub const fn parsed_content(&self) -> Option<&ParsedContent> {
        match self.preview_action() {
            Some(action) => action.parsed_content(),
            None => None,
        }
    }

    /// Return scripture generation data when this plan is scripture-driven.
    pub const fn scripture_content(&self) -> Option<&ScriptureContent> {
        match self.preview_action() {
            Some(action) => action.scripture_content(),
            None => None,
        }
    }

    /// Move this plan behind review while retaining any complete action as the
    /// proposed resolution.
    pub fn require_review(&mut self, reason: String) {
        let previous = std::mem::replace(
            &mut self.disposition,
            PlanDisposition::NeedsReview(ReviewContext::new(None)),
        );
        let proposed_action = match previous {
            PlanDisposition::Ready(action) => Some(action),
            PlanDisposition::NeedsReview(context) => context.into_proposed_action(),
            PlanDisposition::Skip => None,
        };
        self.disposition = PlanDisposition::NeedsReview(ReviewContext::new(proposed_action));
        self.reason = reason;
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
