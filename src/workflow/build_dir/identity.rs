//! Checked plan identities and deterministic input hashes.

use super::{hex, BuildDirError, INPUT_HASH_DOMAIN};
use sha2::Digest;
use sha2::Sha256;
use std::fmt;

/// Planning Center plan identity, checked to be one safe directory name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildPlanId(String);

impl BuildPlanId {
    /// Accept 1 to 64 ASCII letters, digits, `-`, or `_`.
    pub fn new(value: impl Into<String>) -> Result<Self, BuildDirError> {
        let value = value.into();
        let safe = !value.is_empty()
            && value.len() <= 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
        if safe {
            Ok(Self(value))
        } else {
            Err(BuildDirError::InvalidPlanId { value })
        }
    }

    /// Return the plan identity text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Everything a build is derived from, as the exact bytes that were read.
#[derive(Debug, Clone, Copy)]
pub struct BuildInputs<'a> {
    /// Captured Planning Center plan snapshot.
    pub plan_snapshot: &'a [u8],
    /// Fingerprint of the project configuration and native display assets.
    pub render_assets: &'a [u8],
    /// Fingerprints of the Bible corpora the build reads.
    pub bible_corpora: &'a [u8],
    /// Every presentation in the library the review could match, by content.
    pub library: &'a [u8],
    /// The operator's decisions in canonical form.
    pub review_decisions: &'a [u8],
}

/// SHA-256 over every build input, each bound to its name and length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputHash([u8; 32]);

impl InputHash {
    /// Hash one set of build inputs.
    #[must_use]
    pub fn of(inputs: &BuildInputs<'_>) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(INPUT_HASH_DOMAIN);
        for (name, bytes) in [
            ("plan_snapshot", inputs.plan_snapshot),
            ("render_assets", inputs.render_assets),
            ("bible_corpora", inputs.bible_corpora),
            ("library", inputs.library),
            ("review_decisions", inputs.review_decisions),
        ] {
            // Name and length keep a byte from moving between two inputs
            // without changing the hash.
            hasher.update((name.len() as u64).to_be_bytes());
            hasher.update(name.as_bytes());
            hasher.update((bytes.len() as u64).to_be_bytes());
            hasher.update(bytes);
        }
        Self(hasher.finalize().into())
    }
}

impl fmt::Display for InputHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&hex(&self.0))
    }
}
