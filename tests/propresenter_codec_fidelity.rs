//! Byte-exact codec gates for native `ProPresenter` documents.
//!
//! `prost` does not retain unknown protobuf fields. These tests therefore act
//! as schema-drift alarms: a native fixture must encode to exactly the bytes
//! that `ProPresenter` wrote after it has been decoded by `ProFlow`.

#![allow(clippy::expect_used, clippy::panic)]

use proflow::propresenter::deserialize::{detect_presentation_file_format, PresentationFileFormat};
use proflow::propresenter::generated::rv_data;
use proflow::propresenter::package::read_playlist_package;
use prost::Message;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const NATIVE_PRESENTATION_FIXTURES: &[&str] = &[
    "tests/fixtures/propresenter/native/examples/title-nametag.pro",
    "tests/fixtures/propresenter/native/examples/hymn-amazing-grace.pro",
    "tests/fixtures/propresenter/native/examples/scripture-titus-2v11-13-nrsvue.pro",
    "tests/fixtures/propresenter/native/corpus/presentations/announcements.pro",
    "tests/fixtures/propresenter/native/corpus/presentations/call-to-worship.pro",
    "tests/fixtures/propresenter/native/corpus/presentations/heidelberg-catechism-question-1.pro",
    "tests/fixtures/propresenter/native/corpus/presentations/prayer-of-confession.pro",
    "tests/fixtures/propresenter/native/corpus/presentations/psalm-23-surely-goodness.pro",
    "tests/fixtures/propresenter/native/corpus/presentations/we-walk-by-faith-and-not-by-sight.pro",
];

const NATIVE_PLAYLIST_PACKAGES: &[&str] = &[
    "tests/fixtures/propresenter/native/corpus/playlists/native-template-library.proplaylist",
    "tests/fixtures/propresenter/native/corpus/playlists/2026-04-19-1030-traditional.proplaylist",
    "tests/fixtures/propresenter/native/corpus/playlists/2026-04-19-0900-contemporary.proplaylist",
];

const NATIVE_PACKAGE_PRESENTATIONS: &[(&str, &[&str])] = &[(
    "tests/fixtures/propresenter/native/corpus/playlists/native-template-library.proplaylist",
    &[
        "__template_info__.pro",
        "__template_scripture__.pro",
        "__template_song__.pro",
    ],
)];

fn repository_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn first_difference(expected: &[u8], actual: &[u8]) -> Option<usize> {
    expected
        .iter()
        .zip(actual)
        .position(|(expected, actual)| expected != actual)
        .or_else(|| (expected.len() != actual.len()).then_some(expected.len().min(actual.len())))
}

fn assert_exact_round_trip<M>(relative_path: &str)
where
    M: Message + Default,
{
    let path = repository_path(relative_path);
    let original = fs::read(&path).unwrap_or_else(|error| {
        panic!("failed to read native fixture {}: {error}", path.display())
    });
    assert_exact_bytes::<M>(&path.display().to_string(), &original);
}

fn assert_exact_bytes<M>(source: &str, original: &[u8])
where
    M: Message + Default,
{
    let decoded = M::decode(original)
        .unwrap_or_else(|error| panic!("failed to decode native document {source}: {error}"));
    let encoded = decoded.encode_to_vec();

    assert!(
        original == encoded,
        "native document {source} lost or changed protobuf data: input={} bytes, output={} bytes, first difference={:?}",
        original.len(),
        encoded.len(),
        first_difference(original, &encoded),
    );
}

fn read_zip_entry(relative_path: &str, entry_name: &str) -> Vec<u8> {
    let path = repository_path(relative_path);
    read_zip_entry_at(&path, entry_name)
}

fn read_zip_entry_at(path: &Path, entry_name: &str) -> Vec<u8> {
    let file = fs::File::open(path).unwrap_or_else(|error| {
        panic!(
            "failed to open playlist package {}: {error}",
            path.display()
        )
    });
    let mut archive = zip::ZipArchive::new(file).unwrap_or_else(|error| {
        panic!(
            "failed to read playlist package {} as ZIP: {error}",
            path.display()
        )
    });
    let mut entry = archive.by_name(entry_name).unwrap_or_else(|error| {
        panic!(
            "playlist package {} has no {entry_name:?} entry: {error}",
            path.display()
        )
    });
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).unwrap_or_else(|error| {
        panic!(
            "failed to read {entry_name:?} from playlist package {}: {error}",
            path.display()
        )
    });
    bytes
}

#[test]
fn graphics_point_preserves_explicit_negative_zero_x() {
    const WIRE_POINT: &[u8] = &[
        0x09, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, // x = -0.0
        0x11, 0x66, 0x66, 0x66, 0x66, 0x66, 0x22, 0x81, 0x40, // y = 548.3
    ];

    let point = rv_data::graphics::Point::decode(WIRE_POINT)
        .expect("native point wire value should decode");
    assert_eq!(
        point.x.map(f64::to_bits),
        Some((-0.0_f64).to_bits()),
        "the explicit negative-zero field must remain present"
    );
    assert_eq!(point.encode_to_vec(), WIRE_POINT);
}

#[test]
fn native_presentations_round_trip_byte_exactly() {
    for fixture in NATIVE_PRESENTATION_FIXTURES {
        assert_exact_round_trip::<rv_data::Presentation>(fixture);
    }
}

#[test]
fn packaged_playlist_documents_round_trip_byte_exactly() {
    for package in NATIVE_PLAYLIST_PACKAGES {
        let data = read_zip_entry(package, "data");
        assert_exact_bytes::<rv_data::PlaylistDocument>(&format!("{package}:data"), &data);
    }
}

#[test]
fn packaged_presentations_round_trip_byte_exactly() {
    for (package, presentations) in NATIVE_PACKAGE_PRESENTATIONS {
        for presentation in *presentations {
            let data = read_zip_entry(package, presentation);
            assert_exact_bytes::<rv_data::Presentation>(
                &format!("{package}:{presentation}"),
                &data,
            );
        }
    }
}

#[test]
#[ignore = "set PROFLOW_LIVE_LIBRARY_DIR to audit a read-only local ProPresenter library"]
fn live_native_presentations_round_trip_byte_exactly() {
    let library = std::env::var_os("PROFLOW_LIVE_LIBRARY_DIR")
        .map(PathBuf::from)
        .expect("PROFLOW_LIVE_LIBRARY_DIR must point to a ProPresenter library directory");
    let mut presentations = Vec::new();
    let mut failures = Vec::new();
    let mut excluded = Vec::new();
    let mut audited_count = 0usize;
    for entry in WalkDir::new(&library).follow_links(false) {
        match entry {
            Ok(entry) if entry.file_type().is_file() => {
                let path = entry.into_path();
                if path
                    .extension()
                    .and_then(std::ffi::OsStr::to_str)
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("pro"))
                {
                    presentations.push(path);
                }
            }
            Ok(_) => {}
            Err(error) => failures.push(format!("library traversal failed: {error}")),
        }
    }
    presentations.sort();

    assert!(
        !presentations.is_empty(),
        "no .pro files found under {}",
        library.display()
    );

    for path in &presentations {
        let original = match fs::read(path) {
            Ok(original) => original,
            Err(error) => {
                failures.push(format!("{}: read failed: {error}", path.display()));
                continue;
            }
        };
        let format = detect_presentation_file_format(&original);
        if format != PresentationFileFormat::NativePresentation {
            excluded.push(format!("{}: {format}", path.display()));
            continue;
        }
        audited_count += 1;
        let decoded = match rv_data::Presentation::decode(original.as_slice()) {
            Ok(decoded) => decoded,
            Err(error) => {
                failures.push(format!("{}: decode failed: {error}", path.display()));
                continue;
            }
        };
        let encoded = decoded.encode_to_vec();
        if original != encoded {
            failures.push(format!(
                "{}: input={} bytes, output={} bytes, first difference={:?}",
                path.display(),
                original.len(),
                encoded.len(),
                first_difference(&original, &encoded),
            ));
        }
    }

    if !excluded.is_empty() {
        eprintln!(
            "excluded {} non-presentation .pro files from the native codec audit:\n{}",
            excluded.len(),
            excluded.join("\n")
        );
    }
    eprintln!(
        "audited {audited_count} native presentations byte-exactly under {}",
        library.display()
    );
    assert!(audited_count > 0, "no native presentations found");

    assert!(
        failures.is_empty(),
        "{} of {} native presentations were not byte-exact:\n{}",
        failures.len(),
        audited_count,
        failures.join("\n"),
    );
}

#[test]
#[ignore = "set PROFLOW_LIVE_LIBRARY_DIR to audit a read-only local ProPresenter playlist"]
fn live_playlist_document_round_trips_byte_exactly() {
    let library = std::env::var_os("PROFLOW_LIVE_LIBRARY_DIR")
        .map(PathBuf::from)
        .expect("PROFLOW_LIVE_LIBRARY_DIR must point to a ProPresenter library directory");
    let propresenter_root = library
        .parent()
        .and_then(Path::parent)
        .expect("PROFLOW_LIVE_LIBRARY_DIR must end with Libraries/<library name>");
    let playlist_path = propresenter_root.join("Playlists/Library");
    let original = fs::read(&playlist_path).unwrap_or_else(|error| {
        panic!(
            "failed to read native playlist {}: {error}",
            playlist_path.display()
        )
    });

    assert_exact_bytes::<rv_data::PlaylistDocument>(
        &playlist_path.display().to_string(),
        &original,
    );
}

#[test]
#[ignore = "set PROFLOW_LIVE_PLAYLIST_EXPORT_DIR to audit native playlist exports"]
#[allow(
    clippy::too_many_lines,
    reason = "the corpus audit keeps its counters and final aggregate diagnostics in one test"
)]
fn live_exported_playlist_documents_round_trip_byte_exactly() {
    let root = std::env::var_os("PROFLOW_LIVE_PLAYLIST_EXPORT_DIR")
        .map(PathBuf::from)
        .expect("PROFLOW_LIVE_PLAYLIST_EXPORT_DIR must point to exported playlists");
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
        "no playlist exports under {}",
        root.display()
    );

    let mut failures = Vec::new();
    let mut presentation_count = 0usize;
    for path in &packages {
        let package = match read_playlist_package(path) {
            Ok(package) => package,
            Err(error) => {
                failures.push(format!("{}: package read failed: {error}", path.display()));
                continue;
            }
        };
        if !package.document_round_trip_exact {
            let encoded = package.document.encode_to_vec();
            failures.push(format!(
                "{}: data input={} bytes, output={} bytes, first difference={:?}",
                path.display(),
                package.document_data.len(),
                encoded.len(),
                first_difference(&package.document_data, &encoded),
            ));
        }
        for file in package
            .embedded_file_details
            .iter()
            .filter(|file| file.is_presentation)
        {
            presentation_count += 1;
            let Some(original) = package.embedded_file_data.get(&file.name) else {
                failures.push(format!(
                    "{}: missing retained bytes for {}",
                    path.display(),
                    file.name
                ));
                continue;
            };
            let presentation = match rv_data::Presentation::decode(original.as_slice()) {
                Ok(presentation) => presentation,
                Err(error) => {
                    failures.push(format!(
                        "{}:{}: protobuf decode failed: {error}",
                        path.display(),
                        file.name
                    ));
                    continue;
                }
            };
            let encoded = presentation.encode_to_vec();
            if encoded != *original {
                failures.push(format!(
                    "{}:{}: input={} bytes, output={} bytes, first difference={:?}",
                    path.display(),
                    file.name,
                    original.len(),
                    encoded.len(),
                    first_difference(original, &encoded),
                ));
            }
        }
    }

    eprintln!(
        "audited {} native playlist packages and {presentation_count} embedded presentations under {}",
        packages.len(),
        root.display()
    );
    assert!(
        presentation_count > 0,
        "no embedded native presentations found under {}",
        root.display()
    );

    assert!(
        failures.is_empty(),
        "{} exact round-trip failure(s) across {} exported playlist packages and {presentation_count} embedded presentations:\n{}",
        failures.len(),
        packages.len(),
        failures.join("\n"),
    );
}

#[test]
fn custom_text_font_attribute_uses_wire_tag_twelve() {
    // Captured from a current ProPresenter presentation. The outer message is
    // Graphics.Text.Attributes.CustomAttribute; field 1 is IntRange and field
    // 12 is a Font describing ArialMT 80 Regular.
    const WIRE_BYTES: &[u8] = &[
        0x0a, 0x02, 0x10, 0x0d, 0x62, 0x22, 0x0a, 0x07, b'A', b'r', b'i', b'a', b'l', b'M', b'T',
        0x11, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x54, 0x40, 0x4a, 0x05, b'A', b'r', b'i', b'a',
        b'l', 0x52, 0x07, b'R', b'e', b'g', b'u', b'l', b'a', b'r',
    ];

    let attribute = rv_data::graphics::text::attributes::CustomAttribute::decode(WIRE_BYTES)
        .expect("captured custom font attribute should decode");
    let font = match attribute.attribute.as_ref() {
        Some(rv_data::graphics::text::attributes::custom_attribute::Attribute::OriginalFont(
            font,
        )) => font,
        other => panic!("wire tag 12 should decode as a font, got {other:?}"),
    };

    assert_eq!(font.name, "ArialMT");
    assert_eq!(font.size.to_bits(), 80.0_f64.to_bits());
    assert_eq!(font.family, "Arial");
    assert_eq!(font.face, "Regular");
    assert_eq!(attribute.encode_to_vec(), WIRE_BYTES);
}
