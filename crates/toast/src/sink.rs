//! The sink the toast thread drives: spawn / repaint / hide one overlay window.
//!
//! Toast defines this trait (it is toast's vocabulary, not overlay's), and both
//! overlay backends implement it as-is — `OverlayHandle` for a process that owns
//! the winit event loop on the toast side, `OverlayHostHandle` for a host app
//! (virshot, eframe) whose loop paints the child viewports. Toast therefore
//! needs no hand-written adapter, and hosts can wrap the trait to log or to
//! layer their own behavior.

use overlay::OverlayContent;
use overlay::OverlayError;
use overlay::OverlayHandle;
use overlay::OverlayHostHandle;
use overlay::OverlayId;
use overlay::OverlayWindowSpec;

/// Everything toast can do to overlay windows. Implementations must be cheap
/// and never block the toast timer thread for long: the thread drives fades.
pub trait ToastSink: Send + Sync {
    /// Create one window for `spec` painting `content` at `alpha`.
    fn spawn_window(
        &self,
        spec: OverlayWindowSpec,
        content: OverlayContent,
        alpha: f32,
    ) -> Result<OverlayId, OverlayError>;

    /// Replace the content and alpha of a live window.
    fn redraw(
        &self,
        id: OverlayId,
        content: OverlayContent,
        alpha: f32,
    ) -> Result<(), OverlayError>;

    /// Close a window. An id the backend no longer has is not an error worth
    /// panicking over; both overlay backends report it as `Ok`.
    fn hide(&self, id: OverlayId) -> Result<(), OverlayError>;
}

impl ToastSink for OverlayHandle {
    fn spawn_window(
        &self,
        spec: OverlayWindowSpec,
        content: OverlayContent,
        alpha: f32,
    ) -> Result<OverlayId, OverlayError> {
        self.spawn_window(spec, content, alpha)
    }

    fn redraw(
        &self,
        id: OverlayId,
        content: OverlayContent,
        alpha: f32,
    ) -> Result<(), OverlayError> {
        self.redraw(id, content, alpha)
    }

    fn hide(&self, id: OverlayId) -> Result<(), OverlayError> {
        self.hide(id)
    }
}

impl ToastSink for OverlayHostHandle {
    fn spawn_window(
        &self,
        spec: OverlayWindowSpec,
        content: OverlayContent,
        alpha: f32,
    ) -> Result<OverlayId, OverlayError> {
        self.spawn_window(spec, content, alpha)
    }

    fn redraw(
        &self,
        id: OverlayId,
        content: OverlayContent,
        alpha: f32,
    ) -> Result<(), OverlayError> {
        self.redraw(id, content, alpha)
    }

    fn hide(&self, id: OverlayId) -> Result<(), OverlayError> {
        self.hide(id)
    }
}

#[cfg(test)]
mod tests {
    use super::ToastSink;

    /// Both overlay backends satisfy the sink contract out of the box, so a
    /// host needs no hand-written adapter.
    fn assert_sink<T: ToastSink + Send + Sync>() {}

    #[test]
    fn overlay_handles_implement_toast_sink() {
        assert_sink::<overlay::OverlayHandle>();
        assert_sink::<overlay::OverlayHostHandle>();
    }

    #[test]
    fn boxed_sink_crosses_the_thread_boundary() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<std::sync::Arc<dyn ToastSink>>();
    }
}
