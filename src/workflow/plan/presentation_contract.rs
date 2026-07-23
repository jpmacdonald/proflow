//! Semantic expectations that final native presentation bytes must satisfy.

use serde::Serialize;

use super::{PlanDisposition, ReadyAction, ResolvedItemPlan};
use crate::project_config::{
    CueTransform, MacroTransform, RenderRole, RenderStyle, RestyleMacroSelector,
};
use crate::propresenter::PresentationSize;
use crate::workflow::description_parser::SpeakerRole;

/// Native presentation properties promised by one reviewed semantic plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExpectedPresentationContract {
    /// Required canvas for every presentation slide action.
    pub canvas: PresentationSize,
    /// Required number of cues reached by the effective operator traversal.
    pub operator_cues: ExpectedCueCount,
    /// Playlist-selected arrangement name, when one was requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arrangement: Option<String>,
    /// Background media basename required on the first operator cue.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_background: Option<String>,
    /// Whether native macro transitions are retained or completely owned by
    /// the reviewed operation.
    pub macros: ExpectedMacroPolicy,
    /// Whether the final document must carry one representable native
    /// Bible-reference identity.
    pub requires_scripture_metadata: bool,
    /// Whether the final document must label operator-visible scripture cues.
    pub requires_scripture_labels: bool,
}

/// Checked operator-cue cardinality promised before native rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "count", rename_all = "snake_case")]
pub enum ExpectedCueCount {
    /// The presentation must contain at least one operator-visible cue.
    NonEmpty,
    /// The presentation must contain exactly this non-zero number of cues.
    Exact(usize),
}

/// Ownership of macro transitions in the effective operator traversal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "regions", rename_all = "snake_case")]
pub enum ExpectedMacroPolicy {
    /// Reused native macro actions must remain unchanged.
    Preserve,
    /// These are the only macro actions allowed in operator traversal.
    Exact(Vec<ExpectedMacroRegion>),
}

/// One macro transition promised by a generated or restyled presentation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExpectedMacroRegion {
    /// Semantic location where the transition must occur.
    pub selector: ExpectedMacroSelector,
    /// Exact installed macro name required at that location.
    pub macro_name: String,
}

/// Stable semantic selector for a macro-entry region.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExpectedMacroSelector {
    /// Zero-based cue occurrence in effective operator order.
    OperatorCue {
        /// Zero-based position in operator traversal order.
        index: usize,
    },
    /// Zero-based group occurrence in the selected arrangement.
    ArrangementGroup {
        /// Zero-based group occurrence in arrangement order.
        index: usize,
        /// Exact native group names accepted at this occurrence.
        allowed_names: Vec<String>,
    },
}

/// A plan that cannot legally promise materialized presentation bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PresentationContractPlanError {
    /// The plan still requires an operator decision.
    #[error("an unresolved plan cannot define a native presentation contract")]
    NeedsReview,
}

impl ExpectedPresentationContract {
    /// Derive the one native-output promise carried by a resolved plan.
    ///
    /// Skipped plans deliberately promise no presentation. Unresolved plans
    /// fail instead of acquiring guessed native semantics.
    pub fn from_plan(
        plan: &ResolvedItemPlan,
        canvas: PresentationSize,
    ) -> Result<Option<Self>, PresentationContractPlanError> {
        let action = match plan.disposition() {
            PlanDisposition::Skip => return Ok(None),
            PlanDisposition::NeedsReview(_) => {
                return Err(PresentationContractPlanError::NeedsReview)
            }
            PlanDisposition::Ready(action) => action,
        };

        let mut contract = Self {
            canvas,
            operator_cues: ExpectedCueCount::NonEmpty,
            arrangement: action.arrangement().map(str::to_string),
            first_background: action.background().and_then(|background| {
                background
                    .file()
                    .as_path()
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            }),
            macros: ExpectedMacroPolicy::Preserve,
            requires_scripture_metadata: matches!(
                action,
                ReadyAction::GenerateScripture { scripture, .. }
                    if !matches!(scripture.request(), super::ScriptureRequest::Combined(_))
            ),
            requires_scripture_labels: matches!(action, ReadyAction::GenerateScripture { .. }),
        };

        match action {
            ReadyAction::UseExisting { .. } => {}
            ReadyAction::RestyleExisting { transform, .. } => {
                if let CueTransform::RetainOperatorPrefix(count) = transform.cues() {
                    contract.operator_cues = ExpectedCueCount::Exact(count.get());
                }
                if let MacroTransform::Enforce(policy) = transform.macros() {
                    contract.macros = ExpectedMacroPolicy::Exact(
                        policy
                            .regions()
                            .iter()
                            .map(|region| ExpectedMacroRegion {
                                selector: match region.selector() {
                                    RestyleMacroSelector::OperatorCue { index } => {
                                        ExpectedMacroSelector::OperatorCue { index: *index }
                                    }
                                    RestyleMacroSelector::ArrangementGroup {
                                        index,
                                        allowed_names,
                                    } => ExpectedMacroSelector::ArrangementGroup {
                                        index: *index,
                                        allowed_names: allowed_names.iter().cloned().collect(),
                                    },
                                },
                                macro_name: region.enter_macro().to_string(),
                            })
                            .collect(),
                    );
                }
            }
            ReadyAction::EditDescription {
                parsed_content,
                style,
                ..
            }
            | ReadyAction::GenerateDescription {
                parsed_content,
                style,
            } => {
                let has_title = parsed_content
                    .title_text()
                    .is_some_and(|title| !title.is_empty());
                let starts_with_leader = parsed_content
                    .segments()
                    .iter()
                    .find(|segment| !segment.text.is_empty())
                    .is_some_and(|segment| segment.speaker == SpeakerRole::Leader);
                contract.macros = ExpectedMacroPolicy::Exact(generated_macro_regions(
                    style,
                    has_title,
                    starts_with_leader,
                ));
            }
            ReadyAction::GenerateScripture { style, .. } => {
                contract.macros =
                    ExpectedMacroPolicy::Exact(generated_macro_regions(style, true, false));
            }
            ReadyAction::GenerateTitle { style, .. } => {
                contract.operator_cues = ExpectedCueCount::Exact(1);
                let title_role = style.title().unwrap_or_else(|| style.content());
                contract.macros = ExpectedMacroPolicy::Exact(
                    role_macro(title_role, 0, false).into_iter().collect(),
                );
            }
        }

        Ok(Some(contract))
    }
}

fn generated_macro_regions(
    style: &RenderStyle,
    has_title: bool,
    starts_with_leader: bool,
) -> Vec<ExpectedMacroRegion> {
    let mut regions = Vec::new();
    let content_index = usize::from(has_title);
    if has_title {
        let title_role = style.title().unwrap_or_else(|| style.content());
        regions.extend(role_macro(title_role, 0, false));
    }
    regions.extend(role_macro(
        style.content(),
        content_index,
        starts_with_leader,
    ));
    regions
}

fn role_macro(
    role: &RenderRole,
    operator_index: usize,
    starts_with_leader: bool,
) -> Option<ExpectedMacroRegion> {
    role.cue_macro().map(|binding| ExpectedMacroRegion {
        selector: ExpectedMacroSelector::OperatorCue {
            index: operator_index,
        },
        macro_name: binding.select(starts_with_leader).to_string(),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::collections::BTreeMap;

    use super::*;
    use crate::project_config::{
        BackgroundAssetPath, BackgroundId, BackgroundTransform, CueMacro, CueTransform,
        ExistingTransform, MacroTransform, RenderRole, ResolvedBackground, RestyleMacroPolicy,
        RestyleMacroRegion, RestyleMacroSelector, SpeakerPalette,
    };
    use crate::workflow::description_parser::{ParsedContent, ParsedSegment};
    use crate::workflow::plan::{ItemKind, OutputKey, ReviewContext};

    fn plan(disposition: PlanDisposition) -> ResolvedItemPlan {
        ResolvedItemPlan::new(
            OutputKey::new("contract:test".to_string()).expect("valid key"),
            0,
            "Test".to_string(),
            "Test".to_string(),
            "contract fixture".to_string(),
            ItemKind::Other,
            None,
            disposition,
        )
    }

    fn role(id: &str, macro_name: &str) -> RenderRole {
        RenderRole::new(
            id.to_string(),
            id.to_string(),
            BTreeMap::new(),
            Some(CueMacro::new(macro_name.to_string(), None).expect("valid macro")),
            None,
        )
        .expect("valid role")
    }

    fn hymn_style() -> RenderStyle {
        RenderStyle::new(
            Some(ResolvedBackground::new(
                BackgroundId::new("lyrics").expect("valid background id"),
                BackgroundAssetPath::new("backgrounds/lyrics.png").expect("valid background"),
            )),
            role("content", "Song"),
            Some(role("title", "Name Tag/Title")),
            None,
        )
        .expect("valid style")
    }

    fn content(text: &str, title: Option<&str>, speaker: SpeakerRole) -> ParsedContent {
        ParsedContent::new(
            vec![ParsedSegment {
                text: text.to_string(),
                speaker,
                bold: None,
                italic: None,
            }],
            title.map(str::to_string),
        )
    }

    #[test]
    fn skipped_and_unresolved_plans_cannot_promise_artifacts() {
        assert_eq!(
            ExpectedPresentationContract::from_plan(
                &plan(PlanDisposition::Skip),
                PresentationSize::FULL_HD,
            ),
            Ok(None)
        );
        assert_eq!(
            ExpectedPresentationContract::from_plan(
                &plan(PlanDisposition::NeedsReview(ReviewContext::new(None))),
                PresentationSize::FULL_HD,
            ),
            Err(PresentationContractPlanError::NeedsReview)
        );
    }

    #[test]
    fn titled_song_contract_records_both_macro_regions_and_background() {
        let content = ParsedContent::new(
            vec![ParsedSegment {
                text: "Praise God".to_string(),
                speaker: SpeakerRole::Neutral,
                bold: None,
                italic: None,
            }],
            Some("Doxology".to_string()),
        );
        let contract = ExpectedPresentationContract::from_plan(
            &plan(PlanDisposition::Ready(ReadyAction::GenerateDescription {
                parsed_content: content,
                style: hymn_style(),
            })),
            PresentationSize::FULL_HD,
        )
        .expect("resolved contract")
        .expect("materialized presentation");

        assert_eq!(contract.first_background.as_deref(), Some("lyrics.png"));
        assert_eq!(
            contract.macros,
            ExpectedMacroPolicy::Exact(vec![
                ExpectedMacroRegion {
                    selector: ExpectedMacroSelector::OperatorCue { index: 0 },
                    macro_name: "Name Tag/Title".to_string(),
                },
                ExpectedMacroRegion {
                    selector: ExpectedMacroSelector::OperatorCue { index: 1 },
                    macro_name: "Song".to_string(),
                },
            ])
        );
    }

    #[test]
    fn scripture_contract_requires_native_metadata() {
        let scripture = crate::workflow::plan::ScriptureContent::single(
            "John 3:16".to_string(),
            "NRSVue".to_string(),
        )
        .expect("valid scripture");
        let contract = ExpectedPresentationContract::from_plan(
            &plan(PlanDisposition::Ready(ReadyAction::GenerateScripture {
                scripture,
                style: hymn_style(),
            })),
            PresentationSize::FULL_HD,
        )
        .expect("resolved contract")
        .expect("materialized presentation");

        assert!(contract.requires_scripture_metadata);
        assert!(contract.requires_scripture_labels);
        assert!(matches!(
            contract.macros,
            ExpectedMacroPolicy::Exact(regions) if regions.len() == 2
        ));
    }

    #[test]
    fn song_hymn_and_doxology_derive_their_distinct_macro_shapes() {
        let worship = ExpectedPresentationContract::from_plan(
            &plan(PlanDisposition::Ready(ReadyAction::GenerateDescription {
                parsed_content: content("Worship lyric", None, SpeakerRole::Neutral),
                style: hymn_style(),
            })),
            PresentationSize::FULL_HD,
        )
        .expect("resolved worship")
        .expect("worship artifact");
        assert_eq!(
            worship.macros,
            ExpectedMacroPolicy::Exact(vec![ExpectedMacroRegion {
                selector: ExpectedMacroSelector::OperatorCue { index: 0 },
                macro_name: "Song".to_string(),
            }])
        );

        for title in ["Hymn 510", "Doxology"] {
            let titled = ExpectedPresentationContract::from_plan(
                &plan(PlanDisposition::Ready(ReadyAction::GenerateDescription {
                    parsed_content: content("Song lyric", Some(title), SpeakerRole::Neutral),
                    style: hymn_style(),
                })),
                PresentationSize::FULL_HD,
            )
            .expect("resolved titled song")
            .expect("titled song artifact");
            assert!(matches!(
                titled.macros,
                ExpectedMacroPolicy::Exact(ref regions)
                    if regions.len() == 2
                        && regions[0].macro_name == "Name Tag/Title"
                        && regions[1].macro_name == "Song"
            ));
        }
    }

    #[test]
    fn responsive_liturgy_selects_the_leader_entry_macro() {
        let responsive_role = RenderRole::new(
            "liturgy".to_string(),
            "Liturgy".to_string(),
            BTreeMap::new(),
            Some(
                CueMacro::new(
                    "Scripture/Prayer".to_string(),
                    Some("Scripture/Prayer (Highlighted)".to_string()),
                )
                .expect("responsive macro"),
            ),
            Some(SpeakerPalette::new((255, 255, 0), (255, 255, 255))),
        )
        .expect("responsive role");
        let liturgy = ExpectedPresentationContract::from_plan(
            &plan(PlanDisposition::Ready(ReadyAction::GenerateDescription {
                parsed_content: content("Leader words", None, SpeakerRole::Leader),
                style: RenderStyle::new(None, responsive_role, None, None)
                    .expect("responsive style"),
            })),
            PresentationSize::FULL_HD,
        )
        .expect("resolved liturgy")
        .expect("liturgy artifact");
        assert!(matches!(
            liturgy.macros,
            ExpectedMacroPolicy::Exact(ref regions)
                if regions[0].macro_name == "Scripture/Prayer (Highlighted)"
        ));
    }

    #[test]
    fn nametag_and_graphic_contracts_own_their_macro_entries() {
        let nametag = ExpectedPresentationContract::from_plan(
            &plan(PlanDisposition::Ready(ReadyAction::GenerateTitle {
                text: "Speaker".to_string(),
                style: hymn_style(),
            })),
            PresentationSize::FULL_HD,
        )
        .expect("resolved nametag")
        .expect("nametag artifact");
        assert_eq!(nametag.operator_cues, ExpectedCueCount::Exact(1));
        assert!(matches!(
            nametag.macros,
            ExpectedMacroPolicy::Exact(ref regions)
                if regions[0].macro_name == "Name Tag/Title"
        ));

        let graphic_policy = RestyleMacroPolicy::new(vec![RestyleMacroRegion::new(
            RestyleMacroSelector::OperatorCue { index: 0 },
            "Graphics".to_string(),
        )
        .expect("graphic region")])
        .expect("graphic policy");
        let graphic = ExpectedPresentationContract::from_plan(
            &plan(PlanDisposition::Ready(ReadyAction::RestyleExisting {
                file_path: "graphic.pro".into(),
                arrangement: None,
                transform: ExistingTransform::new(
                    BackgroundTransform::Preserve,
                    MacroTransform::Enforce(graphic_policy),
                    CueTransform::Preserve,
                )
                .expect("graphic transform"),
            })),
            PresentationSize::FULL_HD,
        )
        .expect("resolved graphic")
        .expect("graphic artifact");
        assert!(matches!(
            graphic.macros,
            ExpectedMacroPolicy::Exact(ref regions) if regions[0].macro_name == "Graphics"
        ));
    }
}
