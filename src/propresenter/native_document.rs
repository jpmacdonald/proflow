//! Checked ownership states for existing native presentation bytes.

use prost::Message;

use super::deserialize::{decode_presentation_bytes, ProPresenterError};
use super::generated::rv_data;
use super::serialize::{encode_existing_presentation, SerializeError};

/// Existing native bytes that are safe to inspect or reuse unchanged.
///
/// This state deliberately exposes no mutable presentation reference. Unknown
/// protobuf fields can therefore remain preserved in `bytes` even when the
/// current schema cannot represent them.
#[derive(Debug)]
pub struct OpaquePresentation {
    source_label: String,
    bytes: Vec<u8>,
    presentation: rv_data::Presentation,
}

impl OpaquePresentation {
    /// Decode one identified native presentation without claiming that the
    /// current schema can reproduce all of its bytes.
    pub fn decode(data: &[u8], source: impl Into<String>) -> Result<Self, ProPresenterError> {
        let source = source.into();
        let presentation = decode_presentation_bytes(data, &source)?;
        Ok(Self {
            source_label: source,
            bytes: data.to_vec(),
            presentation,
        })
    }

    /// Read the schema-known native structure without permitting mutation.
    pub const fn presentation(&self) -> &rv_data::Presentation {
        &self.presentation
    }

    /// Exact original bytes for unchanged reuse.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Consume this document and retain its exact original bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// Prove exact decode/re-encode parity before permitting native mutation.
    pub fn try_into_editable(self) -> Result<EditablePresentation, NativeEditError> {
        let encoded = self.presentation.encode_to_vec();
        if encoded != self.bytes {
            return Err(NativeEditError::LossyDecode {
                source_label: self.source_label,
            });
        }
        Ok(EditablePresentation {
            presentation: self.presentation,
        })
    }
}

/// Existing native presentation whose complete input bytes are representable
/// by the current schema and may therefore be mutated safely.
#[derive(Debug)]
pub struct EditablePresentation {
    presentation: rv_data::Presentation,
}

impl EditablePresentation {
    /// Inspect the checked editable document.
    pub const fn presentation(&self) -> &rv_data::Presentation {
        &self.presentation
    }

    /// Mutate only after exact input parity was established.
    pub const fn presentation_mut(&mut self) -> &mut rv_data::Presentation {
        &mut self.presentation
    }

    /// Encode the edited document through the existing-identity boundary.
    pub fn encode(&self) -> Result<Vec<u8>, SerializeError> {
        encode_existing_presentation(&self.presentation)
    }
}

/// Existing bytes that cannot safely cross the native mutation boundary.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NativeEditError {
    /// Decoding discarded or canonicalized bytes not represented by the
    /// current protobuf schema.
    #[error("native presentation '{source_label}' is not byte-exact under the current schema")]
    LossyDecode {
        /// Operator-visible source identity.
        source_label: String,
    },
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn presentation() -> rv_data::Presentation {
        rv_data::Presentation {
            uuid: Some(rv_data::Uuid {
                string: "DOCUMENT-ID".to_string(),
            }),
            name: "Native".to_string(),
            ..rv_data::Presentation::default()
        }
    }

    #[test]
    fn unchanged_reuse_preserves_unknown_wire_data_but_editing_rejects_it() {
        let mut bytes = presentation().encode_to_vec();
        bytes.extend_from_slice(&[0xf8, 0x7f, 0x01]);

        let opaque = OpaquePresentation::decode(&bytes, "unknown-field.pro")
            .expect("unknown protobuf fields remain decodable");
        assert_eq!(opaque.bytes(), bytes);
        assert_eq!(
            opaque
                .try_into_editable()
                .expect_err("lossy edit must fail"),
            NativeEditError::LossyDecode {
                source_label: "unknown-field.pro".to_string(),
            }
        );
    }

    #[test]
    fn byte_exact_document_crosses_the_editable_boundary() {
        let bytes = presentation().encode_to_vec();
        let mut editable = OpaquePresentation::decode(&bytes, "exact.pro")
            .expect("native document")
            .try_into_editable()
            .expect("byte-exact document");
        editable.presentation_mut().name = "Edited".to_string();

        let edited = rv_data::Presentation::decode(
            editable
                .encode()
                .expect("encode edited document")
                .as_slice(),
        )
        .expect("decode edited document");
        assert_eq!(edited.name, "Edited");
    }
}
