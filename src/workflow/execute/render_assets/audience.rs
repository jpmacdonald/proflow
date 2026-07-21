//! Configured macro destinations captured from one native Workspace snapshot.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::paths::BuildLocations;
use crate::project_config::ProjectConfig;
use crate::propresenter::audience::{
    AudienceDestinationResolver, AudienceLookDestinations, AudienceWorkspaceLoadError,
};
use crate::propresenter::macros::MacroCache;

use super::RenderAssetIssue;

/// Exact Audience Look destinations for every installed cue-role macro in use.
#[derive(Debug, Default)]
pub(super) struct ConfiguredAudienceDestinations {
    by_macro: BTreeMap<String, AudienceLookDestinations>,
    workspace_source: Option<(PathBuf, [u8; 32])>,
    theme_sources: Vec<(PathBuf, [u8; 32])>,
}

impl ConfiguredAudienceDestinations {
    pub(super) fn capture(
        config: &ProjectConfig,
        locations: &BuildLocations,
        macros: &MacroCache,
        issues: &mut Vec<RenderAssetIssue>,
    ) -> Result<Self, AudienceWorkspaceLoadError> {
        let names = config.referenced_macro_names();
        for name in &names {
            if macros.native(name).is_none() {
                issues.push(RenderAssetIssue::MissingMacro {
                    name: (*name).to_string(),
                });
            }
        }
        if names.iter().all(|name| macros.native(name).is_none()) {
            return Ok(Self::default());
        }

        let mut resolver = AudienceDestinationResolver::load(
            locations.workspace(),
            locations.propresenter_root(),
        )?;
        let mut by_macro = BTreeMap::new();
        for name in names {
            let Some(native) = macros.native(name) else {
                continue;
            };
            match resolver.resolve_macro(native) {
                Ok(destinations) => {
                    by_macro.insert(name.to_string(), destinations);
                }
                Err(source) => issues.push(RenderAssetIssue::AudienceDestination {
                    name: name.to_string(),
                    source,
                }),
            }
        }

        let workspace_source = resolver
            .source_document()
            .map(|(path, digest)| (path.to_path_buf(), digest));
        let mut theme_sources = resolver
            .theme_documents()
            .map(|(path, digest)| (path.to_path_buf(), digest))
            .collect::<Vec<_>>();
        theme_sources.sort_unstable_by(|left, right| left.0.cmp(&right.0));
        Ok(Self {
            by_macro,
            workspace_source,
            theme_sources,
        })
    }

    pub(super) fn for_macro(&self, name: &str) -> Option<&AudienceLookDestinations> {
        self.by_macro.get(name)
    }

    pub(super) fn workspace_source(&self) -> Option<(&Path, [u8; 32])> {
        self.workspace_source
            .as_ref()
            .map(|(path, digest)| (path.as_path(), *digest))
    }

    pub(super) fn theme_sources(&self) -> impl Iterator<Item = (&Path, [u8; 32])> {
        self.theme_sources
            .iter()
            .map(|(path, digest)| (path.as_path(), *digest))
    }
}

#[cfg(test)]
#[path = "audience/tests.rs"]
mod tests;
