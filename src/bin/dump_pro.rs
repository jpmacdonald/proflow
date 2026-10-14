//! Debug tool to dump and analyze `ProPresenter` `.pro` files.
//!
//! Usage:
//!   `cargo run --features dev-tools --bin dump_pro -- <file.pro>`
//!   `cargo run --features dev-tools --bin dump_pro -- <file1.pro> <file2.pro> --diff`
//!
//! This tool outputs a detailed structure of the presentation for debugging
//! slide generation issues.

// Development/debug binary - allow expect/unwrap for simpler error handling
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::uninlined_format_args,
    clippy::too_many_lines
)]

#[path = "dump_pro/presentation.rs"]
mod presentation;
use presentation::dump_presentation;
#[path = "dump_pro/slides.rs"]
mod slides;
use slides::dump_presentation_slide;
#[path = "dump_pro/comparison.rs"]
mod comparison;
use comparison::diff_presentations;

use propresenter::unstable_native::rv_data;
use prost::Message;
use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <file.pro> [file2.pro --diff]", args[0]);
        eprintln!("       {} <file.pro> --json", args[0]);
        std::process::exit(1);
    }

    let path = Path::new(&args[1]);

    if args.contains(&"--json".to_string()) {
        dump_json(path);
    } else if args.len() >= 4 && args.contains(&"--diff".to_string()) {
        let path2 = Path::new(&args[2]);
        diff_presentations(path, path2);
    } else {
        dump_presentation(path);
    }
}

fn load_presentation(path: &Path) -> rv_data::Presentation {
    let data = fs::read(path).unwrap_or_else(|e| {
        eprintln!("Failed to read {}: {e}", path.display());
        std::process::exit(1);
    });

    rv_data::Presentation::decode(data.as_slice()).unwrap_or_else(|e| {
        eprintln!("Failed to decode {}: {e}", path.display());
        std::process::exit(1);
    })
}

fn dump_json(path: &Path) {
    let presentation = load_presentation(path);
    println!(
        "{}",
        serde_json::to_string_pretty(&presentation)
            .expect("failed to serialize presentation as JSON")
    );
}
