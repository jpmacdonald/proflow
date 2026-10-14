//! Rename one cue group of a `ProPresenter` presentation, in place.
//!
//! ```text
//! rename_group <file.pro> <current name> <new name>
//! ```
//!
//! The file is edited only if every byte of it survives a decode and
//! re-encode unchanged, exactly one group has the current name, and no group
//! already has the new one. The original is kept beside it as
//! `<file>.before-rename`. Only the group's name changes.
#![allow(clippy::print_stdout)]

use std::path::PathBuf;

use anyhow::{bail, Context};
use propresenter::native_document::OpaquePresentation;

fn main() -> anyhow::Result<()> {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let [file, current, new] = arguments.as_slice() else {
        bail!("usage: rename_group <file.pro> <current name> <new name>");
    };
    let file = PathBuf::from(file);
    let original = std::fs::read(&file).with_context(|| format!("read {}", file.display()))?;
    let mut editable =
        OpaquePresentation::decode(&original, file.display().to_string())?.try_into_editable()?;

    let groups = &mut editable.presentation_mut().cue_groups;
    let named = |name: &str| {
        groups
            .iter()
            .filter(|group| group.group.as_ref().is_some_and(|group| group.name == name))
            .count()
    };
    if named(new) != 0 {
        bail!("a group named '{new}' already exists");
    }
    if named(current) != 1 {
        bail!(
            "expected exactly one group named '{current}', found {}",
            named(current)
        );
    }
    for group in groups.iter_mut().filter_map(|group| group.group.as_mut()) {
        if group.name == *current {
            group.name.clone_from(new);
        }
    }

    let edited = editable.encode()?;
    let backup = file.with_extension("pro.before-rename");
    std::fs::write(&backup, &original).with_context(|| format!("write {}", backup.display()))?;
    let temporary = file.with_extension("pro.renaming");
    std::fs::write(&temporary, &edited)?;
    std::fs::rename(&temporary, &file)?;
    println!(
        "renamed group '{current}' to '{new}' in {}; original kept at {}",
        file.display(),
        backup.display()
    );
    Ok(())
}
