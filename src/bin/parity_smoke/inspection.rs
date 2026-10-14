//! Inspect fixture playlists and presentations for native format parity.

use super::{
    build_playlist, compare_playlist_packages, embedded_presentation_summaries,
    infer_archive_shape, linked_presentation_filename, playlist, presentation_items,
    presentation_media_dependencies, read_playlist_package, write_playlist_document_for_fidelity,
    Path, PlaylistArchiveShape, PlaylistEntry, PlaylistFixture, PlaylistMetadata,
    PlaylistPackageIssue, PlaylistSmokeReport, PresentationFixture, PresentationSmokeReport,
    Result, SelectedArrangement, SmokeIssue, Uuid,
};
use anyhow::Context;
use propresenter::unstable_native::rv_data;
use prost::Message;

pub fn inspect_playlist_fixture(
    fixture_root: &Path,
    fixture: &PlaylistFixture,
) -> Result<PlaylistSmokeReport> {
    let path = fixture_root.join(&fixture.path);
    let package = read_playlist_package(&path)
        .with_context(|| format!("read playlist package {}", path.display()))?;
    let items = presentation_items(package.document());
    let embedded_presentations = embedded_presentation_summaries(&package);
    let mut issues = Vec::new();
    let round_trip_compatible = match fixture.mode {
        PlaylistArchiveShape::PresentationsOnly => match round_trip_compare(&path, &package) {
            Ok(comparison) => {
                let compatible = comparison.issues.is_empty();
                issues.extend(comparison.issues);
                Some(compatible)
            }
            Err(error) => {
                issues.push(PlaylistPackageIssue {
                    kind: "round_trip_reconstruction_error".to_string(),
                    index: None,
                    message: format!("{error:#}"),
                });
                Some(false)
            }
        },
        PlaylistArchiveShape::ContainsMedia => None,
    };
    if infer_archive_shape(&package) != fixture.mode {
        issues.push(PlaylistPackageIssue {
            kind: "fixture_mode_mismatch".to_string(),
            index: None,
            message: format!(
                "manifest expected {:?}, package is {:?}",
                fixture.mode,
                infer_archive_shape(&package)
            ),
        });
    }
    if items.len() != fixture.item_count {
        issues.push(PlaylistPackageIssue {
            kind: "fixture_item_count_mismatch".to_string(),
            index: None,
            message: format!(
                "manifest expected {}, found {}",
                fixture.item_count,
                items.len()
            ),
        });
    }
    if package.embedded_file_count() != fixture.embedded_file_count {
        issues.push(PlaylistPackageIssue {
            kind: "fixture_embedded_count_mismatch".to_string(),
            index: None,
            message: format!(
                "manifest expected {}, found {}",
                fixture.embedded_file_count,
                package.embedded_file_count()
            ),
        });
    }
    let media_file_count = package
        .embedded_file_details()
        .filter(|file| !file.is_presentation)
        .count();
    if media_file_count != fixture.media_file_count {
        issues.push(PlaylistPackageIssue {
            kind: "fixture_media_count_mismatch".to_string(),
            index: None,
            message: format!(
                "manifest expected {}, found {}",
                fixture.media_file_count, media_file_count
            ),
        });
    }
    for required in &fixture.required_embedded_files {
        if !package.has_embedded_file(required) {
            issues.push(PlaylistPackageIssue {
                kind: "fixture_required_embedded_file_missing".to_string(),
                index: None,
                message: format!("manifest-required member {required:?} is missing"),
            });
        }
    }

    Ok(PlaylistSmokeReport {
        path: fixture.path.clone(),
        provenance: fixture.provenance.clone(),
        producer_version: fixture.producer_version.clone(),
        operating_system: fixture.operating_system.clone(),
        export_mode: fixture.export_mode.clone(),
        covered_native_capabilities: fixture.covered_native_capabilities.clone(),
        independent_native_export: fixture.independent_native_export,
        mode: infer_archive_shape(&package),
        item_count: items.len(),
        embedded_file_count: package.embedded_file_count(),
        media_file_count,
        embedded_presentation_count: embedded_presentations.len(),
        round_trip_compatible,
        issues,
    })
}

fn round_trip_compare(
    expected_path: &Path,
    package: &propresenter::package::PlaylistPackage,
) -> Result<propresenter::package::PlaylistPackageComparison> {
    let items = presentation_items(package.document());
    let entries = items
        .iter()
        .map(|item| -> Result<PlaylistEntry> {
            let embedded_filename = linked_presentation_filename(item).with_context(|| {
                format!("playlist item {:?} has no document filename", item.name)
            })?;
            let embedded_data = package
                .embedded_file(&embedded_filename)
                .map(<[u8]>::to_vec);
            let selected_arrangement = selected_arrangement(item, embedded_data.as_deref())?;
            let presentation_path = item
                .local_relative_path
                .as_ref()
                .map(|path| format!("/Users/jimmy/Documents/ProPresenter/{path}"))
                .or_else(|| item.absolute_string.clone())
                .with_context(|| {
                    format!("playlist item {:?} has no presentation path", item.name)
                })?;
            let entry = if let Some(data) = embedded_data {
                PlaylistEntry::embedded(item.name.clone(), presentation_path, data)?
            } else {
                PlaylistEntry::linked(item.name.clone(), presentation_path)?
            };
            Ok(entry
                .with_selected_arrangement(selected_arrangement)?
                .with_user_music_key(item.user_music_key.map(|(music_key, music_scale)| {
                    rv_data::MusicKeyScale {
                        music_key,
                        music_scale,
                    }
                })))
        })
        .collect::<Result<Vec<_>>>()?;

    let playlist_name = package
        .document()
        .root_node
        .as_ref()
        .and_then(|root| match &root.children_type {
            Some(playlist::ChildrenType::Playlists(playlists)) => playlists.playlists.first(),
            _ => None,
        })
        .map(|playlist| playlist.name.as_str())
        .context("fixture is missing its primary child playlist")?;
    let metadata = PlaylistMetadata::from_document(package.document())
        .context("fixture playlist is missing producer metadata")?;
    let playlist = build_playlist(playlist_name, &entries, &metadata);
    let actual_path =
        std::env::temp_dir().join(format!("proflow-parity-{}.proplaylist", Uuid::new_v4()));
    write_playlist_document_for_fidelity(&playlist, &entries, &actual_path)
        .with_context(|| format!("write round-trip playlist {}", actual_path.display()))?;
    let comparison = compare_playlist_packages(expected_path, &actual_path).with_context(|| {
        format!(
            "compare {} to {}",
            expected_path.display(),
            actual_path.display()
        )
    })?;
    std::fs::remove_file(&actual_path)
        .with_context(|| format!("remove round-trip playlist {}", actual_path.display()))?;
    Ok(comparison)
}

fn selected_arrangement(
    item: &propresenter::package::PlaylistItemSummary,
    embedded_data: Option<&[u8]>,
) -> Result<Option<SelectedArrangement>> {
    let Some(uuid) = item.arrangement_uuid.as_deref() else {
        return Ok(None);
    };
    let uuid = Uuid::parse_str(uuid)
        .with_context(|| format!("invalid arrangement UUID for {:?}", item.name))?;
    let name = if item.arrangement_name.trim().is_empty() {
        let presentation = embedded_data
            .map(rv_data::Presentation::decode)
            .transpose()
            .with_context(|| format!("decode embedded presentation for {:?}", item.name))?;
        presentation
            .as_ref()
            .and_then(|presentation| {
                presentation.arrangements.iter().find(|arrangement| {
                    arrangement
                        .uuid
                        .as_ref()
                        .is_some_and(|candidate| candidate.string == uuid.to_string())
                })
            })
            .map(|arrangement| arrangement.name.clone())
            .with_context(|| {
                format!(
                    "playlist item {:?} has arrangement UUID {uuid} but no recoverable name",
                    item.name
                )
            })?
    } else {
        item.arrangement_name.clone()
    };
    SelectedArrangement::new(uuid, name)
        .map(Some)
        .with_context(|| format!("incomplete arrangement metadata for {:?}", item.name))
}

pub fn inspect_presentation_fixture(
    fixture_root: &Path,
    fixture: &PresentationFixture,
) -> Result<PresentationSmokeReport> {
    let path = fixture_root.join(&fixture.path);
    let data = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    let presentation = rv_data::Presentation::decode(data.as_slice())
        .with_context(|| format!("decode {}", path.display()))?;
    let dependencies = presentation_media_dependencies(&presentation);
    let cue_count = presentation.cues.len();
    let cue_group_count = presentation.cue_groups.len();
    let arrangement_count = presentation.arrangements.len();
    let media_dependency_count = dependencies.len();
    let mut issues = Vec::new();

    push_mismatch(
        &mut issues,
        "presentation_name_mismatch",
        "name",
        &fixture.name,
        &presentation.name,
    );
    push_mismatch(
        &mut issues,
        "presentation_cue_count_mismatch",
        "cue count",
        &fixture.cue_count,
        &cue_count,
    );
    push_mismatch(
        &mut issues,
        "presentation_group_count_mismatch",
        "cue group count",
        &fixture.cue_group_count,
        &cue_group_count,
    );
    push_mismatch(
        &mut issues,
        "presentation_arrangement_count_mismatch",
        "arrangement count",
        &fixture.arrangement_count,
        &arrangement_count,
    );
    push_mismatch(
        &mut issues,
        "presentation_media_dependency_count_mismatch",
        "media dependency count",
        &fixture.media_dependency_count,
        &media_dependency_count,
    );
    let matches_manifest = issues.is_empty();

    Ok(PresentationSmokeReport {
        path: fixture.path.clone(),
        provenance: fixture.provenance.clone(),
        producer_version: fixture.producer_version.clone(),
        operating_system: fixture.operating_system.clone(),
        export_mode: fixture.export_mode.clone(),
        covered_native_capabilities: fixture.covered_native_capabilities.clone(),
        independent_native_export: fixture.independent_native_export,
        name: presentation.name,
        cue_count,
        cue_group_count,
        arrangement_count,
        media_dependency_count,
        matches_manifest,
        issues,
    })
}

fn push_mismatch<T>(issues: &mut Vec<SmokeIssue>, kind: &str, label: &str, expected: &T, actual: &T)
where
    T: std::fmt::Display + PartialEq,
{
    if expected != actual {
        issues.push(SmokeIssue {
            kind: kind.to_string(),
            message: format!("manifest expected {label} {expected}, found {actual}"),
        });
    }
}
