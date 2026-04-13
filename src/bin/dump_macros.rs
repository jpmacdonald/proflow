//! Dump `ProPresenter` macros for inspection.
#![allow(missing_docs, clippy::unwrap_used, clippy::expect_used)]

use proflow::propresenter::generated::rv_data;
use prost::Message;

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        let home = dirs::home_dir().expect("no home dir");
        home.join("Documents/ProPresenter/Configuration/Macros")
            .to_string_lossy()
            .to_string()
    });

    let data = std::fs::read(&path).expect("Failed to read macros file");
    let doc = rv_data::MacrosDocument::decode(data.as_slice()).expect("Failed to decode");

    println!("Macros: {} total\n", doc.macros.len());

    for m in &doc.macros {
        let uuid = m.uuid.as_ref().map_or("???", |u| u.string.as_str());
        println!("  {} ({})", m.name, uuid);
        for (i, action) in m.actions.iter().enumerate() {
            println!("    action {i}: type={}", action.r#type);
        }
    }

    if !doc.macro_collections.is_empty() {
        println!("\nCollections:");
        for coll in &doc.macro_collections {
            let uuid = coll.uuid.as_ref().map_or("???", |u| u.string.as_str());
            println!("  {} ({}) — {} items", coll.name, uuid, coll.items.len());
        }
    }
}
