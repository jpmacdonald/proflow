use std::collections::HashMap;
use std::path::Path;

use prost::Message;
use sha2::{Digest, Sha256};

use super::{AudienceDestinationResolver, AudienceWorkspaceLoadError};
use crate::propresenter::generated::rv_data::ProPresenterWorkspace;

impl AudienceDestinationResolver {
    /// Read and compile one exact native workspace.
    ///
    /// `show_root` is the `ProPresenter` root that owns `Configuration`,
    /// `Themes`, and `Libraries`. It is explicit so stale absolute native URLs
    /// can be resolved against the workspace currently under review.
    pub(crate) fn load(
        workspace_path: &Path,
        show_root: &Path,
    ) -> Result<Self, AudienceWorkspaceLoadError> {
        let metadata = std::fs::metadata(workspace_path).map_err(|source| {
            AudienceWorkspaceLoadError::Read {
                path: workspace_path.to_path_buf(),
                source,
            }
        })?;
        if !metadata.is_file() {
            return Err(AudienceWorkspaceLoadError::NotRegular {
                path: workspace_path.to_path_buf(),
            });
        }
        let bytes =
            std::fs::read(workspace_path).map_err(|source| AudienceWorkspaceLoadError::Read {
                path: workspace_path.to_path_buf(),
                source,
            })?;
        let workspace = ProPresenterWorkspace::decode(bytes.as_slice()).map_err(|source| {
            AudienceWorkspaceLoadError::Decode {
                path: workspace_path.to_path_buf(),
                source,
            }
        })?;
        Ok(Self {
            workspace,
            show_root: show_root.to_path_buf(),
            themes: HashMap::new(),
            source_path: Some(workspace_path.to_path_buf()),
            source_sha256: Some(Sha256::digest(&bytes).into()),
        })
    }

    /// Compile one already-decoded workspace without mutating it.
    #[cfg(test)]
    pub(crate) fn from_workspace(workspace: ProPresenterWorkspace, show_root: &Path) -> Self {
        Self {
            workspace,
            show_root: show_root.to_path_buf(),
            themes: HashMap::new(),
            source_path: None,
            source_sha256: None,
        }
    }
}
