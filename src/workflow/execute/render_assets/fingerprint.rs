//! Content fingerprints for the exact native assets parsed at startup.

use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::project_config::ProjectConfig;
use crate::propresenter::macros::MacroCache;
use crate::propresenter::theme::ThemeCache;

use super::audience::ConfiguredAudienceDestinations;

/// SHA-256 evidence for one parsed native document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NativeAssetDigest {
    /// Operator-facing asset kind.
    pub kind: &'static str,
    /// Exact source path read into the immutable runtime snapshot.
    pub path: String,
    /// Lowercase SHA-256 of the bytes that were parsed.
    pub sha256: String,
}

/// Stable identity of the configuration and display assets used for rendering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RenderAssetFingerprint {
    /// Versioned canonical fingerprint contract.
    pub schema: &'static str,
    /// Semantic project configuration revision.
    pub config_sha256: String,
    /// Configured native theme document, when rendering uses a theme.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<NativeAssetDigest>,
    /// Installed native macro document, when one exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub macros: Option<NativeAssetDigest>,
    /// Native Workspace that defined configured macro Audience Looks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audience_workspace: Option<NativeAssetDigest>,
    /// Exact alternate theme documents reached from configured macro Looks.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub audience_themes: Vec<NativeAssetDigest>,
    /// Aggregate revision of this complete record.
    pub revision: String,
}

impl RenderAssetFingerprint {
    pub(super) fn capture(
        config: &ProjectConfig,
        themes: &ThemeCache,
        macros: &MacroCache,
        audience: &ConfiguredAudienceDestinations,
    ) -> Result<Self, RenderAssetFingerprintError> {
        let config_sha256 = hash_serialized(config)?;
        let theme = themes
            .source_document()
            .map(|(path, digest)| native_digest("theme", path, digest))
            .transpose()?;
        let macros = macros
            .source_document()
            .map(|(path, digest)| native_digest("macros", path, digest))
            .transpose()?;
        let audience_workspace = audience
            .workspace_source()
            .map(|(path, digest)| native_digest("audience_workspace", path, digest))
            .transpose()?;
        let audience_themes = audience
            .theme_sources()
            .map(|(path, digest)| native_digest("audience_theme", path, digest))
            .collect::<Result<Vec<_>, _>>()?;
        let revision = aggregate_revision(
            &config_sha256,
            theme.as_ref(),
            macros.as_ref(),
            audience_workspace.as_ref(),
            &audience_themes,
        )?;
        Ok(Self {
            schema: "proflow.render-assets.v2",
            config_sha256,
            theme,
            macros,
            audience_workspace,
            audience_themes,
            revision,
        })
    }
}

#[derive(Serialize)]
struct RevisionMaterial<'a> {
    schema: &'static str,
    config_sha256: &'a str,
    theme: Option<&'a NativeAssetDigest>,
    macros: Option<&'a NativeAssetDigest>,
    audience_workspace: Option<&'a NativeAssetDigest>,
    audience_themes: &'a [NativeAssetDigest],
}

/// Failure to encode an otherwise checked render-asset snapshot canonically.
#[derive(Debug, thiserror::Error)]
pub enum RenderAssetFingerprintError {
    /// One native source path cannot be represented exactly in JSON evidence.
    #[error("{kind} asset path is not valid UTF-8: {}", path.display())]
    NonUtf8Path {
        /// Operator-facing native asset kind.
        kind: &'static str,
        /// Exact source path rejected before lossy conversion.
        path: PathBuf,
    },
    /// Semantic config or fingerprint material could not be serialized.
    #[error("failed to fingerprint checked render assets: {0}")]
    Serialize(#[from] serde_json::Error),
}

fn native_digest(
    kind: &'static str,
    path: &Path,
    digest: [u8; 32],
) -> Result<NativeAssetDigest, RenderAssetFingerprintError> {
    let path = path
        .to_str()
        .ok_or_else(|| RenderAssetFingerprintError::NonUtf8Path {
            kind,
            path: path.to_path_buf(),
        })?;
    Ok(NativeAssetDigest {
        kind,
        path: path.to_string(),
        sha256: digest_hex(&digest),
    })
}

fn hash_serialized(value: &impl Serialize) -> Result<String, serde_json::Error> {
    Ok(digest_hex(
        &Sha256::digest(serde_json::to_vec(value)?).into(),
    ))
}

fn aggregate_revision(
    config_sha256: &str,
    theme: Option<&NativeAssetDigest>,
    macros: Option<&NativeAssetDigest>,
    audience_workspace: Option<&NativeAssetDigest>,
    audience_themes: &[NativeAssetDigest],
) -> Result<String, serde_json::Error> {
    hash_serialized(&RevisionMaterial {
        schema: "proflow.render-assets.v2",
        config_sha256,
        theme,
        macros,
        audience_workspace,
        audience_themes,
    })
}

fn digest_hex(digest: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[cfg(unix)]
    #[test]
    fn native_asset_path_is_never_lossily_fingerprinted() {
        use std::os::unix::ffi::OsStringExt;

        let path = PathBuf::from(std::ffi::OsString::from_vec(vec![b'/', 0xff]));
        let error = native_digest("theme", &path, [0; 32])
            .expect_err("non-UTF-8 native path must not enter exact evidence");

        assert!(matches!(
            error,
            RenderAssetFingerprintError::NonUtf8Path {
                kind: "theme",
                path: actual,
            } if actual == path
        ));
    }

    #[test]
    fn aggregate_revision_binds_identical_bytes_to_the_exact_asset_path() {
        let digest = "ab".repeat(32);
        let first = NativeAssetDigest {
            kind: "theme",
            path: "/Themes/First.proTheme".to_string(),
            sha256: digest.clone(),
        };
        let second = NativeAssetDigest {
            kind: "theme",
            path: "/Themes/Second.proTheme".to_string(),
            sha256: digest,
        };

        let first_revision =
            aggregate_revision("config", Some(&first), None, None, &[]).expect("first fingerprint");
        let second_revision = aggregate_revision("config", Some(&second), None, None, &[])
            .expect("second fingerprint");

        assert_ne!(first_revision, second_revision);
    }
}
