//! Audit a generated playlist after current-version `ProPresenter` import/save-back.
//!
//! Usage:
//! `release_audit <generated.proplaylist> <saved-back.proplaylist> [approved-thumbnails actual-thumbnails]`

#![allow(clippy::print_stdout)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use image::ImageReader;
use proflow::propresenter::package::{compare_playlist_packages, PlaylistPackageComparison};
use serde::Serialize;
use walkdir::WalkDir;

#[derive(Debug, Serialize)]
struct ReleaseAuditReport {
    generated_package: String,
    saved_back_package: String,
    compatible: bool,
    package: PlaylistPackageComparison,
    thumbnails: Option<ThumbnailAudit>,
}

#[derive(Debug, Serialize)]
struct ThumbnailAudit {
    compatible: bool,
    approved_root: String,
    actual_root: String,
    missing: Vec<String>,
    unexpected: Vec<String>,
    compared: Vec<ThumbnailComparison>,
}

#[derive(Debug, Serialize)]
struct ThumbnailComparison {
    path: String,
    approved_size: (u32, u32),
    actual_size: (u32, u32),
    differing_pixels: u64,
    maximum_channel_delta: u8,
    compatible: bool,
}

fn main() -> ExitCode {
    match run() {
        Ok(report) => {
            match serde_json::to_string_pretty(&report) {
                Ok(json) => println!("{json}"),
                Err(error) => {
                    eprintln!("render release audit: {error}");
                    return ExitCode::FAILURE;
                }
            }
            if report.compatible {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ReleaseAuditReport> {
    let mut args = std::env::args_os().skip(1);
    let generated = args.next().map(PathBuf::from).context(usage())?;
    let saved_back = args.next().map(PathBuf::from).context(usage())?;
    let approved_thumbnails = args.next().map(PathBuf::from);
    let actual_thumbnails = args.next().map(PathBuf::from);
    if args.next().is_some() || approved_thumbnails.is_some() != actual_thumbnails.is_some() {
        anyhow::bail!(usage());
    }

    let package = compare_playlist_packages(&generated, &saved_back).with_context(|| {
        format!(
            "compare generated package {} with saved-back package {}",
            generated.display(),
            saved_back.display()
        )
    })?;
    let thumbnails = approved_thumbnails
        .zip(actual_thumbnails)
        .map(|(approved, actual)| compare_thumbnail_directories(&approved, &actual))
        .transpose()?;
    let compatible = package.compatible
        && thumbnails
            .as_ref()
            .is_none_or(|comparison| comparison.compatible);

    Ok(ReleaseAuditReport {
        generated_package: generated.display().to_string(),
        saved_back_package: saved_back.display().to_string(),
        compatible,
        package,
        thumbnails,
    })
}

const fn usage() -> &'static str {
    "usage: release_audit <generated.proplaylist> <saved-back.proplaylist> [approved-thumbnails actual-thumbnails]"
}

fn compare_thumbnail_directories(
    approved_root: &Path,
    actual_root: &Path,
) -> Result<ThumbnailAudit> {
    let approved = thumbnail_files(approved_root)?;
    let actual = thumbnail_files(actual_root)?;
    let approved_names = approved.keys().cloned().collect::<BTreeSet<_>>();
    let actual_names = actual.keys().cloned().collect::<BTreeSet<_>>();
    let missing = approved_names
        .difference(&actual_names)
        .cloned()
        .collect::<Vec<_>>();
    let unexpected = actual_names
        .difference(&approved_names)
        .cloned()
        .collect::<Vec<_>>();
    let compared = approved_names
        .intersection(&actual_names)
        .map(|name| compare_thumbnails(name, &approved[name], &actual[name]))
        .collect::<Result<Vec<_>>>()?;
    let compatible = missing.is_empty()
        && unexpected.is_empty()
        && compared.iter().all(|comparison| comparison.compatible);

    Ok(ThumbnailAudit {
        compatible,
        approved_root: approved_root.display().to_string(),
        actual_root: actual_root.display().to_string(),
        missing,
        unexpected,
        compared,
    })
}

fn thumbnail_files(root: &Path) -> Result<BTreeMap<String, PathBuf>> {
    if !root.is_dir() {
        anyhow::bail!("thumbnail root is not a directory: {}", root.display());
    }
    let mut files = BTreeMap::new();
    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry.with_context(|| format!("traverse {}", root.display()))?;
        if !entry.file_type().is_file() || !is_supported_image(entry.path()) {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(root)
            .with_context(|| format!("relativize {}", entry.path().display()))?;
        let key = relative.to_string_lossy().replace('\\', "/");
        files.insert(key, entry.into_path());
    }
    Ok(files)
}

fn is_supported_image(path: &Path) -> bool {
    path.extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "jpg" | "jpeg" | "png" | "tif" | "tiff"
            )
        })
}

fn compare_thumbnails(
    relative_path: &str,
    approved_path: &Path,
    actual_path: &Path,
) -> Result<ThumbnailComparison> {
    let approved = ImageReader::open(approved_path)
        .with_context(|| format!("open {}", approved_path.display()))?
        .decode()
        .with_context(|| format!("decode {}", approved_path.display()))?
        .to_rgba8();
    let actual = ImageReader::open(actual_path)
        .with_context(|| format!("open {}", actual_path.display()))?
        .decode()
        .with_context(|| format!("decode {}", actual_path.display()))?
        .to_rgba8();
    let approved_size = approved.dimensions();
    let actual_size = actual.dimensions();
    let (differing_pixels, maximum_channel_delta) = if approved_size == actual_size {
        approved.pixels().zip(actual.pixels()).fold(
            (0_u64, 0_u8),
            |(different, maximum), (left, right)| {
                let pixel_maximum = left
                    .0
                    .iter()
                    .zip(right.0.iter())
                    .map(|(left, right)| left.abs_diff(*right))
                    .max()
                    .unwrap_or(0);
                (
                    different + u64::from(pixel_maximum != 0),
                    maximum.max(pixel_maximum),
                )
            },
        )
    } else {
        (u64::MAX, u8::MAX)
    };
    Ok(ThumbnailComparison {
        path: relative_path.to_string(),
        approved_size,
        actual_size,
        differing_pixels,
        maximum_channel_delta,
        compatible: approved_size == actual_size && differing_pixels == 0,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use image::{ImageBuffer, Rgba};

    use super::*;

    fn write_pixel(path: &Path, value: u8) {
        ImageBuffer::from_pixel(2, 1, Rgba([value, 0, 0, 255]))
            .save(path)
            .expect("write thumbnail");
    }

    #[test]
    fn thumbnail_oracle_compares_decoded_pixels_and_membership() {
        let approved = tempfile::tempdir().expect("approved directory");
        let actual = tempfile::tempdir().expect("actual directory");
        write_pixel(&approved.path().join("cue-1.png"), 10);
        write_pixel(&actual.path().join("cue-1.png"), 10);

        let matching = compare_thumbnail_directories(approved.path(), actual.path())
            .expect("compare matching thumbnails");
        assert!(matching.compatible);
        assert_eq!(matching.compared[0].differing_pixels, 0);

        write_pixel(&actual.path().join("cue-1.png"), 11);
        write_pixel(&actual.path().join("cue-2.png"), 10);
        let changed = compare_thumbnail_directories(approved.path(), actual.path())
            .expect("compare changed thumbnails");
        assert!(!changed.compatible);
        assert_eq!(changed.unexpected, ["cue-2.png"]);
        assert_eq!(changed.compared[0].differing_pixels, 2);
        assert_eq!(changed.compared[0].maximum_channel_delta, 1);
    }
}
