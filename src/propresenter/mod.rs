//! `ProPresenter` file format support.
//!
//! This module provides types and utilities for reading, writing, and
//! manipulating `ProPresenter` presentation files (.pro) and playlist files (.proplaylist).

/// Presentation analysis tools.
pub mod analyze;
/// Builder pattern for creating presentations.
pub mod builder;
/// Conversion between data model and protobuf types.
pub mod convert;
/// High-level data model types.
pub mod data_model;
/// File deserialization (reading .pro files).
pub mod deserialize;
/// Export editor content to .pro files.
pub mod export;
/// Extract plain text from presentations.
pub mod extract;
/// Generated protobuf types.
pub mod generated;
/// Presentation comparison and parsing utilities.
pub mod parser;
/// Playlist file support (.proplaylist).
pub mod playlist;
/// Template-based slide generation.
pub mod template;
/// RTF conversion utilities.
pub mod rtf;
/// File serialization (writing .pro files).
pub mod serialize;
/// UUID generation utilities.
pub mod uuid;

// Re-export commonly used types (suppress warnings for public API)
#[allow(unused_imports)]
pub use builder::*;
#[allow(unused_imports)]
pub use data_model::{
    BaseSlide, Color, Element, Font, ParagraphStyle, Point, Presentation, Rect, Shadow, Size,
    Slide, TextElement,
};
#[allow(unused_imports)]
pub use deserialize::*;
#[allow(unused_imports)]
pub use parser::*;
#[allow(unused_imports)]
pub use serialize::*;
