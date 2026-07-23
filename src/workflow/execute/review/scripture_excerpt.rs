//! Reconciliation of description-bounded scripture proposals.

use crate::bible::BibleCorpusSnapshot;
use crate::workflow::execute::BuildServiceError;
use crate::workflow::plan::{ReadyAction, ResolvedItemPlan, ScriptureRequest};

pub(super) fn reconcile_description_scripture_excerpts(
    plans: &mut [ResolvedItemPlan],
    bible: &BibleCorpusSnapshot,
) -> Result<(), BuildServiceError> {
    for plan in plans {
        if !plan.needs_review() {
            continue;
        }
        let Some((reference, display_reference, bible_version, excerpt_text)) =
            plan.preview_action().and_then(|action| match action {
                ReadyAction::GenerateScripture { scripture, .. } => match scripture.request() {
                    ScriptureRequest::PrefixExcerpt {
                        reference,
                        display_reference,
                        bible_version,
                        excerpt_text,
                    } => Some((
                        reference.to_string(),
                        display_reference.to_string(),
                        bible_version.to_string(),
                        excerpt_text.to_string(),
                    )),
                    ScriptureRequest::Single { .. } | ScriptureRequest::Combined(_) => None,
                },
                _ => None,
            })
        else {
            continue;
        };
        if let Err(error) =
            validate_description_scripture_excerpt(bible, &reference, &bible_version, &excerpt_text)
        {
            plan.reason =
                format!("Partial scripture '{display_reference}' requires review: {error}");
            continue;
        }

        if !plan.approve_proposed_action()? {
            continue;
        }
        plan.reason = format!(
            "Generate description-bounded scripture slides ({display_reference} {bible_version})"
        );
    }
    Ok(())
}

fn validate_description_scripture_excerpt(
    bible: &BibleCorpusSnapshot,
    reference_text: &str,
    bible_version: &str,
    excerpt_text: &str,
) -> Result<(), String> {
    let reference = crate::bible::parse_scripture_ref(reference_text)
        .ok_or_else(|| format!("cannot parse whole-verse lookup '{reference_text}'"))?;
    let version = crate::bible::BibleVersion::from_name(bible_version)
        .ok_or_else(|| format!("unsupported Bible version '{bible_version}'"))?;
    let (header, verses) = bible
        .lookup_verses(&reference, version)
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

    #[test]
    fn exodus_partial_description_is_proved_against_local_nrsvue_text() {
        let bible =
            BibleCorpusSnapshot::capture(Path::new(env!("CARGO_MANIFEST_DIR")).join("data/bibles"))
                .expect("capture Bible corpora");

        assert_eq!(
            validate_description_scripture_excerpt(
                &bible,
                "Exodus 16:1-4",
                "NRSVue",
                EXODUS_DESCRIPTION,
            ),
            Ok(())
        );
        assert!(validate_description_scripture_excerpt(
            &bible,
            "Exodus 16:1-4",
            "NRSVue",
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
}
