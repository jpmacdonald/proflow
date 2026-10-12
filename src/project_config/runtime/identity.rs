//! Typed identities carried by the compiled configuration.
//!
//! The wire model keeps these as plain strings so the editable JSON contract
//! and its collected validation issues do not change. Compilation is the one
//! place that turns them into these types.

use std::borrow::Borrow;
use std::fmt;

use serde::Serialize;

use super::render::{identifier_problem, IdentifierProblem};

/// Key of one presentation type that compiled to a runtime policy.
///
/// Only presentation-policy compilation mints a key, so holding one proves
/// the configuration defines that type.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct PresentationTypeKey(String);

impl PresentationTypeKey {
    /// Name the presentation type whose policy was just compiled.
    pub(super) fn compiled(key: &str) -> Self {
        Self(key.to_string())
    }

    /// Return the key exactly as the configuration spells it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for PresentationTypeKey {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PresentationTypeKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Why a configured name cannot be an exact native lookup key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum NameRejection {
    /// The name is empty or only whitespace.
    Blank,
    /// The name has padding or control characters.
    Malformed {
        name: String,
        problem: IdentifierProblem,
    },
}

fn checked_name(name: String) -> Result<String, NameRejection> {
    if name.trim().is_empty() {
        return Err(NameRejection::Blank);
    }
    match identifier_problem(&name) {
        Some(problem) => Err(NameRejection::Malformed { name, problem }),
        None => Ok(name),
    }
}

/// Exact name of one macro in the installed `ProPresenter` macro collection.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct MacroName(String);

impl MacroName {
    pub(super) fn new(name: String) -> Result<Self, NameRejection> {
        checked_name(name).map(Self)
    }

    /// Return the name exactly as the macro collection spells it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MacroName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Exact name of one slide in the project's `ProPresenter` theme.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ThemeSlideName(String);

impl ThemeSlideName {
    pub(super) fn new(name: String) -> Result<Self, NameRejection> {
        checked_name(name).map(Self)
    }

    /// Return the name exactly as the theme spells it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ThemeSlideName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl PartialEq<str> for PresentationTypeKey {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for PresentationTypeKey {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl PartialEq<str> for MacroName {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for MacroName {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl PartialEq<str> for ThemeSlideName {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for ThemeSlideName {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}
