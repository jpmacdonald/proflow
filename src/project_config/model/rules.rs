//! Rule precedence, decisions, and target serialization.

use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;

/// Matching rule with explicit precedence for items.
#[derive(Debug, Clone, schemars::JsonSchema)]
#[schemars(with = "ItemRuleConfigWire")]
pub struct ItemRuleConfig {
    /// Stable rule identifier.
    pub id: String,
    /// Match criteria.
    pub match_spec: MatchSpec,
    /// The single outcome produced by a match.
    pub outcome: ItemRuleOutcome,
    /// Explicit precedence tier used when more than one rule matches.
    pub tier: RuleTier,
    /// Free-form notes.
    pub notes: Option<String>,
}

/// Explicit precedence for classification rules.
///
/// The highest matching tier wins. Multiple matches in that tier remain
/// ambiguous and require review, so array position never decides behavior.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum RuleTier {
    /// Last-resort rule, such as the generic rule for every song.
    CatchAll,
    /// Domain fallback that wins over a generic catch-all.
    Fallback,
    /// Ordinary specific rule.
    #[default]
    Primary,
}

impl RuleTier {
    /// Numeric precedence used only for deterministic tier comparison.
    pub const fn precedence(self) -> u8 {
        match self {
            Self::Primary => 2,
            Self::Fallback => 1,
            Self::CatchAll => 0,
        }
    }

    /// Stable operator-facing name used in ambiguity diagnostics.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Fallback => "fallback",
            Self::CatchAll => "catch-all",
        }
    }
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

#[derive(Deserialize, schemars::JsonSchema)]
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
    #[serde(default)]
    tier: RuleTier,
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
            tier: wire.tier,
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
            )) + usize::from(self.tier != RuleTier::Primary)
                + usize::from(self.notes.is_some());
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
        if self.tier != RuleTier::Primary {
            map.serialize_entry("tier", &self.tier)?;
        }
        if let Some(notes) = &self.notes {
            map.serialize_entry("notes", notes)?;
        }
        map.end()
    }
}

/// Match criteria for a rule.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
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
    /// Optional typed Planning Center category.
    pub category: Option<MatchCategory>,
    /// Whether the item contains a scripture reference.
    pub has_scripture_ref: Option<bool>,
    /// Restrict the rule to specific service types.
    #[serde(default)]
    pub service_type: Vec<String>,
}

/// Planning Center category accepted by an item-rule matcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MatchCategory {
    /// Text or liturgical content.
    Text,
    /// Static graphical content.
    Graphic,
    /// A title or nametag item.
    Title,
    /// A song or hymn item.
    Song,
    /// An uncategorized Planning Center item.
    Other,
}

impl MatchSpec {
    pub(in crate::project_config) const fn is_empty(&self) -> bool {
        self.title_prefix.is_empty()
            && self.title_contains.is_empty()
            && self.description_contains.is_empty()
            && self.category.is_none()
            && self.has_scripture_ref.is_none()
            && self.service_type.is_empty()
    }
}

/// Bounded contextual decision made from PCO fields.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DecisionConfig {
    /// Choose one existing library file from configured choices.
    ChooseExistingFile {
        /// PCO fields available for the choice (`title`, `description`, `note`).
        #[serde(default)]
        context_fields: Vec<DecisionContextField>,
        /// Human-readable instructions surfaced in review contexts.
        instructions: Option<String>,
        /// Allowed choices.
        #[serde(default)]
        choices: BTreeMap<String, DecisionChoiceConfig>,
        /// What to do when no choice or multiple choices match.
        on_ambiguous: Option<AmbiguousDecisionPolicy>,
    },
}

/// Planning Center text field included in a contextual decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DecisionContextField {
    /// The Planning Center item title.
    Title,
    /// The Planning Center item description.
    Description,
    /// The Planning Center item note.
    Note,
}

impl DecisionContextField {
    pub(in crate::project_config) const fn as_str(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Description => "description",
            Self::Note => "note",
        }
    }
}

/// One allowed contextual choice.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
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
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
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
#[derive(
    Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum AmbiguousDecisionPolicy {
    /// Ask the user/LLM supervisor before building.
    #[default]
    Ask,
    /// Skip the item.
    Skip,
}

/// Explicit rule action.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
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
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
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
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SpeakerSource {
    /// Use the resolved speaker for the matched item.
    Resolved,
}

/// One explicit output target.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
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
