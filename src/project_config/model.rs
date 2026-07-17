//! Editable JSON schema for project-level service build configuration.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::path::{Component, Path};
use std::str::FromStr;

/// Editable JSON representation of project config.
///
/// This type owns the stable wire format and may temporarily contain invalid
/// cross-references while a candidate is being authored. Runtime planning must
/// use [`super::ProjectConfig`], which can only be constructed by validating
/// this raw value.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawProjectConfig {
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
    pub service_groups: BTreeMap<String, ServiceGroupConfig>,
    /// Existing presentations that must occur in matching service playlists.
    #[serde(default)]
    pub required_playlist_items: Vec<RequiredPlaylistItemConfig>,
    /// Named background assets, relative to the project data root.
    #[serde(default)]
    pub backgrounds: BTreeMap<BackgroundId, BackgroundAssetPath>,
    /// Named cue roles that bind semantic slide regions to `ProPresenter` assets.
    #[serde(default)]
    pub cue_roles: BTreeMap<String, CueRoleConfig>,
    /// Named presentation types.
    #[serde(default)]
    pub presentation_types: BTreeMap<String, PresentationTypeConfig>,
    /// Ordered item rules.
    #[serde(default)]
    pub item_rules: Vec<ItemRuleConfig>,
    /// Known people metadata.
    #[serde(default)]
    pub people: BTreeMap<String, PersonConfig>,
    /// Structured override rules.
    #[serde(default)]
    pub overrides: Vec<OverrideRuleConfig>,
}

impl Default for RawProjectConfig {
    fn default() -> Self {
        Self {
            version: default_version(),
            metadata: ProjectMetadata::default(),
            defaults: ProjectDefaults::default(),
            service_groups: BTreeMap::new(),
            required_playlist_items: Vec::new(),
            backgrounds: BTreeMap::new(),
            cue_roles: BTreeMap::new(),
            presentation_types: BTreeMap::new(),
            item_rules: Vec::new(),
            people: BTreeMap::new(),
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
    /// Exact registered `ProPresenter` library used for sources and canonical writes.
    #[serde(default)]
    pub library: LibraryName,
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
            library: LibraryName::default(),
            theme: None,
            background: None,
            days_ahead: None,
            bible_version: None,
            presentation_size: crate::propresenter::PresentationSize::FULL_HD,
        }
    }
}

/// Checked name of one registered `ProPresenter` presentation library.
///
/// Library names are filesystem identities below `Libraries`, so separators,
/// traversal components, padding, and control characters are rejected at the
/// config boundary.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LibraryName(String);

impl LibraryName {
    /// Parse one exact registered-library name.
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.is_empty()
            || value.trim() != value
            || value.chars().any(char::is_control)
            || value.contains(['/', '\\'])
            || value.len() > 128
        {
            return Err(
                "library name must be 1-128 unpadded characters with no controls".to_string(),
            );
        }
        let mut components = Path::new(&value).components();
        if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
            return Err("library name must be one normal path component".to_string());
        }
        Ok(Self(value))
    }

    /// Exact directory name below the native `Libraries` root.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for LibraryName {
    fn default() -> Self {
        Self("Default".to_string())
    }
}

impl std::fmt::Display for LibraryName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for LibraryName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for LibraryName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Validated identifier for a project-owned background asset.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
    /// Semantic text fields mapped to exact named graphics elements.
    ///
    /// An empty map enables the conventional `body` field only when the theme
    /// slide has exactly one meaningful text destination. Explicit mappings
    /// allow multi-field templates without relying on element order or UUIDs.
    #[serde(default)]
    pub text_slots: BTreeMap<String, String>,
    /// Macro placed on the first cue entering this role.
    pub enter_macro: Option<String>,
    /// Alternate entry macro when a cue's first visible speaker is the leader.
    pub leader_enter_macro: Option<String>,
    /// Editor colors used to preserve leader/audience text runs.
    pub speaker_colors: Option<SpeakerColorConfig>,
}

/// Editor colors for semantic liturgical speakers.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeakerColorConfig {
    /// Leader/liturgist text color in the projector-oriented editor theme.
    pub leader: RgbColor,
    /// Congregational text color in the projector-oriented editor theme.
    pub audience: RgbColor,
}

/// Checked six-digit RGB color serialized as `#RRGGBB`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RgbColor((u8, u8, u8));

impl RgbColor {
    /// Build one RGB color from explicit channel values.
    pub const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self((red, green, blue))
    }

    /// Return the checked native RGB components.
    pub const fn components(self) -> (u8, u8, u8) {
        self.0
    }
}

impl Serialize for RgbColor {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&format!(
            "#{:02X}{:02X}{:02X}",
            self.0 .0, self.0 .1, self.0 .2
        ))
    }
}

impl<'de> Deserialize<'de> for RgbColor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let hex = value
            .strip_prefix('#')
            .ok_or_else(|| serde::de::Error::custom("RGB color must use #RRGGBB notation"))?;
        if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(serde::de::Error::custom(
                "RGB color must use #RRGGBB notation",
            ));
        }
        let component = |range: std::ops::Range<usize>| {
            u8::from_str_radix(&hex[range], 16).map_err(serde::de::Error::custom)
        };
        Ok(Self::new(
            component(0..2)?,
            component(2..4)?,
            component(4..6)?,
        ))
    }
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
    /// Operator-region macro transitions enforced when restyling.
    pub macro_transitions: Option<RestyleMacroConfig>,
    /// Number of operator-visible cue occurrences retained when restyling.
    pub operator_cue_limit: Option<NonZeroUsize>,
    /// Arrangement override.
    pub arrangement: Option<String>,
    /// Maximum logical lines per generated content slide.
    pub max_lines_per_slide: Option<NonZeroUsize>,
    /// Human-readable description.
    pub description: Option<String>,
}

/// Exact macro names for the two valid native presentation entry shapes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestyleMacroConfig {
    /// Ordered operator regions and their exact entry macros.
    pub regions: Vec<RestyleMacroRegionConfig>,
}

/// One configured native region transition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestyleMacroRegionConfig {
    /// Deterministic native selector for the region entry.
    pub selector: RestyleMacroSelectorConfig,
    /// Exact installed macro applied to the selected cue.
    pub enter_macro: String,
}

/// Native evidence used to select a region without guessing from its text.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RestyleMacroSelectorConfig {
    /// Zero-based cue occurrence in selected operator order.
    OperatorCue {
        /// Zero-based cue occurrence.
        index: usize,
    },
    /// Zero-based selected-arrangement group occurrence with accepted exact names.
    ArrangementGroup {
        /// Zero-based group occurrence in the selected arrangement.
        index: usize,
        /// Exact native group names accepted at this occurrence.
        names: Vec<String>,
    },
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
    pub(super) const fn is_empty(&self) -> bool {
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
        choices: BTreeMap<String, DecisionChoiceConfig>,
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
    /// Liturgical text, including responsive and marker-based descriptions;
    /// unmarked prose defaults to a leader.
    Liturgical,
    /// Liturgical text whose unmarked prose is congregational participation.
    LiturgicalAudience,
    /// A title/composer/performer content nametag.
    ContentNametag,
}

/// What the runtime should do with the resolved content.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutputStrategy {
    /// Skip this item.
    Skip,
    /// Preserve an existing file unchanged, including all native graphics and media.
    PreserveExisting,
    /// Replace only the presentation background in the selected library file.
    RestyleExisting,
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

const fn default_version() -> u16 {
    4
}
