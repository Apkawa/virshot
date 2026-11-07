//! Typed capture failures (thiserror), so callers match instead of parsing
//! strings. Library encoder failures arrive as a boxed error description —
//! `windows-capture` reports its own error types, which this crate turns into
//! a message and never retries.

/// Everything that can go wrong capturing and encoding a recording.
#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    /// File/IO failure writing the recording or reading configuration.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// The encoder/capture pipeline refused or failed; carries the library's
    /// error description.
    #[error("encoder failed: {0}")]
    Encoder(String),
    /// Capture was requested before a configuration was supplied.
    #[error("capture is not configured")]
    NotConfigured,
    /// Screen capture is Windows-only; on Linux every capture path returns
    /// this instead of touching the OS.
    #[error("screen capture is Windows-only; unsupported on this platform")]
    UnsupportedPlatform,
    /// The output path was rejected (empty, not a directory, unsafe name).
    #[error("invalid output path: {0}")]
    InvalidPath(String),
}
