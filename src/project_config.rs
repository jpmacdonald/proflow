//! Project-level service build configuration.
//!
//! This module owns the config contract for headless runtime behavior.
//! Only the v4 schema is supported.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::num::NonZeroUsize;
use std::path::{Component, Path};
use std::str::FromStr;

/// Project config — v4 is the only supported schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    /// Schema version — must be 4.
    #[serde(default = "default_version")]
    pub version: u16,
    /// Optional descriptive metadata.
    #[serde(default)]
    pub metadata: ProjectMetadata,
    /// Runtime defaults shared by headless entrypoints.
    #[serde(default)]
    pub defaults: ProjectDefaults,
    /// Reusable service groups.
    #[serde(default)]
    pub service_groups: HashMap<String, ServiceGroupConfig>,
    /// Existing presentations that must occur in matching service playlists.
    #[serde(default)]
    pub required_playlist_items: Vec<RequiredPlaylistItemConfig>,
    /// Named background assets, relative to the project data root.
    #[serde(default)]
    pub backgrounds: HashMap<BackgroundId, BackgroundAssetPath>,
    /// Named cue roles that bind semantic slide regions to `ProPresenter` assets.
    #[serde(default)]
    pub cue_roles: HashMap<String, CueRoleConfig>,
    /// Named presentation types.
    #[serde(default)]
    pub presentation_types: HashMap<String, PresentationTypeConfig>,
    /// Ordered item rules.
    #[serde(default)]
    pub item_rules: Vec<ItemRuleConfig>,
    /// Known people metadata.
    #[serde(default)]
    pub people: HashMap<String, PersonConfig>,
    /// Structured override rules.
    #[serde(default)]
    pub overrides: Vec<OverrideRuleConfig>,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            version: default_version(),
            metadata: ProjectMetadata::default(),
            defaults: ProjectDefaults::default(),
            service_groups: HashMap::new(),
            required_playlist_items: Vec::new(),
            backgrounds: HashMap::new(),
            cue_roles: HashMap::new(),
            presentation_types: HashMap::new(),
            item_rules: Vec::new(),
            people: HashMap::new(),
            overrides: Vec::new(),
        }
    }
}

/// Descriptive project metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectMetadata {
    /// Human-readable project name.
    pub name: Option<String>,
    /// Default timezone identifier.
    pub timezone: Option<String>,
    /// Free-form notes.
    pub notes: Option<String>,
}

/// Project-wide defaults for runtime behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectDefaults {
    /// Default `ProPresenter` theme containing configured cue-role slides.
    pub theme: Option<String>,
    /// Default background asset identifier for rendered presentations.
    pub background: Option<BackgroundId>,
    /// Default lookahead window for builds.
    pub days_ahead: Option<i64>,
    /// Bible translation used only when a scripture item does not name one.
    pub bible_version: Option<crate::bible::BibleVersion>,
    /// Required slide-canvas size for generated and selected presentations.
    #[serde(default)]
    pub presentation_size: crate::propresenter::PresentationSize,
}

impl Default for ProjectDefaults {
    fn default() -> Self {
        Self {
            theme: None,
            background: None,
            days_ahead: None,
            bible_version: None,
            presentation_size: crate::propresenter::PresentationSize::FULL_HD,
        }
    }
}

/// Validated identifier for a project-owned background asset.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BackgroundId(String);

impl BackgroundId {
    /// Parse and validate a background identifier.
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        let bytes = value.as_bytes();
        if bytes.is_empty() {
            return Err("background id must not be empty".to_string());
        }
        if bytes.len() > 64 {
            return Err("background id must be at most 64 ASCII characters".to_string());
        }
        let valid_alnum = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
        if !valid_alnum(bytes[0]) {
            return Err(
                "background id must start with a lowercase ASCII letter or digit".to_string(),
            );
        }
        if !bytes[1..]
            .iter()
            .all(|byte| valid_alnum(*byte) || matches!(*byte, b'_' | b'-'))
        {
            return Err(
                "background id may contain only lowercase ASCII letters, digits, '_' and '-'"
                    .to_string(),
            );
        }
        Ok(Self(value))
    }

    /// Return the validated identifier text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for BackgroundId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for BackgroundId {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl Serialize for BackgroundId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for BackgroundId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Validated project-relative path to a supported background image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundAssetPath(String);

impl BackgroundAssetPath {
    /// Parse and validate a project-relative background image path.
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.is_empty() {
            return Err("background path must not be empty".to_string());
        }
        if value.chars().any(char::is_control) {
            return Err("background path must not contain control characters".to_string());
        }
        if value.contains('\\') {
            return Err("background path must use '/' separators".to_string());
        }
        if value
            .split('/')
            .next()
            .map(str::as_bytes)
            .is_some_and(|first_component| {
                first_component.len() == 2
                    && first_component[0].is_ascii_alphabetic()
                    && first_component[1] == b':'
            })
        {
            return Err("background path must be relative, without a drive prefix".to_string());
        }
        if value
            .split('/')
            .any(|component| component.is_empty() || matches!(component, "." | ".."))
        {
            return Err(
                "background path must contain only normal relative path components".to_string(),
            );
        }
        let path = Path::new(&value);
        if path.is_absolute()
            || !path
                .components()
                .all(|component| matches!(component, Component::Normal(_)))
        {
            return Err(
                "background path must contain only normal relative path components".to_string(),
            );
        }
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase);
        if !matches!(
            extension.as_deref(),
            Some("jpg" | "jpeg" | "png" | "tiff" | "tif")
        ) {
            return Err(
                "background path must use a jpg, jpeg, png, tiff, or tif extension".to_string(),
            );
        }
        Ok(Self(value))
    }

    /// Return the validated relative path.
    pub fn as_path(&self) -> &Path {
        Path::new(&self.0)
    }
}

impl Serialize for BackgroundAssetPath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for BackgroundAssetPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Named binding between a semantic cue role and `ProPresenter` assets.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CueRoleConfig {
    /// Theme slide name used for cues in this role.
    pub slide: String,
    /// Macro placed on the first cue entering this role.
    pub enter_macro: Option<String>,
    /// Alternate entry macro when every content segment is colored.
    pub all_content_colored_macro: Option<String>,
}

/// Binding between presentation regions and named cue roles.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DisplayBindingConfig {
    /// Every generated cue uses one role.
    Single {
        /// Named cue role.
        role: String,
    },
    /// The title cue and content cues use distinct roles.
    Split {
        /// Cue role for the title cue.
        title: String,
        /// Cue role for content cues.
        content: String,
    },
}

/// Named set of service types.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceGroupConfig {
    /// Service type names belonging to the group.
    #[serde(default)]
    pub service_types: Vec<String>,
}

/// One existing presentation that must appear in every matching playlist.
///
/// The runtime inserts it only when the exact resolved library file is absent,
/// so a Planning Center item and this invariant cannot create duplicates.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequiredPlaylistItemConfig {
    /// Stable key used in preview/build decisions.
    pub id: String,
    /// Existing/static presentation type that supplies slide semantics.
    pub use_type: String,
    /// Exact library filename, with or without the `.pro` suffix.
    pub library_file: String,
    /// Semantic playlist edge where a missing item is inserted.
    pub placement: RequiredPlaylistPlacement,
    /// Optional service group; absence means every service type.
    pub service_group: Option<String>,
}

/// Supported semantic positions for a required playlist item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequiredPlaylistPlacement {
    /// Before all Planning Center-derived entries.
    Start,
    /// After all Planning Center-derived entries.
    End,
}

/// Presentation type behavior.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresentationTypeConfig {
    /// Conceptual kind of output.
    #[serde(default)]
    pub kind: ItemKind,
    /// Source of content data.
    #[serde(default)]
    pub content_source: ContentSourceKind,
    /// Parser used when `content_source` is `description`.
    pub description_parser: Option<DescriptionParserKind>,
    /// Output behavior for the content.
    #[serde(default)]
    pub output_strategy: OutputStrategy,
    /// Cue-role binding used when rendering presentation content.
    pub display: Option<DisplayBindingConfig>,
    /// Background asset identifier used when rendering presentation content.
    pub background: Option<BackgroundId>,
    /// Arrangement override.
    pub arrangement: Option<String>,
    /// Maximum logical lines per generated content slide.
    pub max_lines_per_slide: Option<NonZeroUsize>,
    /// Human-readable description.
    pub description: Option<String>,
}

/// Ordered matching rule for items.
#[derive(Debug, Clone)]
pub struct ItemRuleConfig {
    /// Stable rule identifier.
    pub id: String,
    /// Match criteria.
    pub match_spec: MatchSpec,
    /// The single outcome produced by a match.
    pub outcome: ItemRuleOutcome,
    /// Free-form notes.
    pub notes: Option<String>,
}

/// The single outcome produced by an item rule.
///
/// This is serialized using flat `use_type`, `action`, `decision`, or `expand`
/// fields. In memory, the enum prevents absent and contradictory outcomes.
#[derive(Debug, Clone)]
pub enum ItemRuleOutcome {
    /// Resolve the matched item through one presentation type.
    UseType {
        /// Presentation type key.
        type_key: String,
        /// Optional target information for the presentation.
        target: Option<TargetSpec>,
    },
    /// Apply an explicit skip or review action.
    Action(RuleAction),
    /// Resolve a bounded contextual decision.
    Decision(DecisionConfig),
    /// Produce multiple ordered outputs.
    Expand(ExpansionRule),
}

/// A non-empty sequence of expansion steps.
#[derive(Debug, Clone)]
pub struct ExpansionRule {
    first: ExpansionStep,
    rest: Vec<ExpansionStep>,
}

impl ExpansionRule {
    /// Build a non-empty expansion sequence.
    pub const fn new(first: ExpansionStep, rest: Vec<ExpansionStep>) -> Self {
        Self { first, rest }
    }

    /// Iterate through expansion steps in configured order.
    pub fn iter(&self) -> impl Iterator<Item = &ExpansionStep> {
        std::iter::once(&self.first).chain(self.rest.iter())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ItemRuleConfigWire {
    id: String,
    #[serde(rename = "match", default)]
    match_spec: MatchSpec,
    use_type: Option<String>,
    action: Option<RuleAction>,
    decision: Option<DecisionConfig>,
    #[serde(default)]
    expand: Vec<ExpansionStep>,
    target: Option<TargetSpec>,
    notes: Option<String>,
}

impl<'de> Deserialize<'de> for ItemRuleConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = ItemRuleConfigWire::deserialize(deserializer)?;
        let outcome_count = usize::from(wire.use_type.is_some())
            + usize::from(wire.action.is_some())
            + usize::from(wire.decision.is_some())
            + usize::from(!wire.expand.is_empty());
        if outcome_count != 1 {
            return Err(serde::de::Error::custom(
                "must define exactly one outcome: action, decision, use_type, or non-empty expand",
            ));
        }
        if wire.target.is_some() && wire.use_type.is_none() {
            return Err(serde::de::Error::custom(
                "target is only valid with a use_type outcome; expansion targets belong on each step",
            ));
        }

        let outcome = match (wire.use_type, wire.action, wire.decision) {
            (Some(type_key), None, None) => ItemRuleOutcome::UseType {
                type_key,
                target: wire.target,
            },
            (None, Some(action), None) => ItemRuleOutcome::Action(action),
            (None, None, Some(decision)) => ItemRuleOutcome::Decision(decision),
            (None, None, None) => {
                let mut steps = wire.expand.into_iter();
                let first = steps.next().ok_or_else(|| {
                    serde::de::Error::custom("expand outcome must contain at least one step")
                })?;
                ItemRuleOutcome::Expand(ExpansionRule::new(first, steps.collect()))
            }
            _ => {
                return Err(serde::de::Error::custom(
                    "must define exactly one outcome: action, decision, use_type, or non-empty expand",
                ));
            }
        };

        Ok(Self {
            id: wire.id,
            match_spec: wire.match_spec,
            outcome,
            notes: wire.notes,
        })
    }
}

impl Serialize for ItemRuleConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;

        let field_count =
            3 + usize::from(matches!(
                &self.outcome,
                ItemRuleOutcome::UseType {
                    target: Some(_),
                    ..
                }
            )) + usize::from(self.notes.is_some());
        let mut map = serializer.serialize_map(Some(field_count))?;
        map.serialize_entry("id", &self.id)?;
        map.serialize_entry("match", &self.match_spec)?;
        match &self.outcome {
            ItemRuleOutcome::UseType { type_key, target } => {
                map.serialize_entry("use_type", type_key)?;
                if let Some(target) = target {
                    map.serialize_entry("target", target)?;
                }
            }
            ItemRuleOutcome::Action(action) => map.serialize_entry("action", action)?,
            ItemRuleOutcome::Decision(decision) => map.serialize_entry("decision", decision)?,
            ItemRuleOutcome::Expand(expansion) => {
                map.serialize_entry("expand", &expansion.iter().collect::<Vec<_>>())?;
            }
        }
        if let Some(notes) = &self.notes {
            map.serialize_entry("notes", notes)?;
        }
        map.end()
    }
}

/// Match criteria for a rule.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatchSpec {
    /// Lowercased title prefixes.
    #[serde(default)]
    pub title_prefix: Vec<String>,
    /// Title substrings.
    #[serde(default)]
    pub title_contains: Vec<String>,
    /// Description substrings.
    #[serde(default)]
    pub description_contains: Vec<String>,
    /// Optional category string.
    pub category: Option<String>,
    /// Whether the item contains a scripture reference.
    pub has_scripture_ref: Option<bool>,
    /// Restrict the rule to specific service types.
    #[serde(default)]
    pub service_type: Vec<String>,
}

impl MatchSpec {
    const fn is_empty(&self) -> bool {
        self.title_prefix.is_empty()
            && self.title_contains.is_empty()
            && self.description_contains.is_empty()
            && self.category.is_none()
            && self.has_scripture_ref.is_none()
            && self.service_type.is_empty()
    }
}

/// Bounded contextual decision made from PCO fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DecisionConfig {
    /// Choose one existing library file from configured choices.
    ChooseExistingFile {
        /// PCO fields available for the choice (`title`, `description`, `note`).
        #[serde(default)]
        context_fields: Vec<String>,
        /// Human-readable instructions surfaced in review contexts.
        instructions: Option<String>,
        /// Allowed choices.
        #[serde(default)]
        choices: HashMap<String, DecisionChoiceConfig>,
        /// What to do when no choice or multiple choices match.
        on_ambiguous: Option<AmbiguousDecisionPolicy>,
    },
}

/// One allowed contextual choice.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionChoiceConfig {
    /// Presentation type to use when this choice is selected.
    pub use_type: Option<String>,
    /// Target file for the selected choice.
    pub target: Option<TargetSpec>,
    /// Convenience alias for `target.library_file`.
    pub file: Option<String>,
    /// Text matchers used for deterministic pre-selection.
    #[serde(rename = "match", default)]
    pub match_spec: DecisionChoiceMatch,
}

/// Text matchers for a contextual decision choice.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionChoiceMatch {
    /// Any of these tokens/phrases may select the choice.
    #[serde(default)]
    pub any: Vec<String>,
    /// All of these tokens/phrases must be present to select the choice.
    #[serde(default)]
    pub all: Vec<String>,
    /// None of these tokens/phrases may be present.
    #[serde(default)]
    pub none: Vec<String>,
}

/// Ambiguous contextual decision behavior.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AmbiguousDecisionPolicy {
    /// Ask the user/LLM supervisor before building.
    #[default]
    Ask,
    /// Skip the item.
    Skip,
}

/// Explicit rule action.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuleAction {
    /// Skip the item entirely.
    Skip {
        /// Human-readable reason for skipping the item.
        reason: String,
    },
    /// Mark the item as requiring review.
    Review {
        /// Human-readable reason the item needs review.
        reason: String,
    },
}

/// Expansion step used by multi-output rules.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpansionStep {
    /// Presentation type to use for this step.
    pub use_type: String,
    /// Optional speaker source.
    pub speaker: Option<SpeakerSource>,
    /// Optional explicit target for this step.
    pub target: Option<TargetSpec>,
}

/// How a speaker value should be resolved.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeakerSource {
    /// Use the resolved speaker for the matched item.
    Resolved,
}

/// One explicit output target.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum TargetSpec {
    /// Resolve one exact existing library filename.
    ExistingFile {
        /// Explicit library filename to read.
        library_file: String,
    },
    /// Generate a presentation name from the matched item.
    GeneratedName {
        /// Dynamic name template for the generated file.
        name_template: String,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetSpecWire {
    library_file: Option<String>,
    name_template: Option<String>,
}

impl<'de> Deserialize<'de> for TargetSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = TargetSpecWire::deserialize(deserializer)?;
        match (wire.library_file, wire.name_template) {
            (Some(library_file), None) => Ok(Self::ExistingFile { library_file }),
            (None, Some(name_template)) => Ok(Self::GeneratedName { name_template }),
            _ => Err(serde::de::Error::custom(
                "target must define exactly one of library_file or name_template",
            )),
        }
    }
}

impl TargetSpec {
    /// Existing library filename, when this target selects one.
    pub fn library_file(&self) -> Option<&str> {
        match self {
            Self::ExistingFile { library_file } => Some(library_file),
            Self::GeneratedName { .. } => None,
        }
    }

    /// Generated name template, when this target creates one.
    pub fn name_template(&self) -> Option<&str> {
        match self {
            Self::ExistingFile { .. } => None,
            Self::GeneratedName { name_template } => Some(name_template),
        }
    }
}

/// Known person metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersonConfig {
    /// Last name.
    pub last: Option<String>,
    /// Role label.
    pub role: Option<String>,
    /// Preferred nametag filename.
    pub nametag: Option<String>,
}

/// Structured override rule.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OverrideRuleConfig {
    /// When this override applies.
    pub when: OverrideWhen,
    /// Arrangement override.
    pub arrangement: Option<String>,
    /// Background override.
    pub background: Option<BackgroundId>,
}

/// Conditions under which an override applies.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OverrideWhen {
    /// Named service group.
    pub service_group: Option<String>,
    /// Exact service type.
    pub service_type: Option<String>,
    /// Presentation type key.
    pub presentation_type: Option<String>,
}

/// Where content data comes from.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContentSourceKind {
    /// Reuse static content from an existing file.
    #[default]
    Static,
    /// Parse content from a Planning Center description.
    Description,
    /// Generate from Bible data.
    Scripture,
    /// Use linked song metadata / existing library content.
    Song,
}

/// Parser for description-backed presentation content.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DescriptionParserKind {
    /// Liturgical text, including responsive and marker-based descriptions.
    Liturgical,
    /// A title/composer/performer content nametag.
    ContentNametag,
}

/// What the runtime should do with the resolved content.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutputStrategy {
    /// Skip this item.
    Skip,
    /// Use an existing file unchanged.
    UseExisting,
    /// Overwrite an existing file in place.
    EditInPlace,
    /// Generate a new file.
    GenerateNew,
    /// Require review before choosing behavior.
    #[default]
    NeedsReview,
}

/// Conceptual kind of item/presentation.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    /// Song/lyrics item.
    Song,
    /// Scripture item.
    Scripture,
    /// Liturgical text.
    Liturgy,
    /// Speaker or content nametag.
    Nametag,
    /// Announcement/graphic item.
    Announcement,
    /// Graphic-only item.
    Graphic,
    /// Fallback kind.
    #[default]
    Other,
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

/// Error returned while reading project config.
#[derive(Debug, thiserror::Error)]
pub enum ProjectConfigLoadError {
    /// Failed to read the config file.
    #[error("failed to read project config: {0}")]
    Io(#[from] std::io::Error),
    /// Failed to parse the config file.
    #[error("failed to parse project config: {0}")]
    Parse(#[from] serde_json::Error),
    /// Encountered an unsupported or missing config version.
    #[error("unsupported project config version: {0} — migrate to v4")]
    UnsupportedVersion(u64),
    /// Config is missing a version field entirely.
    #[error("config has no version field — migrate to v4")]
    MissingVersion,
    /// Config parsed successfully but violates its domain contract.
    #[error("invalid project config: {0}")]
    Invalid(String),
}

/// Load project config from a file path.
pub fn load_project_config(path: &Path) -> Result<ProjectConfig, ProjectConfigLoadError> {
    let text = std::fs::read_to_string(path)?;
    parse_project_config_str(&text)
}

/// Parse project config from a JSON value.
pub fn parse_project_config_value(
    value: serde_json::Value,
) -> Result<ProjectConfig, ProjectConfigLoadError> {
    match value.get("version").and_then(serde_json::Value::as_u64) {
        Some(4) => {
            let config = serde_json::from_value(value)?;
            let issues = validate_project_config(&config);
            if issues.is_empty() {
                Ok(config)
            } else {
                Err(ProjectConfigLoadError::Invalid(format_validation_issues(
                    &issues,
                )))
            }
        }
        Some(version) => Err(ProjectConfigLoadError::UnsupportedVersion(version)),
        None => Err(ProjectConfigLoadError::MissingVersion),
    }
}

/// Parse project config from a JSON string.
pub fn parse_project_config_str(json: &str) -> Result<ProjectConfig, ProjectConfigLoadError> {
    let value: serde_json::Value = serde_json::from_str(json)?;
    parse_project_config_value(value)
}

/// Serialize project config to pretty JSON with a trailing newline.
pub fn serialize_project_config(config: &ProjectConfig) -> Result<String, serde_json::Error> {
    let mut json = serde_json::to_string_pretty(config)?;
    json.push('\n');
    Ok(json)
}

/// Write project config atomically to disk.
pub fn write_project_config(path: &Path, config: &ProjectConfig) -> std::io::Result<()> {
    let issues = validate_project_config(config);
    if !issues.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "invalid project config: {}",
                format_validation_issues(&issues)
            ),
        ));
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;

    let json = serialize_project_config(config)
        .map_err(|err| std::io::Error::other(format!("serialize project config: {err}")))?;
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("proflow.config.json");
    let temp_path = parent.join(format!(".{filename}.{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp_path, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    result
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// A validation issue found in the loaded project config.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigValidationIssue {
    /// Approximate config path where the issue was detected.
    pub path: String,
    /// Human-readable validation message.
    pub message: String,
}

/// Validate project config references and report issues.
pub fn validate_project_config(config: &ProjectConfig) -> Vec<ConfigValidationIssue> {
    let mut issues = Vec::new();

    if config.version != 4 {
        issues.push(ConfigValidationIssue {
            path: "version".to_string(),
            message: format!("unsupported version {}; expected 4", config.version),
        });
    }
    validate_runtime_defaults(config, &mut issues);
    validate_background_references(config, &mut issues);
    validate_cue_roles(config, &mut issues);
    validate_item_rules(config, &mut issues);
    validate_required_playlist_items(config, &mut issues);
    validate_presentation_types(config, &mut issues);
    validate_overrides(config, &mut issues);

    issues
}

fn validate_presentation_types(config: &ProjectConfig, issues: &mut Vec<ConfigValidationIssue>) {
    for (type_key, ptype) in &config.presentation_types {
        match (ptype.content_source, ptype.description_parser) {
            (ContentSourceKind::Description, None) => issues.push(ConfigValidationIssue {
                path: format!("presentation_types.{type_key}.description_parser"),
                message: "description content requires an explicit description_parser".to_string(),
            }),
            (ContentSourceKind::Description, Some(_)) | (_, None) => {}
            (_, Some(_)) => issues.push(ConfigValidationIssue {
                path: format!("presentation_types.{type_key}.description_parser"),
                message: "description_parser is only valid for description content".to_string(),
            }),
        }
        if let Some(display) = &ptype.display {
            validate_display_binding(config, type_key, display, issues);
        }
        if let Some(background) = &ptype.background {
            if !config.backgrounds.contains_key(background) {
                issues.push(ConfigValidationIssue {
                    path: format!("presentation_types.{type_key}.background"),
                    message: format!("references unknown background '{background}'"),
                });
            }
        }
        validate_presentation_kind_and_source(type_key, ptype, issues);
        validate_content_output_combination(type_key, ptype, issues);
        if ptype.arrangement.is_some()
            && !matches!(ptype.output_strategy, OutputStrategy::UseExisting)
        {
            issues.push(ConfigValidationIssue {
                path: format!("presentation_types.{type_key}.arrangement"),
                message: "arrangement is only valid for use_existing presentations".to_string(),
            });
        }
        match ptype.output_strategy {
            OutputStrategy::UseExisting => {
                for (field, configured) in [
                    ("display", ptype.display.is_some()),
                    ("background", ptype.background.is_some()),
                    ("max_lines_per_slide", ptype.max_lines_per_slide.is_some()),
                ] {
                    if configured {
                        issues.push(ConfigValidationIssue {
                            path: format!("presentation_types.{type_key}.{field}"),
                            message: format!(
                                "{field} is not valid for use_existing because existing files are read-only"
                            ),
                        });
                    }
                }
            }
            OutputStrategy::GenerateNew if ptype.display.is_none() => {
                issues.push(ConfigValidationIssue {
                    path: format!("presentation_types.{type_key}.display"),
                    message: "generate_new requires a display binding".to_string(),
                });
            }
            OutputStrategy::EditInPlace if ptype.display.is_none() => {
                issues.push(ConfigValidationIssue {
                    path: format!("presentation_types.{type_key}.display"),
                    message: "edit_in_place requires a display binding".to_string(),
                });
            }
            OutputStrategy::Skip
            | OutputStrategy::EditInPlace
            | OutputStrategy::GenerateNew
            | OutputStrategy::NeedsReview => {}
        }
    }
}

fn validate_presentation_kind_and_source(
    type_key: &str,
    ptype: &PresentationTypeConfig,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    if (ptype.kind == ItemKind::Song) != (ptype.content_source == ContentSourceKind::Song) {
        issues.push(ConfigValidationIssue {
            path: format!("presentation_types.{type_key}.content_source"),
            message: "song kind and song content_source must be configured together".to_string(),
        });
    }

    let valid_scripture_source = match ptype.kind {
        ItemKind::Scripture => matches!(
            ptype.content_source,
            ContentSourceKind::Static | ContentSourceKind::Scripture
        ),
        _ => ptype.content_source != ContentSourceKind::Scripture,
    };
    if !valid_scripture_source {
        issues.push(ConfigValidationIssue {
            path: format!("presentation_types.{type_key}.content_source"),
            message: "scripture content_source requires scripture kind; scripture kind may use static content for an existing presentation".to_string(),
        });
    }
}

fn validate_content_output_combination(
    type_key: &str,
    ptype: &PresentationTypeConfig,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    let supported = match ptype.output_strategy {
        OutputStrategy::UseExisting => matches!(
            ptype.content_source,
            ContentSourceKind::Static | ContentSourceKind::Song
        ),
        OutputStrategy::EditInPlace => {
            matches!(ptype.content_source, ContentSourceKind::Description)
        }
        OutputStrategy::GenerateNew => matches!(
            ptype.content_source,
            ContentSourceKind::Description | ContentSourceKind::Scripture
        ),
        OutputStrategy::Skip | OutputStrategy::NeedsReview => true,
    };
    if !supported {
        let content_source = match ptype.content_source {
            ContentSourceKind::Static => "static",
            ContentSourceKind::Description => "description",
            ContentSourceKind::Scripture => "scripture",
            ContentSourceKind::Song => "song",
        };
        let output_strategy = match ptype.output_strategy {
            OutputStrategy::Skip => "skip",
            OutputStrategy::UseExisting => "use_existing",
            OutputStrategy::EditInPlace => "edit_in_place",
            OutputStrategy::GenerateNew => "generate_new",
            OutputStrategy::NeedsReview => "needs_review",
        };
        issues.push(ConfigValidationIssue {
            path: format!("presentation_types.{type_key}.output_strategy"),
            message: format!("{content_source} content is not supported by {output_strategy}"),
        });
    }
}

fn validate_overrides(config: &ProjectConfig, issues: &mut Vec<ConfigValidationIssue>) {
    for (idx, override_rule) in config.overrides.iter().enumerate() {
        if let Some(type_key) = &override_rule.when.presentation_type {
            match config.presentation_types.get(type_key) {
                None => issues.push(ConfigValidationIssue {
                    path: format!("overrides[{idx}].when.presentation_type"),
                    message: format!("references unknown presentation type '{type_key}'"),
                }),
                Some(ptype) => {
                    let use_existing = matches!(ptype.output_strategy, OutputStrategy::UseExisting);
                    if override_rule.arrangement.is_some() && !use_existing {
                        issues.push(ConfigValidationIssue {
                            path: format!("overrides[{idx}].arrangement"),
                            message: format!(
                                "arrangement cannot target non-use_existing presentation type '{type_key}'"
                            ),
                        });
                    }
                    if override_rule.background.is_some() && use_existing {
                        issues.push(ConfigValidationIssue {
                            path: format!("overrides[{idx}].background"),
                            message: format!(
                                "background cannot target use_existing presentation type '{type_key}' because existing files are read-only"
                            ),
                        });
                    }
                }
            }
        }

        if let Some(group) = &override_rule.when.service_group {
            if !config.service_groups.contains_key(group) {
                issues.push(ConfigValidationIssue {
                    path: format!("overrides[{idx}].when.service_group"),
                    message: format!("references unknown service group '{group}'"),
                });
            }
        }
        if let Some(background) = &override_rule.background {
            if !config.backgrounds.contains_key(background) {
                issues.push(ConfigValidationIssue {
                    path: format!("overrides[{idx}].background"),
                    message: format!("references unknown background '{background}'"),
                });
            }
        }
    }
}

fn validate_runtime_defaults(config: &ProjectConfig, issues: &mut Vec<ConfigValidationIssue>) {
    if config
        .defaults
        .days_ahead
        .is_some_and(|days| !(1..=365).contains(&days))
    {
        issues.push(ConfigValidationIssue {
            path: "defaults.days_ahead".to_string(),
            message: "must be between 1 and 365".to_string(),
        });
    }
}

fn validate_background_references(config: &ProjectConfig, issues: &mut Vec<ConfigValidationIssue>) {
    if let Some(background) = &config.defaults.background {
        if !config.backgrounds.contains_key(background) {
            issues.push(ConfigValidationIssue {
                path: "defaults.background".to_string(),
                message: format!("references unknown background '{background}'"),
            });
        }
    }
}

fn validate_cue_roles(config: &ProjectConfig, issues: &mut Vec<ConfigValidationIssue>) {
    for (role_key, role) in &config.cue_roles {
        let path = format!("cue_roles.{role_key}");
        if role_key.trim().is_empty() {
            issues.push(ConfigValidationIssue {
                path: "cue_roles".to_string(),
                message: "cue role names must not be blank".to_string(),
            });
        }
        if role.slide.trim().is_empty() {
            issues.push(ConfigValidationIssue {
                path: format!("{path}.slide"),
                message: "slide must not be blank".to_string(),
            });
        }
        for (field, value) in [
            ("enter_macro", role.enter_macro.as_deref()),
            (
                "all_content_colored_macro",
                role.all_content_colored_macro.as_deref(),
            ),
        ] {
            if value.is_some_and(|value| value.trim().is_empty()) {
                issues.push(ConfigValidationIssue {
                    path: format!("{path}.{field}"),
                    message: format!("{field} must not be blank"),
                });
            }
        }
        if role.all_content_colored_macro.is_some() && role.enter_macro.is_none() {
            issues.push(ConfigValidationIssue {
                path: format!("{path}.all_content_colored_macro"),
                message: "all_content_colored_macro requires enter_macro".to_string(),
            });
        }
    }
}

fn validate_display_binding(
    config: &ProjectConfig,
    type_key: &str,
    display: &DisplayBindingConfig,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    match display {
        DisplayBindingConfig::Single { role } => {
            validate_cue_role_reference(config, type_key, "role", role, issues);
        }
        DisplayBindingConfig::Split { title, content } => {
            validate_cue_role_reference(config, type_key, "title", title, issues);
            validate_cue_role_reference(config, type_key, "content", content, issues);
        }
    }
}

fn validate_cue_role_reference(
    config: &ProjectConfig,
    type_key: &str,
    field: &str,
    role_key: &str,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    if !config.cue_roles.contains_key(role_key) {
        issues.push(ConfigValidationIssue {
            path: format!("presentation_types.{type_key}.display.{field}"),
            message: format!("references unknown cue role '{role_key}'"),
        });
    }
}

fn validate_item_rules(config: &ProjectConfig, issues: &mut Vec<ConfigValidationIssue>) {
    let mut rule_ids = HashSet::new();
    for (idx, rule) in config.item_rules.iter().enumerate() {
        validate_nonempty_id(&rule.id, &format!("item_rules[{idx}].id"), issues);
        if !rule_ids.insert(rule.id.as_str()) {
            issues.push(ConfigValidationIssue {
                path: format!("item_rules[{idx}].id"),
                message: format!("duplicate item rule id '{}'", rule.id),
            });
        }
        validate_match_spec(
            &rule.match_spec,
            &format!("item_rules[{idx}].match"),
            issues,
        );

        match &rule.outcome {
            ItemRuleOutcome::UseType { type_key, target } => {
                if !config.presentation_types.contains_key(type_key) {
                    issues.push(ConfigValidationIssue {
                        path: format!("item_rules[{idx}].use_type"),
                        message: format!("references unknown presentation type '{type_key}'"),
                    });
                }
                if let Some(target) = target {
                    let path = format!("item_rules[{idx}].target");
                    validate_target_spec(target, &path, issues);
                    validate_target_for_type(config, type_key, target, &path, false, issues);
                }
            }
            ItemRuleOutcome::Action(_) => {}
            ItemRuleOutcome::Decision(decision) => {
                validate_decision(config, idx, decision, issues);
            }
            ItemRuleOutcome::Expand(expansion) => {
                for (step_idx, step) in expansion.iter().enumerate() {
                    if !config.presentation_types.contains_key(&step.use_type) {
                        issues.push(ConfigValidationIssue {
                            path: format!("item_rules[{idx}].expand[{step_idx}].use_type"),
                            message: format!(
                                "references unknown presentation type '{}'",
                                step.use_type
                            ),
                        });
                    }
                    if let Some(target) = &step.target {
                        let path = format!("item_rules[{idx}].expand[{step_idx}].target");
                        validate_target_spec(target, &path, issues);
                        validate_target_for_type(
                            config,
                            &step.use_type,
                            target,
                            &path,
                            step.speaker.is_some(),
                            issues,
                        );
                    }
                }
            }
        }
    }
}

fn validate_required_playlist_items(
    config: &ProjectConfig,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    let mut ids = HashSet::new();
    for (index, item) in config.required_playlist_items.iter().enumerate() {
        let path = format!("required_playlist_items[{index}]");
        validate_nonempty_id(&item.id, &format!("{path}.id"), issues);
        if !ids.insert(item.id.as_str()) {
            issues.push(ConfigValidationIssue {
                path: format!("{path}.id"),
                message: format!("duplicate required playlist item id '{}'", item.id),
            });
        }
        validate_library_filename(&item.library_file, &format!("{path}.library_file"), issues);

        match config.presentation_types.get(&item.use_type) {
            None => issues.push(ConfigValidationIssue {
                path: format!("{path}.use_type"),
                message: format!("references unknown presentation type '{}'", item.use_type),
            }),
            Some(presentation_type)
                if presentation_type.output_strategy != OutputStrategy::UseExisting
                    || presentation_type.content_source != ContentSourceKind::Static =>
            {
                issues.push(ConfigValidationIssue {
                    path: format!("{path}.use_type"),
                    message:
                        "required playlist items must use a static use_existing presentation type"
                            .to_string(),
                });
            }
            Some(_) => {}
        }

        if let Some(group) = item.service_group.as_deref() {
            if !config.service_groups.contains_key(group) {
                issues.push(ConfigValidationIssue {
                    path: format!("{path}.service_group"),
                    message: format!("references unknown service group '{group}'"),
                });
            }
        }
    }
}

fn validate_target_for_type(
    config: &ProjectConfig,
    type_key: &str,
    target: &TargetSpec,
    path: &str,
    speaker_expansion: bool,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    let Some(ptype) = config.presentation_types.get(type_key) else {
        return;
    };
    match target {
        TargetSpec::GeneratedName { .. } if !speaker_expansion => {
            issues.push(ConfigValidationIssue {
                path: path.to_string(),
                message: "name_template is supported only for a speaker expansion".to_string(),
            });
        }
        TargetSpec::ExistingFile { .. }
            if matches!(
                ptype.output_strategy,
                OutputStrategy::GenerateNew | OutputStrategy::Skip | OutputStrategy::NeedsReview
            ) =>
        {
            issues.push(ConfigValidationIssue {
                path: path.to_string(),
                message: "library_file requires use_existing or edit_in_place output".to_string(),
            });
        }
        TargetSpec::GeneratedName { .. } | TargetSpec::ExistingFile { .. } => {}
    }
}

fn validate_target_spec(target: &TargetSpec, path: &str, issues: &mut Vec<ConfigValidationIssue>) {
    match target {
        TargetSpec::ExistingFile { library_file } => {
            validate_library_filename(library_file, &format!("{path}.library_file"), issues);
        }
        TargetSpec::GeneratedName { name_template } if name_template.trim().is_empty() => {
            issues.push(ConfigValidationIssue {
                path: format!("{path}.name_template"),
                message: "name_template must not be empty".to_string(),
            });
        }
        TargetSpec::GeneratedName { .. } => {}
    }
}

fn validate_library_filename(value: &str, path: &str, issues: &mut Vec<ConfigValidationIssue>) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        issues.push(ConfigValidationIssue {
            path: path.to_string(),
            message: "library_file must not be empty".to_string(),
        });
    } else if trimmed.contains(['/', '\\']) {
        issues.push(ConfigValidationIssue {
            path: path.to_string(),
            message: "library_file must be a filename, not a path".to_string(),
        });
    }
}

fn validate_nonempty_id(id: &str, path: &str, issues: &mut Vec<ConfigValidationIssue>) {
    if id.trim().is_empty() {
        issues.push(ConfigValidationIssue {
            path: path.to_string(),
            message: "id must not be empty".to_string(),
        });
    }
}

fn validate_match_spec(
    match_spec: &MatchSpec,
    path: &str,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    if match_spec.is_empty() {
        issues.push(ConfigValidationIssue {
            path: path.to_string(),
            message: "match must contain at least one criterion".to_string(),
        });
    }
    for (field, values) in [
        ("title_prefix", &match_spec.title_prefix),
        ("title_contains", &match_spec.title_contains),
        ("description_contains", &match_spec.description_contains),
        ("service_type", &match_spec.service_type),
    ] {
        if values.iter().any(|value| value.trim().is_empty()) {
            issues.push(ConfigValidationIssue {
                path: format!("{path}.{field}"),
                message: "match values must not be empty".to_string(),
            });
        }
    }
    if match_spec
        .category
        .as_deref()
        .is_some_and(|category| category.trim().is_empty())
    {
        issues.push(ConfigValidationIssue {
            path: format!("{path}.category"),
            message: "category must not be empty".to_string(),
        });
    }
}

fn format_validation_issues(issues: &[ConfigValidationIssue]) -> String {
    issues
        .iter()
        .map(|issue| format!("{}: {}", issue.path, issue.message))
        .collect::<Vec<_>>()
        .join("; ")
}

fn validate_decision(
    config: &ProjectConfig,
    rule_idx: usize,
    decision: &DecisionConfig,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    match decision {
        DecisionConfig::ChooseExistingFile {
            context_fields,
            choices,
            ..
        } => {
            for (field_idx, field) in context_fields.iter().enumerate() {
                if !matches!(field.as_str(), "title" | "description" | "note") {
                    issues.push(ConfigValidationIssue {
                        path: format!(
                            "item_rules[{rule_idx}].decision.context_fields[{field_idx}]"
                        ),
                        message: format!("unknown context field '{field}'"),
                    });
                }
            }
            if choices.is_empty() {
                issues.push(ConfigValidationIssue {
                    path: format!("item_rules[{rule_idx}].decision.choices"),
                    message: "decision has no choices".to_string(),
                });
            }
            for (choice_key, choice) in choices {
                let choice_path = format!("item_rules[{rule_idx}].decision.choices.{choice_key}");
                let Some(type_key) = &choice.use_type else {
                    issues.push(ConfigValidationIssue {
                        path: format!("{choice_path}.use_type"),
                        message: "decision choice must define use_type".to_string(),
                    });
                    continue;
                };
                match config.presentation_types.get(type_key) {
                    None => issues.push(ConfigValidationIssue {
                        path: format!("{choice_path}.use_type"),
                        message: format!("references unknown presentation type '{type_key}'"),
                    }),
                    Some(ptype)
                        if !matches!(ptype.output_strategy, OutputStrategy::UseExisting) =>
                    {
                        issues.push(ConfigValidationIssue {
                            path: format!("{choice_path}.use_type"),
                            message:
                                "choose_existing_file requires a use_existing presentation type"
                                    .to_string(),
                        });
                    }
                    Some(_) => {}
                }

                let has_file = choice.file.is_some();
                let has_target_file = choice
                    .target
                    .as_ref()
                    .and_then(TargetSpec::library_file)
                    .is_some();
                if usize::from(has_file) + usize::from(has_target_file) != 1 {
                    issues.push(ConfigValidationIssue {
                        path: choice_path.clone(),
                        message: "choice must define exactly one of file or target.library_file"
                            .to_string(),
                    });
                }
                if let Some(file) = &choice.file {
                    validate_library_filename(file, &format!("{choice_path}.file"), issues);
                }
                if let Some(target) = &choice.target {
                    validate_target_spec(target, &format!("{choice_path}.target"), issues);
                }
                if choice.match_spec.any.is_empty()
                    && choice.match_spec.all.is_empty()
                    && choice.match_spec.none.is_empty()
                {
                    issues.push(ConfigValidationIssue {
                        path: format!("{choice_path}.match"),
                        message: "decision choice match must contain at least one criterion"
                            .to_string(),
                    });
                }
                if choice
                    .match_spec
                    .any
                    .iter()
                    .chain(&choice.match_spec.all)
                    .chain(&choice.match_spec.none)
                    .any(|value| value.trim().is_empty())
                {
                    issues.push(ConfigValidationIssue {
                        path: format!("{choice_path}.match"),
                        message: "decision choice match values must not be empty".to_string(),
                    });
                }
            }
        }
    }
}

const fn default_version() -> u16 {
    4
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use tempfile::tempdir;

    const VALID_V4_CONFIG: &str = r#"
        {
          "version": 4,
          "defaults": {
            "theme": "VPC Theme",
            "background": "default",
            "presentation_size": { "width": 1920, "height": 1080 }
          },
          "backgrounds": {
            "default": "backgrounds/default.png"
          },
          "cue_roles": {
            "title": {
              "slide": "Information (Projectors)",
              "enter_macro": "Name Tag/Title"
            },
            "responsive": {
              "slide": "Scripture (Projectors) (Responsive)",
              "enter_macro": "Scripture/Prayer",
              "all_content_colored_macro": "Scripture/Prayer (Highlighted)"
            }
          },
          "service_groups": {
            "seasonal": {
              "service_types": ["Christmas Eve"]
            }
          },
          "presentation_types": {
            "liturgical_weekly": {
              "kind": "liturgy",
              "content_source": "description",
              "description_parser": "liturgical",
              "output_strategy": "edit_in_place",
              "display": {
                "kind": "split",
                "title": "title",
                "content": "responsive"
              },
              "background": "default",
              "max_lines_per_slide": 8,
              "description": "Weekly liturgy"
            },
            "person_nametag": {
              "kind": "nametag",
              "content_source": "static",
              "output_strategy": "use_existing"
            }
          },
          "item_rules": [
            {
              "id": "call_to_worship",
              "match": {
                "title_prefix": ["call to worship"]
              },
              "use_type": "liturgical_weekly",
              "target": {
                "library_file": "Call to Worship.pro"
              }
            },
            {
              "id": "welcome_bundle",
              "match": {
                "title_prefix": ["welcome"]
              },
              "expand": [
                {
                  "use_type": "person_nametag",
                  "speaker": "resolved"
                },
                {
                  "use_type": "liturgical_weekly"
                }
              ]
            }
          ],
          "people": {
            "Robert": {
              "last": "Austell",
              "role": "pastor",
              "nametag": "Robert Nametag"
            }
          }
        }
        "#;

    #[test]
    fn parse_v4_config() {
        let config = parse_project_config_str(VALID_V4_CONFIG).expect("v4 config should parse");
        assert_eq!(config.version, 4);
        assert_eq!(config.defaults.theme.as_deref(), Some("VPC Theme"));
        let presentation_size = config.defaults.presentation_size;
        assert_eq!(presentation_size.width(), 1920);
        assert_eq!(presentation_size.height(), 1080);
        assert_eq!(
            config
                .defaults
                .background
                .as_ref()
                .map(BackgroundId::as_str),
            Some("default")
        );
        let default_background = BackgroundId::new("default").expect("valid background id");
        assert_eq!(
            config.backgrounds[&default_background].as_path(),
            Path::new("backgrounds/default.png")
        );
        assert!(config.presentation_types.contains_key("liturgical_weekly"));
        assert!(config.people.contains_key("Robert"));
        assert_eq!(config.item_rules.len(), 2);
        assert_eq!(config.item_rules[0].id, "call_to_worship");
        let ItemRuleOutcome::UseType { target, .. } = &config.item_rules[0].outcome else {
            panic!("first rule should use a presentation type");
        };
        assert_eq!(
            target.as_ref().and_then(TargetSpec::library_file),
            Some("Call to Worship.pro")
        );
    }

    #[test]
    fn presentation_size_rejects_zero_dimensions() {
        for value in [
            serde_json::json!({"width": 0, "height": 1080}),
            serde_json::json!({"width": 1920, "height": 0}),
        ] {
            serde_json::from_value::<crate::propresenter::PresentationSize>(value)
                .expect_err("zero dimensions must not deserialize");
        }
    }

    #[test]
    fn required_playlist_items_reference_static_existing_types_and_known_groups() {
        let valid = r#"
        {
          "version": 4,
          "service_groups": {
            "weekly": { "service_types": ["Sunday Morning"] }
          },
          "presentation_types": {
            "static_graphic": {
              "kind": "graphic",
              "content_source": "static",
              "output_strategy": "use_existing"
            }
          },
          "required_playlist_items": [{
            "id": "pre_service",
            "use_type": "static_graphic",
            "library_file": "Pre-Service.pro",
            "placement": "start",
            "service_group": "weekly"
          }]
        }
        "#;
        let config = parse_project_config_str(valid)
            .expect("a required static presentation should validate");
        assert_eq!(config.required_playlist_items.len(), 1);
        assert_eq!(
            config.required_playlist_items[0].placement,
            RequiredPlaylistPlacement::Start
        );

        let invalid_group = valid.replace("\"weekly\"\n          }]", "\"missing\"\n          }]");
        let error = parse_project_config_str(&invalid_group)
            .expect_err("unknown service groups must fail validation");
        assert!(error
            .to_string()
            .contains("unknown service group 'missing'"));

        let invalid_type = valid.replace("\"use_existing\"", "\"generate_new\"");
        let error = parse_project_config_str(&invalid_type)
            .expect_err("required generated presentations must fail validation");
        assert!(error.to_string().contains("static use_existing"));
    }

    #[test]
    fn target_spec_requires_exactly_one_target_kind() {
        for invalid in [
            serde_json::json!({}),
            serde_json::json!({
                "library_file": "Welcome.pro",
                "name_template": "{speaker} Welcome"
            }),
        ] {
            let error = serde_json::from_value::<TargetSpec>(invalid)
                .expect_err("ambiguous target should not deserialize");
            assert!(error
                .to_string()
                .contains("exactly one of library_file or name_template"));
        }

        for valid in [
            serde_json::json!({"library_file": "Welcome.pro"}),
            serde_json::json!({"name_template": "{speaker} Welcome"}),
        ] {
            let target = serde_json::from_value::<TargetSpec>(valid.clone())
                .expect("single target kind should deserialize");
            assert_eq!(
                serde_json::to_value(target).expect("target should serialize"),
                valid
            );
        }
    }

    #[test]
    fn target_kind_must_match_the_rule_context() {
        let direct_template = r#"
        {
          "version": 4,
          "presentation_types": {
            "generated": {
              "kind": "liturgy",
              "content_source": "description",
              "description_parser": "liturgical",
              "output_strategy": "generate_new",
              "display": { "kind": "single", "role": "content" }
            }
          },
          "cue_roles": { "content": { "slide": "Content" } },
          "item_rules": [{
            "id": "generated",
            "match": { "title_prefix": ["generated"] },
            "use_type": "generated",
            "target": { "name_template": "{title}" }
          }]
        }
        "#;
        let error = parse_project_config_str(direct_template)
            .expect_err("direct rules must not silently ignore name_template");
        assert!(error.to_string().contains("speaker expansion"));

        let library_target = direct_template.replace(
            "\"name_template\": \"{title}\"",
            "\"library_file\": \"Generated.pro\"",
        );
        let error = parse_project_config_str(&library_target)
            .expect_err("generate_new must not accept a library_file target");
        assert!(error.to_string().contains("library_file requires"));
    }

    #[test]
    fn parses_tagged_single_and_split_display_bindings() {
        let json = r#"
        {
          "version": 4,
          "cue_roles": {
            "title": {
              "slide": "Information (Projectors)"
            },
            "content": {
              "slide": "Scripture (Projectors)"
            }
          },
          "presentation_types": {
            "single": {
              "kind": "liturgy",
              "content_source": "description",
              "description_parser": "liturgical",
              "output_strategy": "generate_new",
              "display": {
                "kind": "single",
                "role": "content"
              }
            },
            "split": {
              "kind": "scripture",
              "content_source": "scripture",
              "output_strategy": "generate_new",
              "display": {
                "kind": "split",
                "title": "title",
                "content": "content"
              }
            }
          }
        }
        "#;

        let config = parse_project_config_str(json).expect("tagged bindings should parse");
        assert!(matches!(
            &config.presentation_types["single"].display,
            Some(DisplayBindingConfig::Single { role }) if role == "content"
        ));
        assert!(matches!(
            &config.presentation_types["split"].display,
            Some(DisplayBindingConfig::Split {
                title,
                content
            }) if title == "title" && content == "content"
        ));
    }

    #[test]
    fn rejects_invalid_background_ids() {
        let invalid_ids = [
            "",
            "Uppercase",
            "-leading-dash",
            "contains.dot",
            "nonascii-é",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ];
        for id in invalid_ids {
            let json =
                format!(r#"{{"version":4,"backgrounds":{{"{id}":"backgrounds/default.png"}}}}"#);
            let error = parse_project_config_str(&json)
                .expect_err("invalid background id must be rejected during parsing");
            assert!(
                matches!(error, ProjectConfigLoadError::Parse(_)),
                "unexpected error for {id:?}: {error}"
            );
        }
        assert!(BackgroundId::new("default-1_2").is_ok());
    }

    #[test]
    fn rejects_invalid_background_asset_paths() {
        for path in [
            "../secret.png",
            "/tmp/background.png",
            "backgrounds/./default.png",
            "backgrounds/default.gif",
            "backgrounds\\default.png",
            "C:/background.png",
            "backgrounds/default\0.png",
        ] {
            let value = serde_json::json!({
                "version": 4,
                "backgrounds": { "default": path }
            });
            let error = parse_project_config_value(value)
                .expect_err("invalid background path must be rejected during parsing");
            assert!(
                matches!(error, ProjectConfigLoadError::Parse(_)),
                "unexpected error for {path:?}: {error}"
            );
        }
        assert!(BackgroundAssetPath::new("backgrounds/default.TIFF").is_ok());
    }

    #[test]
    fn rejects_unknown_background_and_cue_role_references() {
        let json = r#"
        {
          "version": 4,
          "defaults": {
            "background": "missing_default",
            "presentation_size": { "width": 1920, "height": 1080 }
          },
          "presentation_types": {
            "generated": {
              "content_source": "description",
              "description_parser": "liturgical",
              "output_strategy": "generate_new",
              "display": { "kind": "single", "role": "missing_role" },
              "background": "missing_type"
            }
          },
          "overrides": [{
            "when": { "presentation_type": "generated" },
            "background": "missing_override"
          }]
        }
        "#;

        let error = parse_project_config_str(json).expect_err("unknown references must fail");
        let message = error.to_string();
        for expected in [
            "defaults.background",
            "missing_role",
            "missing_type",
            "missing_override",
        ] {
            assert!(
                message.contains(expected),
                "missing {expected:?} in {message}"
            );
        }
    }

    #[test]
    fn rejects_styling_on_read_only_presentations() {
        let json = r#"
        {
          "version": 4,
          "backgrounds": { "default": "backgrounds/default.png" },
          "cue_roles": { "lyrics": { "slide": "Lyrics" } },
          "presentation_types": {
            "song": {
              "kind": "song",
              "content_source": "song",
              "output_strategy": "use_existing",
              "display": { "kind": "single", "role": "lyrics" },
              "background": "default",
              "max_lines_per_slide": 8,
              "arrangement": "Default"
            }
          }
        }
        "#;

        let error = parse_project_config_str(json).expect_err("read-only styling must fail");
        let message = error.to_string();
        for field in ["display", "background", "max_lines_per_slide"] {
            assert!(message.contains(field), "missing {field:?} in {message}");
        }
        assert!(!message.contains("arrangement is not valid"));
    }

    #[test]
    fn validates_rendering_requirements_and_cue_role_macros() {
        let missing_display = r#"
        {
          "version": 4,
          "presentation_types": {
            "generated": {
              "content_source": "description",
              "description_parser": "liturgical",
              "output_strategy": "generate_new"
            }
          }
        }
        "#;
        let error = parse_project_config_str(missing_display)
            .expect_err("generate_new without display must fail");
        assert!(error.to_string().contains("requires a display binding"));

        let alternate_without_entry = r#"
        {
          "version": 4,
          "cue_roles": {
            "responsive": {
              "slide": "Responsive",
              "all_content_colored_macro": "Highlighted"
            }
          }
        }
        "#;
        let error = parse_project_config_str(alternate_without_entry)
            .expect_err("alternate macro without entry macro must fail");
        assert!(error.to_string().contains("requires enter_macro"));

        let edit_without_display = r#"
        {
          "version": 4,
          "presentation_types": {
            "weekly": {
              "content_source": "description",
              "description_parser": "liturgical",
              "output_strategy": "edit_in_place"
            }
          }
        }
        "#;
        let error = parse_project_config_str(edit_without_display)
            .expect_err("edit_in_place without a display binding must fail");
        assert!(error
            .to_string()
            .contains("edit_in_place requires a display binding"));

        let edit_with_unbound_line_limit = r#"
        {
          "version": 4,
          "presentation_types": {
            "weekly": {
              "kind": "liturgy",
              "content_source": "description",
              "description_parser": "liturgical",
              "output_strategy": "edit_in_place",
              "max_lines_per_slide": 8
            }
          }
        }
        "#;
        let error = parse_project_config_str(edit_with_unbound_line_limit)
            .expect_err("an edit without a display binding must fail");
        assert!(error
            .to_string()
            .contains("edit_in_place requires a display binding"));
    }

    #[test]
    fn validates_arrangements_only_for_existing_presentations() {
        let direct_rendered_arrangement = r#"
        {
          "version": 4,
          "cue_roles": { "content": { "slide": "Content" } },
          "presentation_types": {
            "rendered": {
              "kind": "liturgy",
              "content_source": "description",
              "description_parser": "liturgical",
              "output_strategy": "generate_new",
              "display": { "kind": "single", "role": "content" },
              "arrangement": "Standard"
            }
          }
        }
        "#;
        let error = parse_project_config_str(direct_rendered_arrangement)
            .expect_err("a rendered presentation must not declare an arrangement");
        assert!(error
            .to_string()
            .contains("arrangement is only valid for use_existing"));

        let targeted_rendered_arrangement = r#"
        {
          "version": 4,
          "cue_roles": { "content": { "slide": "Content" } },
          "presentation_types": {
            "rendered": {
              "kind": "liturgy",
              "content_source": "description",
              "description_parser": "liturgical",
              "output_strategy": "generate_new",
              "display": { "kind": "single", "role": "content" }
            }
          },
          "overrides": [{
            "when": { "presentation_type": "rendered" },
            "arrangement": "Seasonal"
          }]
        }
        "#;
        let error = parse_project_config_str(targeted_rendered_arrangement)
            .expect_err("an override must not assign an arrangement to rendered content");
        assert!(error
            .to_string()
            .contains("arrangement cannot target non-use_existing"));

        let existing_and_broad = r#"
        {
          "version": 4,
          "cue_roles": { "content": { "slide": "Content" } },
          "presentation_types": {
            "existing": {
              "kind": "graphic",
              "content_source": "static",
              "output_strategy": "use_existing",
              "arrangement": "Default"
            },
            "rendered": {
              "kind": "liturgy",
              "content_source": "description",
              "description_parser": "liturgical",
              "output_strategy": "generate_new",
              "display": { "kind": "single", "role": "content" }
            }
          },
          "overrides": [{
            "when": { "service_type": "Christmas Eve" },
            "arrangement": "Seasonal"
          }]
        }
        "#;
        parse_project_config_str(existing_and_broad)
            .expect("existing and broad arrangement configuration should remain valid");
    }

    #[test]
    fn rejects_background_override_targeting_use_existing_type() {
        let config = r#"
        {
          "version": 4,
          "backgrounds": { "seasonal": "backgrounds/seasonal.png" },
          "presentation_types": {
            "existing": {
              "kind": "graphic",
              "content_source": "static",
              "output_strategy": "use_existing"
            }
          },
          "overrides": [{
            "when": { "presentation_type": "existing" },
            "background": "seasonal"
          }]
        }
        "#;

        let error = parse_project_config_str(config)
            .expect_err("a background override must not target read-only existing content");
        assert!(error
            .to_string()
            .contains("background cannot target use_existing presentation type 'existing'"));
    }

    #[test]
    fn rejects_unsupported_content_output_and_kind_source_combinations() {
        for (name, body, expected) in [
            (
                "existing_description",
                r#"{
                  "kind": "liturgy",
                  "content_source": "description",
                  "description_parser": "liturgical",
                  "output_strategy": "use_existing"
                }"#,
                "description content is not supported by use_existing",
            ),
            (
                "edited_static",
                r#"{
                  "kind": "graphic",
                  "content_source": "static",
                  "output_strategy": "edit_in_place"
                }"#,
                "static content is not supported by edit_in_place",
            ),
            (
                "generated_song",
                r#"{
                  "kind": "song",
                  "content_source": "song",
                  "output_strategy": "generate_new"
                }"#,
                "song content is not supported by generate_new",
            ),
            (
                "song_kind_without_song_source",
                r#"{
                  "kind": "song",
                  "content_source": "static",
                  "output_strategy": "needs_review"
                }"#,
                "song kind and song content_source must be configured together",
            ),
            (
                "scripture_source_without_scripture_kind",
                r#"{
                  "kind": "liturgy",
                  "content_source": "scripture",
                  "output_strategy": "needs_review"
                }"#,
                "scripture content_source requires scripture kind",
            ),
        ] {
            let json = format!(
                r#"{{
                  "version": 4,
                  "presentation_types": {{ "{name}": {body} }}
                }}"#
            );
            let error = parse_project_config_str(&json)
                .expect_err("an unsupported presentation contract must fail validation");
            assert!(
                error.to_string().contains(expected),
                "missing {expected:?} in {error}"
            );
        }
    }

    #[test]
    fn allows_existing_static_scripture_presentations() {
        let config = r#"
        {
          "version": 4,
          "presentation_types": {
            "scripture_existing": {
              "kind": "scripture",
              "content_source": "static",
              "output_strategy": "use_existing"
            }
          }
        }
        "#;

        parse_project_config_str(config)
            .expect("an existing scripture presentation is a valid static source");
    }

    #[test]
    fn reject_legacy_config() {
        let json = r#"{ "theme": "Legacy", "item_types": {} }"#;
        let err = parse_project_config_str(json).unwrap_err();
        assert!(
            matches!(err, ProjectConfigLoadError::MissingVersion),
            "expected MissingVersion, got: {err}"
        );
    }

    #[test]
    fn reject_v1_config() {
        let json = r#"{ "version": 1, "theme": "Legacy" }"#;
        let err = parse_project_config_str(json).unwrap_err();
        assert!(
            matches!(err, ProjectConfigLoadError::UnsupportedVersion(1)),
            "expected UnsupportedVersion(1), got: {err}"
        );
    }

    #[test]
    fn validate_project_config_reports_unknown_refs() {
        let mut config = ProjectConfig::default();
        config.item_rules.push(ItemRuleConfig {
            id: "bad_rule".to_string(),
            match_spec: MatchSpec {
                category: Some("text".to_string()),
                ..MatchSpec::default()
            },
            outcome: ItemRuleOutcome::UseType {
                type_key: "missing_type".to_string(),
                target: None,
            },
            notes: None,
        });
        config.overrides.push(OverrideRuleConfig {
            when: OverrideWhen {
                service_group: Some("missing_group".to_string()),
                presentation_type: Some("missing_type".to_string()),
                ..OverrideWhen::default()
            },
            ..OverrideRuleConfig::default()
        });

        let issues = validate_project_config(&config);
        assert_eq!(issues.len(), 3);
    }

    #[test]
    fn repo_project_config_is_v4_and_valid() {
        let config = parse_project_config_str(include_str!("../data/proflow.config.json"))
            .expect("repo config should parse");

        assert_eq!(config.version, 4);
        assert!(validate_project_config(&config).is_empty());
    }

    #[test]
    fn starter_project_config_is_valid() {
        parse_project_config_str(include_str!("../examples/starter-config.json"))
            .expect("starter config should parse and validate");
    }

    #[test]
    fn rejects_v3_config() {
        let value = serde_json::json!({
            "version": 3,
            "metadata": {
                "name": "Example"
            }
        });

        let error = parse_project_config_value(value).expect_err("v3 must be rejected");
        assert!(matches!(
            error,
            ProjectConfigLoadError::UnsupportedVersion(3)
        ));
    }

    #[test]
    fn write_project_config_round_trips() {
        let dir = tempdir().expect("tempdir should create");
        let path = dir.path().join("proflow.config.json");
        let mut config = ProjectConfig::default();
        config.metadata.name = Some("Round Trip".to_string());

        write_project_config(&path, &config).expect("config should write");
        let loaded = load_project_config(&path).expect("config should reload");

        assert_eq!(loaded.metadata.name.as_deref(), Some("Round Trip"));
    }

    #[test]
    fn typed_rule_outcomes_round_trip_through_flat_json() {
        let config = parse_project_config_str(include_str!("../examples/starter-config.json"))
            .expect("starter config should parse");

        assert!(matches!(
            &config.item_rules[0].outcome,
            ItemRuleOutcome::Action(RuleAction::Skip { .. })
        ));
        assert!(matches!(
            &config.item_rules[1].outcome,
            ItemRuleOutcome::UseType { type_key, .. } if type_key == "song"
        ));
        assert!(matches!(
            &config.item_rules[3].outcome,
            ItemRuleOutcome::Expand(expansion) if expansion.iter().count() == 2
        ));

        let serialized = serialize_project_config(&config).expect("config should serialize");
        let value: serde_json::Value =
            serde_json::from_str(&serialized).expect("serialized config should be JSON");
        assert_eq!(value["item_rules"][1]["use_type"], "song");
        assert!(value["item_rules"][1].get("action").is_none());
        assert_eq!(
            value["item_rules"][3]["expand"].as_array().map(Vec::len),
            Some(2)
        );

        parse_project_config_str(&serialized).expect("serialized config should parse again");
    }

    #[test]
    fn rejects_unknown_nested_field() {
        let json = r#"
        {
          "version": 4,
          "item_rules": [{
            "id": "typo",
            "match": { "title_prefx": ["sermon"] },
            "action": { "kind": "skip", "reason": "manual" }
          }]
        }
        "#;

        let error = parse_project_config_str(json).expect_err("typo must be rejected");
        assert!(matches!(error, ProjectConfigLoadError::Parse(_)));
        assert!(error.to_string().contains("title_prefx"));
    }

    #[test]
    fn rejects_empty_match() {
        let json = r#"
        {
          "version": 4,
          "item_rules": [{
            "id": "matches_everything",
            "match": {},
            "action": { "kind": "skip", "reason": "manual" }
          }]
        }
        "#;

        let error = parse_project_config_str(json).expect_err("empty match must be rejected");
        assert!(matches!(error, ProjectConfigLoadError::Invalid(_)));
        assert!(error.to_string().contains("at least one criterion"));
    }

    #[test]
    fn rejects_library_paths_where_exact_filenames_are_required() {
        let json = r#"
        {
          "version": 4,
          "presentation_types": {
            "static": {
              "content_source": "static",
              "output_strategy": "use_existing"
            }
          },
          "item_rules": [{
            "id": "escaped_target",
            "match": { "title_prefix": ["welcome"] },
            "use_type": "static",
            "target": { "library_file": "folder/Welcome.pro" }
          }]
        }
        "#;

        let error = parse_project_config_str(json).expect_err("library paths must be rejected");
        assert!(matches!(error, ProjectConfigLoadError::Invalid(_)));
        assert!(error.to_string().contains("must be a filename, not a path"));
    }

    #[test]
    fn rejects_contradictory_rule_outcomes() {
        let json = r#"
        {
          "version": 4,
          "presentation_types": {
            "static": { "output_strategy": "use_existing" }
          },
          "item_rules": [{
            "id": "contradictory",
            "match": { "title_prefix": ["sermon"] },
            "use_type": "static",
            "action": { "kind": "skip", "reason": "manual" }
          }]
        }
        "#;

        let error =
            parse_project_config_str(json).expect_err("contradictory outcomes must be rejected");
        assert!(matches!(error, ProjectConfigLoadError::Parse(_)));
        assert!(error.to_string().contains("exactly one outcome"));
    }

    #[test]
    fn rejects_missing_or_empty_rule_outcome() {
        for rule_body in ["", r#", "expand": []"#] {
            let json = format!(
                r#"
                {{
                  "version": 4,
                  "item_rules": [{{
                    "id": "missing",
                    "match": {{ "title_prefix": ["sermon"] }}{rule_body}
                  }}]
                }}
                "#
            );
            let error = parse_project_config_str(&json)
                .expect_err("missing or empty outcome must be rejected");
            assert!(matches!(error, ProjectConfigLoadError::Parse(_)));
            assert!(error.to_string().contains("exactly one outcome"));
        }
    }

    #[test]
    fn rejects_duplicate_rule_ids() {
        let json = r#"
        {
          "version": 4,
          "item_rules": [
            {
              "id": "duplicate",
              "match": { "title_prefix": ["one"] },
              "action": { "kind": "skip", "reason": "one" }
            },
            {
              "id": "duplicate",
              "match": { "title_prefix": ["two"] },
              "action": { "kind": "skip", "reason": "two" }
            }
          ]
        }
        "#;

        let error = parse_project_config_str(json).expect_err("duplicate ids must be rejected");
        assert!(matches!(error, ProjectConfigLoadError::Invalid(_)));
        assert!(error.to_string().contains("duplicate item rule id"));
    }

    #[test]
    fn rejects_out_of_range_lookahead_windows() {
        let json = r#"
        {
          "version": 4,
          "defaults": {
            "days_ahead": 0,
            "presentation_size": { "width": 1920, "height": 1080 }
          }
        }
        "#;

        let error = parse_project_config_str(json).expect_err("invalid days must be rejected");

        assert!(matches!(error, ProjectConfigLoadError::Invalid(_)));
        assert!(error.to_string().contains("defaults.days_ahead"));
    }
}
