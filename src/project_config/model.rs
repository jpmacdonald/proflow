//! Editable JSON schema for project-level service build configuration.

mod identity;
pub use identity::{BackgroundAssetPath, BackgroundId, LibraryName};
mod rendering;
pub use rendering::{CueRoleConfig, DisplayBindingConfig, RgbColor, SpeakerColorConfig};
mod rules;
pub use rules::{
    AmbiguousDecisionPolicy, DecisionChoiceConfig, DecisionChoiceMatch, DecisionConfig,
    DecisionContextField, ExpansionRule, ExpansionStep, ItemRuleConfig, ItemRuleOutcome,
    MatchCategory, MatchSpec, RuleAction, RuleTier, SpeakerSource, TargetSpec,
};

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::num::NonZeroUsize;

/// Editable JSON representation of project config.
///
/// This type owns the stable wire format and may temporarily contain invalid
/// cross-references while a candidate is being authored. Runtime planning must
/// use [`super::ProjectConfig`], which can only be constructed by validating
/// this raw value.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RawProjectConfig {
    /// Schema version — must be 4.
    #[schemars(range(min = 4, max = 4))]
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
    /// Exact recurring identities in the configured presentation library.
    #[serde(default)]
    pub library_identities: Vec<super::LibraryIdentityConfig>,
    /// Item rules with explicit precedence tiers.
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
            library_identities: Vec::new(),
            item_rules: Vec::new(),
            people: BTreeMap::new(),
            overrides: Vec::new(),
        }
    }
}

/// Descriptive project metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
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
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
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
    #[schemars(range(min = 1, max = 365))]
    pub days_ahead: Option<i64>,
    /// Bible translation used only when a scripture item does not name one.
    pub bible_version: Option<crate::bible::BibleVersion>,
    /// Item-rule ID whose explicit speaker is the final fallback for missing speakers.
    pub speaker_fallback_rule: Option<String>,
    /// Required slide-canvas size for generated and selected presentations.
    #[serde(default)]
    pub presentation_size: propresenter::PresentationSize,
}

impl Default for ProjectDefaults {
    fn default() -> Self {
        Self {
            library: LibraryName::default(),
            theme: None,
            background: None,
            days_ahead: None,
            bible_version: None,
            speaker_fallback_rule: None,
            presentation_size: propresenter::PresentationSize::FULL_HD,
        }
    }
}

/// Named set of service types.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
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
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RequiredPlaylistPlacement {
    /// Before all Planning Center-derived entries.
    Start,
    /// After all Planning Center-derived entries.
    End,
}

/// Presentation type behavior.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
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
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RestyleMacroConfig {
    /// Ordered operator regions and their exact entry macros.
    pub regions: Vec<RestyleMacroRegionConfig>,
}

/// One configured native region transition.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RestyleMacroRegionConfig {
    /// Deterministic native selector for the region entry.
    pub selector: RestyleMacroSelectorConfig,
    /// Exact installed macro applied to the selected cue.
    pub enter_macro: String,
}

/// Native evidence used to select a region without guessing from its text.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
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

/// Known person metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
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
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
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
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
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
#[derive(
    Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema,
)]
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
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
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
#[derive(
    Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema,
)]
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
#[derive(
    Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema,
)]
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
