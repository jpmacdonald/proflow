//! Checked configuration and installed assets for one build runtime.

use std::fmt;
use std::path::PathBuf;

use crate::bible::{BibleCorpusError, BibleVersion};
use crate::paths::BuildLocations;
use crate::project_config::ProjectConfig;
use crate::propresenter::background::{resolve_background_image, BackgroundImageError};
use crate::propresenter::macros::{MacroCache, MacroCacheLoadError};
use crate::propresenter::resolution::{inspect_slide_size, PresentationSizeError};
use crate::propresenter::theme::{ThemeCache, ThemeCacheLoadError, ThemeSlideError};
use crate::propresenter::PresentationSize;

/// Failure to capture one coherent project/configured-native-asset snapshot.
#[derive(Debug, thiserror::Error)]
pub enum RenderAssetSnapshotError {
    /// The configured theme could not be loaded from the snapshot's locations.
    #[error(transparent)]
    Theme(#[from] ThemeCacheLoadError),
    /// The native macro document could not be loaded from the snapshot's locations.
    #[error(transparent)]
    Macros(#[from] MacroCacheLoadError),
    /// One or more checked config bindings do not resolve in the installed assets.
    #[error(transparent)]
    Unresolved(#[from] RenderAssetIssues),
}

/// One invalid binding between checked project configuration and installed assets.
#[derive(Debug, thiserror::Error)]
pub enum RenderAssetIssue {
    /// A cue role could not resolve its configured theme slide.
    #[error("cue role '{role}' cannot use its configured theme slide: {source}")]
    ThemeSlide {
        /// Stable cue-role identifier.
        role: String,
        /// Native theme lookup failure.
        #[source]
        source: ThemeSlideError,
    },
    /// A semantic text field names no native graphics element on its theme slide.
    #[error("cue role '{role}' maps text field '{field}' to missing native slot '{native_slot}'")]
    MissingTextSlot {
        /// Stable cue-role identifier.
        role: String,
        /// Semantic field supplied by generated content.
        field: String,
        /// Exact native graphics-element name configured as its destination.
        native_slot: String,
    },
    /// A configured theme slide does not use the project output dimensions.
    #[error("cue role '{role}' theme slide '{slide}' has {problem}; expected {expected}")]
    ThemeSlideSize {
        /// Stable cue-role identifier.
        role: String,
        /// Exact installed theme-slide name.
        slide: String,
        /// Required project canvas dimensions.
        expected: PresentationSize,
        /// Concrete native size failure.
        problem: ThemeSlideSizeProblem,
    },
    /// A cue role names a macro absent from the installed macro document.
    #[error("cue role '{role}' references {field} '{name}' which is not installed")]
    MissingMacro {
        /// Stable cue-role identifier.
        role: String,
        /// Config field containing the macro name.
        field: &'static str,
        /// Exact configured macro name.
        name: String,
    },
    /// A configured background cannot be resolved inside the project data bundle.
    #[error("background '{id}' at '{}': {source}", path.display())]
    Background {
        /// Stable configured background identifier.
        id: String,
        /// Configured project-relative path.
        path: PathBuf,
        /// Image resolution or validation failure.
        #[source]
        source: BackgroundImageError,
    },
    /// An installed Bible corpus is malformed or duplicates another translation.
    #[error(transparent)]
    BibleCorpus(#[from] BibleCorpusError),
    /// The configured default Bible translation has no installed corpus.
    #[error("default Bible version '{version}' has no corpus at {}", path.display())]
    MissingBibleCorpus {
        /// Configured translation.
        version: &'static str,
        /// Expected project-data path.
        path: PathBuf,
    },
}

/// Concrete mismatch between a theme slide and the required project canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeSlideSizeProblem {
    /// The theme slide has a valid canvas of a different size.
    Mismatch(PresentationSize),
    /// The theme slide has no usable canvas dimensions.
    Invalid(PresentationSizeError),
}

impl fmt::Display for ThemeSlideSizeProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mismatch(actual) => write!(formatter, "canvas {actual}"),
            Self::Invalid(error) => write!(formatter, "invalid canvas ({error})"),
        }
    }
}

/// Deterministically ordered installed-asset issues collected in one validation pass.
#[derive(Debug)]
pub struct RenderAssetIssues {
    issues: Vec<RenderAssetIssue>,
}

impl RenderAssetIssues {
    /// Return every invalid installed-asset binding.
    #[must_use]
    pub fn issues(&self) -> &[RenderAssetIssue] {
        &self.issues
    }
}

impl fmt::Display for RenderAssetIssues {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("configured render assets are unavailable: ")?;
        for (index, issue) in self.issues.iter().enumerate() {
            if index != 0 {
                formatter.write_str("; ")?;
            }
            write!(formatter, "{issue}")?;
        }
        Ok(())
    }
}

impl std::error::Error for RenderAssetIssues {}

/// Immutable configuration and native render assets for one build runtime.
///
/// Construction proves every configured theme slide, text slot, macro,
/// background, and Bible corpus against one checked location snapshot. No host
/// layer has a second validation step to remember.
pub struct RenderAssetSnapshot {
    config: ProjectConfig,
    locations: BuildLocations,
    themes: ThemeCache,
    macros: MacroCache,
}

impl RenderAssetSnapshot {
    /// Load and validate one coherent snapshot from a checked config and location set.
    pub fn load(
        config: ProjectConfig,
        locations: BuildLocations,
    ) -> Result<Self, RenderAssetSnapshotError> {
        let themes =
            ThemeCache::load_from_dir(config.defaults().theme.as_deref(), locations.themes())?;
        let macros = MacroCache::load_optional(locations.macros())?;
        validate_bindings(&config, &locations, &themes, &macros)?;
        Ok(Self {
            config,
            locations,
            themes,
            macros,
        })
    }

    /// Checked project configuration that selected this snapshot's assets.
    #[must_use]
    pub const fn config(&self) -> &ProjectConfig {
        &self.config
    }

    /// Checked locations from which this snapshot was loaded.
    #[must_use]
    pub const fn locations(&self) -> &BuildLocations {
        &self.locations
    }

    pub(crate) const fn themes(&self) -> &ThemeCache {
        &self.themes
    }

    pub(crate) const fn macros(&self) -> &MacroCache {
        &self.macros
    }
}

fn validate_bindings(
    config: &ProjectConfig,
    locations: &BuildLocations,
    themes: &ThemeCache,
    macros: &MacroCache,
) -> Result<(), RenderAssetIssues> {
    let mut issues = Vec::new();
    validate_cue_roles(config, themes, macros, &mut issues);

    for (id, relative_path) in config.backgrounds() {
        if let Err(source) =
            resolve_background_image(locations.project_data_root(), relative_path.as_path())
        {
            issues.push(RenderAssetIssue::Background {
                id: id.to_string(),
                path: relative_path.as_path().to_path_buf(),
                source,
            });
        }
    }

    let bible_root = locations.project_data_root().join("bibles");
    if let Err(source) = crate::bible::validate_bible_corpora(&bible_root) {
        issues.push(RenderAssetIssue::BibleCorpus(source));
    }
    if let Some(version) = config.defaults().bible_version {
        let path = bible_root.join(version.file_name());
        if !path.is_file() {
            issues.push(RenderAssetIssue::MissingBibleCorpus {
                version: BibleVersion::name(version),
                path,
            });
        }
    }

    issues.sort_by_cached_key(ToString::to_string);
    if issues.is_empty() {
        Ok(())
    } else {
        Err(RenderAssetIssues { issues })
    }
}

fn validate_cue_roles(
    config: &ProjectConfig,
    themes: &ThemeCache,
    macros: &MacroCache,
    issues: &mut Vec<RenderAssetIssue>,
) {
    for (role_id, role) in config.cue_roles() {
        let resolved = if role.text_slots.is_empty() {
            themes.text_template(&role.slide).map(|slide| (slide, None))
        } else {
            themes
                .slide_template(&role.slide)
                .map(|template| (template.slide(), Some(template)))
        };

        match resolved {
            Ok((slide, template)) => {
                if let Some(template) = template {
                    for (field, native_slot) in &role.text_slots {
                        if !template.named_slots().any(|name| name == native_slot) {
                            issues.push(RenderAssetIssue::MissingTextSlot {
                                role: role_id.clone(),
                                field: field.clone(),
                                native_slot: native_slot.clone(),
                            });
                        }
                    }
                }
                let expected = config.defaults().presentation_size;
                match inspect_slide_size(slide) {
                    Ok(actual) if actual == expected => {}
                    Ok(actual) => issues.push(RenderAssetIssue::ThemeSlideSize {
                        role: role_id.clone(),
                        slide: role.slide.clone(),
                        expected,
                        problem: ThemeSlideSizeProblem::Mismatch(actual),
                    }),
                    Err(error) => issues.push(RenderAssetIssue::ThemeSlideSize {
                        role: role_id.clone(),
                        slide: role.slide.clone(),
                        expected,
                        problem: ThemeSlideSizeProblem::Invalid(error),
                    }),
                }
            }
            Err(source) => issues.push(RenderAssetIssue::ThemeSlide {
                role: role_id.clone(),
                source,
            }),
        }

        for (field, name) in [
            ("enter_macro", role.enter_macro.as_deref()),
            ("leader_enter_macro", role.leader_enter_macro.as_deref()),
        ] {
            if let Some(name) = name {
                if macros.find(name).is_none() {
                    issues.push(RenderAssetIssue::MissingMacro {
                        role: role_id.clone(),
                        field,
                        name: name.to_string(),
                    });
                }
            }
        }
    }
}
