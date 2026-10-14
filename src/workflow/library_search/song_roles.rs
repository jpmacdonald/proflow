//! Classify song roles and score their compatibility.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SongRole {
    Unknown,
    Hymn,
    Special,
}

pub(super) fn infer_requested_song_role(song_title: &str, item_title: &str) -> SongRole {
    let titles = [song_title, item_title].map(|title| title.trim().to_lowercase());
    if titles.iter().any(|title| is_hymn_song_role(title)) {
        return SongRole::Hymn;
    }
    if titles.iter().any(|title| is_special_song_role(title)) {
        return SongRole::Special;
    }
    SongRole::Unknown
}

fn is_hymn_song_role(title: &str) -> bool {
    let title = title.trim_start();
    title.starts_with('#')
        || title.starts_with("[hymn]")
        || title.contains("gtg #")
        || title.contains("hymn #")
        || starts_with_role_word(title, "hymn")
}

fn is_special_song_role(title: &str) -> bool {
    const BRACKETED: &[&str] = &[
        "[anthem]",
        "[youth choir]",
        "[choir]",
        "[special music]",
        "[solo]",
        "[duet]",
        "[prelude]",
        "[postlude]",
    ];
    const PLAIN: &[&str] = &[
        "anthem",
        "youth choir",
        "choir",
        "special music",
        "solo",
        "duet",
        "prelude",
        "postlude",
        "offertory",
        "handbell",
        "organ prelude",
        "organ postlude",
    ];
    let title = title.trim_start();
    BRACKETED.iter().any(|prefix| title.starts_with(prefix))
        || PLAIN
            .iter()
            .any(|prefix| starts_with_role_word(title, prefix))
}

fn starts_with_role_word(title: &str, prefix: &str) -> bool {
    title.strip_prefix(prefix).is_some_and(|suffix| {
        suffix.is_empty()
            || suffix.chars().next().is_some_and(|character| {
                character.is_whitespace() || matches!(character, ':' | '-')
            })
    })
}

pub(super) fn file_song_role(file_name: &str) -> SongRole {
    let lower = file_name.trim().to_lowercase();
    if is_hymn_song_role(&lower) {
        return SongRole::Hymn;
    }
    if is_special_song_role(&lower) {
        return SongRole::Special;
    }
    SongRole::Unknown
}

pub(super) const fn role_compatibility_score(requested_role: SongRole, file_role: SongRole) -> i32 {
    match (requested_role, file_role) {
        (SongRole::Unknown, _) | (_, SongRole::Unknown) => 0,
        (SongRole::Hymn, SongRole::Hymn) | (SongRole::Special, SongRole::Special) => 20,
        (SongRole::Hymn, SongRole::Special) | (SongRole::Special, SongRole::Hymn) => -20,
    }
}
