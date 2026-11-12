//! Icon glyphs, colors, and embedded textures.

use std::sync::Arc;

use crate::hotkeys::Registration;

use super::binding::reserved_tooltip;

/// Bind-button minimum width so the three rows stay visually aligned.
pub(crate) const BIND_BUTTON_MIN_WIDTH: f32 = 170.0;

/// Status-icon colors (warning = red-orange, info = blue).
pub(crate) const WARNING_COLOR: egui::Color32 = egui::Color32::from_rgb(246, 118, 100);
const INFO_COLOR: egui::Color32 = egui::Color32::from_rgb(118, 176, 240);

/// Painted size of the warning triangle in a status row, in logical points.
/// The SAME slot is reserved for the ASCII `!` fallback so the label after
/// the icon never jumps between the texture and the fallback rendering.
pub(crate) const WARNING_ICON_SIZE: f32 = 16.0;

/// Status icon kinds shown right of the bind button (spec §4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IconKind {
    /// Registration failed; the typed reason goes in the tooltip.
    Warning,
    /// The action/slot is reserved (out of MVP scope).
    Info,
}

/// An icon plus its tooltip text, derived from a registration status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StatusIcon {
    pub(crate) kind: IconKind,
    pub(crate) tooltip: String,
}

/// Registration status → icon (spec §4.4): `Failed` → warning with the typed
/// reason as tooltip, `Reserved` → info, `Registered` → nothing (the inline
/// status text is gone; main still logs it).
pub(crate) fn icon_for_status(status: &Registration) -> Option<StatusIcon> {
    match status {
        Registration::Registered { .. } => None,
        Registration::Failed { reason, .. } => Some(StatusIcon {
            kind: IconKind::Warning,
            tooltip: reason.clone(),
        }),
        Registration::Reserved { .. } => Some(StatusIcon {
            kind: IconKind::Info,
            tooltip: reserved_tooltip(),
        }),
    }
}

/// Glyph for an icon kind. `⚠`/`ℹ` are NOT in egui's bundled fonts (checked
/// the locked cmaps), so the icons are colored ASCII letters — the colors carry
/// the meaning, the letter keeps the shape unambiguous without a symbol font.
/// The warning icon renders as the embedded triangle texture when it loaded;
/// this glyph is its fallback.
pub(crate) fn icon_glyph(kind: IconKind) -> &'static str {
    match kind {
        IconKind::Warning => "!",
        IconKind::Info => "i",
    }
}

/// Color for an icon kind (via `RichText::color`).
pub(crate) fn icon_color(kind: IconKind) -> egui::Color32 {
    match kind {
        IconKind::Warning => WARNING_COLOR,
        IconKind::Info => INFO_COLOR,
    }
}

/// Decode the embedded warning-triangle asset into a straight-alpha egui
/// color image. egui 0.37 core has no PNG loader, so the bytes go through
/// `icons::rgba` (precedent: `crates/overlay/src/content.rs` and
/// `crates/icons/examples/icons_gallery.rs`).
///
/// Failure mode: [`icons::IconsError`] on invalid or corrupt PNG bytes —
/// logged at WARN (this runs once, at window creation) and `None`, so
/// status rows fall back to the ASCII `!` glyph and never lose their marker.
fn warning_color_image() -> Option<egui::ColorImage> {
    match icons::rgba(icons::WARNING_ICON_PNG) {
        Ok((rgba, width, height)) => Some(egui::ColorImage::from_rgba_unmultiplied(
            [width as usize, height as usize],
            &rgba,
        )),
        Err(err) => {
            tracing::warn!(
                "Warning icon decode failed: {err}; status rows fall back to the \"!\" glyph"
            );
            None
        }
    }
}

/// Upload the decoded warning asset as a texture (`Context::load_texture`)
/// ONCE, in the eframe `create` closure — the same shape the gallery example
/// uses. `None` when the decode failed (see [`warning_color_image`]).
pub(crate) fn warning_icon_texture(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    warning_color_image().map(|image| {
        ctx.load_texture(
            "virshot-warning-icon",
            image,
            egui::TextureOptions::default(),
        )
    })
}

/// Decode the embedded app-icon asset and upload it as a texture for the
/// About header, mirroring [`warning_icon_texture`]: egui 0.37 core has no
/// PNG loader and the `egui_extras` loaders are OFF (no Uri/Bytes image
/// path), so the bytes go through `icons::rgba` → `ColorImage::
/// from_rgba_unmultiplied` → `Context::load_texture` — texture route only.
/// Wrapped in `Arc` so the viewport callback can share the handle without
/// re-uploading per paint (`TextureHandle::drop` frees the texture id).
///
/// Failure mode: [`icons::IconsError`] on invalid or corrupt PNG bytes —
/// logged at WARN and `None`, so the About header paints its text without
/// an icon and never panics.
pub(crate) fn about_icon_texture(ctx: &egui::Context) -> Option<Arc<egui::TextureHandle>> {
    match icons::rgba(icons::APP_ICON_PNG) {
        Ok((rgba, width, height)) => Some(Arc::new(ctx.load_texture(
            "virshot-about-icon",
            egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &rgba),
            egui::TextureOptions::default(),
        ))),
        Err(err) => {
            tracing::warn!(
                "About app icon decode failed: {err}; the About header paints text only"
            );
            None
        }
    }
}

#[cfg(test)]
#[path = "icons.tests.rs"]
mod tests;
