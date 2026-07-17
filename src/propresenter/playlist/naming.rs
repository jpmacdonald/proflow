use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::domain::PlaylistEntry;
use crate::propresenter::generated::rv_data::url;
use crate::propresenter::package::PlaylistItemSummary;
use crate::propresenter::SlideType;

pub(super) fn presentation_filename(path: &str) -> Option<String> {
    let filename = path
        .trim()
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|filename| !filename.is_empty())?;
    let filename = percent_decode_file_component(filename)?;
    Path::new(&filename)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pro"))
        .then_some(filename)
}

/// Return the archive filename linked by a decoded presentation item.
pub fn linked_presentation_filename(item: &PlaylistItemSummary) -> Option<String> {
    item.local_relative_path
        .as_deref()
        .or(item.storage_relative_path.as_deref())
        .or(item.external_relative_path.as_deref())
        .and_then(presentation_filename)
        .or_else(|| {
            item.absolute_string
                .as_deref()
                .and_then(presentation_filename)
        })
}

fn percent_decode_file_component(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hi = bytes.get(index + 1).and_then(|byte| hex_value(*byte))?;
            let lo = bytes.get(index + 2).and_then(|byte| hex_value(*byte))?;
            decoded.push((hi << 4) | lo);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub(super) fn embedded_filenames(
    entries: &[PlaylistEntry],
) -> Result<Vec<Option<String>>, super::domain::PlaylistError> {
    let mut used_names: HashMap<String, (String, String)> = HashMap::new();
    let mut embedded_sources = HashSet::new();

    entries
        .iter()
        .map(|entry| {
            entry
                .embedded_data()
                .map(|_| {
                    if !embedded_sources.insert(entry.presentation_path()) {
                        return Ok(None);
                    }
                    let base = entry.embedded_filename().to_string();
                    let key = base.to_lowercase();
                    if let Some((first_basename, first_path)) = used_names.get(&key) {
                        return Err(
                            super::domain::PlaylistError::DuplicateEmbeddedPresentationBasename {
                                basename: first_basename.clone(),
                                first_presentation_path: first_path.clone(),
                                conflicting_presentation_path: entry
                                    .presentation_path()
                                    .to_string(),
                            },
                        );
                    }
                    used_names.insert(key, (base.clone(), entry.presentation_path().to_string()));
                    Ok(Some(base))
                })
                .transpose()
                .map(Option::flatten)
        })
        .collect()
}

fn path_to_file_url(path: &str) -> String {
    if path.starts_with("file://") {
        return path.to_string();
    }
    format!("file://{}", percent_encode_file_path(path))
}

fn percent_encode_file_path(path: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(path.len());
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'/'
            | b'-'
            | b'_'
            | b'.'
            | b'~'
            | b','
            | b'('
            | b')'
            | b'\'' => encoded.push(char::from(byte)),
            b' ' => encoded.push_str("%20"),
            _ => {
                encoded.push('%');
                encoded.push(char::from(HEX[usize::from(byte >> 4)]));
                encoded.push(char::from(HEX[usize::from(byte & 0x0F)]));
            }
        }
    }
    encoded
}

fn extract_relative_path(path: &str) -> Option<url::RelativeFilePath> {
    let rel_path = if let Some(index) = path.find("Libraries/") {
        path[index..].to_string()
    } else {
        Path::new(path)
            .file_name()
            .and_then(|name| name.to_str())
            .map(String::from)?
    };

    Some(url::RelativeFilePath::Local(url::LocalRelativePath {
        root: url::local_relative_path::Root::Show as i32,
        path: rel_path,
    }))
}

pub(super) fn document_path_for_presentation_path(
    path: &str,
) -> (String, Option<url::RelativeFilePath>) {
    (path_to_file_url(path), extract_relative_path(path))
}

/// Sanitize a name for use as a filename, applying type-specific rules.
pub fn sanitize_filename(name: &str, slide_type: SlideType) -> String {
    match slide_type {
        SlideType::Lyrics => sanitize_song(name),
        SlideType::Scripture => sanitize_scripture(name),
        _ => sanitize_general(name),
    }
}

fn sanitize_song(name: &str) -> String {
    strip_unsafe_chars(name)
}

fn sanitize_scripture(name: &str) -> String {
    let mut value = strip_parens(name);
    for prefix in &["Scripture Reading", "Scripture", "Reading"] {
        if let Some(rest) = value.strip_prefix(prefix) {
            value = rest
                .strip_prefix(" - ")
                .or_else(|| rest.strip_prefix(": "))
                .or_else(|| rest.strip_prefix(" -"))
                .or_else(|| rest.strip_prefix(':'))
                .unwrap_or(rest)
                .trim()
                .to_string();
            break;
        }
    }

    let chars: Vec<char> = value.chars().collect();
    let mut result = String::with_capacity(value.len());
    for (index, &character) in chars.iter().enumerate() {
        if character == ':'
            && index > 0
            && chars[index - 1].is_ascii_digit()
            && index + 1 < chars.len()
            && chars[index + 1].is_ascii_digit()
        {
            result.push('v');
        } else {
            result.push(character);
        }
    }
    strip_unsafe_chars(result.trim())
}

fn sanitize_general(name: &str) -> String {
    let value = strip_parens(name);
    let chars: Vec<char> = value.chars().collect();
    let mut result = String::with_capacity(value.len());
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == ':' {
            if result.ends_with(' ') {
                result.pop();
            }
            result.push_str(" - ");
            if index + 1 < chars.len() && chars[index + 1] == ' ' {
                index += 1;
            }
        } else {
            result.push(chars[index]);
        }
        index += 1;
    }
    strip_unsafe_chars(result.trim())
}

fn strip_parens(name: &str) -> String {
    let mut result = String::with_capacity(name.len());
    let mut depth = 0u32;
    for character in name.chars() {
        match character {
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            ')' => {}
            _ if depth == 0 => result.push(character),
            _ => {}
        }
    }
    result.trim().to_string()
}

fn strip_unsafe_chars(name: &str) -> String {
    name.chars()
        .filter(|character| {
            !matches!(
                character,
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
            )
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Failure to derive a safe, nonempty presentation filename stem.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CanonicalPresentationNameError {
    /// Removing type prefixes, speaker annotations, and unsafe characters left no name.
    #[error("presentation name has no safe filename characters after normalization")]
    Empty,
}

/// Sanitize a name into a checked canonical presentation filename stem.
pub fn canonical_presentation_name(
    name: &str,
    slide_type: SlideType,
) -> Result<String, CanonicalPresentationNameError> {
    let normalized = sanitize_filename(name, slide_type);
    if normalized.is_empty() {
        Err(CanonicalPresentationNameError::Empty)
    } else {
        Ok(normalized)
    }
}

/// Compute the canonical output path for a `.proplaylist` file.
pub fn playlist_output_path(output_directory: &Path, name: &str) -> PathBuf {
    let safe_name: String = name
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || matches!(character, ' ' | '-' | ',' | '(' | ')') {
                character
            } else {
                '_'
            }
        })
        .collect();
    output_directory.join(format!("{safe_name}.proplaylist"))
}

#[cfg(test)]
pub(super) fn file_url_for_test(path: &str) -> String {
    path_to_file_url(path)
}
