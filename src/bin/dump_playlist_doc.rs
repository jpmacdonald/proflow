//! Debug tool to dump a raw `ProPresenter` playlist document.
//!
//! Usage:
//!   `cargo run --features dev-tools --bin dump_playlist_doc -- <Playlists/Library>`

#![allow(clippy::expect_used, clippy::unwrap_used)]

use proflow::propresenter::package::read_playlist_package;
use proflow::propresenter::unstable_native::rv_data::{self, playlist, playlist_item};
use prost::Message;
use std::{env, fs, path::Path};

fn main() {
    let path = env::args().nth(1).expect("usage: dump_playlist_doc <path>");
    let path = Path::new(&path);
    let doc = read_playlist_package(path).map_or_else(
        |_| {
            let data = fs::read(path).expect("read playlist document");
            rv_data::PlaylistDocument::decode(data.as_slice()).expect("decode PlaylistDocument")
        },
        proflow::propresenter::package::PlaylistPackage::into_document,
    );

    println!(
        "PlaylistDocument type={} tags={} application={:?}",
        doc.r#type,
        doc.tags.len(),
        doc.application_info
    );
    if let Some(root) = &doc.root_node {
        dump_playlist(root, 0);
    }
}

fn dump_playlist(node: &rv_data::Playlist, indent: usize) {
    let pad = "  ".repeat(indent);
    println!(
        "{pad}- playlist name={:?} type={} expanded={} uuid={:?}",
        node.name,
        node.r#type,
        node.expanded,
        node.uuid.as_ref().map(|u| u.string.as_str())
    );

    for child in &node.children {
        dump_playlist(child, indent + 1);
    }

    match &node.children_type {
        Some(playlist::ChildrenType::Playlists(playlists)) => {
            for child in &playlists.playlists {
                dump_playlist(child, indent + 1);
            }
        }
        Some(playlist::ChildrenType::Items(items)) => {
            for (idx, item) in items.items.iter().enumerate() {
                dump_item(item, indent + 1, idx);
            }
        }
        None => {}
    }
}

fn dump_item(item: &rv_data::PlaylistItem, indent: usize, idx: usize) {
    let pad = "  ".repeat(indent);
    match &item.item_type {
        Some(playlist_item::ItemType::Presentation(presentation)) => {
            let loc = presentation.document_path.as_ref();
            println!(
                "{pad}{idx:02}. presentation name={:?} arrangement={:?} arrangement_name={:?} user_music_key={:?} absolute={:?} relative={:?}",
                item.name,
                presentation.arrangement.as_ref().map(|u| u.string.as_str()),
                presentation.arrangement_name,
                presentation.user_music_key,
                loc.and_then(|url| url.storage.as_ref()).map(|s| format!("{s:?}")),
                loc.and_then(|url| url.relative_file_path.as_ref())
            );
        }
        Some(playlist_item::ItemType::Header(header)) => println!(
            "{pad}{idx:02}. header name={:?} actions={}",
            item.name,
            header.actions.len()
        ),
        Some(playlist_item::ItemType::Cue(_)) => {
            println!("{pad}{idx:02}. cue name={:?}", item.name);
        }
        Some(playlist_item::ItemType::PlanningCenter(_)) => {
            println!("{pad}{idx:02}. planning_center name={:?}", item.name);
        }
        Some(playlist_item::ItemType::Placeholder(_)) => {
            println!("{pad}{idx:02}. placeholder name={:?}", item.name);
        }
        None => println!("{pad}{idx:02}. item name={:?} type=none", item.name),
    }
}
