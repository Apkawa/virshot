//! Which monitors get a chip, and where on each one.
//!
//! Corner insets and clamping come from `overlay::place`; toast adds one rule on
//! top: a chip that does not fit a monitor is skipped rather than clamped to the
//! monitor's origin (a chip hanging off a small monitor is worse than no chip).
//!
//! Monitors are PHYSICAL rects with their DPI scale attached
//! (`overlay::MonitorRect`); a chip's design size is in logical points, so it is
//! multiplied by that scale before placement. Insets stay physical pixels.

use overlay::Corner;
use overlay::MonitorRect;
use overlay::place;

use crate::toast::MonitorSel;

/// Placement targets for one toast: one per selected monitor that can hold the
/// chip, carrying the placed PHYSICAL rect and that monitor's scale (so the
/// overlay backend divides the rect by the right pixels-per-point). An
/// out-of-range `MonitorSel::Index` selects nothing.
pub(crate) fn placements(
    monitors: &[MonitorRect],
    monitor: MonitorSel,
    corner: Corner,
    logical_size: [f64; 2],
) -> Vec<MonitorRect> {
    let selected: Vec<MonitorRect> = match monitor {
        MonitorSel::All => monitors.to_vec(),
        MonitorSel::Index(index) => monitors
            .get(index)
            .copied()
            .map(|monitor| vec![monitor])
            .unwrap_or_default(),
    };
    selected
        .iter()
        .filter(|monitor| fits(monitor, logical_size))
        .filter_map(|monitor| {
            place(monitor.rect, corner, physical_size(monitor, logical_size)).map(|rect| {
                MonitorRect {
                    rect,
                    scale: monitor.scale,
                }
            })
        })
        .collect()
}

/// The chip size in PHYSICAL pixels on this monitor.
fn physical_size(monitor: &MonitorRect, logical_size: [f64; 2]) -> [f64; 2] {
    let scale = f64::from(monitor.scale);
    [logical_size[0] * scale, logical_size[1] * scale]
}

/// Can a chip of `logical_size` sit inside this monitor, once scaled to its DPI?
fn fits(monitor: &MonitorRect, logical_size: [f64; 2]) -> bool {
    let [width, height] = physical_size(monitor, logical_size);
    width <= monitor.rect.w && height <= monitor.rect.h
}

#[cfg(test)]
mod tests {
    use super::fits;
    use super::placements;
    use crate::toast::MonitorSel;
    use overlay::CORNER_INSET_X;
    use overlay::CORNER_INSET_Y;
    use overlay::Corner;
    use overlay::MonitorRect;
    use overlay::Rect;

    fn monitor(x: f64, y: f64, w: f64, h: f64) -> MonitorRect {
        MonitorRect::new(Rect::new(x, y, w, h), 1.0)
    }

    fn chip() -> [f64; 2] {
        [220.0, 36.0]
    }

    #[test]
    fn monitor_sel_all_covers_every_monitor() {
        let monitors = [
            monitor(0.0, 0.0, 1920.0, 1080.0),
            monitor(1920.0, 0.0, 1280.0, 720.0),
        ];
        let placed = placements(&monitors, MonitorSel::All, Corner::BottomRight, chip());
        assert_eq!(placed.len(), 2, "one chip per monitor");
        assert_eq!(placed[0].rect.x, 1920.0 - 220.0 - CORNER_INSET_X);
        assert_eq!(placed[0].rect.y, 1080.0 - 36.0 - CORNER_INSET_Y);
        assert_eq!(placed[1].rect.x, 1920.0 + 1280.0 - 220.0 - CORNER_INSET_X);
        assert_eq!(placed[1].rect.y, 720.0 - 36.0 - CORNER_INSET_Y);
    }

    #[test]
    fn monitor_sel_index_selects_one_monitor() {
        let monitors = [
            monitor(0.0, 0.0, 1920.0, 1080.0),
            monitor(1920.0, 0.0, 1280.0, 720.0),
        ];
        let placed = placements(&monitors, MonitorSel::Index(1), Corner::TopLeft, chip());
        assert_eq!(placed.len(), 1);
        assert_eq!(placed[0].rect.x, 1920.0 + CORNER_INSET_X);
        assert_eq!(placed[0].rect.y, CORNER_INSET_Y);
    }

    #[test]
    fn monitor_sel_index_out_of_range_skips() {
        let monitors = [monitor(0.0, 0.0, 1920.0, 1080.0)];
        assert!(
            placements(&monitors, MonitorSel::Index(5), Corner::BottomRight, chip()).is_empty(),
            "a monitor index nobody has shows nothing"
        );
        assert!(placements(&[], MonitorSel::All, Corner::BottomRight, chip()).is_empty());
    }

    #[test]
    fn placement_skips_monitor_when_chip_does_not_fit() {
        // overlay::place clamps a too-large chip to the monitor origin; toast
        // skips the monitor instead of spilling the chip past its edge.
        let monitors = [
            monitor(0.0, 0.0, 100.0, 40.0),
            monitor(200.0, 0.0, 1920.0, 1080.0),
        ];
        let placed = placements(&monitors, MonitorSel::All, Corner::BottomRight, chip());
        assert_eq!(
            placed.len(),
            1,
            "only the monitor the chip fits gets a chip"
        );
        assert_eq!(placed[0].rect.x, 200.0 + 1920.0 - 220.0 - CORNER_INSET_X);
    }

    #[test]
    fn fits_requires_both_dimensions() {
        assert!(fits(&monitor(0.0, 0.0, 220.0, 36.0), chip()));
        assert!(!fits(&monitor(0.0, 0.0, 219.0, 36.0), chip()));
        assert!(!fits(&monitor(0.0, 0.0, 220.0, 35.0), chip()));
    }

    #[test]
    fn placement_scales_the_chip_to_the_monitor_dpi() {
        // A 220x36 point chip is 330x54 px on a 150% monitor, and the scale
        // travels with the placement so the backend can convert it back.
        let monitors = [MonitorRect::new(Rect::new(0.0, 0.0, 1920.0, 1080.0), 1.5)];
        let placed = placements(&monitors, MonitorSel::All, Corner::BottomRight, chip());
        assert_eq!(placed.len(), 1);
        assert_eq!(placed[0].scale, 1.5);
        assert_eq!(placed[0].rect.w, 330.0);
        assert_eq!(placed[0].rect.h, 54.0);
        assert_eq!(placed[0].rect.x, 1920.0 - 330.0 - CORNER_INSET_X);
        assert_eq!(placed[0].rect.y, 1080.0 - 54.0 - CORNER_INSET_Y);
    }

    #[test]
    fn a_scaled_chip_that_does_not_fit_a_scaled_monitor_is_skipped() {
        // 220 px chip on a 220 px monitor fits at 100%; the same monitor at 150%
        // needs 330 px, so the chip is skipped instead of spilling.
        let monitors = [MonitorRect::new(Rect::new(0.0, 0.0, 220.0, 36.0), 1.5)];
        assert!(
            placements(&monitors, MonitorSel::All, Corner::BottomRight, chip()).is_empty(),
            "the scaled chip no longer fits"
        );
    }
}
