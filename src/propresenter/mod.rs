//! ProPresenter file format support.
//!
//! This module provides types and utilities for reading, writing, and
//! manipulating ProPresenter presentation files (.pro).

pub mod analyze;
pub mod builder;
pub mod convert;
pub mod data_model;
pub mod deserialize;
pub mod generated;
pub mod parser;
pub mod rtf;
pub mod serialize;
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
