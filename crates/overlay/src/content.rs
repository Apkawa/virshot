//! Overlay content: what a window paints, and how it paints into one `egui::Ui`.
//!
//! One responsibility — content and its painting. Window/GL plumbing lives in
//! `service` and `gl_boot`. The `alpha` argument is an opacity multiplier applied
//! through `egui::Ui::set_opacity`, so text, fills, and image textures all fade
//! together; a transparent window still clears to nothing behind the content.

use crate::error::OverlayError;

/// What an overlay window paints into its whole window rect.
pub enum OverlayContent {
    /// Wrapped, centered text on a dark chip (the toast shape).
    Text(String),
    /// Raw PNG/JPEG/WebP bytes, stretched to fill the window rect.
    Image(Vec<u8>),
    /// Callback painting the whole window. The overlay paints no background for
    /// this variant: the callback owns every pixel, colors included.
    Painter(Box<dyn Fn(&mut egui::Ui) + Send>),
}

impl From<String> for OverlayContent {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<&str> for OverlayContent {
    fn from(text: &str) -> Self {
        Self::Text(text.to_owned())
    }
}

impl From<Vec<u8>> for OverlayContent {
    fn from(bytes: Vec<u8>) -> Self {
        Self::Image(bytes)
    }
}

impl From<Box<dyn Fn(&mut egui::Ui) + Send>> for OverlayContent {
    fn from(painter: Box<dyn Fn(&mut egui::Ui) + Send>) -> Self {
        Self::Painter(painter)
    }
}

/// Chip colors, matching `virshot`'s toast chip.
const CHIP_FILL: egui::Color32 = egui::Color32::from_rgb(24, 24, 24);
const CHIP_TEXT: egui::Color32 = egui::Color32::from_rgb(230, 230, 230);

/// Cache of the decoded image texture, keyed by the exact bytes last painted.
///
/// Re-uploads only when the bytes change, via `egui::TextureHandle::set`, so a
/// repaint of the same image costs no decode.
pub(crate) struct ImageCache {
    bytes: Option<Vec<u8>>,
    handle: Option<egui::TextureHandle>,
}

impl ImageCache {
    pub(crate) fn new() -> Self {
        Self {
            bytes: None,
            handle: None,
        }
    }

    /// Texture for `bytes`, decoding and uploading only when the bytes changed.
    ///
    /// Fails with `OverlayError::ImageDecode` when the bytes are not a decodable
    /// image, and `OverlayError::ImageTooLarge` when a side exceeds the GL
    /// implementation's texture limit (`max_texture_side`).
    fn texture_for(
        &mut self,
        ctx: &egui::Context,
        bytes: &[u8],
        max_texture_side: usize,
    ) -> Result<egui::TextureHandle, OverlayError> {
        if let Some(handle) = &self.handle
            && self.bytes.as_deref() == Some(bytes)
        {
            return Ok(handle.clone());
        }

        let image = image::load_from_memory(bytes).map_err(|err| OverlayError::ImageDecode {
            message: err.to_string(),
        })?;
        let width = image.width() as usize;
        let height = image.height() as usize;
        if width > max_texture_side || height > max_texture_side {
            return Err(OverlayError::ImageTooLarge {
                width,
                height,
                max: max_texture_side,
            });
        }

        let pixels = image.to_rgba8();
        let color_image =
            egui::ColorImage::from_rgba_unmultiplied([width, height], pixels.as_raw());
        let options = egui::TextureOptions::default();
        let handle = match &mut self.handle {
            Some(handle) => {
                handle.set(color_image, options);
                handle.clone()
            }
            None => {
                let created = ctx.load_texture("overlay-image", color_image, options);
                self.handle = Some(created.clone());
                created
            }
        };
        self.bytes = Some(bytes.to_vec());
        Ok(handle)
    }
}

/// Paint `content` into the whole `ui` rect with `alpha` opacity.
///
/// Fails only for bad `Image` bytes (see `ImageCache::texture_for`); the caller
/// logs the error and keeps the window running.
pub(crate) fn paint(
    ui: &mut egui::Ui,
    content: &OverlayContent,
    alpha: f32,
    cache: &mut ImageCache,
    max_texture_side: usize,
) -> Result<(), OverlayError> {
    let rect = ui.max_rect();
    ui.set_opacity(alpha.clamp(0.0, 1.0));

    match content {
        OverlayContent::Text(text) => {
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::ZERO, CHIP_FILL);
            ui.visuals_mut().override_text_color = Some(CHIP_TEXT);
            ui.centered_and_justified(|ui| {
                ui.label(text);
            });
        }
        OverlayContent::Image(bytes) => {
            let handle = cache.texture_for(ui.ctx(), bytes, max_texture_side)?;
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::ZERO, CHIP_FILL);
            let uv = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0));
            ui.painter()
                .image(handle.id(), rect, uv, egui::Color32::WHITE);
        }
        OverlayContent::Painter(paint_fn) => paint_fn(ui),
    }
    Ok(())
}
