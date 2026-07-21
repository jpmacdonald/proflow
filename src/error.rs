//! Application error types.
//!
//! Provides unified error handling with actionable context for debugging.

use thiserror::Error;

/// Application result type alias
pub type Result<T> = std::result::Result<T, Error>;

/// Application error types with specific context for actionable debugging
#[derive(Debug, Error)]
pub enum Error {
    /// IO error with path context
    #[error("IO error at {path:?}: {source}")]
    Io {
        /// The underlying IO error.
        source: std::io::Error,
        /// File path where the error occurred, if known.
        path: Option<std::path::PathBuf>,
    },

    /// Network error (connection, timeout, DNS)
    #[error("Network error: {0}")]
    Network(String),

    /// `Planning Center` API error with status context
    #[error("Planning Center API error: {message}")]
    PlanningCenter {
        /// Human-readable error description.
        message: String,
        /// HTTP status code, if from an HTTP response.
        status: Option<u16>,
        /// Actionable suggestion for resolving the error.
        hint: Option<&'static str>,
    },

    /// Two consecutive direct Planning Center reads disagreed, so no coherent
    /// normalized plan snapshot was available for review or commit.
    #[error(
        "Planning Center plan '{plan_id}' changed while it was being captured ({first_revision} then {second_revision}); retry after edits settle"
    )]
    PlanningCenterSnapshotUnstable {
        /// Stable plan identity being captured.
        plan_id: String,
        /// Revision of the first direct normalized read.
        first_revision: String,
        /// Revision of the immediately following direct normalized read.
        second_revision: String,
    },

    /// Configuration error with guidance
    #[error("Configuration error: {message}. {hint}")]
    Config {
        /// Description of the configuration problem.
        message: String,
        /// Actionable guidance for fixing the issue.
        hint: &'static str,
    },

    /// File parsing error
    #[error("Parse error in {file:?}: {message}")]
    Parse {
        /// File that failed to parse, if known.
        file: Option<std::path::PathBuf>,
        /// Description of the parse failure.
        message: String,
    },

    /// `ProPresenter` format error
    #[error("ProPresenter format error: {0}")]
    ProPresenter(String),

    /// Scripture lookup error
    #[error("Scripture lookup failed: {0}")]
    Scripture(String),

    /// Playlist generation error
    #[error("Playlist generation failed: {0}")]
    Playlist(String),
}

impl Error {
    /// Create an IO error with path context
    pub fn io(source: std::io::Error, path: impl Into<Option<std::path::PathBuf>>) -> Self {
        Self::Io {
            source,
            path: path.into(),
        }
    }

    /// Create a `Planning Center` error with optional status and hint
    pub fn pco(message: impl Into<String>) -> Self {
        Self::PlanningCenter {
            message: message.into(),
            status: None,
            hint: None,
        }
    }

    /// Create a `Planning Center` error with HTTP status
    pub fn pco_status(message: impl Into<String>, status: u16) -> Self {
        let hint = match status {
            401 => Some("Check PCO_APP_ID and PCO_SECRET environment variables"),
            403 => Some("Your API credentials may lack required permissions"),
            404 => Some("The requested resource was not found"),
            429 => Some("Rate limited - wait a moment and try again"),
            500..=599 => Some("Planning Center server error - try again later"),
            _ => None,
        };
        Self::PlanningCenter {
            message: message.into(),
            status: Some(status),
            hint,
        }
    }

    /// Create a config error with actionable hint
    pub fn config(message: impl Into<String>, hint: &'static str) -> Self {
        Self::Config {
            message: message.into(),
            hint,
        }
    }

    /// Create a parse error with file context
    pub fn parse(message: impl Into<String>, file: impl Into<Option<std::path::PathBuf>>) -> Self {
        Self::Parse {
            file: file.into(),
            message: message.into(),
        }
    }
}

// Domain error conversions
impl From<crate::propresenter::deserialize::ProPresenterError> for Error {
    fn from(e: crate::propresenter::deserialize::ProPresenterError) -> Self {
        Self::ProPresenter(e.to_string())
    }
}

impl From<crate::propresenter::serialize::SerializeError> for Error {
    fn from(e: crate::propresenter::serialize::SerializeError) -> Self {
        Self::ProPresenter(e.to_string())
    }
}

impl From<crate::propresenter::playlist::PlaylistError> for Error {
    fn from(e: crate::propresenter::playlist::PlaylistError) -> Self {
        Self::Playlist(e.to_string())
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io {
            source: e,
            path: None,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn pco_status_provides_hints() {
        let err = Error::pco_status("Unauthorized", 401);
        match err {
            Error::PlanningCenter { hint: Some(h), .. } => {
                assert!(h.contains("PCO_APP_ID"));
            }
            _ => panic!("Expected PlanningCenter error with hint"),
        }
    }
}
