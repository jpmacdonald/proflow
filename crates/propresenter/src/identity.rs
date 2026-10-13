//! Identities for the objects a build creates.
//!
//! A native document names its cues, slides, actions and groups by UUID.
//! Minting those at random would make two builds of the same content two
//! different files. Every identity a build creates is instead derived from
//! what it identifies, so building the same thing twice writes the same bytes.

use sha2::{Digest, Sha256};

use crate::generated::rv_data;

/// A UUID derived from `domain` and `parts`.
///
/// RFC 9562 reserves version 8 for application-defined hashes such as this.
pub fn derived_uuid(domain: &str, parts: &[&[u8]]) -> rv_data::Uuid {
    let mut hasher = Sha256::new();
    hasher.update(domain.as_bytes());
    for part in parts {
        // The length keeps a byte from moving between two parts.
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part);
    }
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    rv_data::Uuid {
        string: uuid::Uuid::from_bytes(bytes).to_string(),
    }
}

/// The identities of one generated presentation, in the order its objects
/// are created.
///
/// The sequence depends only on the presentation's name, so rendering the
/// same specification again yields the same identities. Each one is distinct.
#[derive(Debug)]
pub struct IdentitySource {
    presentation: String,
    issued: u64,
}

impl IdentitySource {
    pub fn for_presentation(name: &str) -> Self {
        Self {
            presentation: name.to_string(),
            issued: 0,
        }
    }

    /// The next identity in this presentation's sequence.
    pub fn next(&mut self) -> rv_data::Uuid {
        let uuid = derived_uuid(
            "proflow:generated-presentation:v1",
            &[self.presentation.as_bytes(), &self.issued.to_be_bytes()],
        );
        self.issued += 1;
        uuid
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn a_presentation_always_gets_the_same_distinct_identities() {
        let sequence = |name: &str| {
            let mut identities = IdentitySource::for_presentation(name);
            (0..50)
                .map(|_| identities.next().string)
                .collect::<Vec<_>>()
        };

        let first = sequence("John 3v16 NIV");
        assert_eq!(first, sequence("John 3v16 NIV"));
        assert_eq!(first.iter().collect::<BTreeSet<_>>().len(), 50);
        assert!(first
            .iter()
            .all(|identity| uuid::Uuid::parse_str(identity).is_ok()));
        assert!(sequence("John 3v17 NIV")
            .iter()
            .all(|identity| !first.contains(identity)));
    }

    #[test]
    fn a_derived_identity_separates_its_parts() {
        assert_ne!(
            derived_uuid("d", &[b"ab", b"c"]),
            derived_uuid("d", &[b"a", b"bc"])
        );
        assert_ne!(derived_uuid("d", &[b"a"]), derived_uuid("e", &[b"a"]));
    }
}
