//! Monitor enumeration for overlay/toast placement: one source of truth for
//! the monitor list, the PRIMARY flag, and the physical rects both consumers
//! need (`src/gui.rs` and `crates/toast/examples/toast_debug.rs` enumerate via
//! `eframe::CreationContext::winit_window()`, which returns
//! `&Arc<winit::window::Window>`).
//!
//! Responsibilities (and only these): enumerate → plain [`MonitorEntry`]
//! values so the pure half is unit-testable without winit handles
//! (`MonitorHandle` cannot be constructed in tests); convert entries to
//! [`MonitorRect`] (skip zero-size entries — Windows mirrors pseudo-monitors
//! are enumerated and report size 0 — and attach each monitor's DPI scale);
//! format the startup debug line; provide fallback rects when a platform
//! reports no monitor geometry.
//!
//! **Ordering rule, stated once here:** [`rects`] moves the flagged entry to
//! index 0 and keeps the enumeration order of the rest, so toast
//! `MonitorSel::Index(0)` means "the primary monitor" for every consumer.
//! winit's enumeration order is not a contract: Windows `EnumDisplayMonitors`
//! delivers callbacks with no documented ordering guarantee.
//!
//! **Primary flag ranking:** `Window::primary_monitor()` handle equality first
//! (exact on Windows/X11/macOS). When it is `None` (Wayland/Web report none)
//! or no enumerated handle compares equal (Windows handle identity across
//! calls is not documented, and a settings change can break it), the first
//! entry at position exactly `(0,0)` with a non-zero size is flagged — on
//! Windows the primary monitor defines the virtual-screen origin. Exact
//! `(0,0)` matching, not containment: containment would also match a monitor
//! whose right edge is at `x = 0`. If nothing matches, no entry is flagged and
//! `rects` keeps enumeration order.
//!
//! **Thread affinity:** enumerate on the thread that owns the winit event
//! loop (the eframe main thread for host apps); never from the toast timer
//! thread (`crates/toast/src/service.rs` is plain `std`, no winit).
//!
//! **Safety:** zero `unsafe` — every winit call here is a safe public wrapper.
//! One caveat inherited from winit itself: `MonitorHandle::name()` can panic
//! on Windows when `GetMonitorInfoW` fails (winit unwraps internally); the
//! consumers already accepted that risk.

use crate::placement::{MonitorRect, Rect};
use winit::window::Window;

/// One monitor exactly as winit reports it: PHYSICAL position and size, the
/// monitor's own DPI scale (`scale_factor()`, monitor DPI / 96), the primary
/// flag, and the name where the platform supplies one (`None` elsewhere).
/// Plain fields keep the pure half unit-testable without winit handles.
#[derive(Debug)]
pub struct MonitorEntry {
    /// PHYSICAL desktop position x (negative for monitors left of the primary).
    pub x: i32,
    /// PHYSICAL desktop position y (negative for monitors above the primary).
    pub y: i32,
    /// PHYSICAL width in pixels; `0` marks a pseudo-monitor that placement
    /// must skip.
    pub width: u32,
    /// PHYSICAL height in pixels; `0` marks a pseudo-monitor that placement
    /// must skip.
    pub height: u32,
    /// The monitor's pixels-per-point, as reported by winit.
    pub scale: f64,
    /// Primary flag: set by handle equality in [`from_winit_window`], or by
    /// the `(0,0)` origin fallback; set by the caller for pure builders.
    pub is_primary: bool,
    /// Monitor name where the platform reports one (Windows: `\\.\DISPLAY1`).
    pub name: Option<String>,
}

/// Build entries from raw winit-shaped tuples
/// `(x, y, width, height, scale, name, is_primary)`, keeping the given
/// enumeration order. The primary flag is supplied by the caller because
/// `MonitorHandle` equality is only available in the winit layer.
pub fn entries(
    raw: impl IntoIterator<Item = (i32, i32, u32, u32, f64, Option<String>, bool)>,
) -> Vec<MonitorEntry> {
    raw.into_iter()
        .map(
            |(x, y, width, height, scale, name, is_primary)| MonitorEntry {
                x,
                y,
                width,
                height,
                scale,
                is_primary,
                name,
            },
        )
        .collect()
}

/// Flag the first entry at position exactly `(0,0)` with a non-zero size as
/// primary, but only when no entry is flagged yet — the fallback for platforms
/// where `primary_monitor()` is `None` (Wayland/Web) or where the primary
/// handle does not compare equal to an enumerated one. Containment is
/// deliberately not used: a monitor placed left of the primary can also
/// contain `(0,0)`.
fn flag_origin_as_primary(entries: &mut [MonitorEntry]) {
    if entries.iter().any(|entry| entry.is_primary) {
        return;
    }
    if let Some(entry) = entries
        .iter_mut()
        .find(|entry| entry.x == 0 && entry.y == 0 && entry.width > 0 && entry.height > 0)
    {
        entry.is_primary = true;
    }
}

/// Enumerate every monitor `window` reports, with the primary flag computed
/// from `Window::primary_monitor()` handle equality and the `(0,0)` origin
/// fallback (see the module doc). Call on the thread that owns the event
/// loop: eframe's main thread via `cc.winit_window()` for host apps.
pub fn from_winit_window(window: &Window) -> Vec<MonitorEntry> {
    let primary = window.primary_monitor();
    let mut built = entries(window.available_monitors().map(|monitor| {
        let position = monitor.position();
        let size = monitor.size();
        (
            position.x,
            position.y,
            size.width,
            size.height,
            monitor.scale_factor(),
            monitor.name(),
            Some(&monitor) == primary.as_ref(),
        )
    }));
    flag_origin_as_primary(&mut built);
    built
}

/// Placement rects from entries: zero-size entries are skipped, each rect is
/// built through `MonitorRect::new` (a non-positive or non-finite scale
/// becomes `1.0`), and the flagged entry moves to index 0 — so
/// `MonitorSel::Index(0)` means "primary" — while the rest keeps enumeration
/// order. Without a flagged entry the enumeration order is kept unchanged.
///
/// Does not log: callers compare `entries.len()` with `rects.len()` to warn
/// about skipped pseudo-monitors.
pub fn rects(entries: &[MonitorEntry]) -> Vec<MonitorRect> {
    let mut rects: Vec<MonitorRect> = Vec::new();
    let mut primary_index: Option<usize> = None;
    for entry in entries {
        if entry.width == 0 || entry.height == 0 {
            continue;
        }
        if entry.is_primary {
            primary_index = Some(rects.len());
        }
        rects.push(MonitorRect::new(
            Rect::new(
                f64::from(entry.x),
                f64::from(entry.y),
                f64::from(entry.width),
                f64::from(entry.height),
            ),
            entry.scale as f32,
        ));
    }
    if let Some(index) = primary_index
        && index != 0
    {
        let primary_rect = rects.remove(index);
        rects.insert(0, primary_rect);
    }
    rects
}

/// The startup placement-evidence line: one `#N …` entry per monitor joined by
/// ` | ` — index, name (or `unnamed`), PHYSICAL position and size, scale to two
/// decimals, and a ` primary` suffix on the flagged entry. Enumeration order,
/// not the `rects` order: the line reports what the OS said, and the flag
/// column names the primary wherever it sits.
///
/// `None` means there is no winit window at all; `Some(&[])` means winit
/// enumerated nothing. Both sentinel wordings are fixed because the Windows
/// rig checklist matches on them.
pub fn debug_line(entries: Option<&[MonitorEntry]>) -> String {
    let Some(entries) = entries else {
        return "no winit window (toast uses the root viewport's rect)".to_owned();
    };
    if entries.is_empty() {
        return "winit reported no monitors".to_owned();
    }
    entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            format!(
                "#{index} {} pos=({},{}) size={}x{} scale={:.2}{}",
                entry.name.as_deref().unwrap_or("unnamed"),
                entry.x,
                entry.y,
                entry.width,
                entry.height,
                entry.scale,
                if entry.is_primary { " primary" } else { "" },
            )
        })
        .collect::<Vec<String>>()
        .join(" | ")
}

/// The "window exists, monitors do not" fallback: the host window's own
/// PHYSICAL inner size at the window's scale factor, origin `(0,0)` — the only
/// geometry such a platform reports.
pub fn fallback_rect(window: &Window) -> MonitorRect {
    let size = window.inner_size();
    MonitorRect::new(
        Rect::new(0.0, 0.0, f64::from(size.width), f64::from(size.height)),
        window.scale_factor() as f32,
    )
}

/// The "no winit window at all" fallback: egui root-viewport size in POINTS
/// scaled to PHYSICAL pixels, origin `(0,0)`, monitor scale unknown (`1.0`).
/// `ppp` is floored to `1.0` (mirrors `egui_ctx.pixels_per_point().max(1.0)`;
/// `f64::max` also turns a NaN into `1.0`), and each dimension is clamped to
/// at least `1.0` px so placement never sees a degenerate rect.
pub fn fallback_rect_from_points(points: (f32, f32), ppp: f32) -> MonitorRect {
    let scale = f64::from(ppp).max(1.0);
    MonitorRect::new(
        Rect::new(
            0.0,
            0.0,
            (f64::from(points.0) * scale).max(1.0),
            (f64::from(points.1) * scale).max(1.0),
        ),
        1.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> (i32, i32, u32, u32, f64, Option<String>, bool) {
        (x, y, width, height, 1.0, None, false)
    }

    #[test]
    fn entries_maps_tuple_fields_and_keeps_enumeration_order() {
        let built = entries(vec![
            (
                1920,
                0,
                1280,
                720,
                1.25,
                Some(r"\\.\DISPLAY2".to_owned()),
                false,
            ),
            (0, 0, 1920, 1080, 1.0, None, true),
        ]);
        assert_eq!(built.len(), 2);
        assert_eq!(built[0].x, 1920);
        assert_eq!(built[0].y, 0);
        assert_eq!(built[0].width, 1280);
        assert_eq!(built[0].height, 720);
        assert_eq!(built[0].scale, 1.25);
        assert_eq!(built[0].name.as_deref(), Some(r"\\.\DISPLAY2"));
        assert!(!built[0].is_primary);
        assert!(built[1].is_primary);
        assert_eq!(built[1].name, None);
    }

    #[test]
    fn rects_skips_zero_size_entries() {
        let built = entries(vec![raw(0, 0, 1920, 1080), raw(-1920, 0, 0, 0)]);
        let rects = rects(&built);
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0].rect, Rect::new(0.0, 0.0, 1920.0, 1080.0));
    }

    #[test]
    fn rects_moves_the_primary_entry_first_and_keeps_rest_order() {
        let built = entries(vec![
            raw(-1920, 0, 1280, 720),
            raw(1920, 0, 1280, 720),
            (0, 0, 1920, 1080, 1.0, None, true),
        ]);
        let rects = rects(&built);
        assert_eq!(rects[0].rect, Rect::new(0.0, 0.0, 1920.0, 1080.0));
        assert_eq!(rects[1].rect, Rect::new(-1920.0, 0.0, 1280.0, 720.0));
        assert_eq!(rects[2].rect, Rect::new(1920.0, 0.0, 1280.0, 720.0));
    }

    #[test]
    fn rects_without_primary_keeps_enumeration_order() {
        let built = entries(vec![raw(-1920, 0, 1280, 720), raw(0, 0, 1920, 1080)]);
        let rects = rects(&built);
        assert_eq!(rects[0].rect, Rect::new(-1920.0, 0.0, 1280.0, 720.0));
        assert_eq!(rects[1].rect, Rect::new(0.0, 0.0, 1920.0, 1080.0));
    }

    #[test]
    fn rects_attaches_scale_and_normalizes_a_bad_scale() {
        let built = entries(vec![
            (0, 0, 1920, 1080, 1.5, None, false),
            (0, 0, 1920, 1080, 0.0, None, false),
        ]);
        let rects = rects(&built);
        assert_eq!(rects[0].scale, 1.5);
        assert_eq!(rects[1].scale, 1.0);
    }

    #[test]
    fn debug_line_none_and_empty_report_sentinel_wording() {
        assert_eq!(
            debug_line(None),
            "no winit window (toast uses the root viewport's rect)"
        );
        assert_eq!(debug_line(Some(&[])), "winit reported no monitors");
    }

    #[test]
    fn debug_line_formats_entries_and_flags_only_the_primary() {
        let built = entries(vec![
            (
                0,
                0,
                1920,
                1080,
                1.0,
                Some(r"\\.\DISPLAY1".to_owned()),
                true,
            ),
            (-1920, 0, 1280, 720, 1.25, None, false),
        ]);
        assert_eq!(
            debug_line(Some(&built)),
            r"#0 \\.\DISPLAY1 pos=(0,0) size=1920x1080 scale=1.00 primary | #1 unnamed pos=(-1920,0) size=1280x720 scale=1.25"
        );
    }

    #[test]
    fn fallback_rect_from_points_scales_points_and_guards_bad_ppp() {
        let rect = fallback_rect_from_points((800.0, 600.0), 1.5);
        assert_eq!(rect.rect, Rect::new(0.0, 0.0, 1200.0, 900.0));
        assert_eq!(rect.scale, 1.0);
        for ppp in [0.0, -3.0, 0.5, f32::NAN] {
            let guarded = fallback_rect_from_points((800.0, 600.0), ppp);
            assert_eq!(
                guarded.rect,
                Rect::new(0.0, 0.0, 800.0, 600.0),
                "ppp {ppp} must fall back to 1.0"
            );
        }
        let clamped = fallback_rect_from_points((-10.0, 5.0), 1.0);
        assert_eq!(clamped.rect, Rect::new(0.0, 0.0, 1.0, 5.0));
    }

    #[test]
    fn flag_origin_fallback_flags_zero_origin_only_without_primary() {
        let mut built = entries(vec![
            raw(1920, 0, 1280, 720),
            raw(0, 0, 1920, 1080),
            raw(0, 0, 0, 0),
        ]);
        flag_origin_as_primary(&mut built);
        assert!(built[1].is_primary);
        assert!(!built[0].is_primary);
        assert!(!built[2].is_primary);

        let mut flagged = entries(vec![
            raw(0, 0, 1920, 1080),
            (1920, 0, 1280, 720, 1.0, None, true),
        ]);
        flag_origin_as_primary(&mut flagged);
        assert!(!flagged[0].is_primary);
        assert!(flagged[1].is_primary);

        let mut unmatched = entries(vec![raw(1920, 0, 1280, 720)]);
        flag_origin_as_primary(&mut unmatched);
        assert!(!unmatched[0].is_primary);
    }
}
