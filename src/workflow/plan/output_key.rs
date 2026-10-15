//! Checked identities for planned workflow outputs.

use std::fmt;

use serde::Serialize;

/// Stable identity for one planned workflow output.
///
/// Keys are checked at ad-hoc boundaries and generated through the three
/// canonical workflow forms. Dynamic key segments use collision-free percent
/// encoding for structural delimiters and characters that cannot appear in a
/// checked key.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct OutputKey(String);

/// Invalid ad-hoc output identity rejected before it enters a plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum OutputKeyError {
    /// No output identity was provided.
    #[error("output key cannot be blank")]
    Blank,
    /// The key carries semantically irrelevant surrounding whitespace.
    #[error("output key cannot contain surrounding whitespace")]
    Padded,
    /// The key contains a control character.
    #[error("output key cannot contain control characters")]
    ControlCharacter,
}

impl OutputKey {
    /// Check one ad-hoc output identity.
    pub fn new(value: String) -> Result<Self, OutputKeyError> {
        if value.trim().is_empty() {
            return Err(OutputKeyError::Blank);
        }
        if value.trim() != value {
            return Err(OutputKeyError::Padded);
        }
        if value.chars().any(char::is_control) {
            return Err(OutputKeyError::ControlCharacter);
        }
        Ok(Self(value))
    }

    /// Generate the primary output identity for a Planning Center item.
    pub fn primary(pco_item_id: &str) -> Self {
        Self(format!("pco:{}:main", encode_key_segment(pco_item_id)))
    }

    /// Generate one expanded output identity for a Planning Center rule step.
    pub fn expanded(pco_item_id: &str, step_index: usize, use_type: &str) -> Self {
        Self(format!(
            "pco:{}:expand:{step_index}:{}",
            encode_key_segment(pco_item_id),
            encode_key_segment(use_type)
        ))
    }

    /// Generate the identity for a project-required presentation.
    pub fn required(required_item_id: &str) -> Self {
        Self(format!("required:{}", encode_key_segment(required_item_id)))
    }

    /// Borrow the checked key text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OutputKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl TryFrom<String> for OutputKey {
    type Error = OutputKeyError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl PartialEq<str> for OutputKey {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for OutputKey {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

fn encode_key_segment(segment: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";

    let mut encoded = String::with_capacity(segment.len());
    for character in segment.chars() {
        if character == '%'
            || character == ':'
            || character.is_control()
            || character.is_whitespace()
        {
            let mut bytes = [0; 4];
            for byte in character.encode_utf8(&mut bytes).bytes() {
                encoded.push('%');
                encoded.push(char::from(HEX[usize::from(byte >> 4)]));
                encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
            }
        } else {
            encoded.push(character);
        }
    }
    encoded
}
