//! Window bootstrap: eframe startup, window icon, monitor rects.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;

use overlay::OverlayHost;
use toast::ToastService;

use crate::capture;
use crate::capture::ToastHandoff;
use crate::config::Settings;
use crate::fonts;
use crate::gamepad::PadCaptureMailbox;
use crate::hotkeys::HotkeysController;
use crate::hotkeys::Registration;
use crate::tray::TrayController;
use crate::update;

use super::SettingsApp;
use super::icons::warning_icon_texture;

/// Initial window size per spec §4.4 (~720×520; position/size not persisted).
const INITIAL_WINDOW_WIDTH: f32 = 720.0;
const INITIAL_WINDOW_HEIGHT: f32 = 520.0;

/// GUI failures: window/graphics setup only. Tab logic itself cannot fail —
/// Apply failures are logged and the app keeps running (spec §5).
#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("settings window could not start: {message}")]
    Gui { message: String },
}

/// Monitor list for the toast stack plus the startup placement-evidence line,
/// from ONE enumeration pass: `overlay::monitors` (crates/overlay) is the
/// single source of truth for monitor geometry — enumeration, the primary
/// flag, the zero-size skip, the primary-first ordering, and the debug-line
/// wording all live there; this adapter only wires them and logs. Rects are
/// PHYSICAL desktop pixels, each carrying its own DPI scale (spec §4.3):
/// `OverlayHost` divides a rect by the CHILD window's pixels-per-point
/// (`crates/overlay/src/host.rs`), never by the root's. The PRIMARY monitor
/// comes FIRST — toast `MonitorSel::Index(0)` means "the primary monitor"
/// (crates/toast); `overlay::monitors` flags the OS primary monitor, not the
/// monitor the settings window happens to sit on. A monitor with no usable
/// size is skipped (WARN with a count); when winit reports no usable rects
/// the toast falls back to the host window's own physical rect, and with no
/// winit window at all to the root viewport's size scaled back from points —
/// the app stays alive either way. Called on the MAIN thread at startup,
/// never in the capture hot path; winit is reached only through
/// `cc.winit_window()` (`virshot` has no winit dependency).
fn host_monitor_rects(cc: &eframe::CreationContext<'_>) -> (Vec<overlay::MonitorRect>, String) {
    if let Some(window) = cc.winit_window() {
        let entries = overlay::monitors::from_winit_window(window);
        // The debug line keeps winit's raw enumeration order — zero-size
        // pseudo-monitors included — because the startup log is evidence of
        // what the OS reported, not of what placement used.
        let debug = overlay::monitors::debug_line(Some(&entries));
        let rects = overlay::monitors::rects(&entries);
        // `rects` skips zero-size entries without logging; count the difference
        // here so pseudo-monitors still show up in the log.
        if entries.len() > rects.len() {
            tracing::warn!(
                "Monitor geometry unavailable; skipped {} zero-size monitor(s) for toast placement",
                entries.len() - rects.len()
            );
        }
        if !rects.is_empty() {
            return (rects, debug);
        }
        // The window exists but reported no usable geometry: its own physical
        // inner size at its own scale is the only rect such a platform offers.
        tracing::warn!("Monitor enumeration failed; toast falls back to the window's own rect");
        return (vec![overlay::monitors::fallback_rect(window)], debug);
    }
    // No winit window at all: egui reports POINTS, so scale back to physical
    // pixels; `fallback_rect_from_points` floors `ppp` to 1.0 and clamps each
    // dimension to ≥1 px — the same arithmetic as the old inline fallback.
    tracing::warn!("No winit window; toast falls back to the root viewport's monitor size");
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

/// Window-icon source for `ViewportBuilder::with_icon`: decode the embedded
/// app-icon asset (`icons::APP_ICON_PNG`) into `egui::IconData`. The icon is
/// attached at WINDOW CREATION because Windows captures `ICON_BIG` there
/// (eframe's `AppTitleIconSetter`); a runtime `set_icon` alone is not enough.
///
/// Failure mode: [`icons::IconsError::IconDecodeError`] on a corrupt asset —
/// logged at WARN and the window opens iconless (errors are values; the app
/// stays alive).
fn app_window_icon() -> Option<Arc<egui::IconData>> {
    match icons::icon_data(icons::APP_ICON_PNG) {
        Ok(data) => Some(Arc::new(data)),
        Err(err) => {
            tracing::warn!("App window icon decode failed: {err}; window opens without an icon");
            None
        }
    }
}

/// Open the settings window on the MAIN thread (the hotkey manager lives
/// there; winit/eframe pumps Win32 messages — Windows WM_HOTKEY affinity;
/// tray event handling here satisfies the main-thread affinity every backend
/// expects). `handoff` is the latest-only toast-handle slot shared with the
/// hotkey listener and the pad pump: the `create` closure below builds the
/// toast stack (`overlay::OverlayHost` + `toast::ToastService`) and attaches
/// the service handle to the slot, so every trigger can attach a chip to its
/// capture (spec §4.3). `tray` is the tray controller (spec §4.5) or `None`
/// after a logged tray failure; `settings_mailbox` is the settings mailbox
/// `main` created for the hotkey listener and the pad pump (Apply publishes
/// it), and `pad_capture` is the pad-capture mailbox shared with the pump
/// (§4.4, Stage 5). `update_tx`/`update_rx` are the check channel `main.rs`
/// created (Stage 7 ownership — the GUI does not create one): the Receiver is
/// drained ONLY in `App::logic`, and the Sender is what scheduled check
/// threads send on. `restart_requested` is the shared install→restart flag:
/// `App::logic` sets it and closes the viewport when an install succeeds, so
/// this call returns and `main.rs` restarts the new binary (spec §5).
/// Returns only if graphics setup failed; the caller falls
/// back to the idle loop so the process stays resident (spec §5). The toast
/// stack is owned by the app, so it lives for the whole loop and drops with it.
#[allow(clippy::too_many_arguments)] // Startup wiring: one call site in `main`, each shared
// mailbox named explicitly (same shape as `SettingsApp::new`).
pub(crate) fn run(
    config_path: PathBuf,
    settings: Settings,
    controller: Option<HotkeysController>,
    status: Vec<Registration>,
    handoff: ToastHandoff,
    tray: Option<TrayController>,
    settings_mailbox: Arc<Mutex<Settings>>,
    pad_capture: PadCaptureMailbox,
    update_tx: mpsc::Sender<update::CheckOutcome>,
    update_rx: mpsc::Receiver<update::CheckOutcome>,
    restart_requested: Arc<AtomicBool>,
) -> Result<(), Error> {
    let viewport = egui::ViewportBuilder::default()
        .with_title(rust_i18n::t!("bottom.window_title"))
        .with_inner_size([INITIAL_WINDOW_WIDTH, INITIAL_WINDOW_HEIGHT])
        // eframe derives the alpha-capable GL config from the ROOT viewport
        // builder; the toast child viewports need it to be transparent.
        .with_transparent(true)
        // The settings window is a dialog, not a workspace: no minimize, no
        // maximize (spec §4.4). The close button and resize stay enabled — the
        // X button hides to the tray (§4.5), the bottom buttons close for real.
        // Honesty limit: egui-winit maps these flags onto winit's
        // `WindowButtons`, and only the Windows backend acts on them
        // (`winit-0.30.13/platform_impl/windows/window.rs:305-317` →
        // `WindowFlags::MINIMIZABLE`/`MAXIMIZABLE`); X11/Wayland do not
        // implement it at all (`winit-0.30.13/window.rs:1045`), so the chrome
        // is accepted on Windows — not on this Linux host or under Xvfb.
        .with_minimize_button(false)
        .with_maximize_button(false);
    // The window icon must be attached HERE (creation): Windows captures
    // ICON_BIG at that point (eframe's AppTitleIconSetter). A decode failure
    // is logged and the window opens iconless.
    let viewport = match app_window_icon() {
        Some(icon) => viewport.with_icon(icon),
        None => viewport,
    };
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "virshot",
        options,
        Box::new(move |cc| {
            // Font fallback for the localized UI (spec §2): the bundled faces cover
            // Latin + Cyrillic only, so zh-CN/ja/ko glyphs would render as tofu.
            // Installed BEFORE the first frame and shared by every viewport of this
            // context — settings window, toast chips, overlay all localize.
            fonts::install(&cc.egui_ctx);
            // Toast stack (spec §4.3): the host backend holds the monitor
            // geometry, the timer thread owns chip lifetimes, and the wake
            // context repaints this loop on fade ticks.
            // One enumeration feeds both the toast rects and the rig line.
            let (monitors, placement_line) = host_monitor_rects(cc);
            // What the OS actually reported, in the same physical numbers the
            // toast places chips in: the log is the Windows rig's evidence for
            // telling a placement defect from a presentation one.
            tracing::info!("Toast placement monitors: {placement_line}");
            let host = OverlayHost::new();
            host.set_monitors(monitors.clone());
            // Placement diagnostics on stderr (`overlay: spawn/correction/drift`
            // lines): chips are rare — one per shot success/fail — so the
            // Windows rig gets placement evidence for free.
            host.set_debug(true);
            let service =
                ToastService::spawn(Arc::new(host.handle()), monitors, Some(cc.egui_ctx.clone()));
            capture::attach_handoff(&handoff, service.handle());
            tracing::info!(
                "Toast service started on {} monitor(s)",
                host.monitors().len()
            );
            // Warning status icon: decode + upload the embedded asset ONCE
            // here (window creation), not per frame — egui 0.37 has no PNG
            // loader, so the bytes go through `icons::rgba` and
            // `Context::load_texture`. A decode failure is logged once and
            // the rows keep the ASCII `!` fallback.
            let mut app = SettingsApp::new(
                config_path,
                settings,
                controller,
                status,
                service,
                host,
                handoff,
                tray,
                settings_mailbox,
                pad_capture,
                // The check channel `main.rs` created (Stage 7): drained ONLY
                // in `App::logic`; the sender drives the interval checks that
                // pass schedules (spec §4.7).
                update_rx,
                update_tx,
                restart_requested,
            );
            app.warning_icon = warning_icon_texture(&cc.egui_ctx);
            // Bind-probe wake (Stage 14): the listener thread records a probe
            // press on its own thread, so this context is what makes the logic
            // pass run right away instead of waiting for the tray tick. Attached
            // ONCE here at window creation — exactly one attach per process, and
            // the controller outlives the window (hidden-to-tray keeps the loop
            // running, §4.5). A controller-less host has nothing to attach to.
            if let Some(controller) = &mut app.controller {
                controller.attach_probe_wake(cc.egui_ctx.clone());
            }
            Ok(Box::new(app))
        }),
    )
    .map_err(|err| Error::Gui {
        message: err.to_string(),
    })
}

#[cfg(test)]
#[path = "window.tests.rs"]
mod tests;
