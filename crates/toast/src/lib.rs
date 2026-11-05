//! Toast notifications on top of `overlay`.
//!
//! The public API never blocks: `ToastHandle` pushes a command into an inbox and
//! returns. A dedicated plain `std` timer thread owns the lifecycle (hold →
//! fade → hide) and drives the overlay backend through the [`ToastSink`] trait.
//! There is no async runtime, no tokio, and no winit here — the overlay crate
//! owns the one `EventLoop` per process, and hosts drive `OverlayHost` on their
//! own loop.
//!
//! Host wiring:
//! - thread-backend host: hand it an `OverlayHandle` (it implements [`ToastSink`]);
//! - eframe/virshot host: hand it `OverlayHost::handle()` (also a [`ToastSink`]),
//!   keep calling `OverlayHost::update(&ctx)` on the host pass, and pass
//!   `Some(ctx.clone)` so the toast thread wakes the host loop for fade frames.
//! - monitors are injected at spawn as `overlay::MonitorRect`s: PHYSICAL
//!   desktop rects with each monitor's DPI scale attached. The chip's logical
//!   size is multiplied by that scale, so both backends receive a rect in
//!   physical pixels (the host backend converts it back to points internally).
//!   An empty monitor list makes every toast a logged no-op.

#![forbid(unsafe_code)]

mod chip;
mod color;
mod error;
mod image;
mod placement;
mod rules;
mod service;
mod sink;
mod toast;

pub use color::ToastColor;
pub use error::ToastError;
pub use image::load;
pub use service::{ToastHandle, ToastService};
pub use sink::ToastSink;
pub use toast::DEFAULT_HOLD_MS;
pub use toast::MonitorSel;
pub use toast::Toast;
