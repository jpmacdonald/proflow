//! Conservative reconciliation of Planning Center scripture excerpts.

use super::Verse;

/// Why supplied scripture text could not prove one exact prefix excerpt.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ScriptureExcerptError {
    /// Planning Center did not supply passage text.
    #[error("Planning Center scripture text is empty")]
    Empty,
    /// The supplied wording does not match the selected local translation.
    #[error("Planning Center scripture text is not one exact prefix of the local Bible passage")]
    Mismatch,
    /// The supplied text ends before any wording from the final requested verse.
    #[error("Planning Center scripture text does not reach the final requested verse")]
    MissingFinalVerse,
    /// A partial reference was paired with the complete final verse.
    #[error(
        "Planning Center scripture text contains the complete final verse, not a prefix excerpt"
    )]
    CompleteFinalVerse,
}

#[derive(Debug)]
struct CanonicalToken {
    normalized: String,
    source: TokenSource,
}

#[derive(Debug, Clone, Copy)]
enum TokenSource {
    Label,
    VerseText { verse_index: usize, end: usize },
}

/// Reconcile caller-supplied text with a whole-verse lookup.
///
/// The comparison stream is exactly `verse-number + verse-text` for every
/// looked-up verse. Punctuation, whitespace, and case are normalized, but no
/// words may be added, omitted, or changed. The Planning Center description
/// must be one strict prefix reaching the final requested verse. Returned
/// wording comes from the local Bible corpus, truncated at the
/// uniquely matched token boundary.
pub fn reconcile_prefix_excerpt(
    verses: &[Verse],
    supplied: &str,
) -> Result<Vec<Verse>, ScriptureExcerptError> {
    let supplied = normalized_tokens(supplied);
    if supplied.is_empty() {
        return Err(ScriptureExcerptError::Empty);
    }
    let canonical = canonical_tokens(verses);
    if canonical.is_empty()
        || supplied.len() > canonical.len()
        || !canonical
            .iter()
            .map(|token| token.normalized.as_str())
            .zip(&supplied)
            .all(|(expected, actual)| expected == actual)
    {
        return Err(ScriptureExcerptError::Mismatch);
    }
    if supplied.len() == canonical.len() {
        return Err(ScriptureExcerptError::CompleteFinalVerse);
    }

    let final_index = verses
        .len()
        .checked_sub(1)
        .ok_or(ScriptureExcerptError::Mismatch)?;
    let cutoff = canonical
        .get(supplied.len() - 1)
        .ok_or(ScriptureExcerptError::Mismatch)?;
    let TokenSource::VerseText { verse_index, end } = cutoff.source else {
        return Err(ScriptureExcerptError::MissingFinalVerse);
    };
    if verse_index != final_index {
        return Err(ScriptureExcerptError::MissingFinalVerse);
    }

    let mut reconciled = verses.to_vec();
    let final_text = &verses[final_index].text;
    reconciled[final_index].text = prefix_through_punctuation(final_text, end);
    Ok(reconciled)
}

fn canonical_tokens(verses: &[Verse]) -> Vec<CanonicalToken> {
    let mut tokens = Vec::new();
    for (verse_index, verse) in verses.iter().enumerate() {
        tokens.push(CanonicalToken {
            normalized: verse.number.to_string(),
            source: TokenSource::Label,
        });
        tokens.extend(
            tokens_with_ends(&verse.text)
                .into_iter()
                .map(|(normalized, end)| CanonicalToken {
                    normalized,
                    source: TokenSource::VerseText { verse_index, end },
                }),
        );
    }
    tokens
}

fn normalized_tokens(text: &str) -> Vec<String> {
    tokens_with_ends(text)
        .into_iter()
        .map(|(token, _)| token)
        .collect()
}

fn tokens_with_ends(text: &str) -> Vec<(String, usize)> {
    let mut tokens = Vec::new();
    let mut start = None;
    for (index, character) in text.char_indices() {
        if character.is_alphanumeric() {
            start.get_or_insert(index);
        } else if let Some(token_start) = start.take() {
            tokens.push((text[token_start..index].to_lowercase(), index));
        }
    }
    if let Some(token_start) = start {
        tokens.push((text[token_start..].to_lowercase(), text.len()));
    }
    tokens
}

fn prefix_through_punctuation(text: &str, token_end: usize) -> String {
    let mut end = token_end;
    for (offset, character) in text[token_end..].char_indices() {
        if character.is_alphanumeric() {
            break;
        }
        end = token_end + offset + character.len_utf8();
    }
    text[..end].trim_end().to_string()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn verse(number: u32, text: &str) -> Verse {
        Verse {
            number,
            text: text.to_string(),
        }
    }

    #[test]
    fn canonical_labels_select_one_verified_prefix_and_keep_local_punctuation() {
        let verses = vec![
            verse(1, "The whole congregation set out."),
            verse(2, "The people complained against Moses."),
            verse(
                3,
                "Then the Lord said, “Gather enough for that day. In this way I will test them.”",
            ),
        ];
        let supplied = "1 The whole congregation set out!\n\
                        2 The people complained against Moses.\n\
                        3 Then the Lord said: gather enough for that day.";

        let reconciled = reconcile_prefix_excerpt(&verses, supplied).expect("unique prefix");

        assert_eq!(reconciled[0].text, verses[0].text);
        assert_eq!(reconciled[1].text, verses[1].text);
        assert_eq!(
            reconciled[2].text,
            "Then the Lord said, “Gather enough for that day."
        );
    }

    #[test]
    fn rejects_changed_wording_or_a_cutoff_before_the_final_verse() {
        let verses = vec![verse(1, "Alpha beta."), verse(2, "Gamma delta epsilon.")];

        assert_eq!(
            reconcile_prefix_excerpt(&verses, "1 Alpha changed 2 Gamma delta"),
            Err(ScriptureExcerptError::Mismatch)
        );
        assert_eq!(
            reconcile_prefix_excerpt(&verses, "1 Alpha beta"),
            Err(ScriptureExcerptError::MissingFinalVerse)
        );
    }

    #[test]
    fn rejects_the_complete_labeled_passage_for_a_partial_reference() {
        let verses = vec![verse(7, "Alpha beta."), verse(8, "Gamma delta.")];

        assert_eq!(
            reconcile_prefix_excerpt(&verses, "7 Alpha beta 8 Gamma delta"),
            Err(ScriptureExcerptError::CompleteFinalVerse)
        );
    }
}
