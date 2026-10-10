//! Typed failures exposed by the native text-fit process boundary.

use std::path::PathBuf;
use std::time::Duration;

/// Native text measurement failure.
#[derive(Debug, thiserror::Error)]
pub enum TextFitError {
    /// Native `TextKit` measurement is available only on macOS.
    #[error("native TextKit measurement is unavailable on platform '{0}'")]
    UnsupportedPlatform(String),
    /// The platform did not provide a private per-user cache directory.
    #[error("the operating system did not provide a local user cache directory for the bundled native text-fit helper")]
    LocalCacheUnavailable,
    /// The embedded helper could not be materialized into the local cache.
    #[error("failed to {operation} bundled native text-fit helper at {}: {source}", path.display())]
    BundledHelperCache {
        /// Cache operation that failed.
        operation: &'static str,
        /// Cache path involved in the failure.
        path: PathBuf,
        /// Filesystem failure.
        #[source]
        source: std::io::Error,
    },
    /// A cached helper is not a regular, non-symbolic-link file.
    #[error("bundled native text-fit helper cache entry is not a regular file: {}", path.display())]
    InvalidBundledHelperCacheEntry {
        /// Invalid cache path.
        path: PathBuf,
    },
    /// The selected helper bytes could not be read for receipt identity.
    #[error("failed to read native text-fit helper identity at {}: {source}", path.display())]
    ReadHelperIdentity {
        /// Executable whose bytes could not be hashed.
        path: PathBuf,
        /// Filesystem failure.
        #[source]
        source: std::io::Error,
    },
    /// Cached helper bytes do not match the binary's embedded helper.
    #[error("bundled native text-fit helper cache digest mismatch at {}", path.display())]
    BundledHelperDigestMismatch {
        /// Corrupt or substituted cache path.
        path: PathBuf,
    },
    /// The configured helper executable could not be started.
    #[error("native text-fit helper is unavailable at {}: {source}", executable.display())]
    HelperUnavailable {
        /// Configured helper executable.
        executable: PathBuf,
        /// Operating-system process error.
        #[source]
        source: std::io::Error,
    },
    /// The bounded helper-response reader thread could not be created.
    #[error("failed to start native text-fit response reader: {0}")]
    StartResponseReader(std::io::Error),
    /// A request received no response before its hard deadline.
    #[error("native text-fit helper did not respond within {timeout:?}")]
    ResponseTimeout {
        /// Configured hard response deadline.
        timeout: Duration,
    },
    /// One response exceeded the bounded JSON-lines frame size.
    #[error("native text-fit response exceeded the {limit}-byte frame limit")]
    ResponseFrameTooLarge {
        /// Maximum accepted frame length.
        limit: usize,
    },
    /// The helper closed stdout inside a JSON-lines frame.
    #[error("native text-fit helper returned a truncated response frame")]
    TruncatedResponseFrame,
    /// A prior terminal helper failure permanently invalidated this session.
    #[error("native text-fit session is poisoned after a terminal helper failure")]
    SessionPoisoned,
    /// The request sequence cannot allocate another unique identifier.
    #[error("native text-fit request sequence is exhausted")]
    RequestSequenceExhausted,
    /// JSON request serialization failed.
    #[error("failed to encode native text-fit request: {0}")]
    EncodeRequest(serde_json::Error),
    /// The helper input pipe rejected a request.
    #[error("failed to write native text-fit request: {0}")]
    WriteRequest(std::io::Error),
    /// The helper output pipe could not be read.
    #[error("failed to read native text-fit response: {0}")]
    ReadResponse(std::io::Error),
    /// The helper stopped before producing a response.
    #[error("native text-fit helper terminated before responding (status: {status:?})")]
    HelperTerminated {
        /// Exit status when available.
        status: Option<std::process::ExitStatus>,
    },
    /// A response was not valid protocol JSON.
    #[error("failed to decode native text-fit response: {0}")]
    DecodeResponse(serde_json::Error),
    /// Rust and helper speak different contract versions.
    #[error("native text-fit protocol version mismatch: expected {expected}, got {actual}")]
    ProtocolVersionMismatch {
        /// Rust contract version.
        expected: u32,
        /// Helper contract version.
        actual: u32,
    },
    /// A response belongs to a different request.
    #[error("native text-fit response ID mismatch: expected {expected}, got {actual}")]
    ResponseIdMismatch {
        /// Active request identifier.
        expected: u64,
        /// Returned request identifier.
        actual: u64,
    },
    /// The helper returned an internally inconsistent contract value.
    #[error("invalid native text-fit helper response: {0}")]
    HelperProtocol(String),
    /// One or more required fonts are unavailable.
    #[error("required native fonts are unavailable: {0:?}")]
    MissingFonts(Vec<String>),
    /// The helper rejected malformed RTF.
    #[error("native text stack rejected final RTF: {0}")]
    InvalidRtf(String),
    /// Parsed RTF contains a feature this oracle cannot measure faithfully.
    #[error("native text stack found unsupported RTF content: {0}")]
    UnsupportedRtfContent(String),
    /// CoreText resolved a font but did not expose readable program bytes.
    #[error("native text stack could not identify a resolved font program: {0}")]
    FontProgramUnavailable(String),
    /// The helper rejected a native scale behavior.
    #[error("native text-fit helper does not support scale behavior '{0}'")]
    UnsupportedScaleBehavior(String),
    /// The helper rejected a native text transform.
    #[error("native text-fit helper does not support transform '{0}'")]
    UnsupportedTransform(String),
    /// The helper rejected a native vertical-alignment mode.
    #[error("native text-fit helper does not support vertical alignment '{0}'")]
    UnsupportedVerticalAlignment(String),
    /// The helper could not identify the Apple text-stack runtime it used.
    #[error("native text-fit helper could not identify its runtime: {0}")]
    RuntimeIdentityUnavailable(String),
    /// `AppKit` failed while producing a layout.
    #[error("native text layout failed: {0}")]
    LayoutFailed(String),
    /// A future helper error is preserved instead of being misclassified.
    #[error("native text-fit helper rejected the request ({code}): {message}")]
    HelperRejected {
        /// Stable helper error code.
        code: String,
        /// Human-readable helper detail.
        message: String,
    },
}
