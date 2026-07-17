use super::*;
use crate::propresenter::playlist::naming::file_url_for_test;

#[test]
fn scripture_strips_prefix_and_converts_colons() {
    assert_eq!(
        sanitize_filename(
            "Scripture - 1 Kings 18:18-21 (Connie)",
            SlideType::Scripture
        ),
        "1 Kings 18v18-21"
    );
    assert_eq!(
        sanitize_filename("Scripture: 1 Kings 18:18-21", SlideType::Scripture),
        "1 Kings 18v18-21"
    );
    assert_eq!(
        sanitize_filename("Reading - John 3:16", SlideType::Scripture),
        "John 3v16"
    );
}

#[test]
fn scripture_bare_reference() {
    assert_eq!(
        sanitize_filename("Matthew 6:1-2", SlideType::Scripture),
        "Matthew 6v1-2"
    );
    assert_eq!(
        sanitize_filename("Psalm 119:105-106", SlideType::Scripture),
        "Psalm 119v105-106"
    );
}

#[test]
fn scripture_strips_speaker_parens() {
    assert_eq!(
        sanitize_filename("Scripture (Robert)", SlideType::Scripture),
        ""
    );
    assert_eq!(
        sanitize_filename("Scripture - John 3:16 (Robert)", SlideType::Scripture),
        "John 3v16"
    );
}

#[test]
fn song_keeps_parens() {
    assert_eq!(
        sanitize_filename("Firm Foundation (He Won't)", SlideType::Lyrics),
        "Firm Foundation (He Won't)"
    );
    assert_eq!(
        sanitize_filename("Morning By Morning (I Will Trust)", SlideType::Lyrics),
        "Morning By Morning (I Will Trust)"
    );
    assert_eq!(
        sanitize_filename("Oceans (Where Feet May Fail)", SlideType::Lyrics),
        "Oceans (Where Feet May Fail)"
    );
}

#[test]
fn song_strips_unsafe_chars() {
    assert_eq!(sanitize_filename("What?", SlideType::Lyrics), "What");
}

#[test]
fn general_strips_speaker_parens() {
    assert_eq!(
        sanitize_filename("Welcome (Robert)", SlideType::Graphic),
        "Welcome"
    );
    assert_eq!(
        sanitize_filename("Children's Message (Connie)", SlideType::Title),
        "Children's Message"
    );
    assert_eq!(
        sanitize_filename("Benediction (Robert)", SlideType::Text),
        "Benediction"
    );
}

#[test]
fn general_colon_to_dash() {
    assert_eq!(
        sanitize_filename("Prelude: Truro Procession", SlideType::Text),
        "Prelude - Truro Procession"
    );
    assert_eq!(
        sanitize_filename("Sermon: Showdown (Robert)", SlideType::Title),
        "Sermon - Showdown"
    );
}

#[test]
fn general_unsafe_chars_are_stripped() {
    assert_eq!(
        sanitize_filename("He said \"hello\"", SlideType::Text),
        "He said hello"
    );
}

#[test]
fn general_name_passes_through() {
    assert_eq!(
        sanitize_filename("Amazing Grace", SlideType::Text),
        "Amazing Grace"
    );
}

#[test]
fn source_path_owns_the_archive_filename() {
    let entry = linked_entry(
        "Display Alias",
        "/Libraries/Default/Morning By Morning (I Will Trust).pro",
    );
    assert_eq!(
        entry.embedded_filename(),
        "Morning By Morning (I Will Trust).pro"
    );
}

#[test]
fn file_urls_encode_reserved_filename_characters() {
    assert_eq!(
        file_url_for_test("/Libraries/Default/[Hymn] A&B #1.pro"),
        "file:///Libraries/Default/%5BHymn%5D%20A%26B%20%231.pro"
    );
    assert_eq!(
        file_url_for_test("file:///Libraries/Default/Already%20Encoded.pro"),
        "file:///Libraries/Default/Already%20Encoded.pro"
    );
}

#[test]
fn canonical_name_replaces_colon_with_v() {
    assert_eq!(
        canonical_presentation_name("Matthew 3:16-17", SlideType::Scripture),
        Ok("Matthew 3v16-17".to_string())
    );
}

#[test]
fn canonical_name_rejects_empty_normalized_output() {
    assert_eq!(
        canonical_presentation_name("(Prayer)", SlideType::Title),
        Err(CanonicalPresentationNameError::Empty)
    );
}

#[test]
fn output_path_requires_and_uses_the_explicit_directory() {
    let directory = Path::new("/reviewed/playlist-output");
    assert_eq!(
        playlist_output_path(directory, "Sunday: Service"),
        directory.join("Sunday_ Service.proplaylist")
    );
}
