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
