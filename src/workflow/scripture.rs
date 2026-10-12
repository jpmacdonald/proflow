//! Scripture reference parsing and formatting utilities.

use std::collections::HashSet;

use crate::bible::{BibleTranslations, BibleVersion};

/// One fully parsed scripture reference and its resolved Bible version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ParsedScriptureRef {
    pub reference: String,
    pub version: BibleVersion,
}

/// One terminal `a` reference represented as a whole lookup plus display form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ParsedPrefixScriptureRef {
    pub reference: String,
    pub display_reference: String,
    pub version: BibleVersion,
}

/// A malformed multi-reference title must never be partially generated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ScriptureRefsError {
    Missing,
    MissingVersion,
    UnsupportedVersion(String),
    ConflictingVersions {
        first: BibleVersion,
        second: BibleVersion,
    },
    Invalid(String),
    PartialVerse(String),
    MixedVersionsWithImplicit,
}

impl std::fmt::Display for ScriptureRefsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => formatter.write_str("No scripture reference"),
            Self::MissingVersion => formatter
                .write_str("No Bible version was supplied and no project default is configured"),
            Self::UnsupportedVersion(version) => {
                write!(formatter, "Unsupported Bible version '{version}'")
            }
            Self::ConflictingVersions { first, second } => write!(
                formatter,
                "Conflicting explicit Bible versions '{}' and '{}'",
                first.name(), second.name()
            ),
            Self::Invalid(reference) => {
                write!(formatter, "Invalid scripture reference '{reference}'")
            }
            Self::PartialVerse(reference) => write!(
                formatter,
                "Partial-verse reference '{reference}' cannot be generated from whole-verse Bible data"
            ),
            Self::MixedVersionsWithImplicit => formatter
                .write_str("Mixed Bible versions require an explicit version on every reference"),
        }
    }
}

/// Parse every semicolon-separated scripture reference in a title.
///
/// A single explicit version acts as the default for all references. Multiple
/// explicit versions are preserved, but then every reference must name its
/// version. Any invalid segment rejects the whole title instead of silently
/// generating a partial presentation.
pub(super) fn parse_scripture_refs(
    title: &str,
    configured_default: Option<BibleVersion>,
    known_speaker: Option<&str>,
    translations: &BibleTranslations,
) -> Result<Vec<ParsedScriptureRef>, ScriptureRefsError> {
    let stripped = strip_scripture_heading(title, known_speaker);
    let mut parts: Vec<String> = stripped
        .split(';')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    combine_implicit_same_chapter_range(&mut parts, known_speaker, translations)?;
    inherit_book_from_previous_reference(&mut parts, known_speaker, translations)?;
    if parts.is_empty() {
        return Err(ScriptureRefsError::Missing);
    }

    let declarations = parts
        .iter()
        .map(|part| reference_and_version(part, known_speaker, translations))
        .collect::<Result<Vec<_>, _>>()?;
    let distinct_versions: HashSet<BibleVersion> = declarations
        .iter()
        .filter_map(|(_, version)| *version)
        .collect();
    let default_version = match distinct_versions.len() {
        0 => configured_default,
        1 => distinct_versions.iter().copied().next(),
        _ => None,
    };

    parts
        .iter()
        .zip(declarations)
        .map(|(part, (reference_text, explicit))| {
            let version = explicit.or(default_version).ok_or({
                if distinct_versions.is_empty() {
                    ScriptureRefsError::MissingVersion
                } else {
                    ScriptureRefsError::MixedVersionsWithImplicit
                }
            })?;
            if has_partial_verse_marker(reference_text) {
                return Err(ScriptureRefsError::PartialVerse(reference_text.to_string()));
            }
            let parsed = crate::bible::parse_scripture_ref(reference_text)
                .ok_or_else(|| ScriptureRefsError::Invalid(part.clone()))?;
            Ok(ParsedScriptureRef {
                reference: parsed.to_string(),
                version,
            })
        })
        .collect()
}

fn combine_implicit_same_chapter_range(
    parts: &mut Vec<String>,
    known_speaker: Option<&str>,
    translations: &BibleTranslations,
) -> Result<(), ScriptureRefsError> {
    if parts.len() != 2 {
        return Ok(());
    }
    let (selection, selection_version) =
        reference_and_version(&parts[1], known_speaker, translations)?;
    if !is_verse_selection(selection) {
        return Ok(());
    }
    let (reference, reference_version) =
        reference_and_version(&parts[0], known_speaker, translations)?;
    if crate::bible::parse_scripture_ref(reference).is_none() {
        return Ok(());
    }
    if let (Some(first), Some(second)) = (reference_version, selection_version) {
        if first != second {
            return Err(ScriptureRefsError::ConflictingVersions { first, second });
        }
    }
    let combined = format!("{reference}, {selection}");
    let combined = match reference_version.or(selection_version) {
        Some(version) => format!("{combined} {}", version.name()),
        None => combined,
    };
    parts.truncate(1);
    parts[0] = combined;
    Ok(())
}

/// Give a `chapter:verses` reference the book of the reference before it:
/// in "2 Corinthians 8:1-9; 9:6-11" the second passage is 2 Corinthians 9.
fn inherit_book_from_previous_reference(
    parts: &mut [String],
    known_speaker: Option<&str>,
    translations: &BibleTranslations,
) -> Result<(), ScriptureRefsError> {
    for index in 1..parts.len() {
        let (reference, _) = reference_and_version(&parts[index], known_speaker, translations)?;
        if !is_chapter_and_verses(reference) {
            continue;
        }
        let (previous, _) = reference_and_version(&parts[index - 1], known_speaker, translations)?;
        let Some(book) = crate::bible::parse_scripture_ref(previous).map(|parsed| parsed.book)
        else {
            continue;
        };
        parts[index] = format!("{book} {}", parts[index]);
    }
    Ok(())
}

/// Whether `text` is a chapter and a verse selection with no book: "9:6-11".
fn is_chapter_and_verses(text: &str) -> bool {
    text.split_once(':').is_some_and(|(chapter, verses)| {
        !chapter.is_empty()
            && chapter.chars().all(|character| character.is_ascii_digit())
            && is_verse_selection(verses.trim())
    })
}

fn is_verse_selection(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|character| character.is_ascii_digit() || matches!(character, '-' | ',' | ' '))
}

/// Parse one terminal `a` reference without guessing where its text ends.
///
/// The returned whole-verse reference is suitable only for lookup. Planning
/// Center description text must still prove the exact cutoff before execution.
pub(super) fn parse_prefix_scripture_ref(
    title: &str,
    configured_default: Option<BibleVersion>,
    known_speaker: Option<&str>,
    translations: &BibleTranslations,
) -> Result<ParsedPrefixScriptureRef, ScriptureRefsError> {
    let stripped = strip_scripture_heading(title, known_speaker);
    let parts = stripped
        .split(';')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let [part] = parts.as_slice() else {
        return Err(ScriptureRefsError::PartialVerse(stripped.to_string()));
    };
    let (partial_reference, explicit) = reference_and_version(part, known_speaker, translations)?;
    let version = explicit
        .or(configured_default)
        .ok_or(ScriptureRefsError::MissingVersion)?;
    let Some(suffix) = partial_reference.chars().next_back() else {
        return Err(ScriptureRefsError::Missing);
    };
    if !suffix.eq_ignore_ascii_case(&'a') {
        return Err(ScriptureRefsError::PartialVerse(
            partial_reference.to_string(),
        ));
    }
    let whole_reference =
        partial_reference[..partial_reference.len() - suffix.len_utf8()].trim_end();
    if !whole_reference
        .chars()
        .next_back()
        .is_some_and(|character| character.is_ascii_digit())
    {
        return Err(ScriptureRefsError::PartialVerse(
            partial_reference.to_string(),
        ));
    }
    let parsed = crate::bible::parse_scripture_ref(whole_reference)
        .ok_or_else(|| ScriptureRefsError::Invalid(partial_reference.to_string()))?;
    let reference = parsed.to_string();
    Ok(ParsedPrefixScriptureRef {
        display_reference: format!("{reference}a"),
        reference,
        version,
    })
}

fn has_partial_verse_marker(reference: &str) -> bool {
    let bytes = reference.as_bytes();
    bytes.iter().enumerate().any(|(index, byte)| {
        if !byte.is_ascii_digit() {
            return false;
        }
        let Some(suffix) = bytes.get(index + 1) else {
            return false;
        };
        if !matches!(suffix.to_ascii_lowercase(), b'a' | b'b' | b'c' | b'd') {
            return false;
        }
        bytes.get(index + 2).is_none_or(|following| {
            following.is_ascii_whitespace() || matches!(following, b'-' | b',' | b';' | b')')
        })
    })
}

pub(super) fn has_scripture_ref(title: &str) -> bool {
    crate::bible::parse_scripture_ref(title).is_some()
}

/// Separate explicit declarations from the reference before the permissive
/// Bible parser sees it. Only known speaker names and selected-verse directives
/// are annotations; unknown declarations cannot activate a configured default.
fn reference_and_version<'a>(
    text: &'a str,
    known_speaker: Option<&str>,
    translations: &BibleTranslations,
) -> Result<(&'a str, Option<BibleVersion>), ScriptureRefsError> {
    let mut reference = text.trim();
    let mut version = None;
    loop {
        let (before, declaration) = if let Some(parenthetical) = reference.strip_suffix(')') {
            let Some((before, annotation)) = parenthetical.rsplit_once('(') else {
                return Err(ScriptureRefsError::Invalid(text.to_string()));
            };
            let annotation = annotation.trim();
            if translations.named(annotation).is_none()
                && (known_speaker.is_some_and(|speaker| annotation.eq_ignore_ascii_case(speaker))
                    || annotation.eq_ignore_ascii_case("selected vv")
                    || annotation.eq_ignore_ascii_case("selected verses"))
            {
                reference = before.trim_end();
                continue;
            }
            (before, annotation)
        } else if let Some((before, word)) = reference.rsplit_once(char::is_whitespace) {
            if !word.starts_with(char::is_alphabetic)
                || (translations.named(word).is_none()
                    && !before.chars().any(|character| character.is_ascii_digit()))
            {
                break;
            }
            (before, word)
        } else {
            break;
        };
        let explicit = translations
            .named(declaration)
            .ok_or_else(|| ScriptureRefsError::UnsupportedVersion(declaration.to_string()))?;
        if let Some(previous) = version {
            if previous != explicit {
                return Err(ScriptureRefsError::ConflictingVersions {
                    first: explicit,
                    second: previous,
                });
            }
        }
        version = Some(explicit);
        reference = before.trim_end();
    }
    if reference.contains(['(', ')'])
        || (reference
            .chars()
            .any(|character| character.is_ascii_digit())
            && !reference
                .chars()
                .next_back()
                .is_some_and(|character| character.is_ascii_digit())
            && !has_partial_verse_marker(reference))
    {
        return Err(ScriptureRefsError::Invalid(text.to_string()));
    }
    Ok((reference, version))
}

/// Validate title declarations against an authoritative structured translation.
/// Omitted title translations do not conflict; explicit unsupported values do.
pub(super) fn validate_translation_declarations(
    title: &str,
    version: BibleVersion,
    known_speaker: Option<&str>,
    translations: &BibleTranslations,
) -> Result<(), ScriptureRefsError> {
    for part in strip_scripture_heading(title, known_speaker).split(';') {
        if let Some(explicit) = reference_and_version(part, known_speaker, translations)?.1 {
            if explicit != version {
                return Err(ScriptureRefsError::ConflictingVersions {
                    first: version,
                    second: explicit,
                });
            }
        }
    }
    Ok(())
}

fn strip_scripture_heading<'a>(title: &'a str, known_speaker: Option<&str>) -> &'a str {
    let trimmed = title.trim();
    for prefix in ["Scripture Reading", "Scripture", "Reading"] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return strip_heading_suffix(rest, known_speaker);
        }
    }
    trimmed
}

fn strip_heading_suffix<'a>(rest: &'a str, known_speaker: Option<&str>) -> &'a str {
    let mut value = rest.trim_start();
    if let Some(stripped) = value.strip_prefix('(') {
        if let Some(end) = stripped.find(')') {
            if known_speaker
                .is_some_and(|speaker| stripped[..end].trim().eq_ignore_ascii_case(speaker))
            {
                value = stripped[end + 1..].trim_start();
            }
        }
    }
    value
        .strip_prefix(':')
        .or_else(|| value.strip_prefix('-'))
        .map_or(value, str::trim_start)
        .trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Vec<BibleVersion> {
        BibleTranslations::standard().iter().collect()
    }

    #[test]
    fn a_chapter_and_verses_after_a_reference_is_in_the_same_book() {
        let parsed = |title: &str| {
            parse_scripture_refs(
                title,
                Some(BibleVersion::NRSVue),
                Some("Robert"),
                &BibleTranslations::standard(),
            )
        };
        let reference = |reference: &str, version| ParsedScriptureRef {
            reference: reference.to_string(),
            version,
        };

        assert_eq!(
            parsed("Scripture: 2 Corinthians 8:1-9; 9:6-11 (Robert)"),
            Ok(vec![
                reference("2 Corinthians 8:1-9", BibleVersion::NRSVue),
                reference("2 Corinthians 9:6-11", BibleVersion::NRSVue),
            ])
        );
        assert_eq!(
            parsed("Scripture - John 1:1-5; 3:16; Romans 8:1 NIV"),
            Ok(vec![
                reference("John 1:1-5", BibleVersion::NIV),
                reference("John 3:16", BibleVersion::NIV),
                reference("Romans 8:1", BibleVersion::NIV),
            ])
        );
        // With no reference before it there is no book to take.
        assert!(parsed("Scripture - 9:6-11").is_err());
    }

    #[test]
    fn scripture_refs_handle_speaker_prefix() {
        assert_eq!(
            parse_scripture_refs(
                "Scripture (Adrian) - Luke 8:26-39 NRSVue",
                None,
                Some("Adrian"),
                &BibleTranslations::standard()
            ),
            Ok(vec![ParsedScriptureRef {
                reference: "Luke 8:26-39".to_string(),
                version: BibleVersion::NRSVue,
            }])
        );
    }

    #[test]
    fn rejects_partial_multi_reference_title() {
        assert_eq!(
            parse_scripture_refs(
                "Scripture - Luke 8:26-39; not a reference NRSVue",
                None,
                None,
                &BibleTranslations::standard()
            ),
            Err(ScriptureRefsError::Invalid(
                "not a reference NRSVue".to_string()
            ))
        );
    }

    #[test]
    fn semicolon_shorthand_keeps_ranges_in_one_chapter() {
        assert_eq!(
            parse_scripture_refs(
                "Scripture: Acts 28:1-2; 11-15 (Robert)",
                Some(crate::bible::BibleVersion::NRSVue),
                Some("Robert"),
                &BibleTranslations::standard()
            ),
            Ok(vec![ParsedScriptureRef {
                reference: "Acts 28:1-2, 11-15".to_string(),
                version: BibleVersion::NRSVue,
            }])
        );
    }

    #[test]
    fn partial_verse_requires_review_instead_of_expanding_to_the_whole_verse() {
        assert_eq!(
            parse_scripture_refs(
                "Scripture (Robert) - Exodus 16:1-4a",
                Some(crate::bible::BibleVersion::NRSVue),
                Some("Robert"),
                &BibleTranslations::standard()
            ),
            Err(ScriptureRefsError::PartialVerse(
                "Exodus 16:1-4a".to_string()
            ))
        );
    }

    #[test]
    fn terminal_a_is_parsed_as_a_distinct_whole_lookup_and_display_reference() {
        assert_eq!(
            parse_prefix_scripture_ref(
                "Scripture (Robert) - Exodus 16:1-4a",
                Some(crate::bible::BibleVersion::NRSVue),
                Some("Robert"),
                &BibleTranslations::standard(),
            ),
            Ok(ParsedPrefixScriptureRef {
                reference: "Exodus 16:1-4".to_string(),
                display_reference: "Exodus 16:1-4a".to_string(),
                version: BibleVersion::NRSVue,
            })
        );
    }

    #[test]
    fn non_prefix_partial_suffix_is_not_reinterpreted() {
        assert_eq!(
            parse_prefix_scripture_ref(
                "Scripture - Exodus 16:1-4b",
                Some(crate::bible::BibleVersion::NRSVue),
                Some("Robert"),
                &BibleTranslations::standard(),
            ),
            Err(ScriptureRefsError::PartialVerse(
                "Exodus 16:1-4b".to_string()
            ))
        );
    }

    #[test]
    fn preserves_explicit_mixed_versions() {
        assert_eq!(
            parse_scripture_refs(
                "Scripture - Psalm 23:1-6 NIV; John 3:16 NRSVue",
                None,
                None,
                &BibleTranslations::standard()
            ),
            Ok(vec![
                ParsedScriptureRef {
                    reference: "Psalm 23:1-6".to_string(),
                    version: BibleVersion::NIV,
                },
                ParsedScriptureRef {
                    reference: "John 3:16".to_string(),
                    version: BibleVersion::NRSVue,
                },
            ])
        );
    }

    #[test]
    fn rejects_implicit_reference_among_mixed_versions() {
        assert_eq!(
            parse_scripture_refs(
                "Scripture - Psalm 23:1 NIV; John 3:16; Luke 2:1 NRSVue",
                None,
                None,
                &BibleTranslations::standard()
            ),
            Err(ScriptureRefsError::MixedVersionsWithImplicit)
        );
    }

    #[test]
    fn implicit_version_requires_an_explicit_project_default() {
        assert_eq!(
            parse_scripture_refs(
                "Scripture - John 3:16",
                None,
                None,
                &BibleTranslations::standard()
            ),
            Err(ScriptureRefsError::MissingVersion)
        );
        assert_eq!(
            parse_scripture_refs(
                "Scripture - John 3:16",
                Some(crate::bible::BibleVersion::NIV),
                None,
                &BibleTranslations::standard()
            ),
            Ok(vec![ParsedScriptureRef {
                reference: "John 3:16".to_string(),
                version: BibleVersion::NIV,
            }])
        );
    }

    proptest::proptest! {
        #[test]
        fn property_explicit_translation_is_independent_of_project_default(
            explicit_index in 0..all().len(),
            default_index in 0..=all().len(),
            parenthesized in proptest::bool::ANY,
        ) {
            let explicit = all()[explicit_index];
            let default = all().get(default_index).copied();
            let declaration = if parenthesized { format!("({})", explicit.name()) } else { explicit.name().to_string() };
            let title = format!("Scripture - John 3:16 {declaration} (Robert)");
            let actual = parse_scripture_refs(&title, default, Some("Robert"), &BibleTranslations::standard());
            proptest::prop_assert_eq!(actual, Ok(vec![ParsedScriptureRef {
                reference: "John 3:16".to_string(),
                version: explicit,
            }]));
        }

        #[test]
        fn property_unsupported_translation_never_activates_a_default(
            declaration in "[A-Z]{3,10}",
            default_index in 0..=all().len(),
            parenthesized in proptest::bool::ANY,
        ) {
            proptest::prop_assume!(BibleTranslations::standard().named(&declaration).is_none());
            let default = all().get(default_index).copied();
            let suffix = if parenthesized { format!("({declaration})") } else { declaration.clone() };
            let title = format!("Scripture - John 3:16 {suffix} (Robert)");
            proptest::prop_assert_eq!(
                parse_scripture_refs(&title, default, Some("Robert"), &BibleTranslations::standard()),
                Err(ScriptureRefsError::UnsupportedVersion(declaration))
            );
        }
    }
}
