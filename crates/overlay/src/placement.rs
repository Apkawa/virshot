//! Corner placement math for overlay windows: pure geometry, no UI, no I/O.
//!
//! Mirrors the placement rules proven by `virshot`'s `gui::tests::toast_position_*`
//! tests (see `src/gui.rs`), restated here as a standalone crate so the overlay
//! backend and the toast chip can share one source of truth for corner anchors.
//!
//! Placement space is PHYSICAL desktop pixels: `winit` monitors report physical
//! position/size, and both overlay backends take a physical rect (the host
//! backend converts to egui points internally, see `host.rs`). Insets are the
//! same values on all four corners; the bottom corners use [`CORNER_INSET_Y`]
//! which clears a ~48 px Windows taskbar without any OS work-area APIs.

/// Axis-aligned rectangle in PHYSICAL desktop pixels (e.g. from
/// `winit::window::Window::inner_size` or `MonitorHandle::position()/size()`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// Left edge, desktop coordinates.
    pub x: f64,
    /// Top edge, desktop coordinates.
    pub y: f64,
    /// Width, must be positive for placement to succeed.
    pub w: f64,
    /// Height, must be positive for placement to succeed.
    pub h: f64,
}

impl Rect {
    /// Build a rectangle from an origin and a size.
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }
}

/// One monitor as a placement target: its rect in PHYSICAL desktop pixels, plus
/// that monitor's DPI scale (`winit::MonitorHandle::scale_factor()`, which is
/// the monitor DPI divided by 96).
///
/// The scale travels with the monitor because a chip's design size is in
/// logical points while the desktop rects are physical: the toast multiplies
/// its size by `scale` (`crates/toast`), and the host backend divides a rect by
/// the CHILD window's own pixels-per-point (`host.rs`). The thread backend
/// ignores it — its rects are already physical.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonitorRect {
    /// Monitor rectangle, physical desktop pixels.
    pub rect: Rect,
    /// The monitor's pixels-per-point; a non-positive or non-finite value is
    /// normalized to `1.0`.
    pub scale: f32,
}

impl MonitorRect {
    /// Build a monitor rect, normalizing a non-positive or non-finite scale.
    pub fn new(rect: Rect, scale: f32) -> Self {
        Self {
            rect,
            scale: if scale.is_finite() && scale > 0.0 {
                scale
            } else {
                1.0
            },
        }
    }
}

/// Screen corner an overlay window is anchored to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    /// Top-left corner of the monitor.
    TopLeft,
    /// Top-right corner of the monitor.
    TopRight,
    /// Bottom-left corner of the monitor.
    BottomLeft,
    /// Bottom-right corner of the monitor (the default toast anchor).
    BottomRight,
}

/// Horizontal distance from the monitor edge to the window edge.
pub const CORNER_INSET_X: f64 = 30.0;

/// Vertical distance from the monitor edge to the window edge.
///
/// 60 px keeps the bottom corners clear of the Windows taskbar (~48 px) without
/// querying OS work-area APIs.
pub const CORNER_INSET_Y: f64 = 60.0;

/// Taskbar clearance the bottom-corner inset provides, documented for callers.
pub const TASKBAR_CLEARANCE: f64 = 48.0;

/// Place a `size`-sized window into `corner` of `monitor`, inset by
/// [`CORNER_INSET_X`] / [`CORNER_INSET_Y`]. Every value — monitor, size, insets,
/// result — is PHYSICAL desktop pixels, so one placement math serves both
/// backends.
///
/// The result is clamped to the monitor origin, so a window larger than the
/// monitor pins to the top-left of that monitor instead of spilling into
/// negative desktop coordinates (a monitor at a negative desktop origin keeps
/// its offsets relative to itself).
///
/// Returns `None` when the monitor or the window size is degenerate
/// (non-positive or non-finite), mirroring `virshot`'s "monitor size unknown"
/// semantics.
pub fn place(monitor: Rect, corner: Corner, size: [f64; 2]) -> Option<Rect> {
    let [width, height] = size;
    if monitor.w <= 0.0 || monitor.h <= 0.0 || width <= 0.0 || height <= 0.0 {
        return None;
    }
    if !(monitor.x.is_finite()
        && monitor.y.is_finite()
        && monitor.w.is_finite()
        && monitor.h.is_finite()
        && width.is_finite()
        && height.is_finite())
    {
        return None;
    }

    let left = match corner {
        Corner::TopLeft | Corner::BottomLeft => monitor.x + CORNER_INSET_X,
        Corner::TopRight | Corner::BottomRight => monitor.x + monitor.w - width - CORNER_INSET_X,
    };
    let top = match corner {
        Corner::TopLeft | Corner::TopRight => monitor.y + CORNER_INSET_Y,
        Corner::BottomLeft | Corner::BottomRight => monitor.y + monitor.h - height - CORNER_INSET_Y,
    };

    Some(Rect {
        x: left.max(monitor.x),
        y: top.max(monitor.y),
        w: width,
        h: height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect::new(x, y, w, h)
    }

    /// The toast chip size used by `virshot`'s tests: 200x36 logical px.
    fn chip() -> [f64; 2] {
        [200.0, 36.0]
    }

    #[test]
    fn place_bottom_right_offsets_insets_and_clears_taskbar() {
        let placed = place(
            monitor(0.0, 0.0, 1920.0, 1080.0),
            Corner::BottomRight,
            chip(),
        )
        .expect("a real monitor has a size");
        assert_eq!(placed.x, 1920.0 - 200.0 - CORNER_INSET_X);
        assert_eq!(placed.y, 1080.0 - 36.0 - CORNER_INSET_Y);
        assert_eq!(placed.w, 200.0);
        assert_eq!(placed.h, 36.0);
        // The gap from the window bottom to the monitor bottom clears the taskbar.
        assert!(1080.0 - (placed.y + placed.h) >= TASKBAR_CLEARANCE);
    }

    #[test]
    fn place_top_left_offsets_inset_x_and_y() {
        let placed = place(monitor(0.0, 0.0, 1920.0, 1080.0), Corner::TopLeft, chip())
            .expect("a real monitor has a size");
        assert_eq!(placed.x, CORNER_INSET_X);
        assert_eq!(placed.y, CORNER_INSET_Y);
    }

    #[test]
    fn place_top_right_and_bottom_left_anchor_the_other_corners() {
        let monitor = monitor(0.0, 0.0, 1920.0, 1080.0);
        let top_right =
            place(monitor, Corner::TopRight, chip()).expect("a real monitor has a size");
        assert_eq!(top_right.x, 1920.0 - 200.0 - CORNER_INSET_X);
        assert_eq!(top_right.y, CORNER_INSET_Y);

        let bottom_left =
            place(monitor, Corner::BottomLeft, chip()).expect("a real monitor has a size");
        assert_eq!(bottom_left.x, CORNER_INSET_X);
        assert_eq!(bottom_left.y, 1080.0 - 36.0 - CORNER_INSET_Y);
    }

    #[test]
    fn place_is_none_for_a_degenerate_monitor() {
        assert_eq!(
            place(monitor(0.0, 0.0, 0.0, 1080.0), Corner::BottomRight, chip()),
            None
        );
        assert_eq!(
            place(monitor(0.0, 0.0, 1920.0, 0.0), Corner::BottomRight, chip()),
            None
        );
        assert_eq!(
            place(
                monitor(0.0, 0.0, f64::NAN, 1080.0),
                Corner::BottomRight,
                chip()
            ),
            None
        );
    }

    #[test]
    fn place_is_none_for_a_degenerate_window_size() {
        assert_eq!(
            place(
                monitor(0.0, 0.0, 1920.0, 1080.0),
                Corner::BottomRight,
                [0.0, 36.0]
            ),
            None
        );
        assert_eq!(
            place(
                monitor(0.0, 0.0, 1920.0, 1080.0),
                Corner::BottomRight,
                [200.0, 0.0]
            ),
            None
        );
    }

    #[test]
    fn place_clamps_to_the_monitor_origin_on_a_tiny_monitor() {
        // 100x40 monitor, 200x36 window: negative anchors pin to the monitor origin.
        let placed = place(monitor(0.0, 0.0, 100.0, 40.0), Corner::BottomRight, chip())
            .expect("a tiny monitor still has a size");
        assert_eq!(placed.x, 0.0);
        assert_eq!(placed.y, 0.0);
    }

    #[test]
    fn place_keeps_offsets_relative_to_a_monitor_at_a_negative_desktop_origin() {
        // A secondary monitor left of the primary one sits at negative desktop x.
        let placed = place(
            monitor(-1920.0, 0.0, 1920.0, 1080.0),
            Corner::TopLeft,
            chip(),
        )
        .expect("a real monitor has a size");
        assert_eq!(placed.x, -1920.0 + CORNER_INSET_X);
        assert_eq!(placed.y, CORNER_INSET_Y);

        let bottom_right = place(
            monitor(-1920.0, 0.0, 1920.0, 1080.0),
            Corner::BottomRight,
            chip(),
        )
        .expect("a real monitor has a size");
        assert_eq!(bottom_right.x, -1920.0 + 1920.0 - 200.0 - CORNER_INSET_X);
        assert_eq!(bottom_right.y, 1080.0 - 36.0 - CORNER_INSET_Y);
    }

    #[test]
    fn place_never_returns_a_rect_outside_the_monitor_when_it_fits() {
        let monitor = monitor(0.0, 0.0, 1280.0, 720.0);
        for corner in [
            Corner::TopLeft,
            Corner::TopRight,
            Corner::BottomLeft,
            Corner::BottomRight,
        ] {
            let placed = place(monitor, corner, chip()).expect("a real monitor has a size");
            assert!(placed.x >= monitor.x && placed.x + placed.w <= monitor.x + monitor.w);
            assert!(placed.y >= monitor.y && placed.y + placed.h <= monitor.y + monitor.h);
        }
    }

    #[test]
    fn monitor_rect_keeps_physical_geometry_and_attaches_the_scale() {
        let monitor = MonitorRect::new(Rect::new(1920.0, 0.0, 1280.0, 720.0), 1.5);
        assert_eq!(monitor.rect, Rect::new(1920.0, 0.0, 1280.0, 720.0));
        assert_eq!(monitor.scale, 1.5);
    }

    #[test]
    fn monitor_rect_normalizes_a_non_positive_scale_to_one() {
        for scale in [0.0, -1.0, f32::NAN] {
            let monitor = MonitorRect::new(Rect::new(0.0, 0.0, 1920.0, 1080.0), scale);
            assert_eq!(monitor.scale, 1.0, "scale {scale} must fall back to 1.0");
        }
    }
}
