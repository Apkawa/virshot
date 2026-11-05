//! Integration tests for the decode API of the `icons` crate: the contract is
//! "PNG bytes in, flat RGBA / `egui::IconData` out". Fixtures are built in
//! memory with `image` (regular dep of the package, usable in integration
//! tests — precedent: `crates/overlay/examples/overlay_demo.rs`), so the
//! tests pin the API without the embedded asset statics.

use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder, RgbaImage};

/// 2x2 fixture PNG whose pixels encode their coordinates, so a decode
/// roundtrip can be asserted pixel-by-pixel: R = x*40, G = y*40, B = 120,
/// A = 255.
fn fixture_png() -> Vec<u8> {
    let img = RgbaImage::from_fn(2, 2, |x, y| {
        image::Rgba([x as u8 * 40, y as u8 * 40, 120, 255])
    });
    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(img.as_raw(), 2, 2, ExtendedColorType::Rgba8)
        .expect("fixture PNG must encode");
    png
}

/// The fixture pixels in row-major flat RGBA order.
fn expected_rgba() -> Vec<u8> {
    let mut pixels = Vec::new();
    for (x, y) in [(0u8, 0u8), (1, 0), (0, 1), (1, 1)] {
        pixels.extend_from_slice(&[x * 40, y * 40, 120, 255]);
    }
    pixels
}

#[test]
fn rgba_decodes_png_bytes_to_flat_rgba() {
    let png = fixture_png();

    let (rgba, width, height) = icons::rgba(&png).expect("fixture PNG must decode");

    assert_eq!((width, height), (2, 2));
    assert_eq!(
        rgba.len(),
        2 * 2 * 4,
        "flat RGBA buffer must be width*height*4 bytes"
    );
    assert_eq!(
        rgba,
        expected_rgba(),
        "decoded pixels must roundtrip unchanged"
    );
}

#[test]
fn icon_data_matches_rgba_dims_and_bytes() {
    let png = fixture_png();

    let icon = icons::icon_data(&png).expect("fixture PNG must decode to IconData");

    assert_eq!(icon.width, 2);
    assert_eq!(icon.height, 2);
    assert_eq!(icon.rgba, expected_rgba());
}

#[test]
fn rgba_rejects_garbage_with_icons_error() {
    let error = icons::rgba(b"not-a-png").expect_err("garbage bytes must not decode");

    let message = error.to_string();
    assert!(
        message.to_lowercase().contains("decode"),
        "error message must mention the decode failure: {message}"
    );
}

/// Pixel `(x, y)` of a flat row-major RGBA buffer: index `(y * width + x) * 4`.
fn pixel_at(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * width + x) * 4) as usize;
    [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
}

/// The app icon asset must decode as a real 128x128 RGBA PNG, not an
/// empty or wrong-size placeholder.
#[test]
fn app_icon_asset_decodes_128x128() {
    let (rgba, width, height) =
        icons::rgba(icons::APP_ICON_PNG).expect("app icon asset must decode");

    assert_eq!((width, height), (128, 128), "app icon must be 128x128");
    assert_eq!(
        rgba.len(),
        128 * 128 * 4,
        "flat RGBA buffer must be width*height*4 bytes"
    );
}

/// The real app glyph is opaque in its body and rounded off at the corners:
/// all four corners are fully transparent and at least one pixel is fully
/// opaque. A fully transparent placeholder has no opaque body.
#[test]
fn app_icon_asset_has_opaque_body_and_transparent_corners() {
    let (rgba, width, _height) =
        icons::rgba(icons::APP_ICON_PNG).expect("app icon asset must decode");

    for (x, y) in [(0, 0), (127, 0), (0, 127), (127, 127)] {
        assert_eq!(
            pixel_at(&rgba, width, x, y)[3],
            0,
            "corner ({x}, {y}) must be fully transparent"
        );
    }

    assert!(
        rgba.chunks(4).any(|pixel| pixel[3] == 255),
        "app glyph body must contain opaque pixels (alpha == 255)"
    );
}

/// The warning icon asset must decode as a real 128x128 RGBA PNG.
#[test]
fn warning_asset_decodes_128x128() {
    let (rgba, width, height) =
        icons::rgba(icons::WARNING_ICON_PNG).expect("warning icon asset must decode");

    assert_eq!((width, height), (128, 128), "warning icon must be 128x128");
    assert_eq!(
        rgba.len(),
        128 * 128 * 4,
        "flat RGBA buffer must be width*height*4 bytes"
    );
}

/// The warning mark is a yellow triangle with black details, so the decoded
/// pixels must contain both a yellow-ish and a black-ish opaque pixel. A
/// fully transparent placeholder contains neither.
#[test]
fn warning_asset_has_yellow_and_black_marks() {
    let (rgba, _width, _height) =
        icons::rgba(icons::WARNING_ICON_PNG).expect("warning icon asset must decode");

    assert!(
        rgba.chunks(4)
            .any(|pixel| pixel[0] > 200 && pixel[1] > 140 && pixel[2] < 120 && pixel[3] > 200),
        "warning mark must contain yellow-ish pixels (r > 200, g > 140, b < 120, a > 200)"
    );

    assert!(
        rgba.chunks(4)
            .any(|pixel| pixel[0] < 80 && pixel[1] < 80 && pixel[2] < 80 && pixel[3] > 200),
        "warning mark must contain black-ish pixels (r < 80, g < 80, b < 80, a > 200)"
    );
}
