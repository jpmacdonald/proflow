//! Commit-time freshness for the exact font programs shaped by `TextKit`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use sha2::{Digest, Sha256};

use super::CueTextFitSummary;

/// Immutable font programs used by all native text measurements in one build.
///
/// The native helper may cache a digest while serving a build. This snapshot
/// retains both that content identity and CoreText's exact local path so the
/// commit boundary can prove that every shaped font still has the same bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontProgramSnapshot {
    programs: Vec<FontProgramFingerprint>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FontProgramFingerprint {
    path: PathBuf,
    sha256: String,
}

impl FontProgramSnapshot {
    pub(crate) fn capture<'a>(
        summaries: impl IntoIterator<Item = &'a CueTextFitSummary>,
    ) -> Result<Self, FontProgramFreshnessError> {
        let mut programs = BTreeMap::<PathBuf, String>::new();
        for (path, sha256) in summaries
            .into_iter()
            .flat_map(CueTextFitSummary::font_programs)
        {
            match programs.get(path) {
                Some(existing) if existing != sha256 => {
                    return Err(FontProgramFreshnessError::ConflictingEvidence {
                        path: path.to_path_buf(),
                        first: existing.clone(),
                        second: sha256.to_string(),
                    });
                }
                Some(_) => {}
                None => {
                    programs.insert(path.to_path_buf(), sha256.to_string());
                }
            }
        }
        Ok(Self {
            programs: programs
                .into_iter()
                .map(|(path, sha256)| FontProgramFingerprint { path, sha256 })
                .collect(),
        })
    }

    #[cfg(test)]
    pub(crate) const fn diagnostic() -> Self {
        Self {
            programs: Vec::new(),
        }
    }

    /// Re-hash every exact font program immediately before artifact commit.
    pub(crate) fn verify_current(&self) -> Result<(), FontProgramFreshnessError> {
        for program in &self.programs {
            let bytes =
                std::fs::read(&program.path).map_err(|source| FontProgramFreshnessError::Read {
                    path: program.path.clone(),
                    source,
                })?;
            let actual = digest_hex(&Sha256::digest(bytes));
            if actual != program.sha256 {
                return Err(FontProgramFreshnessError::Changed {
                    path: program.path.clone(),
                    expected: program.sha256.clone(),
                    actual,
                });
            }
        }
        Ok(())
    }
}

/// A CoreText-resolved font program changed after native layout was measured.
#[derive(Debug, thiserror::Error)]
pub enum FontProgramFreshnessError {
    /// Two measurements claimed different bytes for one exact local path.
    #[error(
        "font program '{}' produced conflicting text-fit evidence (SHA-256 {first} and {second})",
        path.display()
    )]
    ConflictingEvidence {
        /// Exact CoreText font-program path.
        path: PathBuf,
        /// First digest captured in this build.
        first: String,
        /// Conflicting digest captured in this build.
        second: String,
    },
    /// The exact program can no longer be read.
    #[error(
        "font program '{}' cannot be revalidated after text layout; review again: {source}",
        path.display()
    )]
    Read {
        /// Exact CoreText font-program path.
        path: PathBuf,
        /// Current filesystem failure.
        #[source]
        source: std::io::Error,
    },
    /// The program is readable but no longer contains the shaped bytes.
    #[error(
        "font program '{}' changed after text layout (expected SHA-256 {expected}, found {actual}); review again",
        path.display()
    )]
    Changed {
        /// Exact CoreText font-program path.
        path: PathBuf,
        /// Digest returned for the measured layout.
        expected: String,
        /// Digest present at commit time.
        actual: String,
    },
}

fn digest_hex(digest: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(digest.len().saturating_mul(2));
    for byte in digest {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_program_snapshot_rejects_bytes_changed_after_layout(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("resolved-font.otf");
        std::fs::write(&path, b"font-program-at-layout")?;
        let expected = digest_hex(&Sha256::digest(b"font-program-at-layout"));
        let snapshot = FontProgramSnapshot {
            programs: vec![FontProgramFingerprint {
                path: path.clone(),
                sha256: expected.clone(),
            }],
        };

        std::fs::write(&path, b"changed-before-commit")?;
        let error = snapshot
            .verify_current()
            .err()
            .ok_or("changed font program unexpectedly passed revalidation")?;

        assert!(matches!(
            error,
            FontProgramFreshnessError::Changed {
                path: changed_path,
                expected: changed_expected,
                actual,
            } if changed_path == path && changed_expected == expected && actual != changed_expected
        ));
        Ok(())
    }
}
