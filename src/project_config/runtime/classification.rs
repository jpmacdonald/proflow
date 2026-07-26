//! Checked classification rules compiled from the editable v4 JSON shape.
//!
//! Runtime planning consumes these types directly. Text normalization, service
//! scopes, decision fields, and presentation references are resolved here once
//! so classification cannot rediscover config invariants for every item.

use std::collections::BTreeSet;
use std::sync::Arc;

mod compile;

pub use compile::{compile_classifications, compile_required_playlist_items};

use super::ExistingTransform;
use super::{ArrangementPolicy, ExistingTransformPolicy, PresentationPolicy, ServiceScope};
use crate::project_config::{
    AmbiguousDecisionPolicy, DecisionContextField, ItemKind, MatchCategory,
    RequiredPlaylistPlacement, RuleAction, RuleTier,
};

/// A presentation type key resolved to its checked runtime policy.
#[derive(Debug, Clone)]
pub struct ResolvedPresentationType {
    key: String,
    policy: Arc<PresentationPolicy>,
}

impl ResolvedPresentationType {
    pub(crate) fn key(&self) -> &str {
        &self.key
    }

    pub(crate) fn policy(&self) -> &PresentationPolicy {
        self.policy.as_ref()
    }

    pub(crate) fn kind(&self) -> ItemKind {
        self.policy.kind()
    }
}

/// Normalized facts shared by every configured matcher for one plan item.
#[derive(Debug)]
pub struct ItemMatchInput {
    category: MatchCategory,
    title: String,
    description: String,
    has_scripture_ref: bool,
    service_type: Option<String>,
}

impl ItemMatchInput {
    pub(crate) fn new(
        category: MatchCategory,
        title: &str,
        description: Option<&str>,
        has_scripture_ref: bool,
        service_type: Option<&str>,
    ) -> Self {
        Self {
            category,
            title: normalize_text(title),
            description: normalize_text(description.unwrap_or_default()),
            has_scripture_ref,
            service_type: service_type.map(normalize_identity),
        }
    }
}

/// Precedence across exact identities and semantic item rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassificationTier {
    /// An exact canonical library identity always wins over generic policy.
    LibraryIdentity,
    /// An ordinary semantic item-rule tier.
    ItemRule(RuleTier),
}

impl ClassificationTier {
    pub(crate) const fn precedence(self) -> u8 {
        match self {
            Self::LibraryIdentity => 3,
            Self::ItemRule(tier) => tier.precedence(),
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::LibraryIdentity => "library identity",
            Self::ItemRule(tier) => tier.as_str(),
        }
    }
}

/// One checked classification whose predicates and outcome are runtime-ready.
#[derive(Debug, Clone)]
pub struct CompiledClassification {
    id: String,
    matcher: CompiledItemMatcher,
    outcome: CompiledRuleOutcome,
    tier: ClassificationTier,
}

impl CompiledClassification {
    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn matches(&self, input: &ItemMatchInput) -> bool {
        self.matcher.matches(input)
    }

    pub(crate) const fn outcome(&self) -> &CompiledRuleOutcome {
        &self.outcome
    }

    pub(crate) const fn tier(&self) -> ClassificationTier {
        self.tier
    }
}

#[derive(Debug, Clone)]
struct CompiledItemMatcher {
    title_prefix: Vec<String>,
    title_contains: Vec<String>,
    description_contains: Vec<String>,
    category: Option<MatchCategory>,
    has_scripture_ref: Option<bool>,
    service_types: Option<BTreeSet<String>>,
}

impl CompiledItemMatcher {
    fn matches(&self, input: &ItemMatchInput) -> bool {
        if self.service_types.as_ref().is_some_and(|service_types| {
            input
                .service_type
                .as_ref()
                .is_none_or(|service_type| !service_types.contains(service_type))
        }) {
            return false;
        }
        if self
            .category
            .is_some_and(|category| category != input.category)
        {
            return false;
        }
        if self
            .has_scripture_ref
            .is_some_and(|expected| expected != input.has_scripture_ref)
        {
            return false;
        }
        if !self.title_prefix.is_empty()
            && !self
                .title_prefix
                .iter()
                .any(|prefix| input.title.starts_with(prefix))
        {
            return false;
        }
        if !self.title_contains.is_empty()
            && !self
                .title_contains
                .iter()
                .any(|needle| input.title.contains(needle))
        {
            return false;
        }
        if !self.description_contains.is_empty()
            && !self
                .description_contains
                .iter()
                .any(|needle| input.description.contains(needle))
        {
            return false;
        }
        true
    }
}

/// One checked outcome from an item rule.
#[derive(Debug, Clone)]
pub enum CompiledRuleOutcome {
    UseType {
        presentation: ResolvedPresentationType,
        target: CompiledDirectTarget,
    },
    Action(RuleAction),
    Decision(CompiledDecision),
    Expand(Vec<CompiledExpansionStep>),
}

/// Target shape accepted by a direct item rule or non-speaker expansion.
#[derive(Debug, Clone)]
pub enum CompiledDirectTarget {
    /// Derive the source or generated name from the Planning Center item.
    Automatic,
    /// Resolve one exact existing library presentation.
    LibraryFile(String),
}

impl CompiledDirectTarget {
    pub(crate) fn library_file(&self) -> Option<&str> {
        match self {
            Self::Automatic => None,
            Self::LibraryFile(file) => Some(file),
        }
    }
}

/// One expansion step narrowed to its two valid execution shapes.
#[derive(Debug, Clone)]
pub enum CompiledExpansionStep {
    /// Build directly from the matched item.
    Direct {
        presentation: ResolvedPresentationType,
        target: CompiledDirectTarget,
    },
    /// Resolve a configured speaker before building.
    Speaker {
        presentation: ResolvedPresentationType,
        target: CompiledSpeakerTarget,
    },
}

impl CompiledExpansionStep {
    pub(crate) const fn presentation(&self) -> &ResolvedPresentationType {
        match self {
            Self::Direct { presentation, .. } | Self::Speaker { presentation, .. } => presentation,
        }
    }
}

/// Target shape accepted after a speaker has been resolved.
#[derive(Debug, Clone)]
pub enum CompiledSpeakerTarget {
    /// Use configured person metadata or the normal item-derived target.
    Automatic,
    /// Resolve one exact existing library presentation.
    LibraryFile(String),
    /// Generate a presentation name from the item and resolved speaker.
    NameTemplate(String),
}

/// A contextual choice whose field set and text predicates are compiled.
#[derive(Debug, Clone)]
pub struct CompiledDecision {
    context_fields: Vec<DecisionContextField>,
    instructions: Option<String>,
    choices: Vec<CompiledDecisionChoice>,
    on_ambiguous: AmbiguousDecisionPolicy,
}

impl CompiledDecision {
    pub(crate) fn matching_choices(
        &self,
        title: &str,
        description: Option<&str>,
        note: Option<&str>,
    ) -> Vec<&CompiledDecisionChoice> {
        let mut values = Vec::with_capacity(self.context_fields.len());
        for field in &self.context_fields {
            match field {
                DecisionContextField::Title => values.push(title),
                DecisionContextField::Description => {
                    if let Some(description) = description {
                        values.push(description);
                    }
                }
                DecisionContextField::Note => {
                    if let Some(note) = note {
                        values.push(note);
                    }
                }
            }
        }
        let context = normalize_text(&values.join("\n"));
        self.choices
            .iter()
            .filter(|choice| choice.matcher.matches(&context))
            .collect()
    }

    pub(crate) const fn on_ambiguous(&self) -> AmbiguousDecisionPolicy {
        self.on_ambiguous
    }

    pub(crate) fn review_reason(
        &self,
        rule_id: &str,
        matched: &[&CompiledDecisionChoice],
    ) -> String {
        let choices = self
            .choices
            .iter()
            .map(|choice| choice.key.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let base = if matched.is_empty() {
            format!(
                "Rule '{rule_id}' needs contextual choice; no choice matched. Choices: {choices}"
            )
        } else {
            let matched = matched
                .iter()
                .map(|choice| choice.key.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            format!("Rule '{rule_id}' needs contextual choice; multiple choices matched: {matched}")
        };
        self.instructions.as_deref().map_or_else(
            || base.clone(),
            |instructions| format!("{base}. {instructions}"),
        )
    }
}

/// One decision choice with a required target and presentation policy.
#[derive(Debug, Clone)]
pub struct CompiledDecisionChoice {
    key: String,
    presentation: ResolvedPresentationType,
    library_file: String,
    matcher: CompiledDecisionMatcher,
}

impl CompiledDecisionChoice {
    pub(crate) fn key(&self) -> &str {
        &self.key
    }

    pub(crate) const fn presentation(&self) -> &ResolvedPresentationType {
        &self.presentation
    }

    pub(crate) fn library_file(&self) -> &str {
        &self.library_file
    }
}

#[derive(Debug, Clone)]
struct CompiledDecisionMatcher {
    any: Vec<String>,
    all: Vec<String>,
    none: Vec<String>,
}

impl CompiledDecisionMatcher {
    fn matches(&self, context: &str) -> bool {
        if self.none.iter().any(|needle| context.contains(needle)) {
            return false;
        }
        if !self.all.is_empty() && !self.all.iter().all(|needle| context.contains(needle)) {
            return false;
        }
        self.any.is_empty() || self.any.iter().any(|needle| context.contains(needle))
    }
}

/// One required playlist item with its scope and static policy checked.
#[derive(Debug, Clone)]
pub struct CompiledRequiredPlaylistItem {
    id: String,
    type_key: String,
    library_file: String,
    placement: RequiredPlaylistPlacement,
    scope: ServiceScope,
    presentation: RequiredPresentation,
}

impl CompiledRequiredPlaylistItem {
    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn type_key(&self) -> &str {
        &self.type_key
    }

    pub(crate) fn library_file(&self) -> &str {
        &self.library_file
    }

    pub(crate) const fn placement(&self) -> RequiredPlaylistPlacement {
        self.placement
    }

    pub(crate) fn applies_to(&self, service_name: Option<&str>) -> bool {
        self.scope.matches(service_name)
    }

    pub(crate) fn presentation_for_service(
        &self,
        service_name: Option<&str>,
    ) -> ResolvedRequiredPresentation {
        match &self.presentation {
            RequiredPresentation::Preserve { kind, arrangement } => {
                ResolvedRequiredPresentation::Preserve {
                    kind: *kind,
                    arrangement: arrangement.for_service(service_name),
                }
            }
            RequiredPresentation::Restyle {
                kind,
                arrangement,
                transform,
            } => ResolvedRequiredPresentation::Restyle {
                kind: *kind,
                arrangement: arrangement.for_service(service_name),
                transform: transform.for_service(service_name),
            },
        }
    }
}

#[derive(Debug, Clone)]
enum RequiredPresentation {
    Preserve {
        kind: ItemKind,
        arrangement: ArrangementPolicy,
    },
    Restyle {
        kind: ItemKind,
        arrangement: ArrangementPolicy,
        transform: ExistingTransformPolicy,
    },
}

/// Service-resolved operation for one checked required playlist item.
pub enum ResolvedRequiredPresentation {
    Preserve {
        kind: ItemKind,
        arrangement: Option<String>,
    },
    Restyle {
        kind: ItemKind,
        arrangement: Option<String>,
        transform: ExistingTransform,
    },
}

fn normalize_phrases(phrases: &[String]) -> Vec<String> {
    phrases
        .iter()
        .map(|phrase| normalize_text(phrase))
        .collect()
}

fn normalize_identity(value: &str) -> String {
    value.to_ascii_lowercase()
}

fn normalize_text(value: &str) -> String {
    value.to_lowercase().replace(['\u{2018}', '\u{2019}'], "'")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::project_config::parse_project_config_str;

    #[test]
    fn compiled_matcher_normalizes_text_and_service_once() {
        let config = parse_project_config_str(
            r#"{
              "version": 4,
              "item_rules": [{
                "id": "normalized",
                "match": {
                  "title_prefix": ["Leader’s Prayer"],
                  "description_contains": ["PEOPLE"],
                  "category": "text",
                  "service_type": ["Sunday Worship"]
                },
                "action": {"kind": "skip", "reason": "fixture"}
              }]
            }"#,
        )
        .expect("config should compile");

        let input = ItemMatchInput::new(
            MatchCategory::Text,
            "LEADER'S PRAYER OF CONFESSION",
            Some("People respond"),
            false,
            Some("sunday worship"),
        );
        assert!(config.compiled_classifications()[0].matches(&input));
    }

    #[test]
    fn canonical_library_identity_has_stronger_precedence_than_item_rules() {
        let config = parse_project_config_str(
            r#"{
              "version": 4,
              "presentation_types": {
                "song": {
                  "kind": "song",
                  "content_source": "song",
                  "output_strategy": "preserve_existing"
                }
              },
              "library_identities": [{
                "id": "g2g_hymn",
                "match": {
                  "kind": "title_prefix",
                  "values": ["g2g #840 it is well with my soul"]
                },
                "use_type": "song",
                "library_file": "[Hymn] It Is Well With My Soul (G2G).pro"
              }],
              "item_rules": [{
                "id": "all_songs",
                "tier": "catch_all",
                "match": {"category": "song"},
                "use_type": "song"
              }]
            }"#,
        )
        .expect("identity config should compile");

        let input = ItemMatchInput::new(
            MatchCategory::Song,
            "G2G #840 It Is Well With My Soul",
            None,
            false,
            Some("10:30am Traditional"),
        );
        let matches = config
            .compiled_classifications()
            .iter()
            .filter(|classification| classification.matches(&input))
            .collect::<Vec<_>>();

        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].id(), "g2g_hymn");
        assert_eq!(matches[0].tier(), ClassificationTier::LibraryIdentity);
        assert!(
            matches[0].tier().precedence() > matches[1].tier().precedence(),
            "exact identity must win without depending on config array order"
        );
    }

    #[test]
    fn compiled_decision_supports_all_only_and_none_only_matchers() {
        let all_only = CompiledDecisionMatcher {
            any: Vec::new(),
            all: vec!["child".to_string(), "baptism".to_string()],
            none: Vec::new(),
        };
        let none_only = CompiledDecisionMatcher {
            any: Vec::new(),
            all: Vec::new(),
            none: vec!["private".to_string()],
        };

        assert!(all_only.matches("child baptism during worship"));
        assert!(!all_only.matches("child dedication"));
        assert!(none_only.matches("public baptism"));
        assert!(!none_only.matches("private baptism"));
    }

    #[test]
    fn required_item_compiles_scope_and_static_policy() {
        let config = parse_project_config_str(
            r#"{
              "version": 4,
              "service_groups": {"traditional": {"service_types": ["10:30am Traditional"]}},
              "presentation_types": {
                "graphic": {"kind":"graphic", "content_source":"static", "output_strategy":"preserve_existing"}
              },
              "required_playlist_items": [{
                "id":"closing", "use_type":"graphic", "library_file":"Closing.pro",
                "placement":"end", "service_group":"traditional"
              }]
            }"#,
        )
        .expect("config should compile");

        let required = &config.compiled_required_playlist_items()[0];
        assert!(required.applies_to(Some("10:30AM TRADITIONAL")));
        assert!(!required.applies_to(Some("9am Contemporary")));
        assert!(matches!(
            required.presentation_for_service(None),
            ResolvedRequiredPresentation::Preserve {
                kind: ItemKind::Graphic,
                ..
            }
        ));
    }
}
