use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Read;
use std::path::Path;

use prost::Message;
use zip::ZipArchive;

use super::model::{PackageError, PackageFileSummary, PlaylistPackage};
use crate::propresenter::deserialize::decode_presentation_bytes;
use crate::propresenter::generated::rv_data;

/// Read and decode a `.proplaylist` package.
pub fn read_playlist_package(path: impl AsRef<Path>) -> Result<PlaylistPackage, PackageError> {
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(file)?;
    let archive_comment = archive.comment().to_vec();
    let mut archive_entries = Vec::with_capacity(archive.len());
    let mut embedded_file_details = Vec::new();
    let mut embedded_files = Vec::new();
    let mut embedded_file_data = BTreeMap::new();
    let mut seen_names = BTreeSet::new();
    let mut data = None;

    for index in 0..archive.len() {
        let mut file = archive.by_index(index)?;
        let name = native_archive_member_name(&file);
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        if !seen_names.insert(name.clone()) {
            return Err(PackageError::DuplicateArchiveEntry(name));
        }
        let summary = package_file_summary(&file, &name);
        archive_entries.push(summary.clone());

        if name == "data" {
            data = Some(bytes);
        } else {
            if summary.is_presentation {
                decode_presentation_bytes(bytes.as_slice(), &name).map_err(|reason| {
                    PackageError::InvalidEmbeddedPresentation {
                        name: name.clone(),
                        reason,
                    }
                })?;
            }
            embedded_file_details.push(summary);
            embedded_files.push(name.clone());
            embedded_file_data.insert(name, bytes);
        }
    }

    let document_data = data.ok_or(PackageError::MissingData)?;
    let document = rv_data::PlaylistDocument::decode(document_data.as_slice())?;
    let document_round_trip_exact = document.encode_to_vec() == document_data;
    Ok(PlaylistPackage {
        document,
        document_data,
        document_round_trip_exact,
        embedded_files,
        embedded_file_details,
        embedded_file_data,
        archive_entries,
        archive_comment,
    })
}

fn native_archive_member_name(file: &zip::read::ZipFile<'_>) -> String {
    std::str::from_utf8(file.name_raw()).map_or_else(
        |_| file.name().to_string(),
        std::string::ToString::to_string,
    )
}

/// Return a compact summary of all presentation items in a playlist document.
fn package_file_summary(file: &zip::read::ZipFile<'_>, name: &str) -> PackageFileSummary {
    let basename = Path::new(name)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(name)
        .to_string();
    PackageFileSummary {
        is_presentation: Path::new(&basename)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("pro")),
        basename,
        size: file.size(),
        crc32: file.crc32(),
        name: name.to_string(),
        compression_method: format!("{:?}", file.compression()),
        is_directory: file.is_dir(),
        version_made_by: file.version_made_by(),
        unix_mode: file.unix_mode(),
        extra_field_ids: nonvolatile_extra_field_ids(file.extra_data()),
        comment: file.comment().to_string(),
    }
}

fn nonvolatile_extra_field_ids(mut data: &[u8]) -> Vec<u16> {
    let mut ids = Vec::new();
    while data.len() >= 4 {
        let id = u16::from_le_bytes([data[0], data[1]]);
        let length = usize::from(u16::from_le_bytes([data[2], data[3]]));
        data = &data[4..];
        if data.len() < length {
            break;
        }
        // 0x5455 is the extended timestamp field. Modification time is
        // intentionally volatile and cannot establish package fidelity.
        if id != 0x5455 {
            ids.push(id);
        }
        data = &data[length..];
    }
    ids
}
