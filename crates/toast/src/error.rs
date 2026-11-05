//! Typed toast failures (thiserror), so callers match instead of parsing
//! strings. Overlay failures are carried as a message: the overlay backend has
//! its own typed error, and toast never retries it.

/// Everything that can go wrong showing or dismissing a toast.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ToastError {
    /// The timer thread is gone (the service was dropped) or its inbox is
    /// closed, including after a panic on the toast thread.
    #[error("toast service is gone")]
    ChannelClosed,
    /// Reading an image file failed.
    #[error("image load failed: {message}")]
    Io { message: String },
    /// An image is over the byte cap; the overlay backend refuses oversized
    /// decoded images too, so this gate keeps the failure local and cheap.
    #[error("image is {bytes} bytes, over the {max} byte limit")]
    ImageTooLarge { bytes: usize, max: usize },
    /// The overlay backend refused the window (unavailable, gone, bad rect).
    #[error("overlay window failed: {message}")]
    Overlay { message: String },
}

impl From<overlay::OverlayError> for ToastError {
    fn from(error: overlay::OverlayError) -> Self {
        Self::Overlay {
            message: error.to_string(),
        }
    }
}
