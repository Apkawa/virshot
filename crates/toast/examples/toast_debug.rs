//! Manual smoke harness for the toast crate: an eframe host that drives
//! `OverlayHost` (the host backend) and shows chips through `ToastService`.
//!
//! Run headless:
//!   `xvfb-run -a ./cargo.sh run --example toast_debug -p toast`
//!
//! It auto-shows one demo toast on startup and logs every sink call
//! (`spawn`/`redraw`/`hide`) to stderr, so a headless run proves the
//! lifecycle without any clicks.

use std::sync::Arc;

use overlay::Corner;
use overlay::MonitorRect;
use overlay::OverlayContent;
use overlay::OverlayError;
use overlay::OverlayHost;
use overlay::OverlayHostHandle;
use overlay::OverlayId;
use overlay::OverlayWindowSpec;
use overlay::Rect;

use toast::MonitorSel;
use toast::Toast;
use toast::ToastColor;
use toast::ToastHandle;
use toast::ToastService;
use toast::ToastSink;

/// Logging sink: forwards to the host backend and prints one line per call.
struct LoggingSink {
    inner: OverlayHostHandle,
    /// The configured monitor rects, for the `monitor=<index>` of the spawn
    /// line: a spec carries a placed rect and a scale, never an index.
    monitors: Vec<MonitorRect>,
}

/// Which configured monitor a placed chip sits on — the first whose PHYSICAL
/// rect fully contains the chip rect. Pure arithmetic, so it is testable
/// without winit or any real window.
fn monitor_of(monitors: &[MonitorRect], rect: Rect) -> Option<usize> {
    monitors
        .iter()
        .enumerate()
        .find(|(_, monitor)| {
            rect.x >= monitor.rect.x
                && rect.y >= monitor.rect.y
                && rect.x + rect.w <= monitor.rect.x + monitor.rect.w
                && rect.y + rect.h <= monitor.rect.y + monitor.rect.h
        })
        .map(|(index, _)| index)
}

impl ToastSink for LoggingSink {
    fn spawn_window(
        &self,
        spec: OverlayWindowSpec,
        content: OverlayContent,
        alpha: f32,
    ) -> Result<OverlayId, OverlayError> {
        let id = self.inner.spawn_window(spec, content, alpha)?;
        let monitor = match monitor_of(&self.monitors, spec.rect) {
            Some(index) => index.to_string(),
            None => "-".to_owned(),
        };
        eprintln!(
            "toast-debug: spawn id={id:?} monitor={monitor} scale={:.2} rect=({:.0},{:.0},{:.0},{:.0}) alpha={alpha:.2}",
            spec.scale, spec.rect.x, spec.rect.y, spec.rect.w, spec.rect.h
        );
        Ok(id)
    }

    fn redraw(
        &self,
        id: OverlayId,
        content: OverlayContent,
        alpha: f32,
    ) -> Result<(), OverlayError> {
        self.inner.redraw(id, content, alpha)?;
        eprintln!("toast-debug: redraw id={id:?} alpha={alpha:.2}");
        Ok(())
    }

    fn hide(&self, id: OverlayId) -> Result<(), OverlayError> {
        self.inner.hide(id)?;
        eprintln!("toast-debug: hide id={id:?}");
        Ok(())
    }
}

/// One PNG thumbnail for image toasts, encoded once at startup.
fn sample_png() -> Vec<u8> {
    let mut bytes = Vec::new();
    let image = image::RgbaImage::from_fn(96, 64, |x, y| {
        image::Rgba([
            40 + (x as u8).saturating_add(8),
            90 + (y as u8).saturating_add(16),
            200 - (x as u8) / 4,
            255,
        ])
    });
    // Encoding failure is cosmetic here: an empty buffer means "no image".
    let _ = image.write_to(
        &mut std::io::Cursor::new(&mut bytes),
        image::ImageFormat::Png,
    );
    bytes
}

/// Monitor list for the host backend, delegated to `overlay::monitors`: every
/// monitor winit reports, PHYSICAL, with the monitor's own DPI scale, and the
/// flagged PRIMARY entry first (the ordering rule lives in `overlay::monitors`).
///
/// Returns the rects *and* the startup debug line, so the caller can print the
/// same evidence the placement module formats: the line keeps winit's raw
/// enumeration order (zero-size pseudo-monitors included) and marks the
/// primary wherever it sits.
///
/// Fallback order is the one the previous inline code used: a window that
/// reports no usable monitor geometry (no monitors, or only zero-size ones)
/// falls back to the window's own physical inner size at the window's scale
/// (`monitors::fallback_rect`); a missing winit window falls back to the egui
/// root-viewport size in POINTS scaled by `pixels_per_point()`
/// (`monitors::fallback_rect_from_points`, which floors `ppp` to `1.0` and
/// clamps each dimension to ≥1 px — the same arithmetic as the old block).
fn host_monitors(cc: &eframe::CreationContext<'_>) -> (Vec<MonitorRect>, String) {
    if let Some(window) = cc.winit_window() {
        let entries = overlay::monitors::from_winit_window(window);
        let debug = overlay::monitors::debug_line(Some(&entries));
        let rects = overlay::monitors::rects(&entries);
        if !rects.is_empty() {
            return (rects, debug);
        }
        // No monitor info on this platform: the window's own physical size is
        // the only rect left, scaled like the window itself.
        return (vec![overlay::monitors::fallback_rect(window)], debug);
    }
    // No winit window at all: egui reports points, so scale back to pixels and
    // leave the monitor scale unknown.
    let size = cc
        .egui_ctx
        .input(|i| {
            let viewport = i.viewport();
            viewport
                .monitor_size
                .or(viewport.inner_rect.map(|rect| rect.size()))
        })
        .unwrap_or(egui::Vec2::new(800.0, 600.0));
    (
        vec![overlay::monitors::fallback_rect_from_points(
            (size.x, size.y),
            cc.egui_ctx.pixels_per_point(),
        )],
        overlay::monitors::debug_line(None),
    )
}

struct ToastDebugApp {
    ctx: egui::Context,
    host: OverlayHost,
    /// Keeps the toast timer thread alive for the whole session; dropping the
    /// app hides any live chip (see `ToastService`'s `Drop`).
    _service: ToastService,
    handle: ToastHandle,
    monitors: Vec<MonitorRect>,
    text: String,
    color: ToastColor,
    corner: Corner,
    duration_seconds: f32,
    on_all_monitors: bool,
    with_image: bool,
    sample_png: Vec<u8>,
    demo_shown: bool,
}

impl ToastDebugApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = cc.egui_ctx.clone();
        // Cosmetic: eframe's default dark visuals make this debug window hard to
        // read; light visuals + the opaque light panel fill in `ui` keep the
        // controls legible. Chips keep their own fill (`overlay::content`).
        ctx.set_visuals(egui::Visuals::light());
        let host = OverlayHost::new();
        // Placement diagnostics on stderr (`overlay: …` lines): this harness is
        // the Windows rig's evidence for separating a placement defect (the
        // window landed elsewhere) from a presentation defect (it landed but
        // shows nothing).
        host.set_debug(true);
        let (rects, monitor_debug) = host_monitors(cc);
        // Startup placement evidence on stderr, under the same label the panel
        // shows below: the debug line names every monitor winit enumerated and
        // marks the PRIMARY one, which is what `MonitorSel::Index(0)` resolves
        // to after `overlay::monitors::rects` reorders the list.
        eprintln!("toast-debug: monitors (physical): {monitor_debug}");
        host.set_monitors(rects);
        // One list, three owners, each needing its own copy: the sink (names the
        // monitor in its spawn line), the service (placement targets), and the
        // app (the window label).
        let monitors = host.monitors();
        let service = ToastService::spawn(
            Arc::new(LoggingSink {
                inner: host.handle(),
                monitors: monitors.clone(),
            }) as Arc<dyn ToastSink>,
            monitors.clone(),
            Some(ctx.clone()),
        );
        let handle = service.handle();
        Self {
            ctx,
            host,
            _service: service,
            handle,
            monitors,
            text: "Saved: shot-2.png".to_owned(),
            color: ToastColor::Success,
            corner: Corner::BottomRight,
            duration_seconds: 2.0,
            on_all_monitors: true,
            with_image: false,
            sample_png: sample_png(),
            demo_shown: false,
        }
    }

    fn toast(&self) -> Toast {
        Toast {
            monitor: if self.on_all_monitors {
                MonitorSel::All
            } else {
                // Index 0 is the PRIMARY monitor: `overlay::monitors::rects`
                // moves the flagged entry to the front, so an unchecked box
                // targets the primary rather than whichever monitor winit
                // happened to enumerate first (Windows gives no order
                // guarantee).
                MonitorSel::Index(0)
            },
            text: self.text.clone(),
            image: self.with_image.then(|| self.sample_png.clone()),
            color: self.color,
            corner: self.corner,
            duration_ms: (self.duration_seconds * 1000.0).round() as u32,
        }
    }
}

impl eframe::App for ToastDebugApp {
    /// Alpha-0 clear, mirroring `src/gui.rs:1053-1059`: eframe's glow backend
    /// uses ONE app-level clear color for ALL viewports — including the toast
    /// child viewports the host creates — so alpha-0 is what lets transparent
    /// windows show through. The debug panel paints its own opaque fill below.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Color32::TRANSPARENT.to_normalized_gamma_f32()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Headless evidence: one auto toast on the first frame, no clicks needed.
        if !self.demo_shown {
            self.demo_shown = true;
            if let Err(error) = self.handle.show(self.toast()) {
                eprintln!("toast-debug: auto show failed: {error}");
            }
        }

        // `CentralPanel::default()` paints an opaque panel fill (light gray with
        // the light visuals set in `new`), so the controls stay readable over an
        // alpha-0 clear; chips are separate windows with their own fill.
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("toast debug");
            ui.label(format!("monitors (physical): {:?}", self.monitors));

            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut self.text);
                ui.checkbox(&mut self.with_image, "image chip");
            });

            ui.horizontal(|ui| {
                ui.radio_value(&mut self.color, ToastColor::Default, "default");
                ui.radio_value(&mut self.color, ToastColor::Success, "success");
                ui.radio_value(&mut self.color, ToastColor::Danger, "danger");
                ui.radio_value(&mut self.color, ToastColor::Warning, "warning");
            });

            ui.horizontal(|ui| {
                ui.radio_value(&mut self.corner, Corner::TopLeft, "top-left");
                ui.radio_value(&mut self.corner, Corner::TopRight, "top-right");
                ui.radio_value(&mut self.corner, Corner::BottomLeft, "bottom-left");
                ui.radio_value(&mut self.corner, Corner::BottomRight, "bottom-right");
            });

            ui.horizontal(|ui| {
                ui.label("hold (s)");
                ui.add(egui::DragValue::new(&mut self.duration_seconds).speed(0.1));
                ui.label("0 = sticky");
            });
            ui.checkbox(&mut self.on_all_monitors, "all monitors");

            ui.horizontal(|ui| {
                if ui.button("Show").clicked()
                    && let Err(error) = self.handle.show(self.toast())
                {
                    eprintln!("toast-debug: show failed: {error}");
                }
                if ui.button("Update (replace)").clicked()
                    && let Err(error) = self.handle.update(self.toast())
                {
                    eprintln!("toast-debug: update failed: {error}");
                }
                if ui.button("Hide").clicked()
                    && let Err(error) = self.handle.dismiss()
                {
                    eprintln!("toast-debug: dismiss failed: {error}");
                }
            });
        });

        // The toast thread never touches the event loop; the host paints the
        // queued window commands here, on the eframe frame pass.
        self.host.update(&self.ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The startup evidence line `host_monitors` prints: pure formatting over
    // raw tuples — no winit handles needed. The flag column proves stderr
    // names the primary wherever winit enumerated it (the `rects` reorder
    // makes that entry `MonitorSel::Index(0)`).
    #[test]
    fn debug_line_marks_the_primary_entry_wherever_it_sits() {
        let built = overlay::monitors::entries(vec![
            (-1920, 0, 1280, 1024, 1.25, None, false),
            (
                0,
                0,
                1920,
                1080,
                1.0,
                Some(r"\\.\DISPLAY1".to_owned()),
                true,
            ),
        ]);
        assert_eq!(
            overlay::monitors::debug_line(Some(&built)),
            r"#0 unnamed pos=(-1920,0) size=1280x1024 scale=1.25 | #1 \\.\DISPLAY1 pos=(0,0) size=1920x1080 scale=1.00 primary",
            "the primary flag must survive the enumeration order"
        );
    }

    // Which monitor a spawn line names: containment of the placed chip rect.
    #[test]
    fn monitor_of_names_the_monitor_the_placed_chip_lands_on() {
        let monitors = vec![
            MonitorRect::new(Rect::new(0.0, 0.0, 1920.0, 1080.0), 1.5),
            MonitorRect::new(Rect::new(1920.0, 0.0, 1280.0, 1024.0), 1.0),
        ];
        assert_eq!(
            monitor_of(&monitors, Rect::new(10.0, 1026.0, 330.0, 54.0)),
            Some(0),
            "a chip inside the primary monitor is monitor 0"
        );
        assert_eq!(
            monitor_of(&monitors, Rect::new(2870.0, 970.0, 330.0, 54.0)),
            Some(1),
            "a chip inside the secondary monitor is monitor 1"
        );
    }

    #[test]
    fn monitor_of_names_nothing_for_a_rect_outside_every_monitor() {
        let monitors = vec![MonitorRect::new(Rect::new(0.0, 0.0, 1920.0, 1080.0), 1.0)];
        assert_eq!(
            monitor_of(&monitors, Rect::new(1900.0, 0.0, 330.0, 54.0)),
            None,
            "a chip that spills past the monitor edge names no monitor"
        );
        assert_eq!(monitor_of(&[], Rect::new(0.0, 0.0, 330.0, 54.0)), None);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Mirrors virshot's root viewport (`src/gui.rs:1166-1172`): eframe derives
    // the alpha-capable GL config from the ROOT viewport builder, and the toast
    // child viewports inherit transparency from it.
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_transparent(true),
        ..Default::default()
    };
    eframe::run_native(
        "toast debug",
        options,
        Box::new(|cc| Ok(Box::new(ToastDebugApp::new(cc)))),
    )
    .map_err(|error| format!("eframe failed: {error}"))?;
    // Keep the service alive until the loop is done so chips are hidden on drop.
    Ok(())
}
