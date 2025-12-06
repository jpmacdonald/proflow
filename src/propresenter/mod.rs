pub mod builder;
pub mod data_model;
pub mod deserialize;
pub mod generated;
pub mod parser;
pub mod rtf;
pub mod serialize;
pub mod convert;
pub mod uuid;
pub mod analyze;

// Re-export commonly used types
pub use builder::*;
pub use data_model::{
    Presentation, Slide, BaseSlide, Element, TextElement, Color, 
    Font, Shadow, Point, Size, Rect, ParagraphStyle
};
pub use deserialize::*;
pub use parser::*;
pub use serialize::*;