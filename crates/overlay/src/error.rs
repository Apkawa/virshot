//! Typed error enum for the overlay facade.

use thiserror::Error;

/// Every failure the overlay facade can report.
///
/// `Send + Sync` so the value travels through the window thread's handshake
/// channel (`std::sync::mpsc`) without extra synchronization.
#[derive(Debug, Error)]
pub enum OverlayError {
    /// `winit` refused to create the per-window event loop (no display server,
    /// or a second event loop on the same platform).
    #[error("overlay event loop creation failed: {message}")]
    EventLoop { message: String },

    /// Window creation failed (missing monitor geometry, OS rejection).
    #[error("overlay window creation failed: {message}")]
    Window { message: String },

    /// OpenGL display/context/surface bootstrap failed.
    #[error("overlay GL bootstrap failed: {message}")]
    Gl { message: String },

    /// `egui_glow` painter creation failed (no shaders, missing GL entry points).
    #[error("overlay painter creation failed: {message}")]
    Painter { message: String },

    /// The window thread ended before it reported readiness: it panicked or was
    /// killed, so no handshake value arrived.
    #[error("overlay window thread ended before the window was ready")]
    ThreadGone,

    /// The command channel of a live window is closed (thread already exited).
    #[error("overlay window command channel is closed")]
    ChannelClosed,

    /// No overlay window is registered under this id (`redraw` after `hide`).
    #[error("no overlay window with id {id}")]
    UnknownWindow { id: super::OverlayId },

    /// The decoded image is larger than the GL implementation's texture limit.
    #[error("overlay image {width}x{height} exceeds the GL texture limit of {max}")]
    ImageTooLarge {
        width: usize,
        height: usize,
        max: usize,
    },

    /// The image bytes are not a decodable PNG/JPEG/WebP image.
    #[error("overlay image could not be decoded: {message}")]
    ImageDecode { message: String },
}
