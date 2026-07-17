//! In-memory catalog of native `ProPresenter` library presentations.
//!
//! The catalog is rebuilt from native files at startup. Workflow-generated
//! presentations are decoded from exact sealed bytes before commit, then
//! installed as a new catalog snapshot only after the filesystem transaction
//! succeeds.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use prost::Message;
use serde::Serialize;
use thiserror::Error;
use walkdir::WalkDir;

use crate::paths::physical_path_identity;
use crate::propresenter::deserialize::{detect_presentation_file_format, PresentationFileFormat};
use crate::propresenter::generated::rv_data;
use crate::propresenter::resolution::inspect_presentation_size;
use crate::propresenter::PresentationSizeStatus;

mod search;
pub use search::normalize_name;

#[cfg(test)]
mod tests;

/// Failures while building or immutably updating a presentation catalog.
#[derive(Debug, Error)]
pub enum LibraryCatalogError {
    /// The configured library root is absent or is not a directory.
    #[error("presentation library is not a directory: {}", path.display())]
    NotDirectory {
        /// Configured catalog root.
        path: PathBuf,
    },
    /// The configured root or a candidate presentation could not be resolved
    /// to one physical path identity.
    #[error("failed to resolve presentation path '{}': {source}", path.display())]
    ResolvePath {
        /// Path whose identity could not be resolved.
        path: PathBuf,
        /// Filesystem resolution failure.
        #[source]
        source: std::io::Error,
    },
    /// Walking the configured library failed.
    #[error("failed to traverse presentation library '{}': {source}", path.display())]
    Traverse {
        /// Configured catalog root.
        path: PathBuf,
        /// Directory traversal failure.
        #[source]
        source: walkdir::Error,
    },
    /// Reading a presentation failed.
    #[error("failed to read presentation '{}': {source}", path.display())]
    Read {
        /// Presentation that could not be read.
        path: PathBuf,
        /// Filesystem read failure.
        #[source]
        source: std::io::Error,
    },
    /// A `.pro` path did not contain a UTF-8 filename.
    #[error("presentation filename is not valid UTF-8: {}", path.display())]
    InvalidFilename {
        /// Presentation with a non-UTF-8 filename.
        path: PathBuf,
    },
    /// Exact bytes were not a native presentation document.
    #[error("presentation '{}' is not a native presentation ({format})", path.display())]
    UnsupportedFormat {
        /// Presentation target associated with the exact bytes.
        path: PathBuf,
        /// Detected on-disk content format.
        format: PresentationFileFormat,
    },
    /// Native bytes could not be decoded after format detection.
    #[error("failed to decode native presentation '{}': {source}", path.display())]
    Decode {
        /// Native presentation that could not be decoded.
        path: PathBuf,
        /// Protobuf decode failure.
        #[source]
        source: prost::DecodeError,
    },
    /// A requested replacement is outside the catalog root.
    #[error(
        "presentation '{}' is outside library '{}'",
        path.display(),
        library_path.display()
    )]
    OutsideLibrary {
        /// Requested presentation path.
        path: PathBuf,
        /// Root owned by this catalog.
        library_path: PathBuf,
    },
    /// A prepared replacement belongs to a different catalog root.
    #[error("prepared presentation update belongs to another library: {}", path.display())]
    ForeignUpdate {
        /// Prepared presentation path.
        path: PathBuf,
    },
    /// More than one replacement targeted the same presentation.
    #[error("duplicate prepared presentation update: {}", path.display())]
    DuplicateUpdate {
        /// Duplicated presentation path.
        path: PathBuf,
    },
}

type Result<T> = std::result::Result<T, LibraryCatalogError>;

/// Metadata for one native presentation in a [`LibraryCatalog`].
#[derive(Debug, Clone, Serialize)]
pub struct LibraryEntry {
    /// Original filename without extension.
    file_name: String,
    /// Name after stripping common prefixes and numbering.
    normalized_name: String,
    /// Lowercase filename used only for matching.
    #[serde(skip)]
    file_name_lower: String,
    /// Lowercase normalized name used only for matching.
    #[serde(skip)]
    normalized_lower: String,
    /// Human-readable display name.
    display_name: String,
    /// Path relative to the library root.
    relative_path: String,
    /// Absolute or configured-root-relative path on disk.
    full_path: PathBuf,
    /// Named native arrangements available for playlist selection.
    arrangements: Vec<LibraryArrangement>,
    /// Uniformity and dimensions of native presentation slides.
    presentation_size: PresentationSizeStatus,
}

impl LibraryEntry {
    /// Original filename without its extension.
    #[must_use]
    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    /// Filename after removing common library prefixes and numbering.
    #[must_use]
    pub fn normalized_name(&self) -> &str {
        &self.normalized_name
    }

    /// Human-readable title presented to an operator.
    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Path relative to the configured library root.
    #[must_use]
    pub fn relative_path(&self) -> &str {
        &self.relative_path
    }

    /// Exact presentation path used for playlist references.
    #[must_use]
    pub fn full_path(&self) -> &Path {
        &self.full_path
    }

    /// Native arrangements available for playlist selection.
    #[must_use]
    pub fn arrangements(&self) -> &[LibraryArrangement] {
        &self.arrangements
    }

    /// Uniformity and dimensions of the native slide canvas.
    #[must_use]
    pub const fn presentation_size(&self) -> PresentationSizeStatus {
        self.presentation_size
    }
}

/// Native arrangement metadata needed before a playlist build is approved.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum LibraryArrangement {
    /// Arrangement can be selected by its exact native name.
    Complete {
        /// Exact native arrangement name.
        name: String,
    },
    /// Arrangement has an empty name or a missing/malformed UUID.
    Incomplete {
        /// Exact native arrangement name, possibly empty.
        name: String,
    },
}

impl LibraryArrangement {
    /// Exact arrangement name stored in the native presentation.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Complete { name } | Self::Incomplete { name } => name,
        }
    }

    /// Whether the arrangement carries the identity needed for selection.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        matches!(self, Self::Complete { .. })
    }
}

/// An immutable snapshot of native presentations available to the workflow.
#[derive(Debug, Clone)]
pub struct LibraryCatalog {
    entries: Vec<LibraryEntry>,
    library_path: PathBuf,
}

/// A decoded catalog replacement prepared from exact bytes awaiting commit.
#[derive(Debug, Clone)]
pub(crate) struct PreparedLibraryUpdate {
    library_path: PathBuf,
    entry: LibraryEntry,
}

impl LibraryCatalog {
    /// Build a catalog by decoding every native `.pro` file under `library_path`.
    pub fn build(library_path: &Path) -> Result<Self> {
        if !library_path.is_dir() {
            return Err(LibraryCatalogError::NotDirectory {
                path: library_path.to_path_buf(),
            });
        }
        let library_path = physical_path_identity(library_path).map_err(|source| {
            LibraryCatalogError::ResolvePath {
                path: library_path.to_path_buf(),
                source,
            }
        })?;

        let started = Instant::now();
        let mut entries = Vec::new();
        for directory_entry in WalkDir::new(&library_path).follow_links(false) {
            let directory_entry =
                directory_entry.map_err(|source| LibraryCatalogError::Traverse {
                    path: library_path.clone(),
                    source,
                })?;
            if !directory_entry.file_type().is_file() || !is_pro_path(directory_entry.path()) {
                continue;
            }

            let path = directory_entry.path();
            let bytes = std::fs::read(path).map_err(|source| LibraryCatalogError::Read {
                path: path.to_path_buf(),
                source,
            })?;
            let format = detect_presentation_file_format(&bytes);
            if format != PresentationFileFormat::NativePresentation {
                tracing::warn!(
                    path = %path.display(),
                    %format,
                    "excluding unsupported .pro file from presentation catalog"
                );
                continue;
            }
            entries.push(entry_from_bytes(&library_path, path, &bytes)?);
        }

        tracing::info!(
            count = entries.len(),
            elapsed = ?started.elapsed(),
            "cataloged native presentations"
        );
        Ok(Self {
            entries,
            library_path,
        })
    }

    /// All native presentations in this catalog snapshot.
    #[must_use]
    pub fn entries(&self) -> &[LibraryEntry] {
        &self.entries
    }

    /// Metadata for one exact presentation path.
    #[must_use]
    pub fn entry_at(&self, path: &Path) -> Option<&LibraryEntry> {
        let identity = physical_path_identity(path).ok()?;
        self.entries
            .iter()
            .find(|entry| entry.full_path == identity)
    }

    /// Decode a future entry when its physical target belongs to this catalog.
    ///
    /// Outputs outside the library are valid build artifacts and return
    /// `None`; they must not become shadow catalog state.
    pub(crate) fn prepare_owned_update(
        &self,
        full_path: &Path,
        exact_bytes: &[u8],
    ) -> Result<Option<PreparedLibraryUpdate>> {
        let identity = physical_path_identity(full_path).map_err(|source| {
            LibraryCatalogError::ResolvePath {
                path: full_path.to_path_buf(),
                source,
            }
        })?;
        if !identity
            .strip_prefix(&self.library_path)
            .is_ok_and(|relative| !relative.as_os_str().is_empty())
        {
            return Ok(None);
        }
        Ok(Some(PreparedLibraryUpdate {
            library_path: self.library_path.clone(),
            entry: entry_from_bytes(&self.library_path, &identity, exact_bytes)?,
        }))
    }

    /// Return a new snapshot containing every prepared replacement.
    pub(crate) fn with_prepared_updates(&self, updates: &[PreparedLibraryUpdate]) -> Result<Self> {
        let mut updated_paths = HashSet::with_capacity(updates.len());
        for update in updates {
            if update.library_path != self.library_path {
                return Err(LibraryCatalogError::ForeignUpdate {
                    path: update.entry.full_path.clone(),
                });
            }
            if !updated_paths.insert(update.entry.full_path.clone()) {
                return Err(LibraryCatalogError::DuplicateUpdate {
                    path: update.entry.full_path.clone(),
                });
            }
        }

        let mut entries = self.entries.clone();
        for update in updates {
            if let Some(existing) = entries
                .iter_mut()
                .find(|entry| entry.full_path == update.entry.full_path)
            {
                *existing = update.entry.clone();
            } else {
                entries.push(update.entry.clone());
            }
        }
        Ok(Self {
            entries,
            library_path: self.library_path.clone(),
        })
    }
}

struct NativeMetadata {
    arrangements: Vec<LibraryArrangement>,
    presentation_size: PresentationSizeStatus,
}

fn entry_from_bytes(library_path: &Path, full_path: &Path, bytes: &[u8]) -> Result<LibraryEntry> {
    let format = detect_presentation_file_format(bytes);
    if format != PresentationFileFormat::NativePresentation {
        return Err(LibraryCatalogError::UnsupportedFormat {
            path: full_path.to_path_buf(),
            format,
        });
    }
    let stem = full_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| LibraryCatalogError::InvalidFilename {
            path: full_path.to_path_buf(),
        })?;
    let relative_path =
        full_path
            .strip_prefix(library_path)
            .map_err(|_| LibraryCatalogError::OutsideLibrary {
                path: full_path.to_path_buf(),
                library_path: library_path.to_path_buf(),
            })?;
    let normalized_name = normalize_name(stem);
    let metadata = decode_native_metadata(bytes, full_path)?;
    Ok(LibraryEntry {
        file_name: stem.to_string(),
        normalized_name: normalized_name.clone(),
        file_name_lower: stem.to_lowercase(),
        normalized_lower: normalized_name.to_lowercase(),
        display_name: stem.to_string(),
        relative_path: relative_path.to_string_lossy().to_string(),
        full_path: full_path.to_path_buf(),
        arrangements: metadata.arrangements,
        presentation_size: metadata.presentation_size,
    })
}

fn decode_native_metadata(bytes: &[u8], path: &Path) -> Result<NativeMetadata> {
    let presentation =
        rv_data::Presentation::decode(bytes).map_err(|source| LibraryCatalogError::Decode {
            path: path.to_path_buf(),
            source,
        })?;
    let arrangements = presentation
        .arrangements
        .iter()
        .map(|arrangement| {
            if crate::propresenter::arrangement::selectable_arrangement_uuid(
                &presentation,
                arrangement,
            )
            .is_some()
            {
                LibraryArrangement::Complete {
                    name: arrangement.name.clone(),
                }
            } else {
                LibraryArrangement::Incomplete {
                    name: arrangement.name.clone(),
                }
            }
        })
        .collect();
    Ok(NativeMetadata {
        arrangements,
        presentation_size: inspect_presentation_size(&presentation),
    })
}

fn is_pro_path(path: &Path) -> bool {
    path.extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pro"))
}
