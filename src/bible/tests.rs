#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use super::*;

fn bible_source(text: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "John": { "3": { "16": text } }
    }))
    .expect("serialize Bible fixture")
}

fn john_3_16() -> ScriptureRef {
    ScriptureRef {
        book: "John".to_string(),
        chapter: 3,
        verses: VerseSelection::Verse(16),
    }
}

#[test]
fn test_parse_simple_ref() {
    let r = parse_scripture_ref("Isaiah 32:15-17").unwrap();
    assert_eq!(r.book, "Isaiah");
    assert_eq!(r.chapter, 32);
    assert_eq!(r.verses, VerseSelection::Range { start: 15, end: 17 });
}

#[test]
fn test_parse_numbered_book() {
    let r = parse_scripture_ref("1 John 3:1-3").unwrap();
    assert_eq!(r.book, "1 John");
    assert_eq!(r.chapter, 3);
    assert_eq!(r.verses, VerseSelection::Range { start: 1, end: 3 });
}

#[test]
fn test_parse_with_version() {
    let r = parse_scripture_ref("Luke 1:76-79 (NRSV)").unwrap();
    assert_eq!(r.book, "Luke");
    assert_eq!(r.chapter, 1);
    assert_eq!(r.verses, VerseSelection::Range { start: 76, end: 79 });
}

#[test]
fn test_parse_scripture_title_with_speaker_prefix() {
    let r = parse_scripture_ref("Scripture (Adrian) - Luke 8:26-39 NRSVue").unwrap();
    assert_eq!(r.book, "Luke");
    assert_eq!(r.chapter, 8);
    assert_eq!(r.verses, VerseSelection::Range { start: 26, end: 39 });
}

#[test]
fn test_parse_single_verse() {
    let r = parse_scripture_ref("John 3:16").unwrap();
    assert_eq!(r.book, "John");
    assert_eq!(r.chapter, 3);
    assert_eq!(r.verses, VerseSelection::Verse(16));
}

#[test]
fn discontinuous_ranges_are_one_reference() {
    let reference = parse_scripture_ref("Joshua 3:1-5, 9-17")
        .expect("valid discontinuous same-chapter reference");

    assert_eq!(reference.book, "Joshua");
    assert_eq!(reference.chapter, 3);
    assert_eq!(reference.to_string(), "Joshua 3:1-5, 9-17");
    let VerseSelection::Disjoint(ranges) = &reference.verses else {
        panic!("expected checked disjoint ranges");
    };
    assert_eq!(
        ranges
            .iter()
            .flat_map(|range| range.start..=range.end)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5, 9, 10, 11, 12, 13, 14, 15, 16, 17]
    );
}

#[test]
fn discontinuous_ranges_reject_overlap_or_reverse_order() {
    assert!(parse_scripture_ref("Joshua 3:1-5, 5-9").is_none());
    assert!(parse_scripture_ref("Joshua 3:9-17, 1-5").is_none());
}

#[test]
fn whole_chapter_is_a_distinct_reference() {
    let reference =
        parse_scripture_ref("Scripture: Jonah 3 NRSVue").expect("whole chapter reference");

    assert_eq!(reference.book, "Jonah");
    assert_eq!(reference.chapter, 3);
    assert_eq!(reference.verses, VerseSelection::Chapter);
    assert_eq!(reference.to_string(), "Jonah 3");
}

#[test]
fn every_canonical_book_name_parses_without_rewriting_its_letters() {
    let books = BOOK_ALIASES
        .values()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();

    for book in books {
        let parsed = parse_scripture_ref(&format!("{book} 1:1"))
            .unwrap_or_else(|| panic!("canonical book {book:?} must parse"));
        assert_eq!(parsed.book, book);
    }
}

#[test]
fn verse_letter_notation_is_limited_to_the_reference_token() {
    let parsed = parse_scripture_ref("Leviticus 2v11-13").expect("v notation");

    assert_eq!(parsed.book, "Leviticus");
    assert_eq!(parsed.chapter, 2);
    assert_eq!(parsed.verses, VerseSelection::Range { start: 11, end: 13 });
}

#[test]
fn test_superscript() {
    assert_eq!(to_superscript(15), "¹⁵");
    assert_eq!(to_superscript(1), "¹");
    assert_eq!(to_superscript(100), "¹⁰⁰");
}

#[test]
fn reviewed_source_bytes_must_match_the_immutable_snapshot() {
    let root = tempfile::tempdir().expect("temporary Bible root");
    let path = root.path().join(BibleVersion::NRSVue.file_name());
    let original = bible_source("old text");
    std::fs::write(&path, &original).expect("write Bible source");
    let reference = john_3_16();
    let bible =
        BibleCorpusSnapshot::capture(root.path().to_path_buf()).expect("capture Bible snapshot");

    let (_, first) = bible
        .lookup_reviewed_verses(&reference, BibleVersion::NRSVue, &original)
        .expect("first reviewed lookup");
    let changed =
        bible.lookup_reviewed_verses(&reference, BibleVersion::NRSVue, &bible_source("new text"));

    assert_eq!(first[0].text, "old text");
    assert!(changed.is_err());
}

#[test]
fn whole_chapter_lookup_returns_every_verse_in_numeric_order() {
    let source = serde_json::to_vec(&serde_json::json!({
        "Jonah": { "3": { "10": "Tenth", "2": "Second", "1": "First" } }
    }))
    .expect("serialize whole chapter fixture");
    let reference = parse_scripture_ref("Jonah 3").expect("whole chapter reference");
    let root = tempfile::tempdir().expect("temporary Bible root");
    std::fs::write(root.path().join(BibleVersion::NRSVue.file_name()), &source)
        .expect("write whole chapter fixture");
    let bible =
        BibleCorpusSnapshot::capture(root.path().to_path_buf()).expect("capture Bible snapshot");

    let (header, verses) = bible
        .lookup_reviewed_verses(&reference, BibleVersion::NRSVue, &source)
        .expect("whole chapter lookup");

    assert_eq!(
        verses.iter().map(|verse| verse.number).collect::<Vec<_>>(),
        vec![1, 2, 10]
    );
    assert_eq!(header.display(), "Jonah 3 NRSVue");
    assert!(header.missing_verses.is_empty());
}

#[test]
fn snapshot_verification_rejects_corpus_changes() {
    let root = tempfile::tempdir().expect("temporary Bible root");
    let path = root.path().join(BibleVersion::NRSVue.file_name());
    std::fs::write(&path, bible_source("old text")).expect("write first Bible source");
    let bible =
        BibleCorpusSnapshot::capture(root.path().to_path_buf()).expect("capture Bible snapshot");

    let (_, first) = bible
        .lookup_verses(&john_3_16(), BibleVersion::NRSVue)
        .expect("first file lookup");
    std::fs::write(&path, bible_source("new text")).expect("replace Bible source");

    assert_eq!(first[0].text, "old text");
    assert!(matches!(
        bible.verify_current(),
        Err(BibleCorpusError::Changed { .. })
    ));
}

#[test]
fn any_corpus_file_named_by_a_translation_code_is_loaded() {
    let root = tempfile::tempdir().expect("temporary Bible root");
    std::fs::write(
        root.path().join("ESV.json"),
        bible_source("For God so loved"),
    )
    .expect("write corpus");
    std::fs::write(root.path().join("reading plan.json"), b"not a corpus")
        .expect("write unrelated file");
    std::fs::write(root.path().join("NOTES.txt"), b"not a corpus").expect("write notes");
    let esv = BibleVersion::new("esv").expect("code");

    let snapshot = BibleCorpusSnapshot::capture(root.path().to_path_buf())
        .expect("a file that is not named by a code is not a corpus");

    let (header, verses) = snapshot.lookup_verses(&john_3_16(), esv).expect("lookup");
    assert_eq!(header.version, esv);
    assert_eq!(verses[0].text, "For God so loved");
    assert!(matches!(
        snapshot.lookup_verses(&john_3_16(), BibleVersion::NIV),
        Err(ScriptureLookupError::CorpusNotInstalled { version, .. })
            if version == BibleVersion::NIV
    ));
    assert!(snapshot.fingerprint().to_json()["sources"]["ESV"].is_string());
}

#[test]
fn corpus_validation_rejects_duplicate_translation_labels() {
    let root = tempfile::tempdir().expect("temporary Bible root");
    let bytes = bible_source("same mislabeled text");
    std::fs::write(root.path().join(BibleVersion::NRSVue.file_name()), &bytes)
        .expect("write first corpus");
    std::fs::write(root.path().join(BibleVersion::NRSV.file_name()), &bytes)
        .expect("write duplicate corpus");

    let error = validate_bible_corpora(root.path())
        .expect_err("duplicate translation bytes must be rejected");

    assert!(matches!(
        error,
        BibleCorpusError::DuplicateTranslation {
            first: BibleVersion::NRSV,
            second: BibleVersion::NRSVue
        }
    ));
}

#[test]
fn corpus_validation_rejects_malformed_installed_data() {
    let root = tempfile::tempdir().expect("temporary Bible root");
    std::fs::write(
        root.path().join(BibleVersion::NRSVue.file_name()),
        b"not json",
    )
    .expect("write malformed corpus");

    assert!(matches!(
        validate_bible_corpora(root.path()),
        Err(BibleCorpusError::Parse {
            version: BibleVersion::NRSVue,
            ..
        })
    ));
}
