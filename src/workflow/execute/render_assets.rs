//! Checked configuration and installed assets for one build runtime.

use std::fmt;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::bible::BibleCorpusError;
use crate::paths::BuildLocations;
use crate::project_config::ProjectConfig;
use crate::propresenter::audience::{
    AudienceDestinationError, AudienceLookDestinations, AudienceWorkspaceLoadError,
};
use crate::propresenter::background::BackgroundImageError;
use crate::propresenter::macros::{MacroCache, MacroCacheLoadError};
use crate::propresenter::render::TemplateSlotError;
use crate::propresenter::resolution::PresentationSizeError;
use crate::propresenter::theme::{ThemeCache, ThemeCacheLoadError, ThemeSlideError};
use crate::propresenter::PresentationSize;

mod audience;
mod fingerprint;
mod validation;

use audience::ConfiguredAudienceDestinations;
use validation::validate_bindings;

pub use fingerprint::{NativeAssetDigest, RenderAssetFingerprint, RenderAssetFingerprintError};

/// Failure to capture one coherent project/configured-native-asset snapshot.
#[derive(Debug, thiserror::Error)]
pub enum RenderAssetSnapshotError {
    /// The configured theme could not be loaded from the snapshot's locations.
    #[error(transparent)]
    Theme(#[from] ThemeCacheLoadError),
    /// The native macro document could not be loaded from the snapshot's locations.
    #[error(transparent)]
    Macros(#[from] MacroCacheLoadError),
    /// The native Workspace could not be loaded for configured macro Looks.
    #[error(transparent)]
    AudienceWorkspace(#[from] AudienceWorkspaceLoadError),
    /// Checked native asset identities could not be encoded canonically.
    #[error(transparent)]
    Fingerprint(#[from] RenderAssetFingerprintError),
    /// One or more checked config bindings do not resolve in the installed assets.
    #[error(transparent)]
    Unresolved(#[from] RenderAssetIssues),
}

/// A native asset used by the immutable render snapshot changed on disk.
///
/// `ProPresenter` executes macro and Look references against its live files, not
/// the cached protobuf objects used while `ProFlow` rendered a preview. Any drift
/// therefore invalidates the preview and requires a snapshot reload/review.
#[derive(Debug, thiserror::Error)]
pub enum RenderAssetFreshnessError {
    /// One captured native document can no longer be read exactly.
    #[error(
        "{kind} asset '{}' cannot be revalidated after preview; reload assets and review again: {source}",
        path.display()
    )]
    Read {
        /// Operator-facing native asset kind.
        kind: &'static str,
        /// Exact path captured by the render snapshot.
        path: PathBuf,
        /// Current filesystem failure.
        #[source]
        source: std::io::Error,
    },
    /// The document is readable but no longer contains the captured bytes.
    #[error(
        "{kind} asset '{}' changed after preview (expected SHA-256 {expected}, found {actual}); reload assets and review again",
        path.display()
    )]
    Changed {
        /// Operator-facing native asset kind.
        kind: &'static str,
        /// Exact path captured by the render snapshot.
        path: PathBuf,
        /// SHA-256 parsed into the immutable snapshot.
        expected: String,
        /// SHA-256 currently stored at the path.
        actual: String,
    },
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
    /// A production policy names a macro absent from the installed macro document.
    #[error("configured macro '{name}' is not installed")]
    MissingMacro {
        /// Exact configured macro name.
        name: String,
    },
    /// An installed cue-role macro does not select one valid Audience Look graph.
    #[error("configured macro '{name}' has no usable Audience Look destination: {source}")]
    AudienceDestination {
        /// Exact installed/configured macro name.
        name: String,
        /// Native macro-to-Look-to-screen-to-theme resolution failure.
        #[source]
        source: AudienceDestinationError,
    },
    /// A macro-selected audience theme cannot bind a configured role's text fields.
    #[error(
        "cue role '{role}' macro '{name}' cannot bind text on audience screen '{screen_name}' ({screen_uuid}) using theme '{}' slide {slide_uuid}: {source}",
        theme_path.display()
    )]
    AudienceTextBinding {
        /// Configured semantic cue role.
        role: String,
        /// Exact installed/configured macro name.
        name: String,
        /// Operator-visible audience-screen name.
        screen_name: String,
        /// Stable native audience-screen UUID.
        screen_uuid: String,
        /// Exact alternate theme document selected by the Audience Look.
        theme_path: PathBuf,
        /// Stable native theme-slide UUID.
        slide_uuid: String,
        /// Native template-field incompatibility.
        #[source]
        source: TemplateSlotError,
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
    audience_destinations: ConfiguredAudienceDestinations,
    fingerprint: RenderAssetFingerprint,
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
        let audience_destinations = validate_bindings(&config, &locations, &themes, &macros)?;
        let fingerprint =
            RenderAssetFingerprint::capture(&config, &themes, &macros, &audience_destinations)?;
        Ok(Self {
            config,
            locations,
            themes,
            macros,
            audience_destinations,
            fingerprint,
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

    /// Exact screen destinations selected by one configured installed macro.
    ///
    /// Absence means the name was not a configured, successfully resolved
    /// cue-role macro in this snapshot. Construction rejects configured macros
    /// whose native destination graph is invalid.
    pub fn audience_destinations_for_macro(
        &self,
        macro_name: &str,
    ) -> Option<&AudienceLookDestinations> {
        self.audience_destinations.for_macro(macro_name)
    }

    /// Content identity of the exact config, theme, and macro bytes parsed at startup.
    #[must_use]
    pub const fn fingerprint(&self) -> &RenderAssetFingerprint {
        &self.fingerprint
    }

    /// Re-hash every native document whose content can affect rendered cues or
    /// the live Audience Look selected by their macros.
    ///
    /// This is intentionally cheap and narrower than rebuilding the snapshot:
    /// the validated graph remains immutable, while exact source bytes prove
    /// that the graph and templates are still current. The project-config file
    /// itself is not watched: this snapshot owns the parsed [`ProjectConfig`]
    /// value, active builds never reread its source file, and config edits take
    /// effect only when the runtime constructs a new snapshot.
    pub fn verify_current(&self) -> Result<(), RenderAssetFreshnessError> {
        if let Some((path, digest)) = self.themes.source_document() {
            verify_native_source("theme", path, digest)?;
        }
        if let Some((path, digest)) = self.macros.source_document() {
            verify_native_source("macros", path, digest)?;
        }
        if let Some((path, digest)) = self.audience_destinations.workspace_source() {
            verify_native_source("audience workspace", path, digest)?;
        }
        for (path, digest) in self.audience_destinations.theme_sources() {
            verify_native_source("audience theme", path, digest)?;
        }
        Ok(())
    }
}

fn verify_native_source(
    kind: &'static str,
    path: &Path,
    expected: [u8; 32],
) -> Result<(), RenderAssetFreshnessError> {
    let bytes = std::fs::read(path).map_err(|source| RenderAssetFreshnessError::Read {
        kind,
        path: path.to_path_buf(),
        source,
    })?;
    let actual: [u8; 32] = Sha256::digest(bytes).into();
    if actual == expected {
        return Ok(());
    }
    Err(RenderAssetFreshnessError::Changed {
        kind,
        path: path.to_path_buf(),
        expected: digest_hex(&expected),
        actual: digest_hex(&actual),
    })
}

fn digest_hex(digest: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}
