//! Settings window GUI (spec §4.4): the eframe/egui app on the MAIN thread —
//! three tabs (General / Hotkeys / Overlay), an in-app draft of `Settings`
//! (close without Apply discards edits), Apply → `config::save` + live hotkey
//! re-registration via `hotkeys::HotkeysController`, and bind-by-press key
//! capture. UI text is English (spec §2 i18n note).
//!
//! eframe 0.37 API facts verified in the locked source, not from memory:
//! - `App` trait: `logic(&mut self, ctx, frame)` + `ui(&mut self, ui, frame)`
//!   (no `bool` return; exit is controlled by viewport close commands).
//! - Window X click: winit `CloseRequested` → egui `RequestClose`; eframe
//!   shuts down UNLESS the app emits `ViewportCommand::CancelClose` in the
//!   frame that observed `close_requested()` (eframe `epi_integration` +
//!   `glow_integration`). We cancel the close and send `Visible(false)`:
//!   the window hides, the event loop keeps running, the process stays
//!   resident. Tray (Stage 10, spec §4.5): `logic` polls
//!   `tray::TrayController::poll()` every frame — tray events do not wake
//!   the winit loop, so `request_repaint_after(TRAY_POLL_TICK)` keeps the
//!   polling alive while the window is hidden (logic-only pass, negligible
//!   CPU). While hidden, eframe runs `logic` only, no UI pass.
//! - Exit is tray-menu-only (eframe 0.37 has NO `App::should_exit` —
//!   verified in the locked source): tray `Exit` sets `exit_requested` and
//!   sends `ViewportCommand::Close`; the pass that sees `close_requested()`
//!   does NOT cancel, eframe shuts down, `run_native` returns, the process
//!   exits 0. X-close still hides only.
//! - `NativeOptions::viewport` (`ViewportBuilder`): `with_title`,
//!   `with_inner_size` (720×520 per §4.4); decorations/resizable are the OS
//!   defaults (bordered, resizable window).
//! - Widgets: `ComboBox::from_label/from_id_salt(...).selected_text(...).show_ui`,
//!   `ui.selectable_value`, `ui.checkbox`, `ui.text_edit_singleline`,
//!   `DragValue::range`, `ui.add_enabled(false, ...)` for disabled rows.
//! - General tab Browse button (spec §4.4, Stage 14): the `rfd` blocking
//!   folder picker (`FileDialog::new().set_directory(..).pick_folder()`)
//!   started from `browse_start_dir`; cancel or a missing backend is `None` →
//!   WARN log, draft unchanged, app stays alive (see `browse_save_path`).
//! - Hotkeys rows (spec §4.4 v5.0): the bind field is a `Button` whose label is
//!   the current binding (`binding_label`), and registration status is an icon
//!   with a tooltip instead of inline text. Verified in the locked egui 0.37
//!   source: `Button::new(atoms: impl IntoAtoms)` accepts any `Into<WidgetText>`
//!   (`&str`, `String`, `RichText`) and sizes via `min_size(Vec2)` (there is no
//!   `min_width`); `Response::on_hover_text(self, text) -> Self` consumes the
//!   response; and its tooltip is NOT shown on a disabled widget — the disabled
//!   rect row needs `Response::on_disabled_hover_text` instead.
//! - Icon glyphs: with our eframe features (`default_fonts`, no
//!   `monochrome_emoji_fonts`) the bundled fonts are Ubuntu-Light + Hack +
//!   egui-icons (5 logo glyphs only), and none of them contains `⚠`/`ℹ`
//!   (checked the cmaps), so status icons are colored ASCII `!`/`i` via
//!   `RichText::color`.
//! - Bind-by-press reads `ui.input(|i| i.events())` `Event::Key` presses
//!   (spec §4.4: click field → capture mode); Escape cancels capture mode;
//!   unsupported names are ignored. While capturing, live hotkey registrations
//!   are paused via `hotkeys::HotkeysController::pause`/`restore` (Stage 18,
//!   spec §4.4 v5.5) so a press of a live combo reaches this window instead of
//!   firing the screenshot action; restore re-registers the APPLIED settings.
//! - Pad-field bind-by-press (spec §4.1 + §4.4, Stage 5): clicking the pad field
//!   starts capture for that row; the pump suppresses pad shots while ANY field
//!   is capturing and commits the FIRST DOWN event into `gamepad::PadCapture`,
//!   so `logic` drains the mailbox into the draft notation (`unknown:<N>`,
//!   `Guide`, `Share`) — the GUI never receives a callback, only the mailbox.
//!   The device-info label hint of the captured press (spec §9 — the GameInput
//!   header has no `GetButtonLabel` slot) is kept per row
//!   in memory and shown beside the field; on a host without GameInput the
//!   field renders as a disabled placeholder (Linux gilrs is out of scope).
//!
//! Overlay toast viewport (spec §4.3, Stage 9): the `overlay::ToastState`
//! lifecycle is driven from `logic` — so toasts appear even while the settings
//! window is hidden — and painted into a second deferred viewport: transparent,
//! undecorated, always-on-top, mouse-passthrough, faded via
//! `Color32::gamma_multiply`. Verified against the locked egui/eframe/
//! egui-winit/winit 0.37 sources, not from memory:
//! - `Context::show_viewport_deferred(id, builder, cb)` must be called on every
//!   pass while the toast exists; builder diffs become viewport commands
//!   (`OuterPosition`, `InnerSize`, `Decorations`, `MousePassthrough`,
//!   `WindowLevel`) via `ViewportBuilder::patch`.
//! - `ViewportBuilder::with_always_on_top()` (no argument) maps to winit
//!   `set_window_level(AlwaysOnTop)`; `with_mouse_passthrough(true)` maps to
//!   `set_cursor_hittest(false)`.
//! - Transparency: eframe requests an alpha-capable GL config from the ROOT
//!   viewport builder (`native_options.viewport.transparent`) and shares one
//!   clear color across viewports, so `App::clear_color` returns alpha 0 and the
//!   settings window keeps its opaque `CentralPanel` paint.
//! - Platform limits: Wayland ignores `set_outer_position` (the compositor
//!   places the window) and `set_window_level` (no always-on-top); passthrough
//!   works there via an empty input region; X11 transparency is set at window
//!   build time.
//! - egui 0.37 exposes no monitor enumeration to `App` — only
//!   `ViewportInfo::monitor_size` for the CURRENT monitor (`None` on Wayland),
//!   so per-monitor placement (spec §4.3 v5.5, `overlay.on_all_monitors`) uses
//!   `xcap::Monitor::all()` on the MAIN thread at toast time — never in the
//!   capture hot path. `xcap` reports PHYSICAL pixels; the toast builder takes
//!   LOGICAL points (egui-winit multiplies builder position/size by
//!   `pixels_per_point`), so physical ÷ `scale_factor` feeds `toast_positions`.
//!   A monitor whose accessors fail is skipped; a total failure or an empty
//!   list falls back to the egui current-monitor size, then to compositor
//!   placement (WARN, app stays alive).
//! - Toast visibility while the settings window is hidden: a deferred viewport
//!   needs a FULL pass to be created (`ViewportOutput` comes from
//!   `Context::end_pass`; `run_logic` returns none), so a toast started while
//!   the window was hidden never appeared. Sending
//!   `ViewportCommand::RequestPaintWhileHidden` to the root viewport while a
//!   toast is alive makes eframe run full passes for the hidden window
//!   (`egui-winit::ActionRequested::PaintWhileHidden` → `wants_paint()`,
//!   `glow_integration` `is_visible`) — that creates the toast windows and
//!   drives the fade.
//! - Toast visibility on the FIRST frame (spec §4.3 v6.0, Stage 21): the chip
//!   paints at full opacity from its first frame (`overlay::ToastState::alpha`
//!   fades out only at the end — a fade-in ramp multiplies the alpha channel to
//!   0 via `Color32::gamma_multiply` and leaves the window empty), and a repaint
//!   is requested for a toast viewport only once its `ViewportInfo` is in the
//!   pass input (`viewport_has_window`): the window is created at the END of the
//!   spawn pass, so a request from that pass finds no window
//!   (`window_id_from_viewport_id` → `EventResult::Wait`, `run.rs`) AND pins the
//!   viewport's `repaint_delay` to `ZERO`, which deduplicates every later
//!   request (`egui/src/context.rs`) — the window then never gets
//!   `RedrawRequested` and Windows shows its unpainted white background.
//! - One toast window, no white "shadow" (spec §4.3 v6.1, Stage 21): there is
//!   only ever ONE window per monitor, and it is never shown unpainted.
//!   Replacement defers the spawn to the NEXT pass (`toast_window_step`) because
//!   `handle_viewport_output` creates new windows before `remove_viewports_not_in`
//!   destroys the closed ones, so a same-pass spawn showed old + new together.
//!   egui 0.37 has no per-viewport background color (`ViewportBuilder` has no
//!   inner-color field, `ViewportCommand` has no fill command, so
//!   `with_inner_color` cannot be used), so instead the toast window is created
//!   with `with_visible(false)` and painted while hidden —
//!   `RequestPaintWhileHidden` → `ActionRequested::PaintWhileHidden` →
//!   `wants_paint()` makes `glow_integration` paint it and run its callback —
//!   and is shown with `ViewportCommand::Visible(true)` only after that callback
//!   ran. `with_active(false)` keeps the show non-activating
//!   (`SW_SHOWNOACTIVATE` in winit), and `with_always_on_top()` maps to winit
//!   `WindowAttributes::with_window_level(AlwaysOnTop)` → `WS_EX_TOPMOST`
//!   (`egui-winit create_winit_window_attributes`, `winit window_state.rs`).

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::bindings::Action;
use crate::capture;
use crate::config;
use crate::config::Binding;
use crate::config::Format;
use crate::config::OverlayPosition;
use crate::config::Settings;
use crate::gamepad::CaptureField;
use crate::gamepad::PadCaptureMailbox;
use crate::gamepad::PadDownEvent;
use crate::gamepad::active_capture;
use crate::gamepad::pad_button_label;
use crate::gamepad::pad_notation;
use crate::gamepad::set_capture_mode;
use crate::gamepad::take_captured;
use crate::hotkeys::HotkeysController;
use crate::hotkeys::Registration;
use crate::overlay::ToastMailbox;
use crate::overlay::ToastState;
use crate::overlay::should_show;
use crate::overlay::take_latest;
use crate::tray::TrayAction;
use crate::tray::TrayController;

/// Initial window size per spec §4.4 (~720×520; position/size not persisted).
pub(crate) const INITIAL_WINDOW_WIDTH: f32 = 720.0;
pub(crate) const INITIAL_WINDOW_HEIGHT: f32 = 520.0;

/// Tray poll cadence: tray events do not wake the winit event loop, so the
/// logic pass schedules a repaint to drain them promptly even while the
/// window is hidden. 500 ms of logic-only ticks keeps idle CPU far under the
/// §5 budget.
const TRAY_POLL_TICK: Duration = Duration::from_millis(500);

/// Toast chip metrics (spec §4.3): single line, fixed inset from the screen
/// corner. Width grows with the message so long error texts still fit.
const TOAST_HEIGHT: f32 = 36.0;
const TOAST_MIN_WIDTH: f32 = 220.0;
const TOAST_MAX_WIDTH: f32 = 460.0;
/// Empirical corner offsets from the tk reference (`hua_pichot.py`, spec §4.3
/// v6.2): 30 px from the vertical border, 60 px from the horizontal border.
/// The bottom corners clear the Windows taskbar (~48 px) without OS work-area
/// APIs — `windows-sys` `rcWork` was rejected (unsafe FFI vs
/// `forbid(unsafe_code)`). Logical px, same on all four corners.
const TOAST_OFFSET_X: f32 = 30.0;
const TOAST_OFFSET_Y: f32 = 60.0;
const TOAST_CHAR_WIDTH: f32 = 9.0;
const TOAST_PADDING: f32 = 26.0;

/// Toast chip colors (English text on a dark chip; faded via `gamma_multiply`).
const TOAST_FILL: egui::Color32 = egui::Color32::from_rgb(24, 24, 24);
const TOAST_TEXT: egui::Color32 = egui::Color32::from_rgb(230, 230, 230);

/// Toast viewport id: unique per replacement generation AND per monitor, so a
/// replacement never reuses a live window (spec §4.3 — one toast replaces the
/// current one, no stacking).
fn toast_viewport_id(generation: u64, index: usize) -> egui::ViewportId {
    egui::ViewportId::from_hash_of(format!("virshot-toast-{generation}-{index}"))
}

/// One live toast window: its viewport id, where it is placed, and whether it
/// has been painted at least once. `None` position = let the compositor place
/// it (Wayland ignores positions anyway). `painted` is set by the window's own
/// paint callback, so a window is shown only once it has real pixels (see
/// `SettingsApp::update_toast`).
#[derive(Debug, Clone)]
struct ToastViewport {
    id: egui::ViewportId,
    position: Option<egui::Pos2>,
    painted: Arc<AtomicBool>,
}

/// Whether the backend already has a native window for `id`: eframe reports
/// every live viewport as a `ViewportInfo` in the pass input
/// (`glow_integration` fills `raw_input.viewports` from its viewport map), so
/// an entry here means the window exists.
///
/// Repaint bootstrap (spec §4.3 v6.0): a deferred viewport's window is created
/// at the END of the pass that delivers its builder, while egui fires the
/// repaint callback inside that same pass. A request from the spawn pass
/// therefore finds no window (`window_id_from_viewport_id` → `None` →
/// `EventResult::Wait` in `eframe/src/native/run.rs`) and is dropped — and it
/// also pins that viewport's `repaint_delay` to `ZERO`, so every later request
/// is deduplicated away (`egui/src/context.rs`: the callback fires only when
/// `delay < repaint_delay`). The toast window then never receives
/// `RedrawRequested` and Windows composites its unpainted white background:
/// the empty rounded "DWM shadow" strip. Requesting repaint only once the
/// viewport is in the input lands the first request on a real window.
fn viewport_has_window(raw: &egui::RawInput, id: egui::ViewportId) -> bool {
    raw.viewports.contains_key(&id)
}

/// Toast width from the message text: proportional to the character count,
/// clamped so a toast never spans the screen.
pub(crate) fn toast_width(text: &str) -> f32 {
    (text.chars().count() as f32 * TOAST_CHAR_WIDTH + TOAST_PADDING)
        .clamp(TOAST_MIN_WIDTH, TOAST_MAX_WIDTH)
}

/// Toast window size: chip width from the text, fixed chip height.
fn toast_window_size(text: &str) -> egui::Vec2 {
    egui::Vec2::new(toast_width(text), TOAST_HEIGHT)
}

/// Top-left corner of the toast for a screen-corner placement (spec §4.3 v6.2:
/// four corners with the empirical 30/60 offsets). `None` when the monitor
/// size is unknown — on Wayland egui reports no monitor size and the
/// compositor places the window.
pub(crate) fn toast_position(
    monitor_size: Option<egui::Vec2>,
    position: OverlayPosition,
    size: egui::Vec2,
) -> Option<egui::Pos2> {
    let monitor = monitor_size?;
    let left = match position {
        OverlayPosition::TopLeft | OverlayPosition::BottomLeft => TOAST_OFFSET_X,
        OverlayPosition::TopRight | OverlayPosition::BottomRight => {
            monitor.x - size.x - TOAST_OFFSET_X
        }
    };
    let top = match position {
        OverlayPosition::TopLeft | OverlayPosition::TopRight => TOAST_OFFSET_Y,
        OverlayPosition::BottomLeft | OverlayPosition::BottomRight => {
            monitor.y - size.y - TOAST_OFFSET_Y
        }
    };
    Some(egui::Pos2::new(left.max(0.0), top.max(0.0)))
}

/// One monitor, in LOGICAL points, for toast placement (spec §4.3 v5.5).
/// `xcap` reports PHYSICAL pixels; egui viewport positions/sizes are logical
/// points that egui-winit scales by the window's pixels-per-point
/// (`apply_viewport_builder_to_window`), so physical ÷ scale factor is what the
/// builder expects. A local struct (not `xcap::Monitor`) because the helper is
/// pure and unit-testable — `xcap::Monitor` has no public constructor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MonitorRect {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) primary: bool,
}

/// Toast placements (spec §4.3 v5.5): `on_all_monitors` → one placement per
/// monitor at the same corner; otherwise ONE placement on the primary monitor
/// (or the first when no monitor is marked primary). Empty input → empty result
/// (caller falls back to a compositor-placed toast).
pub(crate) fn toast_positions(
    monitors: &[MonitorRect],
    corner: OverlayPosition,
    size: egui::Vec2,
    offset_x: f32,
    offset_y: f32,
    on_all_monitors: bool,
) -> Vec<egui::Pos2> {
    let chosen: Vec<MonitorRect> = if on_all_monitors {
        monitors.to_vec()
    } else if let Some(primary) = monitors.iter().find(|monitor| monitor.primary) {
        vec![*primary]
    } else {
        monitors.iter().take(1).copied().collect()
    };

    chosen
        .iter()
        .map(|monitor| corner_position(*monitor, corner, size, offset_x, offset_y))
        .collect()
}

/// Top-left of a toast anchored at `corner` of `monitor`: inset by `offset_x`
/// from the monitor's vertical border and `offset_y` from its horizontal
/// border. A monitor smaller than the toast keeps the toast at the monitor
/// origin (never pushed further off-monitor by a negative offset); a monitor
/// with negative desktop coordinates keeps its own negative placement.
fn corner_position(
    monitor: MonitorRect,
    corner: OverlayPosition,
    size: egui::Vec2,
    offset_x: f32,
    offset_y: f32,
) -> egui::Pos2 {
    let left = match corner {
        OverlayPosition::TopLeft | OverlayPosition::BottomLeft => monitor.x + offset_x,
        OverlayPosition::TopRight | OverlayPosition::BottomRight => {
            monitor.x + monitor.width - size.x - offset_x
        }
    };
    let top = match corner {
        OverlayPosition::TopLeft | OverlayPosition::TopRight => monitor.y + offset_y,
        OverlayPosition::BottomLeft | OverlayPosition::BottomRight => {
            monitor.y + monitor.height - size.y - offset_y
        }
    };
    egui::Pos2::new(left.max(monitor.x), top.max(monitor.y))
}

/// Monitor rects in logical points from `xcap` (spec §4.3 geometry). Called on
/// the MAIN thread when a toast starts — never in the capture hot path. A
/// monitor whose geometry/accessors fail is skipped; an overall failure is
/// reported by the caller (WARN + unpositioned fallback toast).
fn monitor_rects() -> Result<Vec<MonitorRect>, xcap::XCapError> {
    let mut rects = Vec::new();

    for monitor in xcap::Monitor::all()? {
        let (Ok(x), Ok(y), Ok(width), Ok(height)) =
            (monitor.x(), monitor.y(), monitor.width(), monitor.height())
        else {
            tracing::warn!("Monitor geometry unavailable; skipped for toast placement");
            continue;
        };
        let scale = monitor.scale_factor().unwrap_or(1.0);
        let scale = if scale > 0.0 { scale } else { 1.0 };

        rects.push(MonitorRect {
            x: x as f32 / scale,
            y: y as f32 / scale,
            width: width as f32 / scale,
            height: height as f32 / scale,
            primary: monitor.is_primary().unwrap_or(false),
        });
    }

    Ok(rects)
}

/// Where the toast windows go (spec §4.3 v5.5): `xcap` monitor geometry when
/// available — one placement per monitor with `on_all_monitors`, otherwise the
/// primary monitor. When enumeration fails or reports nothing, ONE fallback
/// placement from egui's current-monitor size (`ViewportInfo::monitor_size`,
/// `None` on Wayland) — and with no size at all the toast stays unpositioned
/// and the compositor places it. Failures are WARNed; the app keeps running.
fn toast_placements(
    ctx: &egui::Context,
    overlay: &config::Overlay,
    size: egui::Vec2,
) -> Vec<Option<egui::Pos2>> {
    let rects = match monitor_rects() {
        Ok(rects) => rects,
        Err(err) => {
            tracing::warn!("Monitor enumeration failed ({err}); using the current monitor");
            Vec::new()
        }
    };

    if !rects.is_empty() {
        return toast_positions(
            &rects,
            overlay.position,
            size,
            TOAST_OFFSET_X,
            TOAST_OFFSET_Y,
            overlay.on_all_monitors,
        )
        .into_iter()
        .map(Some)
        .collect();
    }

    let fallback = toast_position(
        ctx.input(|i| i.viewport().monitor_size),
        overlay.position,
        size,
    );
    if fallback.is_none() {
        tracing::warn!("No monitor geometry available; toast placed by the compositor");
    }
    vec![fallback]
}

/// INFO summary of the toast placements (Windows diagnostics, spec §4.3):
/// logical position of each toast window, or `compositor-placed`.
fn placement_summary(viewports: &[ToastViewport]) -> String {
    let parts: Vec<String> = viewports
        .iter()
        .map(|viewport| match viewport.position {
            Some(position) => format!("({} x {})", position.x, position.y),
            None => "compositor-placed".to_owned(),
        })
        .collect();
    parts.join(" ")
}

/// Fresh toast windows for a spawn: ONE per placement, ids unique within
/// `generation` (spec §4.3 — a replacement never reuses a live window).
fn toast_windows(generation: u64, positions: &[Option<egui::Pos2>]) -> Vec<ToastViewport> {
    positions
        .iter()
        .enumerate()
        .map(|(index, position)| ToastViewport {
            id: toast_viewport_id(generation, index),
            position: *position,
            painted: Arc::new(AtomicBool::new(false)),
        })
        .collect()
}

/// What one pass of toast-window bookkeeping does (spec §4.3 v6.1).
#[derive(Debug, Clone)]
struct ToastWindowStep {
    /// Windows to close this pass (the previous generation).
    close: Vec<egui::ViewportId>,
    /// Windows to show this pass: the previous ones when nothing changed, the
    /// drained queue when it spawns, nothing while a replacement is waiting.
    show: Vec<ToastViewport>,
    /// Placements queued for the NEXT pass (`None` = nothing queued).
    pending: Option<Vec<Option<egui::Pos2>>>,
    /// Whether `show` is a fresh spawn (drives the INFO placement summary).
    spawned: bool,
}

/// Pure toast-window lifecycle mapping (spec §4.3 v6.1: exactly ONE toast
/// window per monitor, a new toast REPLACES the current one — never a stack).
///
/// The spawn of a REPLACEMENT is deferred to the pass after the close, because
/// eframe creates windows for new viewports BEFORE it destroys the windows of
/// viewports that are gone (`handle_viewport_output`: `initialize_all_windows`
/// then `remove_viewports_not_in`), and egui drops an unused child viewport
/// only at the END of the pass (`end_pass`: "it was never used this pass").
/// Spawning in the same pass as the close therefore leaves the OLD window on
/// screen while the new one appears — the second window (the white unpainted
/// strip) the user saw. With this mapping the old windows are gone before the
/// new ones are created, so at most one window per monitor exists at any time.
/// A toast with nothing live (the first one, or a spawn queued by an earlier
/// replacement) spawns in the pass it is requested.
fn toast_window_step(
    live: &[ToastViewport],
    pending: Option<Vec<Option<egui::Pos2>>>,
    requested: Option<Vec<Option<egui::Pos2>>>,
    alive: bool,
    generation: u64,
) -> ToastWindowStep {
    if !alive {
        // The toast ended: close whatever is live and drop anything queued, so
        // a toast that expired before its spawn pass never appears.
        return ToastWindowStep {
            close: live.iter().map(|viewport| viewport.id).collect(),
            show: Vec::new(),
            pending: None,
            spawned: false,
        };
    }

    // A replacement is only a replacement when there is something to close: a
    // first toast (nothing live) spawns in this same pass.
    let replacing = requested.is_some() && !live.is_empty();
    let queued = requested.or(pending);
    let close = if replacing {
        live.iter().map(|viewport| viewport.id).collect()
    } else {
        Vec::new()
    };

    if replacing {
        // The live windows are closed this pass and the fresh placements wait
        // for the NEXT pass, so nothing is painted while the close happens.
        return ToastWindowStep {
            close,
            show: Vec::new(),
            pending: queued,
            spawned: false,
        };
    }

    // A queued spawn only exists while NO window is live (a replacement step
    // clears `live`), so with windows live there is nothing to spawn or queue.
    if live.is_empty()
        && let Some(positions) = queued
    {
        return ToastWindowStep {
            close,
            show: toast_windows(generation, &positions),
            pending: None,
            spawned: true,
        };
    }

    ToastWindowStep {
        close,
        show: live.to_vec(),
        pending: None,
        spawned: false,
    }
}

/// Commands for a LIVE toast window (spec §4.3 v6.1): paint it while it is
/// hidden, and show it ONLY once its ui callback has run — a window shown
/// before that is the white unpainted strip. `RequestPaintWhileHidden` holds
/// for one frame (`egui::ViewportCommand`), so it is re-sent every pass the
/// toast is alive; the same request keeps the fade-out animating.
fn toast_show_commands(painted: bool) -> Vec<egui::ViewportCommand> {
    let mut commands = vec![egui::ViewportCommand::RequestPaintWhileHidden];
    if painted {
        commands.push(egui::ViewportCommand::Visible(true));
    }
    commands
}

/// Start directory for the Browse folder picker (spec §4.4): the draft
/// `save_path` when it exists on disk, else `<home>/Pictures`
/// (`%USERPROFILE%\Pictures` on Windows, `~/Pictures` on Linux). With no home
/// directory the start is the cwd-relative `Pictures`, mirroring
/// `config::default_save_path`'s fallback. The existence check is a plain
/// `is_dir()` on the draft path — no dialog is involved, so the rule is
/// unit-tested against a real scratch directory.
fn browse_start_dir(save_path: &Path, home: Option<&Path>) -> PathBuf {
    if save_path.is_dir() {
        save_path.to_path_buf()
    } else {
        home.map(|home| home.join("Pictures"))
            .unwrap_or_else(|| PathBuf::from("Pictures"))
    }
}

/// Hotkeys tab rows in spec §4.4 order.
pub(crate) const HOTKEY_ROWS: [Action; 3] = [Action::FullScreen, Action::Window, Action::Rect];

/// UI labels for the action rows (English).
fn row_label(action: Action) -> &'static str {
    match action {
        Action::FullScreen => "Full screen",
        Action::Window => "Window",
        Action::Rect => "Rect area",
    }
}

/// GUI labels for formats (config names are `png|jpg|webp`).
fn format_label(format: Format) -> &'static str {
    match format {
        Format::Png => "png",
        Format::Jpeg => "jpg",
        Format::Webp => "webp",
    }
}

/// GUI labels for overlay corners (config enum names).
fn position_label(position: OverlayPosition) -> &'static str {
    match position {
        OverlayPosition::TopLeft => "TopLeft",
        OverlayPosition::TopRight => "TopRight",
        OverlayPosition::BottomLeft => "BottomLeft",
        OverlayPosition::BottomRight => "BottomRight",
    }
}

/// Key names the MVP binding notation supports (`bindings::parse_key`) —
/// bind-by-press accepts exactly these, anything else is ignored.
pub(crate) const SUPPORTED_KEY_NAMES: &[&str] = &[
    "PrintScreen",
    "A",
    "B",
    "C",
    "D",
    "E",
    "F",
    "G",
    "H",
    "I",
    "J",
    "K",
    "L",
    "M",
    "N",
    "O",
    "P",
    "Q",
    "R",
    "S",
    "T",
    "U",
    "V",
    "W",
    "X",
    "Y",
    "Z",
    "0",
    "1",
    "2",
    "3",
    "4",
    "5",
    "6",
    "7",
    "8",
    "9",
    "F1",
    "F2",
    "F3",
    "F4",
    "F5",
    "F6",
    "F7",
    "F8",
    "F9",
    "F10",
    "F11",
    "F12",
];

/// Modifier names in canonical display order (config notation is parsed
/// case-insensitively; `Win` maps to crate META).
pub(crate) const MOD_NAMES: [&str; 4] = ["Alt", "Ctrl", "Shift", "Win"];

/// Bind-button label while bind-by-press is waiting for a key (§4.4).
const CAPTURE_BUTTON_LABEL: &str = "Press keys…";

/// Tooltip while capturing: Escape cancels. Live hotkeys are PAUSED during
/// capture (Stage 18, spec §4.4 v5.5), so a press cannot fire a registered
/// combo (the old PrintScreen quirk note is gone).
const CAPTURE_TOOLTIP: &str = "Press a key combo to bind it — Esc to cancel.";

/// Tooltip for the reserved-slot info icon (§4.4: rect is out of MVP scope).
const RESERVED_TOOLTIP: &str = "reserved — out of MVP scope";

/// Pad-field label while it is capturing a pad press (§4.4, Stage 5). The pump
/// commits the first DOWN event, which also ends the capture; cancelling is a
/// click of the same field again, or starting a keyboard capture — Escape does
/// NOT cancel pad capture (it belongs to keyboard capture only).
const PAD_CAPTURE_BUTTON_LABEL: &str = "Press a button…";

/// Tooltip while the pad field is capturing.
const PAD_CAPTURE_TOOLTIP: &str = "Press a gamepad button to bind it — click again to cancel.";

/// Tooltip for a Windows pad button (spec §4.1: the pad trigger ORs with the
/// keyboard combo through one shared debounce gate).
const PAD_BIND_TOOLTIP: &str = "Gamepad trigger (fires together with the hotkey) — click to bind";

/// Empty pad field on Windows, ready to capture.
const PAD_UNBOUND_LABEL: &str = "pad — click to bind";

/// Pad field on a host with no GameInput backend. Linux gilrs is out of scope
/// (AGENTS.md hardware finding), so the field stays a disabled
/// placeholder there; the pad trigger still works from `config.toml` notation
/// once a Windows pump delivers it.
const GAMEPAD_PLACEHOLDER_LABEL: &str = "(gamepad — Windows only)";

/// Bind-button minimum width so the three rows stay visually aligned.
const BIND_BUTTON_MIN_WIDTH: f32 = 170.0;

/// Status-icon colors (warning = red-orange, info = blue).
const WARNING_COLOR: egui::Color32 = egui::Color32::from_rgb(246, 118, 100);
const INFO_COLOR: egui::Color32 = egui::Color32::from_rgb(118, 176, 240);

/// Tab identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tab {
    General,
    Hotkeys,
    Overlay,
}

impl Tab {
    fn label(self) -> &'static str {
        match self {
            Tab::General => "General",
            Tab::Hotkeys => "Hotkeys",
            Tab::Overlay => "Overlay",
        }
    }
}

/// The egui key name only when `bindings::parse_key` would accept it.
pub(crate) fn supported_key_name(key: egui::Key) -> Option<&'static str> {
    let name = key.name();
    SUPPORTED_KEY_NAMES.iter().copied().find(|n| *n == name)
}

/// Display label for a binding (spec §4.4): modifiers in canonical
/// `MOD_NAMES` order joined with `+`, then the key — `Alt+PrintScreen`,
/// `Ctrl+Shift+S`, bare key when no modifiers. Known modifiers are matched
/// case-insensitively and printed canonical (duplicates collapse to one);
/// unknown names are skipped. The result is also valid §4.1 config notation.
pub(crate) fn binding_label(key: &str, mods: &[String]) -> String {
    let mut parts: Vec<&str> = MOD_NAMES
        .iter()
        .filter(|name| mods.iter().any(|held| held.eq_ignore_ascii_case(name)))
        .copied()
        .collect();
    parts.push(key);
    parts.join("+")
}

/// Modifier names captured from an egui key press, canonical order. Only
/// Alt/Ctrl/Shift: winit never reports Super as a modifier, so `Win` stays
/// settable by editing `config.toml` (accepted MVP limitation, §4.4).
pub(crate) fn mods_from_press(alt: bool, ctrl: bool, shift: bool) -> Vec<String> {
    [(alt, "Alt"), (ctrl, "Ctrl"), (shift, "Shift")]
        .into_iter()
        .filter(|(held, _)| *held)
        .map(|(_, name)| (*name).to_owned())
        .collect()
}

/// Binding built from a captured press: key and mods REPLACE the draft values;
/// the reserved `pad` slot keeps whatever the draft held (§4.1).
fn binding_from_press(key: &str, mods: Vec<String>, pad: Option<String>) -> Binding {
    Binding {
        key: key.to_owned(),
        mods,
        pad,
    }
}

/// Hotkey registration transition driven by a bind-by-press interaction
/// (Stage 18, spec §4.4 v5.5): starting capture pauses registrations, ending
/// capture — commit or cancel — restores the applied settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CaptureTransition {
    Pause,
    Restore,
}

/// A bind-by-press interaction, already classified. Unsupported keys are not
/// an input: capture mode stays on and no transition is produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CaptureInput {
    /// Bind button clicked: starts capture when idle, cancels when capturing.
    Click,
    /// Escape pressed while capturing: cancel.
    Escape,
    /// Supported key pressed while capturing: commit the binding.
    Commit,
}

/// Pure decision mapping for capture-press events (§4.4 v5.5): idle-click
/// pauses the live hotkeys; any capture-end (reclick, Escape, commit)
/// restores them. Inputs outside capture mode produce no transition.
pub(crate) fn capture_transition(
    capturing: bool,
    input: CaptureInput,
) -> Option<CaptureTransition> {
    match input {
        CaptureInput::Click => Some(if capturing {
            CaptureTransition::Restore
        } else {
            CaptureTransition::Pause
        }),
        CaptureInput::Escape | CaptureInput::Commit => {
            capturing.then_some(CaptureTransition::Restore)
        }
    }
}

/// Pure pad-field click decision (§4.4, Stage 5): clicking an idle pad field
/// starts capture for `action`; clicking a capturing field cancels it. The
/// capture rules themselves (suppression, commit, notation) live in `gamepad`;
/// this only decides which row the GUI is waiting on.
pub(crate) fn pad_capture_toggle(capturing: bool, action: Action) -> Option<Action> {
    if capturing { None } else { Some(action) }
}

/// Pure pad-capture drain decision (§4.4, Stage 5): the press the pump committed
/// lands on the row that was capturing, as config notation (`gamepad::
/// pad_notation` — `unknown:<N>` for a controller button, `Guide`/`Share` for a
/// system button). A press with no capture target (cancelled mid-press) is
/// dropped: no row is rewritten.
pub(crate) fn pad_capture_target(
    capturing: Option<Action>,
    event: PadDownEvent,
) -> Option<(Action, String)> {
    capturing.map(|action| (action, pad_notation(event)))
}

/// Pad-field display label (§4.4, Stage 5): a host without a GameInput backend
/// keeps the disabled placeholder (Linux gilrs is out of scope); on Windows the
/// field shows the capture prompt while it waits for a press, the stored
/// notation once bound, and the click-to-bind prompt when empty.
pub(crate) fn pad_field_label(pad_supported: bool, capturing: bool, pad: Option<&str>) -> String {
    if !pad_supported {
        GAMEPAD_PLACEHOLDER_LABEL.to_owned()
    } else if capturing {
        PAD_CAPTURE_BUTTON_LABEL.to_owned()
    } else {
        pad.unwrap_or(PAD_UNBOUND_LABEL).to_owned()
    }
}

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
            tooltip: RESERVED_TOOLTIP.to_owned(),
        }),
    }
}

/// Glyph for an icon kind. `⚠`/`ℹ` are NOT in egui's bundled fonts (checked
/// the locked cmaps), so the icons are colored ASCII letters — the colors carry
/// the meaning, the letter keeps the shape unambiguous without a symbol font.
fn icon_glyph(kind: IconKind) -> &'static str {
    match kind {
        IconKind::Warning => "!",
        IconKind::Info => "i",
    }
}

/// Color for an icon kind (via `RichText::color`).
fn icon_color(kind: IconKind) -> egui::Color32 {
    match kind {
        IconKind::Warning => WARNING_COLOR,
        IconKind::Info => INFO_COLOR,
    }
}

/// Whether the draft differs from the applied settings (Apply button hint;
/// closing without Apply discards the draft — spec §4.4).
pub(crate) fn is_dirty(draft: &Settings, applied: &Settings) -> bool {
    draft != applied
}

/// GUI failures: window/graphics setup only. Tab logic itself cannot fail —
/// Apply failures are logged and the app keeps running (spec §5).
#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("settings window could not start: {message}")]
    Gui { message: String },
}

/// The settings-window app. Owns the draft settings, the hotkey controller
/// (main-thread manager), and bind-by-press state.
pub(crate) struct SettingsApp {
    config_path: PathBuf,
    /// Last applied settings — closing without Apply resets the draft to it.
    applied: Settings,
    draft: Settings,
    controller: Option<HotkeysController>,
    status: Vec<Registration>,
    tab: Tab,
    /// Row whose "Capture press" is waiting for a key event (§4.4 bind-by-press).
    capture_press: Option<Action>,
    /// Row whose pad field is waiting for a pad DOWN event (§4.4, Stage 5).
    /// Windows only — the GameInput pump is what commits it; elsewhere the pad
    /// field renders as a disabled placeholder and this stays `None`.
    capture_pad: Option<Action>,
    /// Pad-capture mailbox shared with the pump (`gamepad::PadCapture`): the
    /// GUI sets the capture, the pump commits a press, `logic` drains it into
    /// the draft — no callback crosses the thread boundary.
    pad_capture: PadCaptureMailbox,
    /// Settings mailbox shared with the hotkey listener and the pad pump
    /// (`main` created it). Published by `apply`; needed here as the fallback
    /// publisher when the hotkey controller is unavailable.
    settings_mailbox: Arc<Mutex<Settings>>,
    /// Device-info label hint of the last captured pad press per row (§9), shown
    /// beside the pad button. In-memory only: a label is device-dependent, so it
    /// is not part of the persisted notation.
    pad_hints: HashMap<Action, String>,
    /// Toast mailbox filled by capture threads (spec §4.3); polled in `logic`.
    toast_mailbox: ToastMailbox,
    /// Current toast lifecycle — one toast at a time, newest replaces (spec §4.3).
    toast: Option<ToastState>,
    /// Live toast windows (spec §4.3 v5.5): one per monitor when
    /// `overlay.on_all_monitors`, otherwise one on the primary monitor.
    toast_viewports: Vec<ToastViewport>,
    /// Placements of a replacement waiting for the NEXT pass (spec §4.3 v6.1):
    /// the fresh windows spawn only after the previous ones are gone, so old
    /// and new toast windows never co-exist. See `toast_window_step`.
    toast_pending: Option<Vec<Option<egui::Pos2>>>,
    /// Increments on every replacement so toast viewport ids stay unique.
    toast_generation: u64,
    /// Tray controller (spec §4.5, Stage 10); `None` when the tray failed to
    /// start (typed error logged by main). Polled in `logic`.
    tray: Option<TrayController>,
    /// Set by the tray `Exit` action only (spec §4.5): the next close request
    /// is not cancelled, so eframe shuts down.
    exit_requested: bool,
}

impl SettingsApp {
    #[allow(clippy::too_many_arguments)]
    fn new(
        config_path: PathBuf,
        settings: Settings,
        controller: Option<HotkeysController>,
        status: Vec<Registration>,
        toast_mailbox: ToastMailbox,
        tray: Option<TrayController>,
        settings_mailbox: Arc<Mutex<Settings>>,
        pad_capture: PadCaptureMailbox,
    ) -> Self {
        Self {
            config_path,
            applied: settings.clone(),
            draft: settings,
            controller,
            status,
            tab: Tab::General,
            capture_press: None,
            capture_pad: None,
            pad_capture,
            settings_mailbox,
            pad_hints: HashMap::new(),
            toast_mailbox,
            toast: None,
            toast_viewports: Vec::new(),
            toast_pending: None,
            toast_generation: 0,
            tray,
            exit_requested: false,
        }
    }

    fn binding_mut(&mut self, action: Action) -> &mut Binding {
        match action {
            Action::FullScreen => &mut self.draft.controls.full_screen,
            Action::Window => &mut self.draft.controls.window,
            Action::Rect => &mut self.draft.controls.rect,
        }
    }

    fn binding(&self, action: Action) -> &Binding {
        match action {
            Action::FullScreen => &self.draft.controls.full_screen,
            Action::Window => &self.draft.controls.window,
            Action::Rect => &self.draft.controls.rect,
        }
    }

    /// Icon for a row's registration status (spec §4.4): warning on a failed
    /// registration, info on the reserved slot, nothing when registered.
    fn status_icon_for(&self, action: Action) -> Option<StatusIcon> {
        self.status
            .iter()
            .find(|registration| registration.action() == action)
            .and_then(icon_for_status)
    }

    /// Apply: save `config.toml` and re-register hotkeys live (spec §4.4).
    /// Failures are logged; the app keeps running.
    fn apply(&mut self) {
        match config::save(&self.draft, &self.config_path) {
            Ok(()) => tracing::info!("Config saved: {}", self.config_path.display()),
            Err(err) => tracing::error!("Config save failed: {err}"),
        }
        self.applied = self.draft.clone();

        match &mut self.controller {
            Some(controller) => match controller.apply(self.draft.clone()) {
                Ok(status) => self.status = status.registrations,
                Err(err) => tracing::error!("Hotkey re-registration failed: {err}"),
            },
            None => {
                // No hotkey controller (Linux without X11, or a Windows manager
                // failure): the pad pump reads the same settings mailbox, so it
                // is published here instead. With a controller, `apply` publishes
                // it (one publisher per Apply).
                let mut mailbox = self
                    .settings_mailbox
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                *mailbox = self.draft.clone();
            }
        }
    }

    /// Keyboard bind-by-press state (Stage 18) plus the pad-capture mailbox:
    /// while ANY field is capturing, the pad shot path stays silent (Stage 5,
    /// spec §4.4). Any keyboard capture change — start or end — clears the pad
    /// capture: one field captures at a time.
    fn set_keyboard_capture(&mut self, action: Option<Action>) {
        self.capture_press = action;
        set_capture_mode(
            &self.pad_capture,
            action.map(|action| (CaptureField::Keyboard, action)),
        );
        self.capture_pad = None;
    }

    /// Start pad capture for one row (§4.4, Stage 5, Windows only). A keyboard
    /// capture is cancelled first (registrations restored): one field captures
    /// at a time.
    fn start_pad_capture(&mut self, action: Action) {
        if self.capture_press.is_some() {
            self.set_keyboard_capture(None);
            if let Some(transition) = capture_transition(true, CaptureInput::Escape) {
                self.apply_capture_transition(transition);
            }
        }
        self.capture_pad = Some(action);
        set_capture_mode(&self.pad_capture, Some((CaptureField::Pad, action)));
        tracing::info!("Pad capture started for {}", action.label());
    }

    /// Cancel pad capture (clicking the field again): the mailbox returns to
    /// idle, so pad triggers fire again.
    fn cancel_pad_capture(&mut self) {
        self.capture_pad = None;
        set_capture_mode(&self.pad_capture, None);
    }

    /// Drain the press the pump committed into the draft (§4.4, Stage 5): the
    /// notation goes to the row that was capturing, capture state ends, and the
    /// device-info label hint is remembered for that row. The draft is NOT
    /// published until Apply (§4.4 semantics unchanged) — the pad trigger fires
    /// the captured bind from then on. Runs in `logic`, so it also works with
    /// the window hidden.
    fn drain_pad_capture(&mut self) {
        let Some(event) = take_captured(&self.pad_capture) else {
            return;
        };
        // The notation is needed on BOTH branches: it goes to the row on a hit
        // and names the dropped press on a miss, so it is computed before the
        // decision rather than only inside the success pattern.
        let notation = pad_notation(event);
        let Some(target) = pad_capture_target(self.capture_pad.take(), event) else {
            // The mailbox is the shared truth, so naming it distinguishes a
            // cancelled pad capture from a keyboard capture that never commits.
            tracing::warn!(
                "Pad press ignored: {notation} — no pad field is capturing (mailbox active {:?})",
                active_capture(&self.pad_capture),
            );
            return;
        };
        let (action, notation) = target;
        self.binding_mut(action).pad = Some(notation.clone());
        // Label hint (§9) of the press just captured, when the device labels it.
        // A re-capture replaces the hint; a press with no label (system button,
        // v0 device, disconnected) clears the stale one, so the row never shows
        // a label from an earlier press.
        match pad_button_label(event) {
            Some(hint) => {
                self.pad_hints.insert(action, hint);
            }
            None => {
                self.pad_hints.remove(&action);
            }
        }
        tracing::info!("Pad bind set in draft: {} → {notation}", action.label());
    }

    /// Drive the hotkey pause/restore transition on the controller (Stage 18).
    /// The controller owns the manager created on the MAIN thread, and `ui`/
    /// `logic` passes run on that same thread (winit/eframe pump), so this
    /// satisfies the WM_HOTKEY thread affinity. No controller (host without
    /// hotkeys) → no-op.
    fn apply_capture_transition(&mut self, transition: CaptureTransition) {
        let Some(controller) = &mut self.controller else {
            return;
        };
        match transition {
            CaptureTransition::Pause => controller.pause(),
            CaptureTransition::Restore => controller.restore(),
        }
    }

    /// Bind-by-press (spec §4.4): clicking the bind button records the next
    /// supported key press, REPLACING the draft binding's key+mods. Escape (or
    /// clicking the button again) cancels; unsupported keys are ignored and
    /// capture mode stays on. winit reports Alt/Ctrl/Shift as event modifiers;
    /// the Win key is NOT a winit modifier, so Win is only settable by editing
    /// `config.toml` — an accepted MVP limitation. While capturing, live
    /// hotkey registrations are PAUSED (Stage 18, spec §4.4 v5.5): a press
    /// reaches the egui window instead of being stolen by the platform hotkey
    /// hook (Windows WM_KEYDOWN→WM_HOTKEY), so binding TO PrintScreen works
    /// and does not fire the screenshot action. On capture end — commit OR
    /// cancel — the LAST APPLIED settings are re-registered; the draft stays
    /// unapplied until Apply (§4.4 semantics unchanged).
    fn handle_capture_press(&mut self, ui: &egui::Ui) {
        let Some(action) = self.capture_press else {
            return;
        };
        for event in ui.input(|i| i.events.clone()) {
            if let egui::Event::Key {
                key,
                pressed: true,
                repeat: false,
                modifiers,
                ..
            } = event
            {
                if key == egui::Key::Escape {
                    self.set_keyboard_capture(None);
                    if let Some(transition) = capture_transition(true, CaptureInput::Escape) {
                        self.apply_capture_transition(transition);
                    }
                    return;
                }
                let Some(name) = supported_key_name(key) else {
                    continue; // unsupported key — stay in capture mode
                };
                let mods = mods_from_press(modifiers.alt, modifiers.ctrl, modifiers.shift);
                let binding = self.binding_mut(action);
                let pad = binding.pad.clone();
                *binding = binding_from_press(name, mods, pad);
                self.set_keyboard_capture(None);
                if let Some(transition) = capture_transition(true, CaptureInput::Commit) {
                    self.apply_capture_transition(transition);
                }
                return;
            }
        }
    }

    /// Toast lifecycle + toast windows (spec §4.3 v5.5). Polls the mailbox
    /// (newest wins), starts/replaces the toast, paints the fade while it
    /// lives, and closes the toast windows once gone. A success toast is
    /// skipped when `overlay.enabled` is false; an error toast always shows.
    ///
    /// Visibility fix (the Windows bug): while the settings window is HIDDEN
    /// eframe runs `App::logic` only, and egui produces `ViewportOutput` (the
    /// builder + ui callback of a deferred viewport) ONLY in
    /// `Context::end_pass` — `run_logic` returns `LogicOutput` with no viewport
    /// output (egui `src/data/output.rs`), and eframe's hidden-window branch
    /// consumes just that (`glow_integration.rs`). So a toast spawned from
    /// `logic` while hidden never became a window — the toast was invisible,
    /// not just misplaced. `ViewportCommand::RequestPaintWhileHidden` on the
    /// ROOT viewport sets `ActionRequested::PaintWhileHidden`, whose
    /// `wants_paint()` makes eframe treat the hidden window as visible for the
    /// pass (`is_visible` in `glow_integration`) → the next pass is a FULL pass
    /// that delivers toast builders and animates the fade (eframe paints
    /// invisible windows directly, throttled to 100 ms — `run.rs`). Sent every
    /// pass while a toast is alive; with no toast the window returns to cheap
    /// logic-only ticks.
    ///
    /// First-frame fix (spec §4.3 v6.0, Stage 21): the chip paints at full
    /// opacity from its first frame — `overlay::ToastState::alpha` fades out
    /// only at the end of the lifecycle, because a fade-in ramp starts at alpha
    /// 0 and `Color32::gamma_multiply` scales the alpha channel too, leaving the
    /// window painted EMPTY. Commands and repaint requests for a toast viewport
    /// are sent only once its window exists (`viewport_has_window`): the window
    /// appears at the end of the spawn pass, so a request from that pass is
    /// dropped by eframe AND silences all later requests (see the helper's doc).
    ///
    /// One window per monitor (spec §4.3 v6.1): a replacement CLOSES the current
    /// windows and queues the fresh placements, spawning them on the NEXT pass
    /// (`toast_window_step`). eframe creates the windows of new viewports
    /// BEFORE it destroys the windows of viewports that are gone
    /// (`handle_viewport_output`: `initialize_all_windows` then
    /// `remove_viewports_not_in`), so spawning in the close pass left the old
    /// window and the new one on screen together.
    ///
    /// No unpainted window (spec §4.3 v6.1): egui 0.37 has NO per-viewport
    /// background color — `ViewportBuilder` has no inner-color field and
    /// `ViewportCommand` has no fill command (`ViewportBuilder::with_inner_color`
    /// does not exist), so the window's own paint is the only thing that can
    /// cover the white DWM glass of a created-but-never-painted window. The toast
    /// window is therefore created HIDDEN (`with_visible(false)`) and painted
    /// while hidden: `RequestPaintWhileHidden` becomes
    /// `ActionRequested::PaintWhileHidden`, whose `wants_paint()` makes eframe
    /// treat the hidden window as painted (`is_visible` in `glow_integration`)
    /// and run its ui callback. Once that callback has run the window really has
    /// pixels, so it is shown with `ViewportCommand::Visible(true)`.
    /// `with_active(false)` makes winit show it with `SW_SHOWNOACTIVATE`
    /// (`winit window_state.rs`), so a toast never steals focus. The price: the
    /// toast appears ~2 passes (~200 ms) after the capture finishes, always
    /// inside the §4.3 hold.
    fn update_toast(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);

        let mut requested: Option<Vec<Option<egui::Pos2>>> = None;
        if let Some(notification) = take_latest(&self.toast_mailbox)
            && should_show(&notification, &self.applied.overlay)
        {
            match &mut self.toast {
                Some(state) => state.replace(notification, &self.applied.overlay, now),
                None => {
                    self.toast = Some(ToastState::new(notification, &self.applied.overlay, now));
                }
            }
            self.toast_generation = self.toast_generation.wrapping_add(1);
            requested = self.toast.as_ref().map(|state| {
                toast_placements(ctx, &self.applied.overlay, toast_window_size(&state.text()))
            });
        }

        let alive = self.toast.as_ref().is_some_and(|state| !state.is_gone(now));
        let step = toast_window_step(
            &self.toast_viewports,
            self.toast_pending.take(),
            requested,
            alive,
            self.toast_generation,
        );
        if !alive {
            self.toast = None;
        }
        self.apply_toast_windows(ctx, step);

        let Some(state) = &self.toast else {
            return;
        };

        let text = state.text();
        let alpha = state.alpha(now);
        let size = toast_window_size(&text);

        // egui needs `show_viewport_deferred` on every pass the child exists;
        // the callback is replaced each pass, so the fade alpha updates.
        for viewport in &self.toast_viewports {
            let builder = egui::ViewportBuilder::default()
                .with_title("virshot")
                .with_inner_size(size)
                .with_visible(false)
                .with_active(false)
                .with_decorations(false)
                .with_transparent(true)
                .with_mouse_passthrough(true)
                .with_always_on_top();
            let builder = match viewport.position {
                Some(position) => builder.with_position(position),
                None => builder,
            };
            let text = text.clone();
            let painted_cb = Arc::clone(&viewport.painted);
            ctx.show_viewport_deferred(viewport.id, builder, move |ui, _class| {
                painted_cb.store(true, Ordering::Relaxed);
                let rect = ui.max_rect();
                ui.painter()
                    .rect_filled(rect, 8.0, TOAST_FILL.gamma_multiply(alpha));
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    &text,
                    egui::FontId::proportional(14.0),
                    TOAST_TEXT.gamma_multiply(alpha),
                );
            });
            // Everything below needs a real window: a request or command sent to
            // a viewport without one is dropped by eframe and silences every
            // later request (`viewport_has_window`).
            if ctx.input(|i| viewport_has_window(&i.raw, viewport.id)) {
                for command in toast_show_commands(viewport.painted.load(Ordering::Relaxed)) {
                    ctx.send_viewport_cmd_to(viewport.id, command);
                }
                ctx.request_repaint_of(viewport.id);
            }
        }

        ctx.request_repaint();
        ctx.send_viewport_cmd_to(
            egui::ViewportId::ROOT,
            egui::ViewportCommand::RequestPaintWhileHidden,
        );
    }

    /// Applies a toast-window step (spec §4.3 v6.1): closes the windows being
    /// replaced and shows the ones that spawn this pass. DEBUG per close batch,
    /// INFO with the placement summary when windows appear.
    fn apply_toast_windows(&mut self, ctx: &egui::Context, step: ToastWindowStep) {
        if !step.close.is_empty() {
            for id in &step.close {
                ctx.send_viewport_cmd_to(*id, egui::ViewportCommand::Close);
            }
            tracing::debug!(
                "Toast viewports closed ({}): toast replaced or finished",
                step.close.len()
            );
        }
        if step.spawned {
            tracing::info!(
                "Toast shown on {} monitor(s): {}",
                step.show.len(),
                placement_summary(&step.show)
            );
        }
        self.toast_viewports = step.show;
        self.toast_pending = step.pending;
    }

    fn general_tab(&mut self, ui: &mut egui::Ui) {
        // The save-path row is rendered before the draft borrow is taken for
        // the rest of the tab: Browse rewrites `draft.general.save_path`
        // after the dialog returns (spec §4.4 — Apply persists it as usual).
        ui.label("Save path");
        let mut browse_clicked = false;
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.draft.general.save_path);
            browse_clicked = ui.button("Browse").clicked();
        });
        if browse_clicked {
            self.browse_save_path();
        }

        let general = &mut self.draft.general;

        ui.label("Full-screen template");
        ui.text_edit_singleline(&mut general.file_full_screen);

        ui.label("Window template");
        ui.text_edit_singleline(&mut general.file_window);

        egui::ComboBox::from_label("Format")
            .selected_text(format_label(general.format))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut general.format, Format::Png, "png");
                ui.selectable_value(&mut general.format, Format::Jpeg, "jpg");
                ui.selectable_value(&mut general.format, Format::Webp, "webp");
            });

        ui.horizontal(|ui| {
            ui.add_enabled(
                general.format == Format::Jpeg,
                egui::DragValue::new(&mut general.jpeg_quality).range(1..=100),
            );
            ui.label("JPEG quality");
        });

        ui.horizontal(|ui| {
            ui.add_enabled(
                general.format == Format::Webp,
                egui::DragValue::new(&mut general.webp_quality).range(1..=100),
            );
            ui.label("WebP quality (WebP is lossless in MVP — quality not applied yet)");
        });
    }

    /// Browse → native folder picker (spec §4.4, `rfd` approved by the user
    /// 2026-11-03). API facts verified in the locked rfd 0.17.2 source, not
    /// from memory: the BLOCKING `rfd::FileDialog` builder —
    /// `FileDialog::new()`, `set_directory<P: AsRef<Path>>(self, path)`,
    /// `pick_folder(self) -> Option<PathBuf>` (`src/file_dialog.rs`) — needs
    /// NO async/tokio feature: default features are `xdg-portal` + `wayland`,
    /// which pull `pollster` (unix-only) for blocking-over-portal; Windows
    /// uses `windows-sys` common controls (`src/backend/win_cid.rs`), Linux
    /// the XDG desktop portal with a zenity fallback
    /// (`src/backend/xdg_desktop_portal.rs`). Every backend failure path
    /// (libdbus missing, portal missing, zenity missing) returns `None` —
    /// rfd never panics there — so no unwraps are needed here.
    ///
    /// The dialog is modal and blocking: the UI pass waits for it (the tray
    /// poll tick resumes afterwards). Cancel and "no backend" are both `None`,
    /// so the draft is left untouched and the app stays alive; only a picked
    /// folder rewrites `draft.general.save_path` (Apply persists it, §4.4).
    fn browse_save_path(&mut self) {
        let start = browse_start_dir(
            Path::new(&self.draft.general.save_path),
            config::home_dir().as_deref(),
        );

        match rfd::FileDialog::new().set_directory(start).pick_folder() {
            Some(folder) => {
                self.draft.general.save_path = folder.display().to_string();
                tracing::info!("Save path picked: {}", self.draft.general.save_path);
            }
            None => tracing::warn!(
                "folder picker unavailable (cancelled, or no native backend on this host)"
            ),
        }
    }

    /// Hotkeys tab (spec §4.4 v5.0): three columns — action name | bind button
    /// (+ status icon) | reserved gamepad slot. No key dropdown, no modifier
    /// checkboxes: the button shows the binding and click → bind-by-press.
    fn hotkeys_tab(&mut self, ui: &mut egui::Ui) {
        for (row, action) in HOTKEY_ROWS.iter().enumerate() {
            let action = *action;
            let reserved = action == Action::Rect;
            let capturing = self.capture_press == Some(action);

            // Owned before the grid closure so the row can still toggle
            // `capture_press` while rendering.
            let label = if capturing {
                CAPTURE_BUTTON_LABEL.to_owned()
            } else {
                let binding = self.binding(action);
                binding_label(&binding.key, &binding.mods)
            };
            let icon = self.status_icon_for(action);

            ui.add_enabled_ui(!reserved, |ui| {
                egui::Grid::new(("hotkey-row", row))
                    .num_columns(3)
                    .show(ui, |ui| {
                        ui.label(row_label(action));

                        ui.horizontal(|ui| {
                            let button = egui::Button::new(label)
                                .min_size(egui::Vec2::new(BIND_BUTTON_MIN_WIDTH, 0.0));
                            let response = if capturing {
                                ui.add(button).on_hover_text(CAPTURE_TOOLTIP)
                            } else {
                                ui.add(button)
                            };
                            if response.clicked() {
                                if let Some(transition) =
                                    capture_transition(capturing, CaptureInput::Click)
                                {
                                    self.apply_capture_transition(transition);
                                }
                                self.set_keyboard_capture(if capturing {
                                    None
                                } else {
                                    Some(action)
                                });
                            }

                            if let Some(icon) = icon {
                                let glyph = ui.label(
                                    egui::RichText::new(icon_glyph(icon.kind))
                                        .color(icon_color(icon.kind))
                                        .strong(),
                                );
                                // `on_hover_text` is silent on disabled
                                // widgets (egui 0.37), and the reserved rect
                                // row renders disabled.
                                if reserved {
                                    glyph.on_disabled_hover_text(icon.tooltip);
                                } else {
                                    glyph.on_hover_text(icon.tooltip);
                                }
                            }
                        });

                        // Pad field (§4.1 + §4.4, Stage 5): on Windows it is a
                        // real bind-by-press button — click starts capture, the
                        // pump commits the first DOWN event into the mailbox and
                        // `logic` writes the notation into the draft; a click
                        // while capturing cancels. On a host without GameInput
                        // it stays a disabled placeholder (Linux gilrs is out of
                        // scope). Inside `ui.add_enabled_ui(!reserved)` the
                        // button is disabled on the rect row, so a reserved
                        // action never gets a pad bind from the UI either.
                        let pad_capturing = self.capture_pad == Some(action);
                        let pad_hint = self.pad_hints.get(&action).cloned();
                        if cfg!(windows) {
                            let pad_label = pad_field_label(
                                true,
                                pad_capturing,
                                self.binding(action).pad.as_deref(),
                            );
                            let pad_button = egui::Button::new(pad_label)
                                .min_size(egui::Vec2::new(BIND_BUTTON_MIN_WIDTH, 0.0));
                            let pad_response = if pad_capturing {
                                ui.add(pad_button).on_hover_text(PAD_CAPTURE_TOOLTIP)
                            } else {
                                ui.add(pad_button).on_hover_text(PAD_BIND_TOOLTIP)
                            };
                            if pad_response.clicked() {
                                match pad_capture_toggle(pad_capturing, action) {
                                    Some(start) => self.start_pad_capture(start),
                                    None => self.cancel_pad_capture(),
                                }
                            }
                            // Device-info label hint (§9) of the last captured
                            // press for this row — in-memory, never persisted.
                            if let Some(hint) = pad_hint {
                                ui.label(format!("({hint})"));
                            }
                        } else {
                            ui.add_enabled(false, egui::Button::new(GAMEPAD_PLACEHOLDER_LABEL));
                        }

                        ui.end_row();
                    });
            });
        }

        if self.controller.is_none() {
            ui.label("Hotkeys unavailable on this host (see log)");
        }
    }

    fn overlay_tab(&mut self, ui: &mut egui::Ui) {
        let overlay = &mut self.draft.overlay;

        ui.checkbox(&mut overlay.enabled, "Show overlay toast");

        ui.horizontal(|ui| {
            ui.add(
                egui::DragValue::new(&mut overlay.duration_ms)
                    .range(250..=10_000)
                    .suffix(" ms"),
            );
            ui.label("Duration");
        });

        egui::ComboBox::from_label("Position")
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

        ui.checkbox(&mut overlay.on_all_monitors, "Show on all monitors");
        ui.label("(one toast per monitor when enabled, otherwise on the primary monitor)");
    }
}

impl eframe::App for SettingsApp {
    /// Runs even while the window is hidden (no UI pass then). Keeps the
    /// process resident after a window close (spec §4.4): cancel the close
    /// request and hide the window instead — the tray (Stage 10) can reopen
    /// it. Closing discards unsaved draft edits (spec §4.4). The toast
    /// lifecycle runs here too, so a hotkey fired while the window is hidden
    /// still shows its toast (spec §4.3). Tray actions are polled every pass:
    /// left click / `Settings` re-shows the window, `Take screenshot` spawns
    /// a full-screen capture with the applied settings + the shared mailbox,
    /// `Exit` requests a real shutdown (spec §4.5 — exit is tray-menu-only).
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(tray) = &self.tray {
            for action in tray.poll() {
                match action {
                    TrayAction::ShowSettings => {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        ctx.request_repaint();
                    }
                    TrayAction::TakeScreenshot => {
                        capture::spawn_capture(capture::CaptureCommand {
                            mode: capture::CaptureMode::FullScreen,
                            settings: self.applied.clone(),
                            toast: self.toast_mailbox.clone(),
                        });
                    }
                    TrayAction::Exit => {
                        self.exit_requested = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        ctx.request_repaint();
                    }
                }
            }
        }

        if ctx.input(|i| i.viewport().close_requested()) {
            if self.exit_requested {
                // Tray Exit: no CancelClose this pass → eframe shuts down,
                // `run_native` returns, the process exits 0.
                tracing::info!("virshot exiting from tray menu");
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                self.draft = self.applied.clone();
            }
        }
        // Pad-capture mailbox (Stage 5): the pump committed a press on its own
        // thread; the newest-only drain writes it into the draft here, so a pad
        // press is recorded even while the settings window is hidden.
        self.drain_pad_capture();

        self.update_toast(ctx);

        // Tray events arrive on crossbeam channels and never wake the winit
        // loop; schedule the next logic pass so polling stays alive while the
        // window is hidden.
        ctx.request_repaint_after(TRAY_POLL_TICK);
    }

    /// Alpha-0 clear so the transparent toast window shows through (eframe's
    /// glow backend uses ONE clear color for all viewports). The settings
    /// window stays fully opaque because `CentralPanel` paints its panel fill.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Color32::TRANSPARENT.to_normalized_gamma_f32()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.handle_capture_press(ui);

        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                for tab in [Tab::General, Tab::Hotkeys, Tab::Overlay] {
                    if ui.selectable_label(self.tab == tab, tab.label()).clicked() {
                        self.tab = tab;
                    }
                }
            });
            ui.separator();

            match self.tab {
                Tab::General => self.general_tab(ui),
                Tab::Hotkeys => self.hotkeys_tab(ui),
                Tab::Overlay => self.overlay_tab(ui),
            }

            ui.separator();
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new("Apply")).clicked() {
                    self.apply();
                }
                if is_dirty(&self.draft, &self.applied) {
                    ui.label("Unsaved changes");
                }
            });
        });
    }
}

/// Open the settings window on the MAIN thread (the hotkey manager lives
/// there; winit/eframe pumps Win32 messages — Windows WM_HOTKEY affinity;
/// tray event handling here satisfies the main-thread affinity every backend
/// expects). `toast_mailbox` is the overlay mailbox shared with the hotkey
/// listener; `tray` is the tray controller (spec §4.5) or `None` after a
/// logged tray failure; `settings_mailbox` is the settings mailbox `main`
/// created for the hotkey listener and the pad pump (Apply publishes it), and
/// `pad_capture` is the pad-capture mailbox shared with the pump (§4.4,
/// Stage 5). Returns only if
/// graphics setup failed; the caller falls back to the idle loop so the process
/// stays resident (spec §5).
#[allow(clippy::too_many_arguments)] // Startup wiring: one call site in `main`, each shared
// mailbox named explicitly (same shape as `SettingsApp::new`).
pub(crate) fn run(
    config_path: PathBuf,
    settings: Settings,
    controller: Option<HotkeysController>,
    status: Vec<Registration>,
    toast_mailbox: ToastMailbox,
    tray: Option<TrayController>,
    settings_mailbox: Arc<Mutex<Settings>>,
    pad_capture: PadCaptureMailbox,
) -> Result<(), Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("virshot — Settings")
            .with_inner_size([INITIAL_WINDOW_WIDTH, INITIAL_WINDOW_HEIGHT])
            // eframe derives the alpha-capable GL config from the ROOT viewport
            // builder; the toast viewport needs it to be transparent.
            .with_transparent(true),
        ..Default::default()
    };

    eframe::run_native(
        "virshot",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(SettingsApp::new(
                config_path,
                settings,
                controller,
                status,
                toast_mailbox,
                tray,
                settings_mailbox,
                pad_capture,
            )))
        }),
    )
    .map_err(|err| Error::Gui {
        message: err.to_string(),
    })
}

#[cfg(test)]
#[path = "gui.tests.rs"]
mod tests;
