//! Example: run with `xvfb-run -a ./cargo.sh run --example icons_gallery -p icons`
//!
//! On hosts with a live Wayland session, force the X11 path instead:
//! `env -u WAYLAND_DISPLAY xvfb-run -a ./cargo.sh run --example icons_gallery -p icons`
//! (a Wayland window stays hidden until the compositor maps it, so no frames
//! are painted and the self-termination never fires).
//!
//! Gallery of every embedded icon asset: one row per icon, painted at
//! [`ICON_SIZE`] next to the asset name label. egui 0.37 has no PNG loader,
//! so the bytes are decoded once via `icons::rgba` and uploaded as textures
//! on the first frame with `Context::load_texture` — the same shape
//! `crates/overlay/src/content.rs` uses. A decode failure is rendered as an
//! error label instead of panicking. Each frame schedules a repaint tick, so
//! even without input events the app keeps counting frames and closes itself
//! after [`MAX_FRAMES`] frames — headless runs exit cleanly.

use std::time::Duration;

use egui::Color32;
use egui::ColorImage;
use egui::Context;
use egui::Pos2;
use egui::Rect;
use egui::Sense;
use egui::TextureOptions;
use egui::Vec2;
use egui::ViewportBuilder;
use egui::ViewportCommand;
use egui::pos2;

/// Painted size of each icon row, in logical points.
const ICON_SIZE: f32 = 64.0;

/// Number of frames before the window closes itself (headless runs exit).
const MAX_FRAMES: usize = 10;

/// Repaint tick, so frames advance even with no input events. The interval
/// lives here and nowhere else in this example (task
/// 2026-11-13_gpu-repaint-throttle, Stage 4). Kept at 100 ms rather than
/// aligned to the shipped 200 ms throttle
/// (`crates/overlay/src/host.rs::REPAINT_INTERVAL`): the gallery repaints the
/// same static rows every frame, and the only thing a frame advances is the
/// counter that closes the window after [`MAX_FRAMES`] — the run is bounded by
/// frames, not by wall clock, so a slower tick paints the same number of
/// frames and only stretches the run from ~1 s to ~2 s. 100 ms is also
/// eframe's invisible-window floor (`run.rs:27`, applied `run.rs:376-382`), so
/// a shorter tick would be silently raised for a hidden window.
const REPAINT_EVERY: Duration = Duration::from_millis(100);

/// Asset name paired with its embedded PNG bytes.
const ICONS: &[(&str, &[u8])] = &[
    ("app", icons::APP_ICON_PNG),
    ("warning", icons::WARNING_ICON_PNG),
];

/// One gallery row: the uploaded texture, or the decode error to display.
enum Row {
    Ready {
        name: &'static str,
        texture: egui::TextureHandle,
    },
    Failed {
        name: &'static str,
        message: String,
    },
}

#[derive(Default)]
struct IconsGallery {
    rows: Vec<Row>,
    textures_loaded: bool,
    frames: usize,
}

impl IconsGallery {
    /// Decode every embedded asset once and upload its texture.
    ///
    /// A failed decode never panics: the row keeps the error message and
    /// renders it as a label.
    fn load_textures(&mut self, ctx: &Context) {
        self.rows = ICONS
            .iter()
            .map(|(name, png)| match icons::rgba(png) {
                Ok((rgba, width, height)) => {
                    let image = ColorImage::from_rgba_unmultiplied(
                        [width as usize, height as usize],
                        &rgba,
                    );
                    Row::Ready {
                        name,
                        texture: ctx.load_texture(*name, image, TextureOptions::default()),
                    }
                }
                Err(err) => Row::Failed {
                    name,
                    message: err.to_string(),
                },
            })
            .collect();
    }
}

impl eframe::App for IconsGallery {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if !self.textures_loaded {
            self.textures_loaded = true;
            self.load_textures(ui.ctx());
        }

        egui::CentralPanel::default().show(ui, |ui| {
            for row in &self.rows {
                match row {
                    Row::Ready { name, texture } => {
                        ui.horizontal(|ui| {
                            let (rect, _response) =
                                ui.allocate_exact_size(Vec2::splat(ICON_SIZE), Sense::hover());
                            let uv = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));
                            ui.painter().image(texture.id(), rect, uv, Color32::WHITE);
                            ui.label(*name);
                        });
                    }
                    Row::Failed { name, message } => {
                        ui.label(format!("{name}: {message}"));
                    }
                }
            }
        });

        self.frames += 1;
        if self.frames >= MAX_FRAMES {
            ui.ctx().send_viewport_cmd(ViewportCommand::Close);
        } else {
            ui.ctx().request_repaint_after(REPAINT_EVERY);
        }
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_title("icons gallery")
            .with_inner_size([280.0, 160.0]),
        ..Default::default()
    };
    eframe::run_native(
        "icons gallery",
        options,
        Box::new(|_cc| Ok(Box::new(IconsGallery::default()))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Upper bound for the headless gallery run: it closes itself after
    /// [`MAX_FRAMES`] ticks, so the tick may not stretch the demo past a few
    /// seconds on a host with no live display.
    const DEMO_LIFETIME: Duration = Duration::from_secs(5);

    /// The interval comes from the named const, and the literal exists only
    /// there: 100 ms, at eframe's invisible-window floor (`run.rs:27`) and so
    /// never silently raised.
    #[test]
    fn repaint_every_named_tick_pins_100ms() {
        assert_eq!(REPAINT_EVERY, Duration::from_millis(100));
    }

    /// Self-termination still lands inside the intended demo lifetime, so the
    /// frame budget stays visible and headless runs exit rather than park.
    #[test]
    fn max_frames_times_repaint_every_closes_inside_demo_lifetime() {
        assert!(REPAINT_EVERY * MAX_FRAMES as u32 <= DEMO_LIFETIME);
    }
}
