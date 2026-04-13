//! Scripture reference parsing and formatting utilities.

/// Split a title with multiple scripture references (separated by `;`) into
/// individual reference strings. Preserves version and speaker info on each.
pub(super) fn split_scripture_refs(title: &str) -> Vec<String> {
    let stripped = title
        .trim_start_matches("Scripture Reading:")
        .trim_start_matches("Scripture Reading -")
        .trim_start_matches("Scripture:")
        .trim_start_matches("Scripture -")
        .trim();

    // Remove speaker parenthetical for splitting, re-detect version
    let no_speaker = stripped
        .rfind('(')
        .map_or(stripped, |i| stripped[..i].trim());

    let version_suffix = detect_version(title);

    let parts: Vec<&str> = no_speaker.split(';').collect();
    if parts.len() <= 1 {
        return vec![no_speaker.to_string()];
    }

    parts
        .iter()
        .map(|part| {
            let trimmed = part.trim();
            let clean = trimmed
                .trim_end_matches("NRSVue")
                .trim_end_matches("NRSV")
                .trim_end_matches("NKJV")
                .trim_end_matches("NIV")
                .trim_end_matches("NLT")
                .trim_end_matches("NASB")
                .trim_end_matches("KJV")
                .trim();
            format!("{clean} {version_suffix}")
        })
        .filter(|s| crate::bible::parse_scripture_ref(s).is_some())
        .collect()
}

pub(super) fn has_scripture_ref(title: &str) -> bool {
    crate::bible::parse_scripture_ref(title).is_some()
}

pub(super) fn detect_version(title: &str) -> &str {
    let upper = title.to_uppercase();
    for (needle, version) in [
        ("NLT", "NLT"),
        ("NRSVUE", "NRSVue"),
        ("NRSV", "NRSV"),
        ("NKJV", "NKJV"),
        ("NIV", "NIV"),
        ("NASB", "NASB"),
        ("KJV", "KJV"),
    ] {
        if upper.contains(needle) {
            return version;
        }
    }
    "NRSVue"
}

pub(super) fn scripture_name(title: &str, version: &str) -> String {
    crate::bible::parse_scripture_ref(title).map_or_else(
        || super::classify::strip_speaker(title),
        |r| {
            let ref_str = r.end_verse.map_or_else(
                || format!("{} {}:{}", r.book, r.chapter, r.start_verse),
                |end| format!("{} {}:{}-{end}", r.book, r.chapter, r.start_verse),
            );
            format!("{ref_str} {version}")
        },
    )
}
