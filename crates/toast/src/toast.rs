//! The notification itself: what to show, where, for how long.

use overlay::Corner;

use crate::color::ToastColor;

/// Which monitors a toast appears on. Spec §4.3: one chip per monitor, and
/// `on_all_monitors` defaults to true, so `All` is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MonitorSel {
    /// One monitor by index into the monitor list the service was spawned with.
    /// An index nobody has shows nothing.
    Index(usize),
    /// Every monitor the service was spawned with. The default: spec §4.3 shows
    /// a toast on every monitor.
    #[default]
    All,
}

/// Default visible hold (ms), spec §4.3: 2000 ms for ordinary toasts.
pub const DEFAULT_HOLD_MS: u32 = 2000;

/// One notification. Text-only chips paint a colored chip with centered text;
/// an image chip paints raw image bytes (decoded by the overlay backend, never
/// here). `Danger` ignores `duration_ms` and holds `ERROR_HOLD_MS`.
#[derive(Debug, Clone)]
pub struct Toast {
    /// Which monitor(s) to show on.
    pub monitor: MonitorSel,
    /// Chip text; unused when `image` is set.
    pub text: String,
    /// Raw PNG/JPEG/WebP bytes for an image chip (`crate::image::load`).
    pub image: Option<Vec<u8>>,
    /// Chip palette and hold rule.
    pub color: ToastColor,
    /// Screen corner the chip is anchored to.
    pub corner: Corner,
    /// Visible hold in ms; `0` = sticky until dismissed.
    pub duration_ms: u32,
}

impl Toast {
    /// A text chip with the default placement (`All` monitors, bottom-right)
    /// and the default hold of [`DEFAULT_HOLD_MS`].
    pub fn text(text: impl Into<String>, color: ToastColor) -> Self {
        Self {
            monitor: MonitorSel::All,
            text: text.into(),
            image: None,
            color,
            corner: Corner::BottomRight,
            duration_ms: DEFAULT_HOLD_MS,
        }
    }

    /// An image chip carrying raw encoded bytes, with the default placement and
    /// hold. Bytes come from [`crate::image::load`] and are decoded by the
    /// overlay backend.
    pub fn image(bytes: Vec<u8>, color: ToastColor) -> Self {
        Self {
            monitor: MonitorSel::All,
            text: String::new(),
            image: Some(bytes),
            color,
            corner: Corner::BottomRight,
            duration_ms: DEFAULT_HOLD_MS,
        }
    }
}
