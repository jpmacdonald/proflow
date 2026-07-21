//! Materialize one playlist from a live `ProPresenter` `Playlists/Library` file.
//!
//! This is a parity helper: real services in the Dropbox show directory are
//! stored in the raw playlist library document, not as exported `.proplaylist`
//! packages. This tool extracts one named playlist, embeds its linked
//! presentations from `Libraries/Default`, and writes a comparable
//! `.proplaylist` package.
//!
//! Usage:
//!   `cargo run --features dev-tools --bin materialize_real_playlist -- <ProPresenter root> <playlist name> <output.proplaylist>`

#![allow(clippy::print_stderr, clippy::print_stdout)]

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use proflow::propresenter::live::materialize_live_playlist;

fn main() -> ExitCode {
    match run() {
        Ok(path) => {
            println!("{}", path.display());
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("{err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<PathBuf> {
    let mut args = std::env::args_os().skip(1);
    let root = args.next().map(PathBuf::from).context(
        "usage: materialize_real_playlist <ProPresenter root> <playlist name> <output.proplaylist>",
    )?;
    let playlist_name = args
        .next()
        .map(|value| value.to_string_lossy().to_string())
        .context(
            "usage: materialize_real_playlist <ProPresenter root> <playlist name> <output.proplaylist>",
        )?;
    let output_path = args.next().map(PathBuf::from).context(
        "usage: materialize_real_playlist <ProPresenter root> <playlist name> <output.proplaylist>",
    )?;
    if args.next().is_some() {
        anyhow::bail!(
            "usage: materialize_real_playlist <ProPresenter root> <playlist name> <output.proplaylist>"
        );
    }

    let report = materialize_live_playlist(&root, &playlist_name, &output_path)?;
    if !report.non_presentation_items.is_empty() {
        eprintln!(
            "retained {} non-presentation playlist item(s) in {:?} without embedded .pro members",
            report.non_presentation_items.len(),
            playlist_name
        );
    }

    Ok(output_path)
}
