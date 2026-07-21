//! Integrity-checked materialization and identity of the embedded helper.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::TextFitError;
use crate::propresenter::text_fit::evidence::TextFitContractSummary;
use crate::propresenter::text_fit::{TEXT_FIT_EVIDENCE_SCHEMA, TEXT_FIT_PROTOCOL_VERSION};

const BUNDLED_HELPER_BYTES: &[u8] = include_bytes!(env!("PROFLOW_TEXT_FIT_ORACLE_PATH"));

pub(super) fn materialize() -> Result<PathBuf, TextFitError> {
    let digest: [u8; 32] = Sha256::digest(BUNDLED_HELPER_BYTES).into();
    let cache_root = dirs::cache_dir()
        .ok_or(TextFitError::LocalCacheUnavailable)?
        .join("proflow")
        .join("native-text-fit")
        .join(digest_hex(&digest));
    std::fs::create_dir_all(&cache_root)
        .map_err(|source| cache_error("create cache directory", &cache_root, source))?;
    std::fs::set_permissions(&cache_root, std::fs::Permissions::from_mode(0o700))
        .map_err(|source| cache_error("secure cache directory", &cache_root, source))?;

    let executable = cache_root.join("proflow-text-fit-oracle");
    match std::fs::symlink_metadata(&executable) {
        Ok(metadata) => {
            validate_cached(&executable, &metadata, digest)?;
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
                .map_err(|source| cache_error("set executable permissions", &executable, source))?;
            return Ok(executable);
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(cache_error("inspect cache entry", &executable, source));
        }
    }

    let temporary = cache_root.join(format!(
        ".proflow-text-fit-oracle.{}.{}.tmp",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|source| cache_error("create temporary helper", &temporary, source))?;
    file.write_all(BUNDLED_HELPER_BYTES)
        .and_then(|()| file.sync_all())
        .map_err(|source| cache_error("write temporary helper", &temporary, source))?;
    std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o700))
        .map_err(|source| cache_error("set temporary helper permissions", &temporary, source))?;
    let temporary_metadata = std::fs::symlink_metadata(&temporary)
        .map_err(|source| cache_error("inspect temporary helper", &temporary, source))?;
    validate_cached(&temporary, &temporary_metadata, digest)?;

    match std::fs::hard_link(&temporary, &executable) {
        Ok(()) => {}
        Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
            let metadata = std::fs::symlink_metadata(&executable).map_err(|source| {
                cache_error("inspect concurrent cache entry", &executable, source)
            })?;
            validate_cached(&executable, &metadata, digest)?;
        }
        Err(source) => {
            return Err(cache_error("install cached helper", &executable, source));
        }
    }
    std::fs::remove_file(&temporary)
        .map_err(|source| cache_error("remove temporary helper", &temporary, source))?;
    File::open(&cache_root)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| cache_error("synchronize cache directory", &cache_root, source))?;

    let metadata = std::fs::symlink_metadata(&executable)
        .map_err(|source| cache_error("inspect installed helper", &executable, source))?;
    validate_cached(&executable, &metadata, digest)?;
    Ok(executable)
}

pub(super) fn contract(executable: &Path) -> Result<TextFitContractSummary, TextFitError> {
    let helper_bytes =
        std::fs::read(executable).map_err(|source| TextFitError::ReadHelperIdentity {
            path: executable.to_path_buf(),
            source,
        })?;
    let helper_sha256 = digest_hex(&Sha256::digest(helper_bytes));
    Ok(TextFitContractSummary::new(
        TEXT_FIT_EVIDENCE_SCHEMA,
        TEXT_FIT_PROTOCOL_VERSION,
        helper_sha256,
    ))
}

fn validate_cached(
    path: &Path,
    metadata: &std::fs::Metadata,
    expected_digest: [u8; 32],
) -> Result<(), TextFitError> {
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(TextFitError::InvalidBundledHelperCacheEntry {
            path: path.to_path_buf(),
        });
    }
    let bytes =
        std::fs::read(path).map_err(|source| cache_error("read cached helper", path, source))?;
    let actual_digest: [u8; 32] = Sha256::digest(&bytes).into();
    if actual_digest != expected_digest {
        return Err(TextFitError::BundledHelperDigestMismatch {
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

fn cache_error(operation: &'static str, path: &Path, source: std::io::Error) -> TextFitError {
    TextFitError::BundledHelperCache {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

fn digest_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}
