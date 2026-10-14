//! Checked library and background identities in the JSON contract.

use serde::Deserialize;
use serde::Serialize;
use std::path::Component;
use std::path::Path;
use std::str::FromStr;

/// Checked name of one registered `ProPresenter` presentation library.
///
/// Library names are filesystem identities below `Libraries`, so separators,
/// traversal components, padding, and control characters are rejected at the
/// config boundary.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, schemars::JsonSchema)]
#[schemars(with = "String")]
pub struct LibraryName(String);

impl LibraryName {
    /// Parse one exact registered-library name.
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.is_empty()
            || value.trim() != value
            || value.chars().any(char::is_control)
            || value.contains(['/', '\\'])
            || value.len() > 128
        {
            return Err(
                "library name must be 1-128 unpadded characters with no controls".to_string(),
            );
        }
        let mut components = Path::new(&value).components();
        if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
            return Err("library name must be one normal path component".to_string());
        }
        Ok(Self(value))
    }

    /// Exact directory name below the native `Libraries` root.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for LibraryName {
    fn default() -> Self {
        Self("Default".to_string())
    }
}

impl std::fmt::Display for LibraryName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for LibraryName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for LibraryName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Validated identifier for a project-owned background asset.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, schemars::JsonSchema)]
#[schemars(with = "String")]
pub struct BackgroundId(String);

impl BackgroundId {
    /// Parse and validate a background identifier.
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        let bytes = value.as_bytes();
        if bytes.is_empty() {
            return Err("background id must not be empty".to_string());
        }
        if bytes.len() > 64 {
            return Err("background id must be at most 64 ASCII characters".to_string());
        }
        let valid_alnum = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
        if !valid_alnum(bytes[0]) {
            return Err(
                "background id must start with a lowercase ASCII letter or digit".to_string(),
            );
        }
        if !bytes[1..]
            .iter()
            .all(|byte| valid_alnum(*byte) || matches!(*byte, b'_' | b'-'))
        {
            return Err(
                "background id may contain only lowercase ASCII letters, digits, '_' and '-'"
                    .to_string(),
            );
        }
        Ok(Self(value))
    }

    /// Return the validated identifier text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for BackgroundId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for BackgroundId {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl Serialize for BackgroundId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for BackgroundId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Validated project-relative path to a supported background image.
#[derive(Debug, Clone, PartialEq, Eq, schemars::JsonSchema)]
#[schemars(with = "String")]
pub struct BackgroundAssetPath(String);

impl BackgroundAssetPath {
    /// Parse and validate a project-relative background image path.
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.is_empty() {
            return Err("background path must not be empty".to_string());
        }
        if value.chars().any(char::is_control) {
            return Err("background path must not contain control characters".to_string());
        }
        if value.contains('\\') {
            return Err("background path must use '/' separators".to_string());
        }
        if value
            .split('/')
            .next()
            .map(str::as_bytes)
            .is_some_and(|first_component| {
                first_component.len() == 2
                    && first_component[0].is_ascii_alphabetic()
                    && first_component[1] == b':'
            })
        {
            return Err("background path must be relative, without a drive prefix".to_string());
        }
        if value
            .split('/')
            .any(|component| component.is_empty() || matches!(component, "." | ".."))
        {
            return Err(
                "background path must contain only normal relative path components".to_string(),
            );
        }
        let path = Path::new(&value);
        if path.is_absolute()
            || !path
                .components()
                .all(|component| matches!(component, Component::Normal(_)))
        {
            return Err(
                "background path must contain only normal relative path components".to_string(),
            );
        }
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase);
        if !matches!(
            extension.as_deref(),
            Some("jpg" | "jpeg" | "png" | "tiff" | "tif")
        ) {
            return Err(
                "background path must use a jpg, jpeg, png, tiff, or tif extension".to_string(),
            );
        }
        Ok(Self(value))
    }

    /// Return the validated relative path.
    pub fn as_path(&self) -> &Path {
        Path::new(&self.0)
    }
}

impl Serialize for BackgroundAssetPath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for BackgroundAssetPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}
