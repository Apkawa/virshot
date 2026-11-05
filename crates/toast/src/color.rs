//! Chip palette per toast color (spec §4.3: default/success/danger/warning).
//!
//! `Default` is the ported `virshot` chip (`src/gui.rs:191-192`: dark chip,
//! light text). `Danger` uses the repo's existing red-orange accent
//! (`src/gui.rs:681` `WARNING_COLOR`) as its text color; success/warning
//! accents have no counterpart in virshot's toast painting (it paints one
//! neutral chip), so those entries are new palette values tuned for the same
//! dark chip.

/// What a toast announces. Mapped to a chip palette and to the hold rule in
/// `rules` (`Danger` holds [`crate::rules::ERROR_HOLD_MS`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastColor {
    /// Neutral chip (the ported virshot toast look).
    Default,
    /// Capture saved — green accent.
    Success,
    /// Capture/Save failed — red accent, always held [`crate::rules::ERROR_HOLD_MS`].
    Danger,
    /// Recoverable problem — amber accent.
    Warning,
}

/// Fill and text colors of one chip. The overlay applies the fade alpha on top
/// (`Ui::set_opacity` multiplies painted colors), so these are full-opacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ChipPalette {
    pub(crate) fill: egui::Color32,
    pub(crate) text: egui::Color32,
}

/// Dark chip fill ported from `virshot/src/gui.rs:191`.
const CHIP_FILL: egui::Color32 = egui::Color32::from_rgb(24, 24, 24);

/// Chip text color ported from `virshot/src/gui.rs:192`.
const CHIP_TEXT: egui::Color32 = egui::Color32::from_rgb(230, 230, 230);

/// Red-orange accent ported from `virshot/src/gui.rs:681` (`WARNING_COLOR`).
const DANGER_TEXT: egui::Color32 = egui::Color32::from_rgb(246, 118, 100);

/// Green accent (no virshot counterpart — new value on the same dark chip).
const SUCCESS_TEXT: egui::Color32 = egui::Color32::from_rgb(126, 200, 126);

/// Amber accent (no virshot counterpart — new value on the same dark chip).
const WARNING_TEXT: egui::Color32 = egui::Color32::from_rgb(240, 190, 90);

/// Tinted variants of [`CHIP_FILL`] so each color reads as its own chip.
const SUCCESS_FILL: egui::Color32 = egui::Color32::from_rgb(24, 40, 26);
const DANGER_FILL: egui::Color32 = egui::Color32::from_rgb(44, 24, 24);
const WARNING_FILL: egui::Color32 = egui::Color32::from_rgb(44, 34, 20);

/// The chip palette for `color`.
pub(crate) fn palette(color: ToastColor) -> ChipPalette {
    match color {
        ToastColor::Default => ChipPalette {
            fill: CHIP_FILL,
            text: CHIP_TEXT,
        },
        ToastColor::Success => ChipPalette {
            fill: SUCCESS_FILL,
            text: SUCCESS_TEXT,
        },
        ToastColor::Danger => ChipPalette {
            fill: DANGER_FILL,
            text: DANGER_TEXT,
        },
        ToastColor::Warning => ChipPalette {
            fill: WARNING_FILL,
            text: WARNING_TEXT,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::ToastColor;
    use super::palette;

    #[test]
    fn palette_maps_each_color() {
        let default = palette(ToastColor::Default);
        let success = palette(ToastColor::Success);
        let danger = palette(ToastColor::Danger);
        let warning = palette(ToastColor::Warning);

        // Ported from virshot's toast painting (src/gui.rs:191-192).
        assert_eq!(default.fill, egui::Color32::from_rgb(24, 24, 24));
        assert_eq!(default.text, egui::Color32::from_rgb(230, 230, 230));
        // Danger's accent is the repo's existing red-orange (src/gui.rs:681).
        assert_eq!(danger.text, egui::Color32::from_rgb(246, 118, 100));

        assert_eq!(success.text, egui::Color32::from_rgb(126, 200, 126));
        assert_eq!(warning.text, egui::Color32::from_rgb(240, 190, 90));

        // Every color is distinguishable from the others.
        let all = [default, success, danger, warning];
        for pair in all.windows(2) {
            assert_ne!(pair[0].fill, pair[1].fill, "fills must differ");
            assert_ne!(pair[0].text, pair[1].text, "text colors must differ");
        }
        for chip in all {
            assert_ne!(chip.fill, chip.text, "a chip needs contrast");
        }
    }
}
