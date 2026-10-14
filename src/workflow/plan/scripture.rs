//! Checked scripture sources for executable plans.

use serde::Serialize;

use crate::bible::BibleVersion;

/// Individual scripture reference within a multi-reference item.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ScriptureRefInfo {
    /// Parsed reference string (e.g., "Isaiah 35:1-6").
    reference: String,
    /// Bible version (e.g., "`NRSVue`").
    version: BibleVersion,
}

impl ScriptureRefInfo {
    /// Create one explicitly versioned scripture reference.
    pub fn new(reference: String, version: BibleVersion) -> Result<Self, ScripturePlanError> {
        validate_reference(&reference)?;
        Ok(Self { reference, version })
    }

    /// Return the parsed scripture reference.
    pub fn reference(&self) -> &str {
        &self.reference
    }

    /// Return the explicit Bible version.
    pub const fn version(&self) -> BibleVersion {
        self.version
    }
}

/// Invalid scripture identity data rejected before it enters an executable plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ScripturePlanError {
    /// No scripture reference was provided.
    #[error("scripture reference cannot be blank")]
    BlankReference,
    /// The reference carries semantically irrelevant surrounding whitespace.
    #[error("scripture reference cannot contain surrounding whitespace")]
    PaddedReference,
    /// The reference contains a control character.
    #[error("scripture reference cannot contain control characters")]
    ControlCharacterInReference,
    /// Reference text is not one canonical passage without annotations.
    #[error(
        "scripture reference must be one canonical passage without translation or annotations"
    )]
    InvalidReference,
    /// A partial scripture reference had no authoritative passage text.
    #[error("partial scripture description cannot be blank")]
    BlankExcerpt,
    /// The visible partial reference did not identify the lookup range's prefix.
    #[error(
        "partial scripture display reference must be the whole lookup reference followed by 'a'"
    )]
    MismatchedExcerptReference,
}

/// One valid scripture source form for generated plans.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct ScriptureContent(ScriptureSource);

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
enum ScriptureSource {
    Single {
        reference: String,
        bible_version: BibleVersion,
    },
    PrefixExcerpt {
        reference: String,
        display_reference: String,
        bible_version: BibleVersion,
        excerpt_text: String,
    },
    SelectedExcerpt {
        reference: String,
        bible_version: BibleVersion,
        excerpt_text: String,
    },
    Combined {
        references: Vec<ScriptureRefInfo>,
    },
}

/// Borrowed view of a checked scripture source.
#[derive(Debug, Clone, Copy)]
pub enum ScriptureRequest<'a> {
    /// One passage using one explicit Bible version.
    Single {
        reference: &'a str,
        bible_version: BibleVersion,
    },
    /// One passage whose final verse is bounded by validated Planning Center text.
    PrefixExcerpt {
        /// Whole-verse local Bible range used for lookup and labels.
        reference: &'a str,
        /// Operator-visible reference retaining its partial-verse suffix.
        display_reference: &'a str,
        /// Explicit local Bible version.
        bible_version: BibleVersion,
        /// Authoritative Planning Center wording and cutoff.
        excerpt_text: &'a str,
    },
    /// One chapter whose selected verse wording comes from Planning Center.
    SelectedExcerpt {
        /// Whole-chapter local Bible reference used for validation and metadata.
        reference: &'a str,
        /// Explicit local Bible version.
        bible_version: BibleVersion,
        /// Authoritative numbered Planning Center wording.
        excerpt_text: &'a str,
    },
    /// Two or more explicitly versioned passages.
    Combined(&'a [ScriptureRefInfo]),
}

impl ScriptureContent {
    /// Create one passage with its required version.
    pub fn single(
        reference: String,
        bible_version: BibleVersion,
    ) -> Result<Self, ScripturePlanError> {
        validate_reference(&reference)?;
        Ok(Self(ScriptureSource::Single {
            reference,
            bible_version,
        }))
    }

    /// Create one description-bounded prefix excerpt over a whole-verse lookup.
    pub fn prefix_excerpt(
        reference: String,
        display_reference: String,
        bible_version: BibleVersion,
        excerpt_text: String,
    ) -> Result<Self, ScripturePlanError> {
        validate_reference(&reference)?;
        if display_reference != format!("{reference}a") {
            return Err(ScripturePlanError::MismatchedExcerptReference);
        }
        if excerpt_text.trim().is_empty() {
            return Err(ScripturePlanError::BlankExcerpt);
        }
        Ok(Self(ScriptureSource::PrefixExcerpt {
            reference,
            display_reference,
            bible_version,
            excerpt_text,
        }))
    }

    /// Create one numbered selected-verse excerpt over a whole chapter.
    pub fn selected_excerpt(
        reference: String,
        bible_version: BibleVersion,
        excerpt_text: String,
    ) -> Result<Self, ScripturePlanError> {
        validate_reference(&reference)?;
        if excerpt_text.trim().is_empty() {
            return Err(ScripturePlanError::BlankExcerpt);
        }
        Ok(Self(ScriptureSource::SelectedExcerpt {
            reference,
            bible_version,
            excerpt_text,
        }))
    }

    /// Create a combined source only when at least two passages are present.
    pub fn combined(references: Vec<ScriptureRefInfo>) -> Option<Self> {
        (references.len() >= 2).then_some(Self(ScriptureSource::Combined { references }))
    }

    /// Inspect the single valid source form carried by this value.
    pub fn request(&self) -> ScriptureRequest<'_> {
        match &self.0 {
            ScriptureSource::Single {
                reference,
                bible_version,
            } => ScriptureRequest::Single {
                reference,
                bible_version: *bible_version,
            },
            ScriptureSource::PrefixExcerpt {
                reference,
                display_reference,
                bible_version,
                excerpt_text,
            } => ScriptureRequest::PrefixExcerpt {
                reference,
                display_reference,
                bible_version: *bible_version,
                excerpt_text,
            },
            ScriptureSource::SelectedExcerpt {
                reference,
                bible_version,
                excerpt_text,
            } => ScriptureRequest::SelectedExcerpt {
                reference,
                bible_version: *bible_version,
                excerpt_text,
            },
            ScriptureSource::Combined { references } => {
                ScriptureRequest::Combined(references.as_slice())
            }
        }
    }
}

fn validate_reference(reference: &str) -> Result<(), ScripturePlanError> {
    if reference.trim().is_empty() {
        return Err(ScripturePlanError::BlankReference);
    }
    if reference.trim() != reference {
        return Err(ScripturePlanError::PaddedReference);
    }
    if reference.chars().any(char::is_control) {
        return Err(ScripturePlanError::ControlCharacterInReference);
    }
    if crate::bible::parse_scripture_ref(reference)
        .is_none_or(|parsed| parsed.to_string() != reference)
    {
        return Err(ScripturePlanError::InvalidReference);
    }
    Ok(())
}
