//! Shared native URL and file-locator semantics.
//!
//! `ProPresenter` commonly stores the same file as both an absolute `file://`
//! URL and a show-relative path. This module owns that translation so package
//! writing, live resolution, media discovery, and inspection agree about
//! escaping and path boundaries.

use std::path::{Component, Path, PathBuf};

use super::generated::rv_data::{self, url};

/// One native file reference with all local resolution semantics compiled once.
///
/// `ProPresenter` can retain a stale absolute storage URL while its show-relative
/// locator points at the active workspace. Resolution therefore always checks
/// show-relative candidates first. The stored source remains available for
/// diagnostics, but callers must not treat it as the preferred local path.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct NativeFileLocator {
    source: String,
    show_relative_paths: Vec<PathBuf>,
    absolute_paths: Vec<PathBuf>,
    basename: Option<String>,
}

/// Checked local resolution of one native file locator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum NativeFileResolution {
    Available(PathBuf),
    Missing(PathBuf),
    Unresolved,
}

impl NativeFileLocator {
    /// Compile the local locator state carried by one native URL.
    ///
    /// Remote and malformed references are retained as unresolved locators so
    /// inspection can report their original source without guessing a file.
    pub(crate) fn from_url(value: &rv_data::Url) -> Option<Self> {
        let source = preferred_source(value)?.to_string();
        if source.trim().is_empty() {
            return None;
        }
        let mut show_relative_paths = Vec::new();
        let mut absolute_paths = Vec::new();

        if let Some(url::RelativeFilePath::Local(local)) = &value.relative_file_path {
            if local.root == url::local_relative_path::Root::Show as i32 {
                if let Some(path) = decode_show_relative_path(&local.path) {
                    push_unique(&mut show_relative_paths, path);
                }
            }
        }

        if let Some(storage) = storage_source(value) {
            // Native presentation and theme links frequently omit
            // relative_file_path but retain an exact show-owned root suffix.
            // These are structural locators, not basename inference.
            if let Some(relative) =
                show_owned_relative_path(storage).and_then(|path| checked_relative_path(&path))
            {
                push_unique(&mut show_relative_paths, relative);
            }
            if let Some(decoded) = decode_file_url_or_path(storage) {
                let path = PathBuf::from(&decoded);
                if path.is_absolute() {
                    push_unique(&mut absolute_paths, path);
                } else if !decoded.contains("://") {
                    if let Some(path) = checked_relative_path(&decoded) {
                        push_unique(&mut show_relative_paths, path);
                    }
                }
            }
        }

        if let Some(url::RelativeFilePath::External(external)) = &value.relative_file_path {
            if let Some(decoded) = percent_decode_strict(&external.path) {
                let path = PathBuf::from(decoded);
                if path.is_absolute() {
                    push_unique(&mut absolute_paths, path);
                }
            }
        }

        let basename = show_relative_paths
            .iter()
            .chain(&absolute_paths)
            .find_map(|path| path.file_name()?.to_str().map(ToOwned::to_owned))
            .or_else(|| decoded_basename(&source));

        Some(Self {
            source,
            show_relative_paths,
            absolute_paths,
            basename,
        })
    }

    pub(crate) fn source(&self) -> &str {
        &self.source
    }

    /// Stored absolute path, retained for inspection only.
    pub(crate) fn stored_absolute_path(&self) -> Option<&Path> {
        self.absolute_paths.first().map(PathBuf::as_path)
    }

    pub(crate) fn basename(&self) -> Option<&str> {
        self.basename.as_deref()
    }

    pub(crate) const fn has_local_candidate(&self) -> bool {
        !self.show_relative_paths.is_empty() || !self.absolute_paths.is_empty()
    }

    /// Resolve the preferred local file, checking the active show before stale
    /// absolute storage. If no candidate exists, return the preferred locator
    /// so the caller can issue a precise missing-file error.
    pub(crate) fn resolve(&self, show_root: Option<&Path>) -> NativeFileResolution {
        let candidates = self.local_candidates(show_root);
        let available = candidates
            .iter()
            .find(|candidate| candidate.is_file())
            .cloned();
        available.map_or_else(
            || {
                candidates.into_iter().next().map_or(
                    NativeFileResolution::Unresolved,
                    NativeFileResolution::Missing,
                )
            },
            NativeFileResolution::Available,
        )
    }

    fn local_candidates(&self, show_root: Option<&Path>) -> Vec<PathBuf> {
        let mut candidates = Vec::new();
        if let Some(show_root) = show_root {
            for relative in &self.show_relative_paths {
                push_unique(&mut candidates, show_root.join(relative));
            }
        }
        for absolute in &self.absolute_paths {
            push_unique(&mut candidates, absolute.clone());
        }
        candidates
    }
}

/// Decode percent escapes, rejecting malformed escapes and invalid UTF-8.
pub fn percent_decode_strict(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hi = bytes.get(index + 1).and_then(|byte| hex_value(*byte))?;
            let lo = bytes.get(index + 2).and_then(|byte| hex_value(*byte))?;
            decoded.push((hi << 4) | lo);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

/// Decode valid percent escapes while preserving malformed ones for
/// best-effort inspection of independently produced files.
pub fn percent_decode_lossy(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
            {
                decoded.push((hi << 4) | lo);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

/// Decode a native file URL or plain path using strict escape handling.
pub fn decode_file_url_or_path(value: &str) -> Option<String> {
    percent_decode_strict(file_url_path(value))
}

/// Decode a native file URL or path for best-effort inspection.
pub fn decode_file_url_or_path_lossy(value: &str) -> String {
    percent_decode_lossy(file_url_path(value))
}

fn file_url_path(value: &str) -> &str {
    value
        .strip_prefix("file://")
        .map_or(value, |rest| rest.strip_prefix("localhost").unwrap_or(rest))
}

/// Encode one path as the absolute string used by native playlist URLs.
pub fn file_url(path: &str) -> String {
    if path.starts_with("file://") {
        return path.to_string();
    }
    format!("file://{}", percent_encode_file_path(path))
}

/// Encode one local path as a canonical absolute native file URL.
pub(super) fn canonical_file_url(path: &Path) -> String {
    let absolute = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    file_url(&absolute.to_string_lossy())
}

/// Return the exact `Libraries/...` suffix at a component boundary.
pub fn library_relative_path(value: &str) -> Option<String> {
    show_relative_path_from_root(value, "Libraries")
}

/// Return the exact known show-owned suffix at a component boundary.
fn show_owned_relative_path(value: &str) -> Option<String> {
    library_relative_path(value).or_else(|| show_relative_path_from_root(value, "Themes"))
}

fn show_relative_path_from_root(value: &str, root: &str) -> Option<String> {
    if has_non_file_uri_scheme(value) {
        return None;
    }
    let decoded = decode_file_url_or_path(value)?;
    let normalized = decoded.replace('\\', "/");
    let components = normalized
        .split('/')
        .filter(|component| !component.is_empty())
        .collect::<Vec<_>>();
    let root = components
        .iter()
        .position(|component| component.eq_ignore_ascii_case(root))?;
    Some(components[root..].join("/"))
}

/// Native local locators may be `file://` URLs or plain paths. Never reinterpret
/// another URI scheme as a show-owned path merely because its URL path happens
/// to contain a `Themes` or `Libraries` component.
fn has_non_file_uri_scheme(value: &str) -> bool {
    value
        .split_once("://")
        .is_some_and(|(scheme, _)| !scheme.eq_ignore_ascii_case("file"))
}

/// Return the final decoded path component.
pub fn decoded_basename(value: &str) -> Option<String> {
    let decoded = decode_file_url_or_path(value)?;
    decoded
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
}

/// Return the final path component without rejecting malformed percent escapes.
pub fn decoded_basename_lossy(value: &str) -> Option<String> {
    decode_file_url_or_path_lossy(value)
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
}

/// Build the two native playlist locators for a presentation path.
pub fn presentation_document_url(path: &str) -> rv_data::Url {
    let relative = library_relative_path(path);
    rv_data::Url {
        platform: url::Platform::Macos as i32,
        storage: Some(url::Storage::AbsoluteString(file_url(path))),
        relative_file_path: relative.map(|path| {
            url::RelativeFilePath::Local(url::LocalRelativePath {
                root: url::local_relative_path::Root::Show as i32,
                path,
            })
        }),
    }
}

/// Build the native absolute and show-relative locators for one local file.
///
/// Existing paths are canonicalized so the serialized absolute URL and the
/// show-relative path describe the same file. A file outside `show_root` keeps
/// only its absolute locator.
pub(super) fn local_file_url(path: &Path, show_root: &Path) -> rv_data::Url {
    let absolute = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let root = show_root
        .canonicalize()
        .unwrap_or_else(|_| show_root.to_path_buf());
    let relative_file_path = absolute
        .strip_prefix(&root)
        .ok()
        .and_then(|relative| checked_relative_path(&relative.to_string_lossy()))
        .map(|relative| {
            url::RelativeFilePath::Local(url::LocalRelativePath {
                root: url::local_relative_path::Root::Show as i32,
                path: relative.to_string_lossy().replace('\\', "/"),
            })
        });
    rv_data::Url {
        platform: url::Platform::Macos as i32,
        storage: Some(url::Storage::AbsoluteString(file_url(
            &absolute.to_string_lossy(),
        ))),
        relative_file_path,
    }
}

/// Return local candidates in native preference order: show-relative first,
/// then storage. Callers decide whether missing candidates are errors.
#[cfg(any(test, feature = "dev-tools"))]
pub(super) fn local_file_candidates(url_value: &rv_data::Url, show_root: &Path) -> Vec<PathBuf> {
    NativeFileLocator::from_url(url_value).map_or_else(Vec::new, |locator| {
        locator.local_candidates(Some(show_root))
    })
}

/// Preferred serialized source used for diagnostics and dependency identity.
pub fn preferred_source(url_value: &rv_data::Url) -> Option<&str> {
    storage_source(url_value).or(match &url_value.relative_file_path {
        Some(url::RelativeFilePath::Local(local)) => Some(local.path.as_str()),
        Some(url::RelativeFilePath::External(external)) => Some(external.path.as_str()),
        None => None,
    })
}

fn decode_show_relative_path(value: &str) -> Option<PathBuf> {
    let decoded = percent_decode_strict(value)?;
    checked_relative_path(&decoded)
}

fn checked_relative_path(value: &str) -> Option<PathBuf> {
    let normalized = value.replace('\\', "/");
    let path = PathBuf::from(normalized);
    if path.as_os_str().is_empty()
        || path.components().any(|component| {
            matches!(
                component,
                Component::Prefix(_) | Component::RootDir | Component::ParentDir
            )
        })
    {
        return None;
    }
    Some(path)
}

/// Infer the show root from an absolute presentation path containing a real
/// `Libraries` component.
pub fn propresenter_root_from_library_path(value: &str) -> Option<PathBuf> {
    let decoded = decode_file_url_or_path(value)?;
    let normalized = decoded.replace('\\', "/");
    let components = normalized.split('/').collect::<Vec<_>>();
    let library = components
        .iter()
        .position(|component| component.eq_ignore_ascii_case("Libraries"))?;
    if library == 0 {
        return None;
    }
    let prefix = components[..library].join("/");
    let root = if normalized.starts_with('/') {
        PathBuf::from(format!("/{prefix}"))
    } else {
        PathBuf::from(prefix)
    };
    root.is_absolute().then_some(root)
}

fn storage_source(url_value: &rv_data::Url) -> Option<&str> {
    match &url_value.storage {
        Some(url::Storage::AbsoluteString(value) | url::Storage::RelativePath(value))
            if !value.trim().is_empty() =>
        {
            Some(value)
        }
        _ => None,
    }
}

fn push_unique(values: &mut Vec<PathBuf>, value: PathBuf) {
    if !values.contains(&value) {
        values.push(value);
    }
}

fn percent_encode_file_path(path: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(path.len());
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'/'
            | b'-'
            | b'_'
            | b'.'
            | b'~'
            | b','
            | b'('
            | b')'
            | b'\'' => encoded.push(char::from(byte)),
            b' ' => encoded.push_str("%20"),
            _ => {
                encoded.push('%');
                encoded.push(char::from(HEX[usize::from(byte >> 4)]));
                encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
            }
        }
    }
    encoded
}

const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_suffix_requires_a_path_component_boundary() {
        assert_eq!(
            library_relative_path("file:///show/Libraries/Default/Song%20One.pro").as_deref(),
            Some("Libraries/Default/Song One.pro")
        );
        assert_eq!(
            library_relative_path("file:///show/NotLibraries/Default/Song.pro"),
            None
        );

        let non_library = presentation_document_url("/exports/Song.pro");
        assert!(non_library.relative_file_path.is_none());
    }

    #[test]
    fn locator_rebases_exact_theme_suffix_to_the_active_show() {
        let directory = tempfile::tempdir().expect("tempdir");
        let show_root = directory.path().join("active-show");
        let active = show_root.join("Themes/VPC Theme/Theme");
        std::fs::create_dir_all(active.parent().expect("theme parent")).expect("theme directory");
        std::fs::write(&active, b"theme").expect("active theme");
        let native = rv_data::Url {
            storage: Some(url::Storage::AbsoluteString(
                "file:///Users/other/Documents/ProPresenter/Themes/VPC%20Theme/Theme".to_string(),
            )),
            ..rv_data::Url::default()
        };

        let locator = NativeFileLocator::from_url(&native).expect("locator");

        assert_eq!(
            locator.resolve(Some(&show_root)),
            NativeFileResolution::Available(active)
        );
        assert!(show_owned_relative_path("file:///show/NotThemes/VPC/Theme").is_none());
    }

    #[test]
    fn remote_theme_url_is_never_rebased_to_a_local_show_file() {
        let directory = tempfile::tempdir().expect("tempdir");
        let show_root = directory.path().join("active-show");
        let local_collision = show_root.join("Themes/Remote/Theme");
        std::fs::create_dir_all(local_collision.parent().expect("theme parent"))
            .expect("theme directory");
        std::fs::write(&local_collision, b"unrelated local theme").expect("local collision");
        let native = rv_data::Url {
            storage: Some(url::Storage::AbsoluteString(
                "https://example.com/Themes/Remote/Theme".to_string(),
            )),
            ..rv_data::Url::default()
        };

        let locator = NativeFileLocator::from_url(&native).expect("diagnostic locator");

        assert!(!locator.has_local_candidate());
        assert_eq!(
            locator.resolve(Some(&show_root)),
            NativeFileResolution::Unresolved
        );
        assert_eq!(
            library_relative_path("https://example.com/Libraries/Default/Song.pro"),
            None
        );
    }

    #[test]
    fn strict_and_lossy_escape_policies_are_explicit() {
        assert_eq!(
            percent_decode_strict("Song%20One.pro").as_deref(),
            Some("Song One.pro")
        );
        assert_eq!(percent_decode_strict("bad%2"), None);
        assert_eq!(percent_decode_lossy("bad%2"), "bad%2");
    }

    #[test]
    fn locator_prefers_the_active_show_file_over_existing_absolute_storage() {
        let directory = tempfile::tempdir().expect("tempdir");
        let show_root = directory.path().join("show");
        let stale_root = directory.path().join("stale");
        std::fs::create_dir_all(show_root.join("Media")).expect("show media");
        std::fs::create_dir_all(&stale_root).expect("stale media");
        let active = show_root.join("Media/background.png");
        let stale = stale_root.join("background.png");
        std::fs::write(&active, b"active").expect("active file");
        std::fs::write(&stale, b"stale").expect("stale file");
        let native = rv_data::Url {
            storage: Some(url::Storage::AbsoluteString(file_url(
                &stale.to_string_lossy(),
            ))),
            relative_file_path: Some(url::RelativeFilePath::Local(url::LocalRelativePath {
                root: url::local_relative_path::Root::Show as i32,
                path: "Media/background.png".to_string(),
            })),
            ..rv_data::Url::default()
        };

        let locator = NativeFileLocator::from_url(&native).expect("locator");

        assert_eq!(
            locator.resolve(Some(&show_root)),
            NativeFileResolution::Available(active)
        );
        assert_eq!(locator.stored_absolute_path(), Some(stale.as_path()));
    }

    #[test]
    fn show_relative_locator_cannot_escape_the_show_root() {
        let native = rv_data::Url {
            storage: Some(url::Storage::AbsoluteString(
                "https://example.com/background.png".to_string(),
            )),
            relative_file_path: Some(url::RelativeFilePath::Local(url::LocalRelativePath {
                root: url::local_relative_path::Root::Show as i32,
                path: "../outside.png".to_string(),
            })),
            ..rv_data::Url::default()
        };

        let locator = NativeFileLocator::from_url(&native).expect("diagnostic locator");

        assert!(!locator.has_local_candidate());
        assert_eq!(
            locator.resolve(Some(Path::new("/show"))),
            NativeFileResolution::Unresolved
        );
    }
}
