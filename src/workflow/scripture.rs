//! Scripture reference parsing and formatting utilities.

use std::collections::HashSet;

use crate::bible::BibleVersion;

/// One fully parsed scripture reference and its resolved Bible version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ParsedScriptureRef {
    pub reference: String,
    pub version: String,
}

/// One terminal `a` reference represented as a whole lookup plus display form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ParsedPrefixScriptureRef {
    pub reference: String,
    pub display_reference: String,
    pub version: String,
}

/// A malformed multi-reference title must never be partially generated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ScriptureRefsError {
    Missing,
    MissingVersion,
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
) -> Result<Vec<ParsedScriptureRef>, ScriptureRefsError> {
    let stripped = strip_trailing_speaker(strip_scripture_heading(title));
    let parts: Vec<&str> = stripped
        .split(';')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    if parts.is_empty() {
        return Err(ScriptureRefsError::Missing);
    }

    let explicit_versions: Vec<Option<&'static str>> =
        parts.iter().map(|part| explicit_version(part)).collect();
    let distinct_versions: HashSet<&str> = explicit_versions.iter().flatten().copied().collect();
    let default_version = match distinct_versions.len() {
        0 => configured_default.map(BibleVersion::name),
        1 => distinct_versions.iter().copied().next(),
        _ => None,
    };

    parts
        .iter()
        .zip(explicit_versions)
        .map(|(part, explicit)| {
            let version = explicit.or(default_version).ok_or({
                if distinct_versions.is_empty() {
                    ScriptureRefsError::MissingVersion
                } else {
                    ScriptureRefsError::MixedVersionsWithImplicit
                }
            })?;
            let reference_text = strip_explicit_version(part, explicit);
            if has_partial_verse_marker(reference_text) {
                return Err(ScriptureRefsError::PartialVerse(reference_text.to_string()));
            }
            let parsed = crate::bible::parse_scripture_ref(reference_text)
                .ok_or_else(|| ScriptureRefsError::Invalid((*part).to_string()))?;
            let reference = parsed.to_string();
            Ok(ParsedScriptureRef {
                reference,
                version: version.to_string(),
            })
        })
        .collect()
}

/// Parse one terminal `a` reference without guessing where its text ends.
///
/// The returned whole-verse reference is suitable only for lookup. Planning
/// Center description text must still prove the exact cutoff before execution.
pub(super) fn parse_prefix_scripture_ref(
    title: &str,
    configured_default: Option<BibleVersion>,
) -> Result<ParsedPrefixScriptureRef, ScriptureRefsError> {
    let stripped = strip_trailing_speaker(strip_scripture_heading(title));
    let parts = stripped
        .split(';')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let [part] = parts.as_slice() else {
        return Err(ScriptureRefsError::PartialVerse(stripped.to_string()));
    };
    let explicit = explicit_version(part);
    let version = explicit
        .or_else(|| configured_default.map(BibleVersion::name))
        .ok_or(ScriptureRefsError::MissingVersion)?;
    let partial_reference = strip_explicit_version(part, explicit).trim();
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
        version: version.to_string(),
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

fn explicit_version(text: &str) -> Option<&'static str> {
    let upper = text.trim().to_uppercase();
    for (needle, version) in [
        ("NRSVUE", "NRSVue"),
        ("NRSV", "NRSV"),
        ("NKJV", "NKJV"),
        ("NASB", "NASB"),
        ("NLT", "NLT"),
        ("NIV", "NIV"),
        ("KJV", "KJV"),
    ] {
        let bare_suffix = upper
            .strip_suffix(needle)
            .is_some_and(|before| before.chars().next_back().is_some_and(char::is_whitespace));
        let parenthesized_suffix = upper.ends_with(&format!("({needle})"));
        if bare_suffix || parenthesized_suffix {
            return Some(version);
        }
    }
    None
}

fn strip_explicit_version<'a>(text: &'a str, version: Option<&str>) -> &'a str {
    let Some(version) = version else {
        return text;
    };
    let uppercase = text.to_uppercase();
    let version = version.to_uppercase();
    let Some(version_start) = uppercase.rfind(&version) else {
        return text;
    };
    text[..version_start]
        .trim_end()
        .trim_end_matches('(')
        .trim_end()
}

fn strip_scripture_heading(title: &str) -> &str {
    let trimmed = title.trim();
    for prefix in ["Scripture Reading", "Scripture", "Reading"] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return strip_heading_suffix(rest);
        }
    }
    trimmed
}

fn strip_heading_suffix(rest: &str) -> &str {
    let mut value = rest.trim_start();
    if let Some(stripped) = value.strip_prefix('(') {
        if let Some(end) = stripped.find(')') {
            value = stripped[end + 1..].trim_start();
        }
    }
    value
        .strip_prefix(':')
        .or_else(|| value.strip_prefix('-'))
        .map_or(value, str::trim_start)
        .trim()
}

fn strip_trailing_speaker(title: &str) -> &str {
    let trimmed = title.trim();
    let Some(open) = trimmed.rfind('(') else {
        return trimmed;
    };
    if !trimmed.ends_with(')') || explicit_version(&trimmed[open..]).is_some() {
        return trimmed;
    }
    trimmed[..open].trim_end()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripture_refs_handle_speaker_prefix() {
        assert_eq!(
            parse_scripture_refs("Scripture (Adrian) - Luke 8:26-39 NRSVue", None),
            Ok(vec![ParsedScriptureRef {
                reference: "Luke 8:26-39".to_string(),
                version: "NRSVue".to_string(),
            }])
        );
    }

    #[test]
    fn rejects_partial_multi_reference_title() {
        assert_eq!(
            parse_scripture_refs("Scripture - Luke 8:26-39; not a reference NRSVue", None),
            Err(ScriptureRefsError::Invalid(
                "not a reference NRSVue".to_string()
            ))
        );
    }

    #[test]
    fn partial_verse_requires_review_instead_of_expanding_to_the_whole_verse() {
        assert_eq!(
            parse_scripture_refs(
                "Scripture (Robert) - Exodus 16:1-4a",
                Some(crate::bible::BibleVersion::NRSVue),
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
            ),
            Ok(ParsedPrefixScriptureRef {
                reference: "Exodus 16:1-4".to_string(),
                display_reference: "Exodus 16:1-4a".to_string(),
                version: "NRSVue".to_string(),
            })
        );
    }

    #[test]
    fn non_prefix_partial_suffix_is_not_reinterpreted() {
        assert_eq!(
            parse_prefix_scripture_ref(
                "Scripture - Exodus 16:1-4b",
                Some(crate::bible::BibleVersion::NRSVue),
            ),
            Err(ScriptureRefsError::PartialVerse(
                "Exodus 16:1-4b".to_string()
            ))
        );
    }

    #[test]
    fn preserves_explicit_mixed_versions() {
        assert_eq!(
            parse_scripture_refs("Scripture - Psalm 23:1-6 NIV; John 3:16 NRSVue", None),
            Ok(vec![
                ParsedScriptureRef {
                    reference: "Psalms 23:1-6".to_string(),
                    version: "NIV".to_string(),
                },
                ParsedScriptureRef {
                    reference: "John 3:16".to_string(),
                    version: "NRSVue".to_string(),
                },
            ])
        );
    }

    #[test]
    fn rejects_implicit_reference_among_mixed_versions() {
        assert_eq!(
            parse_scripture_refs(
                "Scripture - Psalm 23:1 NIV; John 3:16; Luke 2:1 NRSVue",
                None
            ),
            Err(ScriptureRefsError::MixedVersionsWithImplicit)
        );
    }

    #[test]
    fn implicit_version_requires_an_explicit_project_default() {
        assert_eq!(
            parse_scripture_refs("Scripture - John 3:16", None),
            Err(ScriptureRefsError::MissingVersion)
        );
        assert_eq!(
            parse_scripture_refs(
                "Scripture - John 3:16",
                Some(crate::bible::BibleVersion::NIV)
            ),
            Ok(vec![ParsedScriptureRef {
                reference: "John 3:16".to_string(),
                version: "NIV".to_string(),
            }])
        );
    }
}
