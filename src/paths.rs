//! Checked runtime locations for one `ProFlow` process.
//!
//! Environment and workstation discovery happen once at startup. Workflow and
//! rendering code receive this value instead of consulting ambient process
//! state or falling back to the current directory.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::project_config::LibraryName;

/// Project config filename stored under the data directory.
pub const PROJECT_CONFIG_FILE: &str = "proflow.config.json";

/// Every filesystem location used by a service build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildLocations {
    project_data_root: PathBuf,
    project_config: PathBuf,
    presentation_library: PathBuf,
    playlist_output: PathBuf,
    propresenter_root: PathBuf,
    themes: PathBuf,
    macros: PathBuf,
}

/// Unchecked path inputs supplied by discovery, tests, or diagnostic tools.
///
/// Keeping these values in one record makes the transition into
/// [`BuildLocations`] explicit without a positional constructor whose paths
/// are easy to transpose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildLocationInputs {
    /// Root of the portable `ProFlow` data bundle.
    pub project_data_root: PathBuf,
    /// Selected `ProPresenter` library used for matching and canonical writes.
    pub presentation_library: PathBuf,
    /// Destination directory for generated playlist packages.
    pub playlist_output: PathBuf,
    /// `ProPresenter` user-data root.
    pub propresenter_root: PathBuf,
    /// Exact Themes directory selected for this process.
    pub themes: PathBuf,
    /// Exact native macro document selected for this process.
    pub macros: PathBuf,
}

/// Failure to resolve a complete set of runtime locations.
#[derive(Debug, thiserror::Error)]
pub enum BuildLocationsError {
    /// A required directory is missing or is not a directory.
    #[error("{name} is not a directory: {}", path.display())]
    NotDirectory {
        /// Operator-facing location name.
        name: &'static str,
        /// Invalid path.
        path: PathBuf,
    },
    /// An output location exists as a non-directory file.
    #[error("{name} exists but is not a directory: {}", path.display())]
    OutputIsFile {
        /// Operator-facing location name.
        name: &'static str,
        /// Invalid path.
        path: PathBuf,
    },
    /// An environment path points at a different show than `ProPresenter` has active.
    #[error(
        "configured ProPresenter root {} conflicts with the active macOS ProPresenter show {}",
        configured.display(),
        active.display()
    )]
    ConflictingActiveProPresenterRoot {
        /// Root supplied by `PROPRESENTER_DIR`.
        configured: PathBuf,
        /// Show directory reported by the `ProPresenter` macOS preference.
        active: PathBuf,
    },
    /// macOS could not start the system preference reader.
    #[error("could not read the active ProPresenter show preference: {source}")]
    ActiveProPresenterShowRead {
        /// Failure returned while launching `/usr/bin/defaults`.
        #[source]
        source: std::io::Error,
    },
    /// The macOS preference reader failed for a reason other than an absent key.
    #[error("could not read the active ProPresenter show preference ({status}): {diagnostic}")]
    ActiveProPresenterShowReadFailed {
        /// Exit status reported by `/usr/bin/defaults`.
        status: String,
        /// Diagnostic reported by `/usr/bin/defaults`.
        diagnostic: String,
    },
    /// The macOS active-show preference was present but not a usable path.
    #[error("active ProPresenter show preference returned malformed output: {reason}")]
    MalformedActiveProPresenterShow {
        /// Stable explanation of the malformed command output.
        reason: &'static str,
    },
}

impl BuildLocations {
    /// Discover only the portable project-data root.
    ///
    /// Diagnostic workflows that supply an explicit `ProPresenter` root and
    /// shadow output directories can use this without consulting a second set
    /// of ambient path helpers.
    pub fn discover_project_data_root() -> Result<PathBuf, BuildLocationsError> {
        let root = discovered_data_root();
        require_directory("project data root", &root)?;
        Ok(root)
    }

    /// Discover and validate one immutable workstation snapshot.
    pub fn discover(library_name: &LibraryName) -> Result<Self, BuildLocationsError> {
        let project_data_root = Self::discover_project_data_root()?;
        let configured_root = env_path("PROPRESENTER_DIR");
        let active_root = active_macos_show_directory()?;
        let propresenter_root =
            select_propresenter_root(configured_root, active_root, default_propresenter_root())?;
        let presentation_library = propresenter_root
            .join("Libraries")
            .join(library_name.as_str());
        let playlist_output = env_path("PLAYLIST_DIR")
            .unwrap_or_else(|| default_playlist_output_dir(&propresenter_root));
        let themes = env_path("THEMES_DIR").unwrap_or_else(|| propresenter_root.join("Themes"));
        let macros = propresenter_root.join("Configuration/Macros");

        Self::from_inputs(BuildLocationInputs {
            project_data_root,
            presentation_library,
            playlist_output,
            propresenter_root,
            themes,
            macros,
        })
    }

    /// Construct checked locations from explicit paths.
    ///
    /// This is the non-ambient boundary used by tests and diagnostic tools.
    pub fn from_inputs(inputs: BuildLocationInputs) -> Result<Self, BuildLocationsError> {
        require_directory("project data root", &inputs.project_data_root)?;
        require_directory("presentation library", &inputs.presentation_library)?;
        reject_file_output("playlist output directory", &inputs.playlist_output)?;
        require_directory("ProPresenter data root", &inputs.propresenter_root)?;

        Ok(Self {
            project_config: inputs.project_data_root.join(PROJECT_CONFIG_FILE),
            project_data_root: inputs.project_data_root,
            presentation_library: inputs.presentation_library,
            playlist_output: inputs.playlist_output,
            propresenter_root: inputs.propresenter_root,
            themes: inputs.themes,
            macros: inputs.macros,
        })
    }

    /// Root of the portable project data bundle.
    pub fn project_data_root(&self) -> &Path {
        &self.project_data_root
    }

    /// Active project configuration file.
    pub fn project_config(&self) -> &Path {
        &self.project_config
    }

    /// Selected `ProPresenter` presentation library used for matching and writes.
    pub fn presentation_library(&self) -> &Path {
        &self.presentation_library
    }

    /// Destination directory for generated playlist packages.
    pub fn playlist_output(&self) -> &Path {
        &self.playlist_output
    }

    /// `ProPresenter` user-data root containing Configuration, Themes, etc.
    pub fn propresenter_root(&self) -> &Path {
        &self.propresenter_root
    }

    /// Exact Themes directory selected for this process.
    pub fn themes(&self) -> &Path {
        &self.themes
    }

    /// Exact native macro document selected for this process.
    pub fn macros(&self) -> &Path {
        &self.macros
    }

    /// Installed native cue-group document for this `ProPresenter` snapshot.
    #[must_use]
    pub fn groups(&self) -> PathBuf {
        self.propresenter_root.join("Configuration/Groups")
    }
}

fn require_directory(name: &'static str, path: &Path) -> Result<(), BuildLocationsError> {
    if path.is_dir() {
        Ok(())
    } else {
        Err(BuildLocationsError::NotDirectory {
            name,
            path: path.to_path_buf(),
        })
    }
}

fn reject_file_output(name: &'static str, path: &Path) -> Result<(), BuildLocationsError> {
    if path.exists() && !path.is_dir() {
        Err(BuildLocationsError::OutputIsFile {
            name,
            path: path.to_path_buf(),
        })
    } else {
        Ok(())
    }
}

fn select_propresenter_root(
    configured: Option<PathBuf>,
    active: Option<PathBuf>,
    fallback: PathBuf,
) -> Result<PathBuf, BuildLocationsError> {
    if let (Some(configured), Some(active)) = (&configured, &active) {
        if !same_existing_path(configured, active) {
            return Err(BuildLocationsError::ConflictingActiveProPresenterRoot {
                configured: configured.clone(),
                active: active.clone(),
            });
        }
    }
    Ok(active.or(configured).unwrap_or(fallback))
}

fn default_propresenter_root() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join("Documents/ProPresenter")
}

#[cfg(target_os = "macos")]
fn active_macos_show_directory() -> Result<Option<PathBuf>, BuildLocationsError> {
    let output = std::process::Command::new("/usr/bin/defaults")
        .args([
            "read",
            "com.renewedvision.propresenter",
            "applicationShowDirectory",
        ])
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .output()
        .map_err(|source| BuildLocationsError::ActiveProPresenterShowRead { source })?;

    interpret_active_show_preference(
        output.status.success(),
        output.status.to_string(),
        &output.stdout,
        &output.stderr,
    )
}

#[cfg(not(target_os = "macos"))]
const fn active_macos_show_directory() -> Result<Option<PathBuf>, BuildLocationsError> {
    Ok(None)
}

const MISSING_ACTIVE_SHOW_PREFERENCE: &str = "The domain/default pair of (com.renewedvision.propresenter, applicationShowDirectory) does not exist";

fn interpret_active_show_preference(
    success: bool,
    status: String,
    stdout: &[u8],
    stderr: &[u8],
) -> Result<Option<PathBuf>, BuildLocationsError> {
    if !success {
        let diagnostic = std::str::from_utf8(stderr).map_err(|_| {
            BuildLocationsError::MalformedActiveProPresenterShow {
                reason: "failure diagnostic is not UTF-8",
            }
        })?;
        if stdout.iter().all(u8::is_ascii_whitespace)
            && diagnostic.trim().ends_with(MISSING_ACTIVE_SHOW_PREFERENCE)
        {
            return Ok(None);
        }
        return Err(BuildLocationsError::ActiveProPresenterShowReadFailed {
            status,
            diagnostic: diagnostic.trim().to_owned(),
        });
    }

    let value = std::str::from_utf8(stdout).map_err(|_| {
        BuildLocationsError::MalformedActiveProPresenterShow {
            reason: "preference value is not UTF-8",
        }
    })?;
    let value = value.trim();
    if value.is_empty() {
        return Err(BuildLocationsError::MalformedActiveProPresenterShow {
            reason: "preference value is empty",
        });
    }
    if value.chars().any(char::is_control) {
        return Err(BuildLocationsError::MalformedActiveProPresenterShow {
            reason: "preference value contains control characters",
        });
    }
    Ok(Some(expand_user_path(value)))
}

fn default_playlist_output_dir(propresenter_root: &Path) -> PathBuf {
    propresenter_root.join("Playlists/ProFlow")
}

fn env_path(name: &str) -> Option<PathBuf> {
    let value = std::env::var_os(name)?;
    if value.is_empty() || value.to_string_lossy().trim().is_empty() {
        return None;
    }
    Some(expand_user_path(&value))
}

fn same_existing_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

/// Expand one leading `~` against the current user's home directory.
///
/// Other path syntax is preserved verbatim; canonicalization belongs to the
/// boundary that knows whether the path is an input or output.
pub fn expand_user_path(value: impl AsRef<OsStr>) -> PathBuf {
    let path = PathBuf::from(value.as_ref());
    let Ok(relative) = path.strip_prefix("~") else {
        return path;
    };
    dirs::home_dir().map_or_else(|| path.clone(), |home| home.join(relative))
}

/// Resolve an existing path or a potentially absent output to one physical
/// comparison identity.
///
/// `Path::canonicalize` cannot resolve an absent leaf. Resolving the nearest
/// existing ancestor and appending the normalized suffix gives callers one
/// representation for relative paths, parent components, and symlinked
/// directories without requiring the target itself to exist.
pub(crate) fn physical_path_identity(path: &Path) -> std::io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut ancestor = absolute.as_path();
    loop {
        match ancestor.canonicalize() {
            Ok(canonical_ancestor) => {
                let suffix = absolute
                    .strip_prefix(ancestor)
                    .map_err(|error| std::io::Error::other(error.to_string()))?;
                return Ok(append_normalized_suffix(canonical_ancestor, suffix));
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                ancestor = ancestor.parent().ok_or(source)?;
            }
            Err(source) => return Err(source),
        }
    }
}

fn append_normalized_suffix(mut base: PathBuf, suffix: &Path) -> PathBuf {
    for component in suffix.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                base.pop();
            }
            std::path::Component::Normal(component) => base.push(component),
            std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                base.push(component.as_os_str());
            }
        }
    }
    base
}

fn discovered_data_root() -> PathBuf {
    if let Some(base) = env_path("PROFLOW_DATA") {
        return base;
    }

    let workspace_data = PathBuf::from("data");
    if Path::new("Cargo.toml").is_file() && workspace_data.is_dir() {
        return workspace_data;
    }

    let mut candidates = Vec::new();
    if let Some(data) = dirs::data_dir() {
        candidates.push(data.join("proflow"));
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(directory) = executable.parent() {
            candidates.push(directory.join("data"));
        }
    }
    candidates.push(workspace_data);

    candidates
        .into_iter()
        .find(|path| path.is_dir())
        .unwrap_or_else(|| PathBuf::from("data"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn explicit_locations_are_one_checked_snapshot() {
        let root = tempfile::tempdir().expect("temporary root");
        let data = root.path().join("data");
        let library = root.path().join("ProPresenter/Libraries/Default");
        let propresenter = root.path().join("ProPresenter");
        std::fs::create_dir_all(&data).expect("data directory");
        std::fs::create_dir_all(&library).expect("library directory");

        let locations = BuildLocations::from_inputs(BuildLocationInputs {
            project_data_root: data.clone(),
            presentation_library: library.clone(),
            playlist_output: root.path().join("playlists"),
            propresenter_root: propresenter.clone(),
            themes: propresenter.join("Themes"),
            macros: propresenter.join("Configuration/Macros"),
        })
        .expect("checked locations");

        assert_eq!(locations.project_data_root(), data);
        assert_eq!(locations.project_config(), data.join(PROJECT_CONFIG_FILE));
        assert_eq!(locations.presentation_library(), library);
        assert_eq!(locations.propresenter_root(), propresenter);
    }

    #[test]
    fn output_path_cannot_be_an_existing_file() {
        let root = tempfile::tempdir().expect("temporary root");
        let data = root.path().join("data");
        let library = root.path().join("ProPresenter/Libraries/Default");
        let propresenter = root.path().join("ProPresenter");
        let output_file = root.path().join("playlist-output");
        std::fs::create_dir_all(&data).expect("data directory");
        std::fs::create_dir_all(&library).expect("library directory");
        std::fs::write(&output_file, b"not a directory").expect("output fixture");

        let error = BuildLocations::from_inputs(BuildLocationInputs {
            project_data_root: data,
            presentation_library: library,
            playlist_output: output_file.clone(),
            propresenter_root: propresenter.clone(),
            themes: propresenter.join("Themes"),
            macros: propresenter.join("Configuration/Macros"),
        })
        .expect_err("file output must fail");

        assert!(matches!(
            error,
            BuildLocationsError::OutputIsFile { path, .. } if path == output_file
        ));
    }

    #[test]
    fn playlists_default_to_a_dedicated_directory_outside_the_presentation_library() {
        let root = Path::new("/show/ProPresenter");

        assert_eq!(
            default_playlist_output_dir(root),
            PathBuf::from("/show/ProPresenter/Playlists/ProFlow")
        );
    }

    #[test]
    fn physical_identity_normalizes_absent_paths_and_parent_components() {
        let root = tempfile::tempdir().expect("temporary root");
        let library = root.path().join("library");
        std::fs::create_dir(&library).expect("library directory");

        let identity = physical_path_identity(&library.join("nested/../Song.pro"))
            .expect("resolve absent target");

        assert_eq!(
            identity,
            library
                .canonicalize()
                .expect("canonical library")
                .join("Song.pro")
        );
    }

    #[test]
    fn active_show_is_authoritative_and_rejects_a_different_configured_root() {
        let root = tempfile::tempdir().expect("temporary root");
        let configured = root.path().join("configured");
        let active = root.path().join("active");
        std::fs::create_dir(&configured).expect("configured show");
        std::fs::create_dir(&active).expect("active show");

        let error = select_propresenter_root(
            Some(configured.clone()),
            Some(active.clone()),
            root.path().join("fallback"),
        )
        .expect_err("a stale configured clone must not override the active show");

        assert!(matches!(
            error,
            BuildLocationsError::ConflictingActiveProPresenterRoot {
                configured: error_configured,
                active: error_active,
            } if error_configured == configured && error_active == active
        ));
        assert_eq!(
            select_propresenter_root(None, Some(active.clone()), root.path().join("fallback"))
                .expect("active show is sufficient"),
            active
        );
    }

    #[test]
    fn active_show_preference_accepts_one_valid_path() {
        let active = interpret_active_show_preference(
            true,
            "exit status: 0".to_owned(),
            b"/Users/example/Documents/ProPresenter\n",
            b"",
        )
        .expect("valid preference output");

        assert_eq!(
            active,
            Some(PathBuf::from("/Users/example/Documents/ProPresenter"))
        );
    }

    #[test]
    fn absent_active_show_preference_is_the_only_failed_read_that_allows_fallback() {
        let missing = format!("defaults diagnostic prefix\n{MISSING_ACTIVE_SHOW_PREFERENCE}\n");
        let active = interpret_active_show_preference(
            false,
            "exit status: 1".to_owned(),
            b"",
            missing.as_bytes(),
        )
        .expect("an absent preference is not a command failure");

        assert_eq!(active, None);

        let error = interpret_active_show_preference(
            false,
            "exit status: 1".to_owned(),
            b"unexpected output",
            missing.as_bytes(),
        )
        .expect_err("unexpected output must not be mistaken for an absent preference");
        assert!(matches!(
            error,
            BuildLocationsError::ActiveProPresenterShowReadFailed { .. }
        ));
    }

    #[test]
    fn active_show_preference_read_failure_is_typed_and_fails_closed() {
        let error = interpret_active_show_preference(
            false,
            "exit status: 70".to_owned(),
            b"",
            b"preferences service unavailable\n",
        )
        .expect_err("an unknown preference failure must stop discovery");

        assert!(matches!(
            error,
            BuildLocationsError::ActiveProPresenterShowReadFailed {
                status,
                diagnostic,
            } if status == "exit status: 70" && diagnostic == "preferences service unavailable"
        ));
    }

    #[test]
    fn malformed_active_show_preference_is_typed_and_fails_closed() {
        for (value, expected_reason) in [
            (b"".as_slice(), "preference value is empty"),
            (
                b"/Users/example/Documents/ProPresenter\0unexpected".as_slice(),
                "preference value contains control characters",
            ),
            (b"\xff".as_slice(), "preference value is not UTF-8"),
        ] {
            let error =
                interpret_active_show_preference(true, "exit status: 0".to_owned(), value, b"")
                    .expect_err("malformed preference output must stop discovery");

            assert!(matches!(
                error,
                BuildLocationsError::MalformedActiveProPresenterShow { reason }
                    if reason == expected_reason
            ));
        }
    }

    #[cfg(unix)]
    #[test]
    fn physical_identity_resolves_symlinked_ancestors() {
        let root = tempfile::tempdir().expect("temporary root");
        let physical = root.path().join("physical");
        let alias = root.path().join("alias");
        std::fs::create_dir(&physical).expect("physical directory");
        std::os::unix::fs::symlink(&physical, &alias).expect("directory alias");

        assert_eq!(
            physical_path_identity(&alias.join("Song.pro")).expect("resolve alias"),
            physical
                .canonicalize()
                .expect("canonical physical directory")
                .join("Song.pro")
        );
    }
}
