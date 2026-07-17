//! Installed `ProPresenter` cue-group metadata.
//!
//! Named cue groups are application assets, not presentation-local styling.
//! This catalog preserves their exact color, hot key, UUID, and display name
//! so a newly created group has the same native identity as one created in the
//! ProPresenter UI.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use prost::Message;

use super::generated::rv_data;

/// Failure to load the installed cue-group document.
#[derive(Debug, thiserror::Error)]
pub enum GroupCatalogLoadError {
    /// The configured document could not be inspected or read.
    #[error("failed to read cue-group document at {path}: {source}")]
    Read {
        /// Configured document path.
        path: PathBuf,
        /// Filesystem failure.
        source: std::io::Error,
    },
    /// The configured path exists but is not a regular file.
    #[error("cue-group document path is not a regular file: {path}")]
    NotRegular {
        /// Configured document path.
        path: PathBuf,
    },
    /// The installed document is not valid native protobuf data.
    #[error("failed to decode cue-group document at {path}: {source}")]
    Decode {
        /// Configured document path.
        path: PathBuf,
        /// Protobuf failure.
        source: prost::DecodeError,
    },
    /// An installed group name cannot be used as an exact lookup identity.
    #[error("cue-group document at {path} contains invalid group name '{name}'")]
    InvalidName {
        /// Configured document path.
        path: PathBuf,
        /// Invalid installed group name.
        name: String,
    },
    /// An installed group lacks the UUID required for application identity.
    #[error("cue-group document at {path} has group '{name}' without a valid UUID")]
    MissingIdentity {
        /// Configured document path.
        path: PathBuf,
        /// Installed group name.
        name: String,
    },
    /// Two installed group names differ only by case.
    #[error("cue-group document at {path} contains ambiguous names '{first}' and '{duplicate}'")]
    DuplicateName {
        /// Configured document path.
        path: PathBuf,
        /// First installed spelling.
        first: String,
        /// Conflicting installed spelling.
        duplicate: String,
    },
}

/// Immutable exact-name catalog loaded from `Configuration/Groups`.
#[derive(Debug, Default)]
pub struct GroupCatalog {
    groups: HashMap<String, rv_data::Group>,
}

impl GroupCatalog {
    /// Load the installed document, or an empty catalog only when it is absent.
    pub fn load_optional(path: &Path) -> Result<Self, GroupCatalogLoadError> {
        let metadata = match std::fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(source) => {
                return Err(GroupCatalogLoadError::Read {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };
        if metadata.file_type().is_symlink() {
            return match std::fs::metadata(path) {
                Ok(target) if target.is_file() => Self::load_from(path),
                Ok(_) => Err(GroupCatalogLoadError::NotRegular {
                    path: path.to_path_buf(),
                }),
                Err(source) => Err(GroupCatalogLoadError::Read {
                    path: path.to_path_buf(),
                    source,
                }),
            };
        }
        if !metadata.is_file() {
            return Err(GroupCatalogLoadError::NotRegular {
                path: path.to_path_buf(),
            });
        }
        Self::load_from(path)
    }

    /// Load one explicit native cue-group document.
    pub fn load_from(path: &Path) -> Result<Self, GroupCatalogLoadError> {
        let data = std::fs::read(path).map_err(|source| GroupCatalogLoadError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let document = rv_data::ProGroupsDocument::decode(data.as_slice()).map_err(|source| {
            GroupCatalogLoadError::Decode {
                path: path.to_path_buf(),
                source,
            }
        })?;
        let mut groups = HashMap::new();
        let mut canonical_names = HashMap::<String, String>::new();
        for group in document.groups {
            let name = group.name.clone();
            let valid_name =
                !name.is_empty() && name.trim() == name && !name.chars().any(char::is_control);
            let valid_uuid = group
                .uuid
                .as_ref()
                .is_some_and(|uuid| uuid::Uuid::parse_str(&uuid.string).is_ok());
            if !valid_name {
                return Err(GroupCatalogLoadError::InvalidName {
                    path: path.to_path_buf(),
                    name,
                });
            }
            if !valid_uuid {
                return Err(GroupCatalogLoadError::MissingIdentity {
                    path: path.to_path_buf(),
                    name,
                });
            }
            let canonical = name.to_lowercase();
            if let Some(first) = canonical_names.insert(canonical, name.clone()) {
                return Err(GroupCatalogLoadError::DuplicateName {
                    path: path.to_path_buf(),
                    first,
                    duplicate: name,
                });
            }
            groups.insert(name, group);
        }
        Ok(Self { groups })
    }

    /// Return installed names in deterministic order.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        let mut names = self.groups.keys().map(String::as_str).collect::<Vec<_>>();
        names.sort_unstable();
        names
    }

    pub(crate) fn instantiate(&self, name: &str) -> Option<rv_data::Group> {
        let installed = self.groups.get(name)?;
        Some(rv_data::Group {
            uuid: Some(rv_data::Uuid {
                string: uuid::Uuid::new_v4().to_string(),
            }),
            name: installed.name.clone(),
            color: installed.color.clone(),
            hot_key: installed.hot_key.clone(),
            application_group_identifier: installed.uuid.clone(),
            application_group_name: installed.name.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn instantiated_group_preserves_installed_metadata_and_gets_local_uuid() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("Groups");
        let installed_uuid = uuid::Uuid::new_v4().to_string();
        let color = rv_data::Color {
            red: 0.1,
            green: 0.2,
            blue: 0.3,
            alpha: 1.0,
        };
        let document = rv_data::ProGroupsDocument {
            groups: vec![rv_data::Group {
                uuid: Some(rv_data::Uuid {
                    string: installed_uuid.clone(),
                }),
                name: "Verse 1".to_string(),
                color: Some(color.clone()),
                hot_key: Some(rv_data::HotKey {
                    code: 1,
                    control_identifier: "1".to_string(),
                }),
                application_group_identifier: None,
                application_group_name: String::new(),
            }],
        };
        std::fs::write(&path, document.encode_to_vec()).expect("write group document");

        let catalog = GroupCatalog::load_from(&path).expect("load group catalog");
        let group = catalog.instantiate("Verse 1").expect("installed group");

        assert_eq!(group.name, "Verse 1");
        assert_eq!(group.color, Some(color));
        assert_eq!(
            group
                .application_group_identifier
                .as_ref()
                .map(|uuid| uuid.string.as_str()),
            Some(installed_uuid.as_str())
        );
        assert_eq!(group.application_group_name, "Verse 1");
        assert_ne!(
            group.uuid.as_ref().map(|uuid| uuid.string.as_str()),
            Some(installed_uuid.as_str())
        );
    }

    #[test]
    fn duplicate_canonical_names_are_rejected() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("Groups");
        let groups = ["Verse", "verse"]
            .into_iter()
            .map(|name| rv_data::Group {
                uuid: Some(rv_data::Uuid {
                    string: uuid::Uuid::new_v4().to_string(),
                }),
                name: name.to_string(),
                ..rv_data::Group::default()
            })
            .collect();
        std::fs::write(&path, rv_data::ProGroupsDocument { groups }.encode_to_vec())
            .expect("write group document");

        assert!(matches!(
            GroupCatalog::load_from(&path),
            Err(GroupCatalogLoadError::DuplicateName { .. })
        ));
    }

    #[test]
    fn optional_catalog_distinguishes_absence_from_existing_directory() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let missing = directory.path().join("Groups");

        assert!(GroupCatalog::load_optional(&missing)
            .expect("missing catalog is empty")
            .names()
            .is_empty());
        assert!(matches!(
            GroupCatalog::load_optional(directory.path()),
            Err(GroupCatalogLoadError::NotRegular { .. })
        ));
    }
}
