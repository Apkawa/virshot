//! Low-level overlay windows for virshot (docs/spec.md §4.3).
//!
//! One background thread runs a blocking winit event loop; every overlay window
//! lives on that thread, addressed by [`OverlayId`]. Windows are transparent,
//! decorations-less, always-on-top, content-protected, and (where the platform
//! supports it) click-through via `Window::set_cursor_hittest(false)`.
//!
//! The facade is non-blocking except for one thing: `OverlayHandle::spawn_window`
//! waits for the window thread's readiness handshake so a returned id is always
//! paintable. `redraw`, `hide`, and `quit_all` never block — they hand owned
//! commands to the loop thread through winit's `EventLoopProxy`, which also wakes
//! the loop out of `ControlFlow::Wait`.
//!
//! A host application that already owns the process's event loop (virshot runs
//! eframe on the main thread) cannot use that thread backend. It uses
//! [`OverlayHost`] instead: every overlay becomes a child egui viewport of the
//! host loop, and [`OverlayHostHandle`] offers the same facade semantics — with
//! one difference, `spawn_window` never blocks because the viewport is created
//! inside `OverlayHost::update` on the host thread.
//!
//! Coordinate space is PHYSICAL desktop pixels for BOTH backends: the thread
//! backend hands the numbers straight to winit's physical position/size, and the
//! host backend converts a rect to egui points internally (`host.rs`) because
//! eframe multiplies builder position/size — and later viewport commands — by
//! the CHILD window's own pixels-per-point, which only that window knows.
//!
//! Two deliberate deviations from the Stage 2 brief, both forced by the pinned
//! dependency versions:
//!
//! * **Unsafe code.** glutin 0.32.3 / glutin-winit 0.5.0 expose config selection,
//!   context creation, and window-surface creation only as `unsafe fn`, and
//!   `glow::Context::from_loader_function_cstr` is unsafe. There is no safe
//!   wrapper in those crates, and `#![forbid(unsafe_code)]` cannot be relaxed by
//!   a nested `#![allow]` (rustc rejects `allow(unsafe_code)` after `forbid`).
//!   This crate therefore uses `#![deny(unsafe_code)]` at the root plus a
//!   module-scoped `#![allow(unsafe_code)]` confined to `gl_boot`, where the only
//!   `unsafe` calls are direct FFI into the platform GL API — the same shape
//!   `crates/gamepad` uses.
//! * **One event-loop thread, not one per window.** winit 0.30 permits a single
//!   `EventLoop` per process, so windows are multiplexed on one loop and commands
//!   travel through `EventLoopProxy` instead of a per-window `mpsc` sender.
#![deny(unsafe_code)]

mod content;
mod error;
mod gl_boot;
mod host;
mod placement;
mod service;

// Consumer-facing module: consumers import it by path (`overlay::monitors::…`)
// because its function names (`entries`, `rects`, `debug_line`) are too plain
// for the crate-root namespace; the selective `pub use` convention below only
// covers the private modules.
pub mod monitors;

pub use content::OverlayContent;
pub use error::OverlayError;
pub use host::{OverlayHost, OverlayHostHandle};
pub use placement::{
    CORNER_INSET_X, CORNER_INSET_Y, Corner, MonitorRect, Rect, TASKBAR_CLEARANCE, place,
};

use std::collections::HashSet;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;

use winit::event_loop::EventLoopProxy;

use service::{LoopEvent, run};

/// Handle identifying one overlay window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OverlayId(u64);

impl OverlayId {
    pub(crate) fn as_u64(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for OverlayId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Where and how an overlay window is created. `rect` is PHYSICAL desktop
/// pixels for both backends: the thread backend passes it to winit unchanged,
/// and [`OverlayHost`] divides it by the child window's pixels-per-point.
#[derive(Debug, Clone, Copy)]
pub struct OverlayWindowSpec {
    /// Placement rectangle in PHYSICAL desktop pixels, normally the result of
    /// [`place`] applied to a [`MonitorRect`].
    pub rect: Rect,
    /// The TARGET monitor's scale (`MonitorRect::scale`), used by the host
    /// backend as its initial pixels-per-point guess before the child window
    /// reports its own; the thread backend ignores it (its rect is already
    /// physical). `1.0` when the monitor scale is unknown.
    pub scale: f32,
    /// Alpha-capable window with a transparent background.
    pub transparent: bool,
    /// Ignore mouse input; requires platform support, logged if unavailable.
    /// Windows is not supported by either backend: winit 0.30.13 turns
    /// `set_cursor_hittest(false)` into an unattributed `WS_EX_LAYERED` window
    /// that composites nothing, so the flag is ignored there (`host.rs`,
    /// `service.rs`).
    pub click_through: bool,
    /// Ask the OS to keep the window content out of screen captures. On Windows
    /// this is `WDA_EXCLUDEFROMCAPTURE`; on X11/Wayland it may be a no-op.
    pub content_protected: bool,
}

/// Owner of the overlay thread and of the windows it created.
pub struct OverlayHandle {
    state: Mutex<HandleState>,
    next_id: AtomicU64,
}

struct HandleState {
    proxy: Option<EventLoopProxy<LoopEvent>>,
    live: HashSet<OverlayId>,
}

impl OverlayHandle {
    /// Create a handle; the overlay thread starts on the first `spawn_window`.
    pub fn new() -> Self {
        Self {
            state: Mutex::new(HandleState {
                proxy: None,
                live: HashSet::new(),
            }),
            next_id: AtomicU64::new(1),
        }
    }

    /// Create an overlay window and paint `content` at fade `alpha`.
    ///
    /// Blocks until the window thread reports the window is ready; returns
    /// `OverlayError::ThreadGone` if that handshake never arrives and the
    /// thread's own creation error (`Gl`, `Window`, `Painter`) otherwise.
    pub fn spawn_window(
        &self,
        spec: OverlayWindowSpec,
        content: OverlayContent,
        alpha: f32,
    ) -> Result<OverlayId, OverlayError> {
        let id = OverlayId(self.next_id.fetch_add(1, Ordering::Relaxed));

        let mut state = self.state.lock().map_err(|_| OverlayError::ThreadGone)?;
        if state.proxy.is_none() {
            let (proxy_tx, proxy_rx) = mpsc::channel();
            // Detached: the thread exits when `quit_all` or `Drop` arrives.
            let thread = std::thread::spawn(move || run(proxy_tx));
            drop(thread);
            let proxy = proxy_rx.recv().map_err(|_| OverlayError::ThreadGone)?;
            state.proxy = Some(proxy);
        }

        let Some(proxy) = state.proxy.clone() else {
            return Err(OverlayError::ThreadGone);
        };

        let (ack_tx, ack_rx) = mpsc::channel();
        proxy
            .send_event(LoopEvent::Create {
                id,
                spec,
                content,
                alpha,
                ack: ack_tx,
            })
            .map_err(|_| OverlayError::ChannelClosed)?;

        let ready = ack_rx.recv().map_err(|_| OverlayError::ThreadGone)?;
        ready?;
        state.live.insert(id);
        Ok(id)
    }

    /// Replace the content and fade alpha of a live window, then repaint it.
    ///
    /// Returns `OverlayError::UnknownWindow` for ids this handle did not create
    /// or already closed, and `OverlayError::ChannelClosed` once the loop exited.
    pub fn redraw(
        &self,
        id: OverlayId,
        content: OverlayContent,
        alpha: f32,
    ) -> Result<(), OverlayError> {
        let mut state = self.state.lock().map_err(|_| OverlayError::ThreadGone)?;
        if !state.live.contains(&id) {
            return Err(OverlayError::UnknownWindow { id });
        }
        let Some(proxy) = state.proxy.clone() else {
            state.live.remove(&id);
            return Err(OverlayError::ThreadGone);
        };
        match proxy.send_event(LoopEvent::Redraw { id, content, alpha }) {
            Ok(()) => Ok(()),
            Err(_) => {
                state.live.remove(&id);
                Err(OverlayError::ChannelClosed)
            }
        }
    }

    /// Close a window. Closing an unknown or already-closed id is a no-op.
    pub fn hide(&self, id: OverlayId) -> Result<(), OverlayError> {
        let mut state = self.state.lock().map_err(|_| OverlayError::ThreadGone)?;
        state.live.remove(&id);
        let Some(proxy) = state.proxy.clone() else {
            return Ok(());
        };
        match proxy.send_event(LoopEvent::Hide { id }) {
            Ok(()) => Ok(()),
            Err(_) => Err(OverlayError::ChannelClosed),
        }
    }

    /// Close every window and stop the overlay thread. Idempotent.
    pub fn quit_all(&self) -> Result<(), OverlayError> {
        let Ok(mut state) = self.state.lock() else {
            return Err(OverlayError::ThreadGone);
        };
        state.live.clear();
        let Some(proxy) = state.proxy.clone() else {
            return Ok(());
        };
        match proxy.send_event(LoopEvent::QuitAll) {
            Ok(()) => Ok(()),
            Err(_) => Err(OverlayError::ChannelClosed),
        }
    }
}

impl Default for OverlayHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for OverlayHandle {
    fn drop(&mut self) {
        let _ = self.quit_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send<T: Send>() {}
    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn overlay_handle_is_send_and_sync() {
        assert_send_sync::<OverlayHandle>();
        assert_send_sync::<OverlayError>();
    }

    #[test]
    fn overlay_content_is_send_so_it_can_cross_the_thread_boundary() {
        assert_send::<OverlayContent>();
    }

    #[test]
    fn redraw_on_a_never_created_window_reports_unknown_window() {
        let handle = OverlayHandle::new();
        let result = handle.redraw(OverlayId(7), OverlayContent::Text("chip".to_owned()), 1.0);
        assert!(matches!(
            result,
            Err(OverlayError::UnknownWindow { id: OverlayId(7) })
        ));
    }

    #[test]
    fn hide_on_a_never_created_window_is_a_no_op() {
        let handle = OverlayHandle::new();
        assert!(handle.hide(OverlayId(7)).is_ok());
    }

    #[test]
    fn quit_all_before_any_window_exists_is_a_no_op() {
        let handle = OverlayHandle::new();
        assert!(handle.quit_all().is_ok());
    }
}
