#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::path::Path;

use prost::Message;
use uuid::Uuid;

use super::*;
use crate::propresenter::generated::rv_data::{self, playlist};
use crate::propresenter::package::{presentation_items, PlaylistArchiveShape};
use crate::propresenter::SlideType;

fn presentation_bytes(name: &str) -> Vec<u8> {
    rv_data::Presentation {
        name: name.to_string(),
        uuid: Some(rv_data::Uuid {
            string: Uuid::new_v4().to_string(),
        }),
        ..rv_data::Presentation::default()
    }
    .encode_to_vec()
}

fn test_metadata() -> PlaylistMetadata {
    PlaylistMetadata::offline_test()
}

fn linked_entry(name: &str, path: &str) -> PlaylistEntry {
    PlaylistEntry::linked(name, path).expect("valid linked playlist entry")
}

fn embedded_entry(name: &str, path: &str) -> PlaylistEntry {
    PlaylistEntry::embedded(name, path, presentation_bytes(name))
        .expect("valid embedded playlist entry")
}

mod document;
mod naming;
mod package;
