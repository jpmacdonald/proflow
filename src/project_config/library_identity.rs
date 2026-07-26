//! Typed aliases for canonical presentations in the local library.
//!
//! These are identities, not classification policy: once a Planning Center
//! title matches, both the exact native file and its presentation policy are
//! known. Keeping them separate prevents recurring library aliases from
//! becoming precedence-sensitive item rules.

use serde::{Deserialize, Serialize};

/// One canonical presentation identity.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LibraryIdentityConfig {
    /// Stable identity used in receipts and review diagnostics.
    pub id: String,
    /// Deterministic title matcher for this identity.
    #[serde(rename = "match")]
    pub match_spec: LibraryIdentityMatch,
    /// Presentation policy required by the native file's structure.
    pub use_type: String,
    /// Exact canonical filename in the configured library.
    pub library_file: String,
    /// Optional reason this identity must remain distinct.
    pub notes: Option<String>,
}

/// The two supported title relationships for a canonical library identity.
///
/// A tagged enum makes contradictory prefix/substring definitions
/// unrepresentable in the parsed configuration.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LibraryIdentityMatch {
    /// Match when the normalized Planning Center title begins with any value.
    TitlePrefix {
        /// Accepted normalized title prefixes.
        values: Vec<String>,
    },
    /// Match when the normalized Planning Center title contains any value.
    TitleContains {
        /// Accepted normalized title substrings.
        values: Vec<String>,
    },
}

impl LibraryIdentityMatch {
    pub(super) fn values(&self) -> &[String] {
        match self {
            Self::TitlePrefix { values } | Self::TitleContains { values } => values,
        }
    }
}
