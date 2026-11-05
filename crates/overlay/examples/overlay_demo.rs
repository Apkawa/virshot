//! End-to-end demonstration of the `overlay` crate: placement, text chips,
//! a custom painter, an in-memory PNG, fade alpha, and teardown.
//!
//! Headless usage: `xvfb-run -a ./cargo.sh run --example overlay_demo`.

use std::thread::sleep;
use std::time::Duration;

use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder, RgbaImage};
use overlay::{Corner, OverlayContent, OverlayHandle, OverlayWindowSpec, Rect, place};

/// Synthetic monitor: the demo has no display enumeration yet.
const MONITOR: Rect = Rect {
    x: 0.0,
    y: 0.0,
    w: 1920.0,
    h: 1080.0,
};
const CHIP_SIZE: [f64; 2] = [320.0, 64.0];
const IMAGE_SIZE: [f64; 2] = [200.0, 120.0];

/// Emits the Windows click-through notice once per run, not once per window.
static CLICK_THROUGH_NOTE: std::sync::Once = std::sync::Once::new();

fn main() {
    println!("demo: starting overlay demo");
    let handle = OverlayHandle::new();

    // Text chip, top-left corner, fully opaque.
    let Some(top_left) = place(MONITOR, Corner::TopLeft, CHIP_SIZE) else {
        eprintln!("demo: the synthetic monitor is too small for a chip");
        return;
    };
    let chip = spawn(
        &handle,
        top_left,
        OverlayContent::Text("virshot: screenshot saved".to_owned()),
        1.0,
    );

    sleep(Duration::from_millis(900));

    // Fade out in steps; each step replaces content and repaints.
    fade(
        &handle,
        chip,
        OverlayContent::Text("virshot: screenshot saved".to_owned()),
        0.6,
    );
    sleep(Duration::from_millis(500));
    fade(
        &handle,
        chip,
        OverlayContent::Text("virshot: screenshot saved".to_owned()),
        0.25,
    );

    // Custom painter content: a filled rect plus a ring, drawn by the callback.
    fade(
        &handle,
        chip,
        OverlayContent::Painter(Box::new(|ui| {
            let rect = ui.max_rect();
            ui.painter()
                .rect_filled(rect.shrink(8.0), 8, egui::Color32::from_rgb(40, 90, 140));
            ui.painter().circle_stroke(
                rect.center(),
                rect.height() * 0.32,
                egui::Stroke::new(3.0, egui::Color32::from_rgb(230, 230, 230)),
            );
        })),
        1.0,
    );
    sleep(Duration::from_millis(900));

    // Second window: a PNG built in memory, bottom-right.
    let bytes = gradient_png();
    let Some(bottom_right) = place(MONITOR, Corner::BottomRight, IMAGE_SIZE) else {
        eprintln!("demo: the synthetic monitor is too small for the image chip");
        return;
    };
    let image_id = spawn(
        &handle,
        bottom_right,
        OverlayContent::Image(bytes.clone()),
        1.0,
    );

    sleep(Duration::from_millis(900));

    // Corner cycling: hide and re-spawn on the next corner.
    for corner in [Corner::BottomLeft, Corner::TopRight, Corner::TopLeft] {
        if let Err(err) = handle.hide(image_id) {
            eprintln!("demo: hide failed: {err}");
        }
        let Some(rect) = place(MONITOR, corner, IMAGE_SIZE) else {
            eprintln!("demo: the synthetic monitor is too small for a corner");
            return;
        };
        let next = spawn(&handle, rect, OverlayContent::Image(bytes.clone()), 1.0);
        println!("demo: image chip moved to {corner:?} as window {next}");
        sleep(Duration::from_millis(700));
    }

    // Tear down: chips first, then the whole thread.
    for id in [chip, image_id] {
        if let Err(err) = handle.hide(id) {
            eprintln!("demo: hide failed: {err}");
        }
    }
    if let Err(err) = handle.quit_all() {
        eprintln!("demo: quit_all failed: {err}");
    }
    println!("demo: overlay thread stopped");
}

fn spawn(
    handle: &OverlayHandle,
    rect: Rect,
    content: OverlayContent,
    alpha: f32,
) -> overlay::OverlayId {
    let spec = OverlayWindowSpec {
        rect,
        // The thread backend reads `rect` as physical px directly and ignores
        // the scale; the demo's synthetic monitor is 100% DPI.
        scale: 1.0,
        transparent: true,
        click_through: true,
        content_protected: true,
    };
    if spec.click_through && cfg!(windows) {
        CLICK_THROUGH_NOTE.call_once(|| {
            eprintln!("demo: click-through ignored on Windows (winit WS_EX_LAYERED limitation)");
        });
    }
    match handle.spawn_window(spec, content, alpha) {
        Ok(id) => {
            println!(
                "demo: window {id} at x={} y={} w={} h={}",
                rect.x, rect.y, rect.w, rect.h
            );
            id
        }
        Err(err) => {
            eprintln!("demo: spawn_window failed: {err}");
            // Without a window there is nothing to fade or move; stop the demo.
            std::process::exit(1);
        }
    }
}

fn fade(handle: &OverlayHandle, id: overlay::OverlayId, content: OverlayContent, alpha: f32) {
    if let Err(err) = handle.redraw(id, content, alpha) {
        eprintln!("demo: redraw failed: {err}");
    }
}

/// 200x120 RGBA gradient encoded as PNG in memory, fed to `OverlayContent::Image`.
fn gradient_png() -> Vec<u8> {
    let image = RgbaImage::from_fn(200, 120, |x, y| {
        image::Rgba([
            255u32.saturating_sub(x * 2) as u8,
            (128 + y).min(255) as u8,
            (60 + (x + y) % 64) as u8,
            255,
        ])
    });
    let mut bytes = Vec::new();
    match PngEncoder::new(&mut bytes).write_image(
        image.as_raw(),
        200,
        120,
        ExtendedColorType::Rgba8,
    ) {
        Ok(()) => bytes,
        Err(err) => {
            eprintln!("demo: PNG encoding failed: {err}");
            std::process::exit(1);
        }
    }
}
