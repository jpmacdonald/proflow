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
