//! Chip geometry and painted content, ported from `virshot`'s toast painting
//! and `overlay::content::paint`.
//!
//! Sizes are logical design units — the chip the painter draws at. Placement
//! multiplies them by the target monitor's DPI scale (`placement::physical_size`)
//! so the window ends up with PHYSICAL pixels on every monitor.

use overlay::OverlayContent;

use crate::color::ToastColor;
use crate::color::palette;
use crate::toast::Toast;

/// Chip height, ported from `virshot`'s toast chip.
const CHIP_HEIGHT: f64 = 36.0;

/// Narrowest chip, ported from `virshot`'s toast chip.
const CHIP_MIN_WIDTH: f64 = 220.0;

/// Widest chip, ported from `virshot`'s toast chip.
const CHIP_MAX_WIDTH: f64 = 460.0;

/// Per-character width, ported from `virshot`'s toast chip.
const CHAR_WIDTH: f64 = 9.0;

/// Horizontal padding, ported from `virshot`'s toast chip.
const CHIP_PADDING: f64 = 26.0;

/// Corner radius of the chip, ported from the toast paint in `virshot`
/// (`rect_filled(rect, 8.0, …)`).
const CHIP_RADIUS: f32 = 8.0;

/// Image chip size: a square thumbnail, big enough to read on a chip-sized
/// window. virshot's toast never painted an image, so this is a toast-side
/// choice (the overlay demo uses a 200x120 image window).
const IMAGE_CHIP_SIZE: [f64; 2] = [160.0, 160.0];

/// Chip width from text length, ported from `virshot`'s toast width rule.
pub(crate) fn chip_width(text: &str) -> f64 {
    ((text.chars().count() as f64) * CHAR_WIDTH + CHIP_PADDING)
        .clamp(CHIP_MIN_WIDTH, CHIP_MAX_WIDTH)
}

/// Window size for one toast: text chips stretch with the text, image chips are
/// a fixed square.
pub(crate) fn chip_size(toast: &Toast) -> [f64; 2] {
    match &toast.image {
        Some(_) => IMAGE_CHIP_SIZE,
        None => [chip_width(&toast.text), CHIP_HEIGHT],
    }
}

/// What the overlay paints for this toast: raw bytes for an image chip, a
/// painter for a text chip. The overlay applies the fade alpha on top
/// (`Ui::set_opacity`), so no alpha is baked into the content.
pub(crate) fn chip_content(toast: &Toast) -> OverlayContent {
    match &toast.image {
        Some(bytes) => OverlayContent::Image(bytes.clone()),
        None => {
            let colors = chip_colors(toast.color);
            let label = toast.text.clone();
            OverlayContent::Painter(Box::new(move |ui| {
                // Ported from the toast paint in `virshot`
                // (`rect_filled` radius 8 + centered `painter().text`); the fade
                // alpha is applied by the overlay via `Ui::set_opacity`.
                let rect = ui.max_rect();
                ui.painter().rect_filled(rect, CHIP_RADIUS, colors.fill);
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    label.as_str(),
                    egui::FontId::proportional(14.0),
                    colors.text,
                );
            }))
        }
    }
}

/// Paint colors for one chip.
struct ChipColors {
    fill: egui::Color32,
    text: egui::Color32,
}

/// Palette lookup, unpacked for the painter closure.
fn chip_colors(color: ToastColor) -> ChipColors {
    let palette = palette(color);
    ChipColors {
        fill: palette.fill,
        text: palette.text,
    }
}

#[cfg(test)]
mod tests {
    use super::IMAGE_CHIP_SIZE;
    use super::chip_content;
    use super::chip_size;
    use super::chip_width;
    use crate::color::ToastColor;
    use crate::toast::MonitorSel;
    use crate::toast::Toast;
    use overlay::Corner;
    use overlay::OverlayContent;

    fn text_toast(text: &str) -> Toast {
        Toast {
            monitor: MonitorSel::All,
            text: text.to_owned(),
            image: None,
            color: ToastColor::Success,
            corner: Corner::BottomRight,
            duration_ms: 2000,
        }
    }

    fn image_toast(bytes: Vec<u8>) -> Toast {
        Toast {
            image: Some(bytes),
            ..text_toast("capture saved")
        }
    }

    #[test]
    fn chip_width_grows_with_text_and_clamps() {
        // Ported from virshot `toast_width`: chars * 9 + 26, clamped 220..460.
        assert_eq!(chip_width("short"), 220.0, "clamped to the minimum");
        assert_eq!(chip_width("Saved: shot-2.png"), 220.0);
        let medium = chip_width("failed to capture monitor: no monitors found");
        assert!(
            medium > 220.0 && medium < 460.0,
            "mid-length grows: {medium}"
        );
        assert_eq!(
            chip_width(&"x".repeat(1000)),
            460.0,
            "clamped to the maximum"
        );
    }

    #[test]
    fn chip_size_is_fixed_height_for_text() {
        assert_eq!(chip_size(&text_toast("Saved: shot.png")), [220.0, 36.0]);
    }

    #[test]
    fn chip_size_for_image_is_square() {
        assert_eq!(chip_size(&image_toast(vec![1, 2, 3])), IMAGE_CHIP_SIZE);
        assert_eq!(IMAGE_CHIP_SIZE[0], IMAGE_CHIP_SIZE[1]);
    }

    #[test]
    fn chip_size_survives_an_empty_text() {
        assert_eq!(chip_size(&text_toast("")), [220.0, 36.0]);
    }

    #[test]
    fn chip_content_carries_image_bytes_undecoded() {
        // Decode happens on the overlay side; toast forwards raw bytes.
        let content = chip_content(&image_toast(vec![9, 8, 7]));
        match content {
            OverlayContent::Image(bytes) => assert_eq!(bytes, vec![9, 8, 7]),
            // OverlayContent is not Debug; name the variant instead.
            OverlayContent::Text(_) => panic!("image toast must paint image bytes, got Text"),
            OverlayContent::Painter(_) => panic!("image toast must paint image bytes, got Painter"),
        }
    }

    #[test]
    fn chip_content_for_text_paints_a_chip() {
        let content = chip_content(&text_toast("Saved: shot.png"));
        assert!(
            matches!(content, OverlayContent::Painter(_)),
            "a colored chip needs a painter (overlay's Text paints one fixed chip)"
        );
    }
}
