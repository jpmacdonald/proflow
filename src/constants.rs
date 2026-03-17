//! Application constants.
//!
//! Centralizes magic numbers and configuration values for better maintainability.

/// Editor configuration constants.
pub mod editor {
    /// Default wrap column width for text wrapping.
    pub const DEFAULT_WRAP_COLUMN: usize = 80;

    /// Minimum wrap column width allowed.
    pub const MIN_WRAP_COLUMN: usize = 10;

    /// Default viewport height in lines.
    pub const DEFAULT_VIEWPORT_HEIGHT: usize = 20;
}

/// Search and file matching constants.
pub mod search {
    /// Maximum number of search results to display.
    pub const MAX_SEARCH_RESULTS: usize = 20;
}

/// Template and slide generation constants.
pub mod template {
    /// Default maximum visual lines per slide.
    pub const DEFAULT_MAX_LINES_PER_SLIDE: usize = 10;

    /// Minimum wrap column for slide splitting.
    pub const MIN_SLIDE_WRAP: usize = 20;
}

/// Async task constants.
pub mod async_tasks {
    /// Channel buffer size for async task communication.
    pub const CHANNEL_BUFFER_SIZE: usize = 10;
}
