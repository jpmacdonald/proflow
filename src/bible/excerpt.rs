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

/// Why a numbered selected-verse description cannot be trusted as scripture.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SelectedScriptureExcerptError {
    /// Planning Center did not supply passage text.
    #[error("Planning Center selected-verse text is empty")]
    Empty,
    /// The description does not begin with a numbered verse.
    #[error("Planning Center selected-verse text must begin with a verse number")]
    MissingInitialVerse,
    /// A numbered segment has no display text.
    #[error("Planning Center selected verse {verse} has no text")]
    EmptyVerse {
        /// Explicit verse number.
        verse: u32,
    },
    /// Explicit verse numbers are not strictly increasing.
    #[error("Planning Center selected verse numbers must be strictly increasing")]
    NonIncreasingVerses,
    /// An explicit verse number is absent from the selected chapter.
    #[error("Planning Center selected verse {verse} is absent from the local Bible chapter")]
    MissingVerse {
        /// Explicit verse number.
        verse: u32,
    },
    /// Supplied words cannot be reconciled with the corresponding chapter span.
    #[error(
        "Planning Center selected verse {verse} wording does not follow the local Bible chapter"
    )]
    WordingMismatch {
        /// Explicit verse number.
        verse: u32,
    },
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

/// Validate and preserve an authoritative numbered selected-verse description.
///
/// Explicit verse markers must be strictly increasing and present in the local
/// chapter. Each supplied segment must follow the local wording between its
/// marker and the next explicit marker. This permits intentional omissions and
/// ellipses while rejecting reordered or invented words. Returned text remains
/// exactly as supplied by Planning Center.
pub fn reconcile_selected_excerpt(
    chapter: &[Verse],
    supplied: &str,
) -> Result<Vec<Verse>, SelectedScriptureExcerptError> {
    let supplied = supplied.trim();
    if supplied.is_empty() {
        return Err(SelectedScriptureExcerptError::Empty);
    }
    let markers = selected_verse_markers(supplied);
    if markers.first().is_none_or(|marker| marker.start != 0) {
        return Err(SelectedScriptureExcerptError::MissingInitialVerse);
    }

    let mut selected = Vec::with_capacity(markers.len());
    for (index, marker) in markers.iter().enumerate() {
        if index > 0 && markers[index - 1].number >= marker.number {
            return Err(SelectedScriptureExcerptError::NonIncreasingVerses);
        }
        let text_end = markers
            .get(index + 1)
            .map_or(supplied.len(), |next| next.start);
        let text = supplied[marker.text_start..text_end].trim();
        if text.is_empty() {
            return Err(SelectedScriptureExcerptError::EmptyVerse {
                verse: marker.number,
            });
        }
        let next_number = markers.get(index + 1).map(|next| next.number);
        let local_text = chapter
            .iter()
            .filter(|verse| {
                verse.number >= marker.number && next_number.is_none_or(|next| verse.number < next)
            })
            .map(|verse| verse.text.as_str())
            .collect::<Vec<_>>();
        if !chapter.iter().any(|verse| verse.number == marker.number) {
            return Err(SelectedScriptureExcerptError::MissingVerse {
                verse: marker.number,
            });
        }
        let supplied_tokens = normalized_tokens(text);
        let local_tokens = normalized_tokens(&local_text.join(" "));
        if supplied_tokens.is_empty() || !is_ordered_subsequence(&supplied_tokens, &local_tokens) {
            return Err(SelectedScriptureExcerptError::WordingMismatch {
                verse: marker.number,
            });
        }
        selected.push(Verse {
            number: marker.number,
            text: text.to_string(),
        });
    }
    Ok(selected)
}

#[derive(Debug)]
struct SelectedVerseMarker {
    start: usize,
    text_start: usize,
    number: u32,
}

fn selected_verse_markers(text: &str) -> Vec<SelectedVerseMarker> {
    let bytes = text.as_bytes();
    let mut markers = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index].is_ascii_digit() && (index == 0 || bytes[index - 1].is_ascii_whitespace()) {
            let start = index;
            let mut end = index;
            let mut number = 0_u32;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                number = number
                    .saturating_mul(10)
                    .saturating_add(u32::from(bytes[end] - b'0'));
                end += 1;
            }
            if number > 0 && end < bytes.len() && bytes[end].is_ascii_whitespace() {
                while end < bytes.len() && bytes[end].is_ascii_whitespace() {
                    end += 1;
                }
                markers.push(SelectedVerseMarker {
                    start,
                    text_start: end,
                    number,
                });
                index = end;
                continue;
            }
        }
        index += 1;
    }
    markers
}

fn is_ordered_subsequence(needle: &[String], haystack: &[String]) -> bool {
    let mut next = 0;
    for token in haystack {
        if needle.get(next) == Some(token) {
            next += 1;
            if next == needle.len() {
                return true;
            }
        }
    }
    false
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

    #[test]
    fn selected_excerpt_preserves_supplied_ellipses_and_explicit_markers() {
        let chapter = vec![
            verse(1, "Alpha one omitted middle omega."),
            verse(2, "Second verse is omitted."),
            verse(3, "Gamma three complete."),
        ];

        let selected =
            reconcile_selected_excerpt(&chapter, "1 Alpha one… omega. 3 Gamma three complete.")
                .expect("ordered selected excerpt");

        assert_eq!(
            selected,
            vec![
                verse(1, "Alpha one… omega."),
                verse(3, "Gamma three complete.")
            ]
        );
    }

    #[test]
    fn selected_excerpt_rejects_reordered_or_unknown_words() {
        let chapter = vec![verse(1, "Alpha beta gamma."), verse(2, "Delta epsilon.")];

        assert_eq!(
            reconcile_selected_excerpt(&chapter, "1 Gamma alpha"),
            Err(SelectedScriptureExcerptError::WordingMismatch { verse: 1 })
        );
        assert_eq!(
            reconcile_selected_excerpt(&chapter, "1 Alpha 9 Missing"),
            Err(SelectedScriptureExcerptError::MissingVerse { verse: 9 })
        );
    }
}
