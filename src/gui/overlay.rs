//! Overlay tab (spec §4.3/§4.4): the test-button gate and its disabled hover
//! hint, the honest test filename, the chip built from the CURRENT draft with
//! the real save wording, the corner labels, and the publish path through the
//! toast hand-off. The `SettingsApp` methods live in this file; `gui/app.rs`
//! drives them from the `ui` pass. Toast plumbing (`OverlayHost` +
//! `ToastService`) lives in `gui/app.rs`; the update chips live in `gui/update.rs`.

use std::path::Path;

use overlay::Corner;
use toast::MonitorSel;
use toast::Toast;
use toast::ToastColor;

use crate::capture;
use crate::config::Overlay;
use crate::config::OverlayPosition;

use super::SettingsApp;

/// GUI labels for overlay corners (config enum names).
fn position_label(position: OverlayPosition) -> &'static str {
    match position {
        OverlayPosition::TopLeft => "TopLeft",
        OverlayPosition::TopRight => "TopRight",
        OverlayPosition::BottomLeft => "BottomLeft",
        OverlayPosition::BottomRight => "BottomRight",
    }
}

/// Honest filename for the Overlay tab's test chip (Stage 7): no capture and
/// no save happen, so the chip must not name a real file — the fixed name
/// says what it is a test of, matching the spec §4.3 success wording
/// (`Saved: <file name>`).
pub(crate) const OVERLAY_TEST_FILENAME: &str = "test.png";

/// Hover hint for the disabled test button (catalog `overlay.test_disabled_hint`,
/// spec §4.4). The button is gated here in the GUI because a direct
/// `ToastHandle::show` does not consult `overlay.enabled` — that gate lives in
/// `capture::should_show` and only applies to capture-driven chips.
fn overlay_test_disabled_hint() -> String {
    rust_i18n::t!("overlay.test_disabled_hint").into_owned()
}

/// Test-button gate (Stage 7): enabled exactly while the overlay toast
/// checkbox is checked.
fn overlay_test_button_enabled(enabled: bool) -> bool {
    enabled
}

/// Test chip text: the real save success wording (spec §4.3), built by the
/// very helper `capture::toast_from_outcome` uses for a real save, so the
/// button cannot drift from the `Saved: <file name>` format.
pub(crate) fn overlay_test_message(filename: &str) -> String {
    capture::saved_text(Path::new(filename))
}

/// The chip the Overlay tab publishes on a test click, built from the CURRENT
/// draft (spec §4.3): corner, monitor selection, and clamped hold mirror the
/// rules `capture::toast_from_outcome` applies to `config::Overlay` (those
/// helpers are private there — duplicated rather than widening capture's
/// interface, per the stage plan). Success color, text-only: no bytes and no
/// path, so nothing touches disk. The chip shows what the user just
/// configured, even while the draft is unsaved — that is the point of a test.
fn test_overlay_toast(overlay: &Overlay) -> Toast {
    Toast {
        monitor: if overlay.on_all_monitors {
            MonitorSel::All
        } else {
            MonitorSel::Index(0)
        },
        text: overlay_test_message(OVERLAY_TEST_FILENAME),
        image: None,
        color: ToastColor::Success,
        corner: match overlay.position {
            OverlayPosition::TopLeft => Corner::TopLeft,
            OverlayPosition::TopRight => Corner::TopRight,
            OverlayPosition::BottomLeft => Corner::BottomLeft,
            OverlayPosition::BottomRight => Corner::BottomRight,
        },
        duration_ms: u32::try_from(overlay.duration_ms).unwrap_or(u32::MAX),
    }
}

impl SettingsApp {
    pub(crate) fn overlay_tab(&mut self, ui: &mut egui::Ui) {
        let overlay = &mut self.draft.overlay;
        // The test chip is published AFTER the tab body: the tab borrows
        // `draft.overlay` mutably and publishing reads `self.handoff` — the
        // same deferred-action pattern as Browse in the General tab.
        let mut test_chip: Option<Toast> = None;

        ui.horizontal(|ui| {
            ui.checkbox(&mut overlay.enabled, rust_i18n::t!("overlay.enabled_label"));
            let enabled = overlay_test_button_enabled(overlay.enabled);
            let test = ui.add_enabled(
                enabled,
                egui::Button::new(rust_i18n::t!("overlay.test_button")),
            );
            if enabled {
                if test.clicked() {
                    test_chip = Some(test_overlay_toast(overlay));
                }
            } else {
                // `on_disabled_hover_text` consumes the response; a disabled
                // button can never be clicked, so the branches are exclusive.
                test.on_disabled_hover_text(overlay_test_disabled_hint());
            }
        });

        ui.horizontal(|ui| {
            // Catalog `overlay.duration_suffix` is `" ms"` — the leading space
            // is part of the text, so the suffix renders byte-for-byte like
            // the old literal.
            let duration_suffix = rust_i18n::t!("overlay.duration_suffix").into_owned();
            ui.add(
                egui::DragValue::new(&mut overlay.duration_ms)
                    .range(250..=10_000)
                    .suffix(duration_suffix.as_str()),
            );
            ui.label(rust_i18n::t!("overlay.duration_label"));
        });

        egui::ComboBox::from_label(rust_i18n::t!("overlay.position_label"))
            .selected_text(position_label(overlay.position))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut overlay.position, OverlayPosition::TopLeft, "TopLeft");
                ui.selectable_value(&mut overlay.position, OverlayPosition::TopRight, "TopRight");
                ui.selectable_value(
                    &mut overlay.position,
                    OverlayPosition::BottomLeft,
                    "BottomLeft",
                );
                ui.selectable_value(
                    &mut overlay.position,
                    OverlayPosition::BottomRight,
                    "BottomRight",
                );
            });

        ui.checkbox(
            &mut overlay.on_all_monitors,
            rust_i18n::t!("overlay.on_all_monitors_label"),
        );
        ui.label(rust_i18n::t!("overlay.all_monitors_hint"));

        if let Some(chip) = test_chip {
            self.publish_overlay_test_chip(chip);
        }
    }

    /// Publish the Overlay tab's test chip through the SAME hand-off slot
    /// real saves use (spec §4.3): the handle never blocks, and the chip
    /// spawns on the next `overlay_host.update` pass in `logic`. No capture,
    /// no save, no config write, no hotkey re-registration — only the chip
    /// and one INFO line. A missing handle (no toast service) or a closed
    /// channel is WARNed, never fatal (spec §5).
    fn publish_overlay_test_chip(&self, chip: Toast) {
        let Some(handle) = capture::handoff_handle(&self.handoff) else {
            tracing::warn!("Overlay test chip skipped: no toast handle attached");
            return;
        };
        let text = chip.text.clone(); // `show` consumes the chip; the log quotes it
        if let Err(err) = handle.show(chip) {
            tracing::warn!("Overlay test chip failed: {err}");
            return;
        }
        tracing::info!("Overlay test toast shown: {text} (no file written)");
    }
}

#[cfg(test)]
#[path = "overlay.tests.rs"]
mod tests;
