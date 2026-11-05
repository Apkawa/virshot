//! Embedded icon assets for VIRShot.
//!
//! The crate ships the PNG bytes compiled into the binary via `include_bytes!`,
//! so a release build never depends on files next to the executable: hosts get
//! `&'static [u8]` and decode them with `image` (or hand them to
//! `tray-icon`/`eframe` as raw bytes). Assets live in `assets/png/` at the crate
//! root, which is the include path — not a runtime lookup.

#![forbid(unsafe_code)]

mod error;

pub use error::IconsError;

/// Application icon, 128x128 PNG: tray icon and window icon source bytes.
pub static APP_ICON_PNG: &[u8] = include_bytes!("../assets/png/app-128.png");

/// Warning icon, 128x128 PNG: danger/warning toast chip source bytes.
pub static WARNING_ICON_PNG: &[u8] = include_bytes!("../assets/png/warning-128.png");

/// Decode PNG bytes into a flat RGBA buffer plus dimensions.
///
/// Returns `(rgba, width, height)`: `width * height * 4` bytes in row-major
/// order with straight (unmultiplied) alpha, exactly as stored by an RGBA PNG.
///
/// Fails with [`IconsError::IconDecodeError`] when `png` is not a decodable
/// PNG: invalid or corrupt bytes, or a format outside the crate's trimmed
/// `image` feature set (`png` only).
pub fn rgba(png: &[u8]) -> Result<(Vec<u8>, u32, u32), IconsError> {
    let image = image::load_from_memory(png)?.into_rgba8();
    let (width, height) = image.dimensions();
    Ok((image.into_raw(), width, height))
}

/// Decode PNG bytes into [`egui::IconData`] for a window icon
/// (`Viewport::with_icon`).
///
/// `egui::IconData` expects straight/unmultiplied alpha RGBA bytes, which is
/// exactly what an RGBA PNG stores — no premultiply step is needed.
///
/// Fails with [`IconsError::IconDecodeError`] on invalid or corrupt PNG bytes;
/// see [`rgba`].
pub fn icon_data(png_bytes: &[u8]) -> Result<egui::IconData, IconsError> {
    let (rgba, width, height) = rgba(png_bytes)?;
    Ok(egui::IconData {
        rgba,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_icon_png_is_embedded_png() {
        assert!(!APP_ICON_PNG.is_empty());
        assert_eq!(&APP_ICON_PNG[..4], b"\x89PNG");
    }

    #[test]
    fn warning_icon_png_is_embedded_png() {
        assert!(!WARNING_ICON_PNG.is_empty());
        assert_eq!(&WARNING_ICON_PNG[..4], b"\x89PNG");
    }
}
