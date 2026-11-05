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
//! Toast plumbing (spec §4.3, Stage 4): this window OWNS the toast stack —
//! `overlay::OverlayHost` (crates/overlay host-loop backend) and
//! `toast::ToastService` (crates/toast timer thread) are created in the eframe
//! `create` closure of `run`; trigger threads reach the chips through the
//! handle slot (`capture::ToastHandoff`) filled here. The host paints each chip
//! as a child egui viewport on this loop: transparent, undecorated,
//! always-on-top, mouse-passthrough, content-protected, hidden until its first
//! paint, faded out at the end — the crate builds and commands the viewports,
//! this module no longer reimplements any of it. Verified against the locked
//! egui/eframe 0.37 sources, not from memory:
//! - `Context::show_viewport_deferred` only writes viewport output, so
//!   `OverlayHost::update(&ctx)` is valid from `App::logic` on BOTH passes:
//!   a visible root runs `logic` inside the ui pass, and a hidden/minimized
//!   root runs `Context::run_logic` (`eframe epi_integration`), whose
//!   `LogicOutput` carries platform output and viewport commands for ALL
//!   viewports — so chips still spawn, paint, and fade while the settings
//!   window is hidden.
//! - Monitor rects are PHYSICAL desktop pixels with each monitor's DPI scale
//!   attached (`overlay::MonitorRect`), enumerated once through
//!   `overlay::monitors` — the crate's single source of truth for monitor
//!   geometry; this module only wires it and logs — because `OverlayHost`
//!   converts a rect with the CHILD window's own pixels-per-point
//!   (`crates/overlay/src/host.rs`) — the root window's scale belongs to the
//!   settings window, not to a chip. `xcap` stays capture-only. The PRIMARY
//!   monitor comes FIRST — toast `MonitorSel::Index(0)` means primary (spec
//!   §4.3); `overlay::monitors` flags the OS primary monitor. A monitor with
//!   no usable size is skipped (WARN); with no monitor geometry at all the
//!   app falls back to the host window's own physical rect, and an empty list
//!   makes the service log and skip chips (the app stays alive).
//! - `ToastService::spawn(..., wake = Some(ctx.clone()))` wakes this loop for
//!   fade frames; `TRAY_POLL_TICK` repaints keep the host loop fed while idle.
//! - Drop order: `toast_service` is declared BEFORE `overlay_host`, so the
//!   service's `Drop` (Quit + join, hides chips) runs while the host is alive.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use overlay::OverlayHost;
use toast::ToastService;

use crate::bindings::Action;
use crate::capture;
use crate::capture::ToastHandoff;
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
/// (main-thread manager), the toast stack (`ToastService` + `OverlayHost`,
/// spec §4.3), and bind-by-press state.
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
    /// Toast timer thread (spec §4.3): owns chip lifetimes — latest wins,
    /// fade-out at the end, errors held longer. Declared BEFORE `overlay_host`
    /// so its `Drop` (Quit + join, hiding chips) runs while the host backend
    /// is still alive. Read by nothing: this field is the owner, and ownership
    /// is the whole reason it exists.
    #[allow(dead_code)] // Owned for its lifetime only; the service is driven
    // through the handle the triggers hold, and it shuts down on drop.
    toast_service: ToastService,
    /// Toast viewport backend (crates/overlay host loop): chips are child egui
    /// viewports spawned, painted, and faded here; `logic` drives it every
    /// pass (spec §4.3).
    overlay_host: OverlayHost,
    /// Latest-only toast-handle slot shared with the hotkey listener and the
    /// pad pump (`main` created it); `run`'s create closure fills it, so
    /// triggers can attach a chip to their capture (spec §4.3 hand-off).
    handoff: ToastHandoff,
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
        toast_service: ToastService,
        overlay_host: OverlayHost,
        handoff: ToastHandoff,
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
            toast_service,
            overlay_host,
            handoff,
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
                        capture::spawn_capture(
                            capture::CaptureCommand {
                                mode: capture::CaptureMode::FullScreen,
                                // Owned settings for the capture thread (spec
                                // §4.2); `applied` stays valid for the UI.
                                settings: self.applied.clone(),
                            },
                            capture::handoff_handle(&self.handoff),
                        );
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

        // Toast chips (spec §4.3): the crate owns the chip lifetimes
        // (`toast::ToastService`); this line only drives the viewport backend —
        // queued overlays become child egui viewports HERE, so chips spawn,
        // paint, and fade on every pass, including the logic-only pass eframe
        // runs while the settings window is hidden.
        self.overlay_host.update(ctx);

        // Tray events arrive on crossbeam channels and never wake the winit
        // loop; schedule the next logic pass so polling stays alive while the
        // window is hidden. The toast service wakes the same context on its
        // fade ticks.
        ctx.request_repaint_after(TRAY_POLL_TICK);
    }

    /// Alpha-0 clear so transparent windows show through (eframe's glow backend
    /// uses ONE clear color for all viewports — including the toast child
    /// viewports the overlay host creates). The settings window stays fully
    /// opaque because `CentralPanel` paints its panel fill.
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
/// (§4.4, Stage 5). Returns only if graphics setup failed; the caller falls
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
) -> Result<(), Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("virshot — Settings")
            .with_inner_size([INITIAL_WINDOW_WIDTH, INITIAL_WINDOW_HEIGHT])
            // eframe derives the alpha-capable GL config from the ROOT viewport
            // builder; the toast child viewports need it to be transparent.
            .with_transparent(true),
        ..Default::default()
    };

    eframe::run_native(
        "virshot",
        options,
        Box::new(move |cc| {
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
            Ok(Box::new(SettingsApp::new(
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
