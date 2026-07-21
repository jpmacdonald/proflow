//! Read-only audits against independent native `ProPresenter` exports.
//!
//! Native portable exports can retain a presentation's source-machine media
//! URL while storing the same media under the exporting machine's path. A
//! unique basename match is therefore reported as an inferred native rebase.
//! It is useful reverse-engineering evidence, not proof that an export can be
//! relocated or imported successfully.
//!
//! The committed Easter media fixture was exported by `ProPresenter` 18.4 and
//! links presentations relative to `ROOT_USER_HOME`. Current production output
//! deliberately uses `ROOT_SHOW/Libraries/...`. Those locator roots are
//! operator-significant and the package comparator correctly does not normalize
//! them. Consequently this fixture is an independent oracle for native archive,
//! link, and media-dependency shape; the current-version presentations-only
//! fixture separately owns exact production-writer reconstruction parity.

#![allow(clippy::expect_used, clippy::panic)]

use proflow::propresenter::generated::rv_data::{self, playlist, playlist_item, url};
use proflow::propresenter::media::{presentation_media_dependencies, MediaDependency};
use proflow::propresenter::package::read_playlist_package;
use prost::Message;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq, Eq)]
struct MediaReference {
    presentation: String,
    source: String,
    path: String,
    basename: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InferredRebase {
    presentation: String,
    source: String,
    archive_member: String,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct MediaAudit {
    exact_matches: usize,
    inferred_rebases: Vec<InferredRebase>,
    missing: Vec<String>,
    ambiguous: Vec<String>,
    extra_archive_media: Vec<String>,
}

#[test]
fn committed_independent_portable_export_has_complete_media_and_links() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "tests/fixtures/propresenter/native/corpus/playlists/native-easter-portable-media.proplaylist",
    );
    let audit = audit_export(&path);

    assert_media_complete(&path, &audit);
    assert!(
        audit.exact_matches + audit.inferred_rebases.len() > 0,
        "fixture must exercise an actual presentation-to-media dependency"
    );
}

#[test]
#[ignore = "set PROFLOW_NATIVE_EXPORT_FILE to a native .proplaylist export"]
fn native_export_contains_every_discovered_media_dependency() {
    let path = std::env::var_os("PROFLOW_NATIVE_EXPORT_FILE")
        .map(PathBuf::from)
        .expect("PROFLOW_NATIVE_EXPORT_FILE must point to a native .proplaylist export");
    let audit = audit_export(&path);

    report_audit(&path, &audit, true);
    assert_media_complete(&path, &audit);
}

#[test]
#[ignore = "set PROFLOW_LIVE_PLAYLIST_EXPORT_DIR to a directory of native exports"]
fn native_export_corpus_matches_evidenced_archive_and_media_shape() {
    let root = std::env::var_os("PROFLOW_LIVE_PLAYLIST_EXPORT_DIR")
        .map(PathBuf::from)
        .expect("PROFLOW_LIVE_PLAYLIST_EXPORT_DIR must point to native playlist exports");
    let mut packages = WalkDir::new(&root)
        .follow_links(false)
        .into_iter()
        .map(|entry| {
            entry.unwrap_or_else(|error| {
                panic!(
                    "failed to traverse native playlist corpus {}: {error}",
                    root.display()
                )
            })
        })
        .filter(|entry| entry.file_type().is_file())
        .map(walkdir::DirEntry::into_path)
        .filter(|path| {
            path.extension()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|extension| extension.eq_ignore_ascii_case("proplaylist"))
        })
        .collect::<Vec<_>>();
    packages.sort();
    assert!(
        !packages.is_empty(),
        "no .proplaylist files found under {}",
        root.display()
    );

    let mut exact_matches = 0usize;
    let mut inferred_rebases = 0usize;
    for path in &packages {
        let audit = audit_export(path);
        assert_media_complete(path, &audit);
        exact_matches += audit.exact_matches;
        inferred_rebases += audit.inferred_rebases.len();
    }
    eprintln!(
        "audited {} native exports under {}: exact media matches={exact_matches}, inferred unique-basename rebases={inferred_rebases}",
        packages.len(),
        root.display()
    );
}

fn audit_export(path: &Path) -> MediaAudit {
    assert_native_end_records(path);
    assert_playlist_link_integrity(path);
    let file = File::open(path)
        .unwrap_or_else(|error| panic!("failed to open {}: {error}", path.display()));
    let mut archive = zip::ZipArchive::new(file)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));

    let mut archive_media = BTreeSet::new();
    let mut embedded_presentations = Vec::new();
    let mut member_names = Vec::with_capacity(archive.len());
    let mut unique_names = BTreeSet::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("archive entry");
        let name = native_archive_member_name(&entry);
        assert_eq!(
            entry.compression(),
            zip::CompressionMethod::Stored,
            "{}:{name} is not stored",
            path.display()
        );
        assert_eq!(
            entry.version_made_by(),
            (3, 0),
            "{}:{name} creator version is not 3.0",
            path.display()
        );
        assert!(
            unique_names.insert(name.clone()),
            "{} contains duplicate member {name:?}",
            path.display()
        );
        member_names.push(name.clone());
        if name == "data" {
            continue;
        }
        if is_presentation_path(&name) {
            let mut bytes = Vec::new();
            entry
                .read_to_end(&mut bytes)
                .expect("read embedded presentation");
            embedded_presentations.push((name, bytes));
        } else {
            archive_media.insert(normalize_path(&name));
        }
    }
    let mut sorted_names = member_names.clone();
    sorted_names.sort();
    assert_eq!(
        member_names,
        sorted_names,
        "{} members are not globally lexicographic",
        path.display()
    );

    let mut references = Vec::new();
    for (name, bytes) in embedded_presentations {
        let presentation = rv_data::Presentation::decode(bytes.as_slice())
            .unwrap_or_else(|error| panic!("{name} is not a presentation: {error}"));
        references.extend(
            presentation_media_dependencies(&presentation)
                .into_iter()
                .map(|dependency| media_reference(&name, &dependency)),
        );
    }

    audit_media(&references, &archive_media)
}

type EmbeddedPresentationsByName = BTreeMap<String, Vec<(String, rv_data::Presentation)>>;

fn assert_playlist_link_integrity(path: &Path) {
    let package = read_playlist_package(path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    let mut embedded_by_name = EmbeddedPresentationsByName::new();
    let mut embedded_names = BTreeSet::new();
    for file in package
        .embedded_file_details()
        .filter(|file| file.is_presentation)
    {
        let bytes = package
            .embedded_file(&file.name)
            .unwrap_or_else(|| panic!("{} has no retained bytes", file.name));
        let presentation = rv_data::Presentation::decode(bytes)
            .unwrap_or_else(|error| panic!("{} is not a presentation: {error}", file.name));
        embedded_by_name
            .entry(file.basename.to_ascii_lowercase())
            .or_default()
            .push((file.name.clone(), presentation));
        embedded_names.insert(file.name.clone());
    }

    let mut referenced_names = BTreeSet::new();
    let mut failures = Vec::new();
    let document = package.document();
    for root in [
        document.root_node.as_ref(),
        document.live_video_playlist.as_ref(),
        document.downloads_playlist.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        audit_playlist_links(
            root,
            &embedded_by_name,
            &mut referenced_names,
            &mut failures,
        );
    }

    for name in embedded_names.difference(&referenced_names) {
        failures.push(format!(
            "embedded presentation {name:?} is not linked by any playlist item"
        ));
    }
    assert!(
        failures.is_empty(),
        "playlist link integrity failed for {}:\n{}",
        path.display(),
        failures.join("\n")
    );
}

fn audit_playlist_links(
    node: &rv_data::Playlist,
    embedded_by_name: &EmbeddedPresentationsByName,
    referenced_names: &mut BTreeSet<String>,
    failures: &mut Vec<String>,
) {
    for child in &node.children {
        audit_playlist_links(child, embedded_by_name, referenced_names, failures);
    }
    match &node.children_type {
        Some(playlist::ChildrenType::Playlists(playlists)) => {
            for child in &playlists.playlists {
                audit_playlist_links(child, embedded_by_name, referenced_names, failures);
            }
        }
        Some(playlist::ChildrenType::Items(items)) => {
            for item in &items.items {
                audit_playlist_item_link(item, embedded_by_name, referenced_names, failures);
            }
        }
        None => {}
    }
}

fn audit_playlist_item_link(
    item: &rv_data::PlaylistItem,
    embedded_by_name: &EmbeddedPresentationsByName,
    referenced_names: &mut BTreeSet<String>,
    failures: &mut Vec<String>,
) {
    match &item.item_type {
        Some(playlist_item::ItemType::Presentation(selected)) => {
            audit_presentation_link(item, selected, embedded_by_name, referenced_names, failures);
        }
        Some(playlist_item::ItemType::PlanningCenter(planning_center)) => {
            if let Some(linked) = planning_center.linked_data.as_deref() {
                audit_playlist_item_link(linked, embedded_by_name, referenced_names, failures);
            }
        }
        Some(playlist_item::ItemType::Placeholder(placeholder)) => {
            if let Some(linked) = placeholder.linked_data.as_deref() {
                audit_playlist_item_link(linked, embedded_by_name, referenced_names, failures);
            }
        }
        _ => {}
    }
}

fn audit_presentation_link(
    item: &rv_data::PlaylistItem,
    selected: &playlist_item::Presentation,
    embedded_by_name: &EmbeddedPresentationsByName,
    referenced_names: &mut BTreeSet<String>,
    failures: &mut Vec<String>,
) {
    let Some(filename) = linked_presentation_filename(selected) else {
        failures.push(format!(
            "playlist item {:?} has no usable linked .pro filename",
            item.name
        ));
        return;
    };
    let Some(candidates) = embedded_by_name.get(&filename.to_ascii_lowercase()) else {
        failures.push(format!(
            "playlist item {:?} links {filename:?}, which is not embedded",
            item.name
        ));
        return;
    };
    if candidates.len() != 1 {
        failures.push(format!(
            "playlist item {:?} links ambiguous embedded filename {filename:?}: {:?}",
            item.name,
            candidates
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>()
        ));
        return;
    }

    let (archive_name, linked_presentation) = &candidates[0];
    referenced_names.insert(archive_name.clone());

    let Some(selected_uuid) = selected.arrangement.as_ref() else {
        return;
    };
    // Native files commonly retain a stale arrangement UUID while leaving the
    // name empty. Only a complete UUID/name pair expresses a selection that
    // can be checked without guessing.
    if selected.arrangement_name.trim().is_empty() {
        return;
    }
    let matches = linked_presentation.arrangements.iter().any(|arrangement| {
        arrangement.uuid.as_ref().is_some_and(|uuid| {
            uuid.string.eq_ignore_ascii_case(&selected_uuid.string)
                && arrangement.name == selected.arrangement_name
        })
    });
    if !matches {
        failures.push(format!(
            "playlist item {:?} selects arrangement UUID {:?} named {:?}, but {archive_name:?} has no exact match",
            item.name, selected_uuid.string, selected.arrangement_name
        ));
    }
}

fn linked_presentation_filename(selected: &playlist_item::Presentation) -> Option<String> {
    let document_path = selected.document_path.as_ref()?;
    let path = match &document_path.relative_file_path {
        Some(url::RelativeFilePath::Local(local)) => Some(local.path.as_str()),
        Some(url::RelativeFilePath::External(external)) => Some(external.path.as_str()),
        None => match &document_path.storage {
            Some(url::Storage::AbsoluteString(path) | url::Storage::RelativePath(path)) => {
                Some(path.as_str())
            }
            None => None,
        },
    }?;
    presentation_filename(path)
}

fn presentation_filename(path: &str) -> Option<String> {
    let filename = path
        .trim()
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|filename| !filename.is_empty())?;
    let filename = percent_decode(filename)?;
    Path::new(&filename)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pro"))
        .then_some(filename)
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = bytes.get(index + 1).and_then(|byte| hex_value(*byte))?;
            let low = bytes.get(index + 2).and_then(|byte| hex_value(*byte))?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn assert_native_end_records(path: &Path) {
    const TRAILER_SIZE: usize = 98;
    const ZIP64_END: u32 = 0x0606_4b50;
    const ZIP64_LOCATOR: u32 = 0x0706_4b50;
    const LEGACY_END: u32 = 0x0605_4b50;

    let bytes = std::fs::read(path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    assert!(
        bytes.len() >= TRAILER_SIZE,
        "{} is too short for the native ZIP64 trailer",
        path.display()
    );
    let zip64_end = bytes.len() - TRAILER_SIZE;
    let locator = bytes.len() - 42;
    let legacy_end = bytes.len() - 22;
    assert_eq!(read_u32(&bytes, zip64_end), ZIP64_END, "{}", path.display());
    assert_eq!(
        read_u32(&bytes, locator),
        ZIP64_LOCATOR,
        "{}",
        path.display()
    );
    assert_eq!(
        read_u32(&bytes, legacy_end),
        LEGACY_END,
        "{}",
        path.display()
    );

    let central_offset = read_u64(&bytes, zip64_end + 48);
    let expected_size = u64::try_from(bytes.len())
        .expect("archive length fits u64")
        .checked_sub(central_offset)
        .expect("central directory offset is inside archive");
    assert_eq!(
        read_u64(&bytes, zip64_end + 40),
        expected_size,
        "{} ZIP64 central-directory size omits native trailer",
        path.display()
    );
    assert_eq!(
        u64::from(read_u32(&bytes, legacy_end + 12)),
        expected_size,
        "{} legacy central-directory size omits native trailer",
        path.display()
    );
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("four-byte integer"),
    )
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .expect("eight-byte integer"),
    )
}

fn report_audit(path: &Path, audit: &MediaAudit, verbose: bool) {
    eprintln!(
        "native media audit inference for {}: exact={}, inferred_rebases={}, extra_archive_media={}; inferred rebases are unique-basename evidence, not proof of relocatable import",
        path.display(),
        audit.exact_matches,
        audit.inferred_rebases.len(),
        audit.extra_archive_media.len(),
    );
    if verbose && !audit.inferred_rebases.is_empty() {
        eprintln!(
            "inferred native media rebases={:#?}",
            audit.inferred_rebases
        );
    }
    if verbose && !audit.extra_archive_media.is_empty() {
        eprintln!(
            "extra archive media retained by native export={:#?}",
            audit.extra_archive_media
        );
    }
}

fn assert_media_complete(path: &Path, audit: &MediaAudit) {
    assert!(
        audit.missing.is_empty() && audit.ambiguous.is_empty(),
        "native media audit failed for {}:\nmissing archive members={:#?}\nambiguous basename matches={:#?}\ninferred native rebases={:#?}\nextra archive media (diagnostic only)={:#?}",
        path.display(),
        audit.missing,
        audit.ambiguous,
        audit.inferred_rebases,
        audit.extra_archive_media,
    );
}

fn media_reference(presentation: &str, dependency: &MediaDependency) -> MediaReference {
    let path = dependency.stored_absolute_path().map_or_else(
        || normalize_path(dependency.source()),
        |path| normalize_path(&path.to_string_lossy()),
    );
    let basename = dependency
        .basename()
        .and_then(normalized_basename)
        .or_else(|| normalized_basename(&path));
    MediaReference {
        presentation: presentation.to_string(),
        source: dependency.source().to_string(),
        path,
        basename,
    }
}

fn audit_media(references: &[MediaReference], archive_media: &BTreeSet<String>) -> MediaAudit {
    let mut archive_by_basename = BTreeMap::<String, Vec<String>>::new();
    for path in archive_media {
        if let Some(basename) = normalized_basename(path) {
            archive_by_basename
                .entry(basename)
                .or_default()
                .push(path.clone());
        }
    }

    let mut audit = MediaAudit::default();
    let mut matched_archive_media = BTreeSet::new();
    for reference in references {
        if archive_media.contains(&reference.path) {
            audit.exact_matches += 1;
            matched_archive_media.insert(reference.path.clone());
            continue;
        }

        let Some(basename) = reference.basename.as_deref() else {
            audit.missing.push(format!(
                "{}: {:?} has no filename",
                reference.presentation, reference.source
            ));
            continue;
        };
        match archive_by_basename.get(basename).map(Vec::as_slice) {
            Some([archive_member]) => {
                matched_archive_media.insert(archive_member.clone());
                audit.inferred_rebases.push(InferredRebase {
                    presentation: reference.presentation.clone(),
                    source: reference.source.clone(),
                    archive_member: archive_member.clone(),
                });
            }
            Some(candidates) => audit.ambiguous.push(format!(
                "{}: {:?} matches {candidates:?}",
                reference.presentation, reference.source
            )),
            None => audit.missing.push(format!(
                "{}: {:?} (normalized basename {basename:?})",
                reference.presentation, reference.source
            )),
        }
    }

    audit.extra_archive_media = archive_media
        .difference(&matched_archive_media)
        .cloned()
        .collect();
    audit
}

fn is_presentation_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pro"))
}

fn native_archive_member_name(file: &zip::read::ZipFile<'_>) -> String {
    std::str::from_utf8(file.name_raw()).map_or_else(
        |_| file.name().to_string(),
        std::string::ToString::to_string,
    )
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn normalized_basename(path: &str) -> Option<String> {
    normalize_path(path)
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|basename| !basename.is_empty())
        .map(str::to_lowercase)
}

#[test]
fn classifies_exact_rebased_missing_ambiguous_and_extra_media() {
    let archive_media = [
        "/exporter/Media/exact.png",
        "/exporter/Media/rebased.jpg",
        "/one/duplicate.mov",
        "/two/DUPLICATE.mov",
        "/exporter/Media/extra.png",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    let references = vec![
        reference("exact.pro", "/exporter/Media/exact.png"),
        reference("windows.pro", r"C:\source\Media\REBASED.jpg"),
        reference("missing.pro", "/source/Media/missing.tif"),
        reference("ambiguous.pro", "/source/Media/duplicate.mov"),
    ];

    let audit = audit_media(&references, &archive_media);

    assert_eq!(audit.exact_matches, 1);
    assert_eq!(
        audit.inferred_rebases,
        vec![InferredRebase {
            presentation: "windows.pro".to_string(),
            source: r"C:\source\Media\REBASED.jpg".to_string(),
            archive_member: "/exporter/Media/rebased.jpg".to_string(),
        }]
    );
    assert_eq!(audit.missing.len(), 1);
    assert_eq!(audit.ambiguous.len(), 1);
    assert_eq!(
        audit.extra_archive_media,
        vec![
            "/exporter/Media/extra.png".to_string(),
            "/one/duplicate.mov".to_string(),
            "/two/DUPLICATE.mov".to_string(),
        ]
    );
}

fn reference(presentation: &str, path: &str) -> MediaReference {
    MediaReference {
        presentation: presentation.to_string(),
        source: path.to_string(),
        path: normalize_path(path),
        basename: normalized_basename(path),
    }
}
