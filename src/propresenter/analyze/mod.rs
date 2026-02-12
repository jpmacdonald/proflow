//! Presentation analysis tools.

#![allow(dead_code)]

use crate::propresenter::{deserialize, parser};
use std::path::Path;

/// Compare two presentation files and print their differences.
pub fn compare_presentations(original_path: &Path, recreated_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let original = deserialize::read_presentation_file(original_path)?;
    let recreated = deserialize::read_presentation_file(recreated_path)?;

    let diff = parser::compare_presentations(&original, &recreated)?;
    println!("{diff}");

    Ok(())
} 