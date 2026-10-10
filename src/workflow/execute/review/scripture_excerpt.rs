//! Reconciliation of description-bounded scripture proposals.

use crate::bible::{BibleCorpusSnapshot, BibleVersion};
use crate::workflow::execute::BuildServiceError;
use crate::workflow::plan::{ReadyAction, ResolvedItemPlan, ScriptureRequest};

enum DescriptionScriptureProposal {
    Prefix {
        reference: String,
        display_reference: String,
        bible_version: BibleVersion,
        excerpt_text: String,
    },
    Selected {
        reference: String,
        bible_version: BibleVersion,
        excerpt_text: String,
    },
}

pub(super) fn reconcile_description_scripture_excerpts(
    plans: &mut [ResolvedItemPlan],
    bible: &BibleCorpusSnapshot,
) -> Result<(), BuildServiceError> {
    for plan in plans {
        if !plan.needs_review() {
            continue;
        }
        let Some(proposal) = plan.preview_action().and_then(|action| match action {
            ReadyAction::GenerateScripture { scripture, .. } => match scripture.request() {
                ScriptureRequest::PrefixExcerpt {
                    reference,
                    display_reference,
                    bible_version,
                    excerpt_text,
                } => Some(DescriptionScriptureProposal::Prefix {
                    reference: reference.to_string(),
                    display_reference: display_reference.to_string(),
                    bible_version,
                    excerpt_text: excerpt_text.to_string(),
                }),
                ScriptureRequest::SelectedExcerpt {
                    reference,
                    bible_version,
                    excerpt_text,
                } => Some(DescriptionScriptureProposal::Selected {
                    reference: reference.to_string(),
                    bible_version,
                    excerpt_text: excerpt_text.to_string(),
                }),
                ScriptureRequest::Single { .. } | ScriptureRequest::Combined(_) => None,
            },
            _ => None,
        }) else {
            continue;
        };
        let (approved_reason, validation) = match &proposal {
            DescriptionScriptureProposal::Prefix {
                reference,
                display_reference,
                bible_version,
                excerpt_text,
            } => (
                format!(
                    "Generate description-bounded scripture slides ({display_reference} {})",
                    bible_version.name()
                ),
                validate_description_scripture_excerpt(
                    bible,
                    reference,
                    *bible_version,
                    excerpt_text,
                )
                .map_err(|error| {
                    format!("Partial scripture '{display_reference}' requires review: {error}")
                }),
            ),
            DescriptionScriptureProposal::Selected {
                reference,
                bible_version,
                excerpt_text,
            } => (
                format!(
                    "Generate Planning Center selected-verse scripture slides ({reference} {})",
                    bible_version.name()
                ),
                validate_selected_scripture_excerpt(bible, reference, *bible_version, excerpt_text)
                    .map_err(|error| {
                        format!("Selected scripture '{reference}' requires review: {error}")
                    }),
            ),
        };
        if let Err(reason) = validation {
            plan.reason = reason;
            continue;
        }

        if !plan.approve_proposed_action()? {
            continue;
        }
        plan.reason = approved_reason;
    }
    Ok(())
}

fn validate_selected_scripture_excerpt(
    bible: &BibleCorpusSnapshot,
    reference_text: &str,
    bible_version: BibleVersion,
    excerpt_text: &str,
) -> Result<(), String> {
    let reference = crate::bible::parse_scripture_ref(reference_text)
        .ok_or_else(|| format!("cannot parse chapter lookup '{reference_text}'"))?;
    let (header, verses) = bible
        .lookup_verses(&reference, bible_version)
        .map_err(|error| error.to_string())?;
    if !header.missing_verses.is_empty() {
        return Err(format!(
            "local Bible data is missing verses {:?}",
            header.missing_verses
        ));
    }
    crate::bible::reconcile_selected_excerpt(&verses, excerpt_text)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn validate_description_scripture_excerpt(
    bible: &BibleCorpusSnapshot,
    reference_text: &str,
    bible_version: BibleVersion,
    excerpt_text: &str,
) -> Result<(), String> {
    let reference = crate::bible::parse_scripture_ref(reference_text)
        .ok_or_else(|| format!("cannot parse whole-verse lookup '{reference_text}'"))?;
    let (header, verses) = bible
        .lookup_verses(&reference, bible_version)
        .map_err(|error| error.to_string())?;
    if !header.missing_verses.is_empty() {
        return Err(format!(
            "local Bible data is missing verses {:?}",
            header.missing_verses
        ));
    }
    crate::bible::reconcile_prefix_excerpt(&verses, excerpt_text)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::planning_center::types::{Category, Item};
    use crate::project_config::parse_project_config_str;

    const EXODUS_DESCRIPTION: &str = "1 The whole congregation of the Israelites set out from Elim and came to the wilderness of Sin, which is between Elim and Sinai, on the fifteenth day of the second month after they had departed from the land of Egypt.\n\
         2 The whole congregation of the Israelites complained against Moses and Aaron in the wilderness.\n\
         3 The Israelites said to them, ‘If only we had died by the hand of the Lord in the land of Egypt, when we sat by the pots of meat and ate our fill of bread, for you have brought us out into this wilderness to kill this whole assembly with hunger.’\n\
         4 Then the Lord said to Moses, ‘I am going to rain bread from heaven for you, and each day the people shall go out and gather enough for that day.’";
    const SELECTED_SAMUEL_DESCRIPTION: &str = "1 Now the Philistines gathered their armies for battle… 2 Saul and the Israelites gathered and encamped in the valley of Elah and formed ranks against the Philistines. 3 The Philistines stood on the mountain on the one side, and Israel stood on the mountain on the other side, with a valley between them. 4 And there came out from the camp of the Philistines a champion named Goliath…\n\
         12 Now David was the son of… Jesse, who had eight sons… 17 Jesse said to his son David, “Take for your brothers… this parched grain and these ten loaves, and carry them quickly to the camp to your brothers; 18 also take these ten cheeses to the commander of their thousand. See how your brothers fare, and bring some token from them.” … 20 David rose early in the morning, left the sheep with a keeper, took the provisions, and went as Jesse had commanded him. He came to the encampment as the army was going forth to the battle line, shouting the war cry…\n\
         32 David said to Saul, “Let no one’s heart fail because of him; your servant will go and fight with this Philistine… 37 David said, “The Lord, who saved me from the paw of the lion and from the paw of the bear, will save me from the hand of this Philistine.” So Saul said to David, “Go, and may the Lord be with you!”\n\
         38 Saul clothed David with his armor…then David said to Saul, “I cannot walk with these, for I am not used to them.” So David removed them. 40 Then he took his staff in his hand and chose five smooth stones from the wadi and put them in his shepherd’s bag, in the pouch; his sling was in his hand, and he drew near to the Philistine… 48 When the Philistine drew nearer to meet David… 49 David put his hand in his bag, took out a stone, slung it, and struck the Philistine on his forehead; the stone sank into his forehead, and he fell face down on the ground. 50 So David prevailed over the Philistine with a sling and a stone… 51 When the Philistines saw that their champion was dead, they fled.";

    #[test]
    fn exodus_partial_description_is_proved_against_local_nrsvue_text() {
        let bible =
            BibleCorpusSnapshot::capture(Path::new(env!("CARGO_MANIFEST_DIR")).join("data/bibles"))
                .expect("capture Bible corpora");

        assert_eq!(
            validate_description_scripture_excerpt(
                &bible,
                "Exodus 16:1-4",
                BibleVersion::NRSVue,
                EXODUS_DESCRIPTION,
            ),
            Ok(())
        );
        assert!(validate_description_scripture_excerpt(
            &bible,
            "Exodus 16:1-4",
            BibleVersion::NRSVue,
            &EXODUS_DESCRIPTION.replace("gather enough", "gather too much"),
        )
        .is_err());
    }

    #[test]
    fn validated_partial_description_crosses_from_review_to_ready() {
        let config = parse_project_config_str(
            r#"{
              "version": 4,
              "defaults": { "bible_version": "NRSVue" },
              "people": { "Robert": {} },
              "cue_roles": { "scripture": { "slide": "Scripture" } },
              "presentation_types": {
                "scripture": {
                  "kind": "scripture",
                  "content_source": "scripture",
                  "output_strategy": "generate_new",
                  "display": { "kind": "single", "role": "scripture" }
                }
              },
              "item_rules": [{
                "id": "scripture",
                "match": { "title_prefix": ["scripture"] },
                "use_type": "scripture"
              }]
            }"#,
        )
        .expect("partial scripture test config");
        let item = Item {
            id: "partial-scripture".to_string(),
            position: 1,
            title: "Scripture - Exodus 16:1-4a (Robert)".to_string(),
            description: Some(EXODUS_DESCRIPTION.to_string()),
            category: Category::Title,
            note: None,
            song: None,
            scripture: None,
        };
        let mut plans = crate::workflow::classify::build_plan(&[item], &config, None, None);
        assert!(plans[0].needs_review());

        let bible =
            BibleCorpusSnapshot::capture(Path::new(env!("CARGO_MANIFEST_DIR")).join("data/bibles"))
                .expect("capture Bible corpora");
        reconcile_description_scripture_excerpts(&mut plans, &bible)
            .expect("compatible scripture proposal");

        assert!(matches!(
            plans[0].ready_action(),
            Some(ReadyAction::GenerateScripture { scripture, .. })
                if matches!(scripture.request(), ScriptureRequest::PrefixExcerpt { .. })
        ));
        assert_eq!(
            plans[0].reason,
            "Generate description-bounded scripture slides (Exodus 16:1-4a NRSVue)"
        );
    }

    #[test]
    fn selected_samuel_description_is_proved_and_crosses_to_ready() {
        let config = parse_project_config_str(
            r#"{
              "version": 4,
              "defaults": { "bible_version": "NRSVue" },
              "people": { "Robert": {} },
              "cue_roles": { "scripture": { "slide": "Scripture" } },
              "presentation_types": {
                "scripture": {
                  "kind": "scripture",
                  "content_source": "scripture",
                  "output_strategy": "generate_new",
                  "display": { "kind": "single", "role": "scripture" }
                }
              },
              "item_rules": [{
                "id": "scripture",
                "match": { "title_prefix": ["scripture"] },
                "use_type": "scripture"
              }]
            }"#,
        )
        .expect("selected scripture test config");
        let item = Item {
            id: "selected-scripture".to_string(),
            position: 1,
            title: "Scripture: 1 Samuel 17 (selected vv)".to_string(),
            description: Some(SELECTED_SAMUEL_DESCRIPTION.to_string()),
            category: Category::Title,
            note: None,
            song: None,
            scripture: None,
        };
        let mut plans = crate::workflow::classify::build_plan(&[item], &config, None, None);
        assert!(plans[0].needs_review());

        let bible =
            BibleCorpusSnapshot::capture(Path::new(env!("CARGO_MANIFEST_DIR")).join("data/bibles"))
                .expect("capture Bible corpora");
        reconcile_description_scripture_excerpts(&mut plans, &bible)
            .expect("compatible selected scripture proposal");

        assert!(matches!(
            plans[0].ready_action(),
            Some(ReadyAction::GenerateScripture { scripture, .. })
                if matches!(
                    scripture.request(),
                    ScriptureRequest::SelectedExcerpt {
                        reference: "1 Samuel 17",
                        bible_version: BibleVersion::NRSVue,
                        excerpt_text: SELECTED_SAMUEL_DESCRIPTION,
                    }
                )
        ));
        assert_eq!(
            plans[0].reason,
            "Generate Planning Center selected-verse scripture slides (1 Samuel 17 NRSVue)"
        );
    }
}
