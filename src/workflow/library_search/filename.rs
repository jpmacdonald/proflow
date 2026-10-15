//! Resolve explicitly configured filenames without fuzzy matching.

use super::LibraryCatalog;

/// Result of resolving a configured `library_file` target.
///
/// Explicit targets use a deliberately narrow filename policy: surrounding
/// whitespace and a trailing `.pro` extension are ignored, comparison is
/// case-insensitive, and every other character must match. A filename that
/// occurs more than once in the library is ambiguous rather than arbitrarily
/// selecting one path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::workflow) enum ExactLibraryFileMatch {
    Unique(String),
    Missing,
    /// Every library file with the requested name.
    Ambiguous(Vec<String>),
}

/// Resolve a configured library filename exactly and uniquely.
pub(in crate::workflow) fn resolve_exact_library_file(
    index: Option<&LibraryCatalog>,
    requested_file: &str,
) -> ExactLibraryFileMatch {
    let Some(index) = index else {
        return ExactLibraryFileMatch::Missing;
    };
    let Some(requested_key) = exact_filename_key(requested_file) else {
        return ExactLibraryFileMatch::Missing;
    };

    let mut paths = index
        .entries()
        .iter()
        .filter(|entry| exact_filename_key(entry.file_name()).as_ref() == Some(&requested_key))
        .map(|entry| entry.full_path().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    match paths.len() {
        0 => ExactLibraryFileMatch::Missing,
        1 => ExactLibraryFileMatch::Unique(paths.remove(0)),
        _ => ExactLibraryFileMatch::Ambiguous(paths),
    }
}

fn exact_filename_key(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.contains(['/', '\\']) {
        return None;
    }
    let normalized = trimmed.to_lowercase();
    let stem = normalized
        .strip_suffix(".pro")
        .unwrap_or(&normalized)
        .trim();
    (!stem.is_empty()).then(|| stem.to_string())
}
