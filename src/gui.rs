//! Settings window GUI (spec §4.4): the eframe/egui app on the MAIN thread —
//! three tabs (General / Hotkeys / Overlay), an in-app draft of `Settings`, and
//! a bottom row of `Apply | OK | Cancel` right-aligned at the very bottom
//! (§4.4): Apply saves the draft (`config::save` + live hotkey
//! re-registration via `hotkeys::HotkeysController`) and keeps the window open,
//! OK saves then closes for real, Cancel discards the draft and closes — a
//! discard never saves or publishes. Apply/OK also stay disabled while the
//! draft has duplicate triggers (§4.4, Stage 3): `config::save` validates
//! nothing, so [`duplicate_bindings`] rejects a repeated combo or pad in the
//! GUI before save, the colliding controls carry a Warning icon bound to the
//! control they describe (§4.4, Stage 10: keyboard warnings sit LEFT of the
//! bind button, pad warnings LEFT of the pad field — [`keyboard_warning`] and
//! [`pad_warning`] split the status per control), and `apply()` refuses such a
//! draft as defense in depth. UI text is English (spec §2
//! i18n note).
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
//! - Exit is the tray menu or the bottom OK/Cancel buttons (eframe 0.37 has
//!   NO `App::should_exit` — verified in the locked source): tray `Exit` sets
//!   `exit_requested`, OK/Cancel set `close_for_exit` and send
//!   `ViewportCommand::Close`; the pass that sees `close_requested()` decides
//!   through `close_decision` — an exit does NOT cancel, eframe shuts down,
//!   `run_native` returns, the process exits 0. X-close still hides only.
//! - `NativeOptions::viewport` (`ViewportBuilder`): `with_title`,
//!   `with_inner_size` (720×520 per §4.4), `with_icon(Arc<IconData>)` —
//!   attached at creation, since Windows captures `ICON_BIG` there (eframe's
//!   `AppTitleIconSetter`) — plus the dialog chrome of §4.4:
//!   `with_minimize_button(false)` + `with_maximize_button(false)`. Close and
//!   resize stay enabled (bordered, resizable window).
//! - Widgets: `ComboBox::from_label/from_id_salt(...).selected_text(...).show_ui`,
//!   `ui.selectable_value`, `ui.checkbox`, `ui.text_edit_singleline`,
//!   `DragValue::range`, `ui.add_enabled(false, ...)` for disabled rows.
//! - General tab placeholder reference (spec §4.2, Stage 6): an
//!   `egui::CollapsingHeader` right under the Window template field, rows
//!   derived from `template::TOKENS` through [`placeholder_reference_lines`].
//!   egui 0.37 has NO `CollapsibleGroup` (verified in the locked source: the
//!   collapsible widget is `src/containers/collapsing_header.rs`,
//!   `CollapsingHeader::new(text)` + `default_open(bool)` +
//!   `show(ui, |ui| ...)`), so the collapsing header IS the accordion; it stays
//!   collapsed by default because it is a reference, not a control.
//! - General tab Browse button (spec §4.4, Stage 14): the `rfd` blocking
//!   folder picker (`FileDialog::new().set_directory(..).pick_folder()`)
//!   started from `browse_start_dir`; cancel or a missing backend is `None` →
//!   WARN log, draft unchanged, app stays alive (see `browse_save_path`).
//! - Overlay tab test chip (spec §4.3, Stage 7): a `Test overlay` button right
//!   of the "Show overlay toast" checkbox, disabled while the checkbox is off
//!   (a disabled button needs the gate HERE — `overlay.enabled` is only
//!   consulted by `capture::should_show`, and the test chip publishes directly
//!   through the hand-off handle). The click builds a success chip from the
//!   CURRENT draft ([`test_overlay_toast`]) with the real save wording
//!   ([`overlay_test_message`] + `capture::saved_text`) — no capture, no file,
//!   no config write, no hotkey re-registration.
//! - Hotkeys rows (spec §4.4 v5.0): the bind field is a `Button` whose label is
//!   the current binding (`binding_label`), and registration status is an icon
//!   with a tooltip instead of inline text. Verified in the locked egui 0.37
//!   source: `Button::new(atoms: impl IntoAtoms)` accepts any `Into<WidgetText>`
//!   (`&str`, `String`, `RichText`) and sizes via `min_size(Vec2)` (there is no
//!   `min_width`); `Response::on_hover_text(self, text) -> Self` consumes the
//!   response; and its tooltip is NOT shown on a disabled widget — the disabled
//!   rect row needs `Response::on_disabled_hover_text` instead.
//! - Hotkeys alignment (spec §4.4, Stage 2): the tab is ONE `egui::Grid`
//!   (`Grid::new(id_salt).num_columns(3).show(...)`, `src/grid.rs`) so the
//!   column widths are shared across rows — a grid per row sizes its own
//!   columns, which is why the rows never lined up. egui 0.37 has NO `Table`
//!   widget (verified: no table module in the locked source), so `Grid` is the
//!   alignment tool. Each nested scope (`ui.horizontal`, `ui.add_enabled_ui`)
//!   counts as ONE cell (`Placer::advance` advances per allocation). The status
//!   icon is painted BEFORE the action name, so it sits left of the bind button.
//! - Status icons: with our eframe features (`default_fonts`, no
//!   `monochrome_emoji_fonts`) the bundled fonts are Ubuntu-Light + Hack +
//!   egui-icons (5 logo glyphs only), and none of them contains `⚠`/`ℹ`
//!   (checked the cmaps). The warning icon is therefore the embedded
//!   triangle texture (`icons::WARNING_ICON_PNG`, decoded via
//!   `icons::rgba` — egui 0.37 core has no PNG loader, same shape as
//!   `crates/overlay/src/content.rs`), uploaded once at window creation;
//!   a decode failure logs and the row falls back to the colored ASCII
//!   `!`. The info icon stays a colored ASCII `i` via `RichText::color`.
//! - Bind-by-press reads `ui.input(|i| i.events())` `Event::Key` presses
//!   (spec §4.4: click field → capture mode); Escape cancels capture mode;
//!   Backspace clears the keyboard half of the row (key+mods emptied, pad kept)
//!   and ends capture; unsupported names are ignored. While capturing, live
//!   hotkey registrations
//!   are paused via `hotkeys::HotkeysController::pause`/`restore` (Stage 18,
//!   spec §4.4 v5.5) so a press of a live combo reaches this window instead of
//!   firing the screenshot action; restore re-registers the APPLIED settings.
//! - Pad-field bind-by-press (spec §4.1 + §4.4, Stage 5): clicking the pad field
//!   starts capture for that row; the pump suppresses pad shots while ANY field
//!   is capturing and commits the FIRST DOWN event into `gamepad::PadCapture`,
//!   so `logic` drains the mailbox into the draft notation — the GUI never
//!   receives a callback, only the mailbox. The notation IS the human-readable
//!   name: the device-info label of the pressed button (`XboxX`), or the raw
//!   `unknown:<N>` fallback when the device reports no label (v0 devices, §9);
//!   system buttons keep `Guide` / `Share`. The pad button shows the stored
//!   notation verbatim, so the field reads back exactly what config will hold;
//!   no separate label hint exists. On a host without GameInput the field
//!   renders as a disabled placeholder (Linux gilrs is out of scope).
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

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use overlay::Corner;
use overlay::OverlayHost;
use toast::MonitorSel;
use toast::Toast;
use toast::ToastColor;
use toast::ToastService;

use crate::bindings::Action;
use crate::capture;
use crate::capture::ToastHandoff;
use crate::config;
use crate::config::Binding;
use crate::config::Format;
use crate::config::Overlay;
use crate::config::OverlayPosition;
use crate::config::Settings;
use crate::gamepad::CaptureField;
use crate::gamepad::PadCaptureMailbox;
use crate::gamepad::PadDownEvent;
use crate::gamepad::active_capture;
use crate::gamepad::pad_notation;
use crate::gamepad::set_capture_mode;
use crate::gamepad::take_captured;
use crate::hotkeys::HotkeysController;
use crate::hotkeys::ProbePress;
use crate::hotkeys::Registration;
use crate::hotkeys::key_label;
use crate::template::TOKENS;
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

/// Placeholder reference rows for the General tab accordion (spec §4.2): one
/// `(token, description)` line per `template::TOKENS` entry, token spelled with
/// its `%`. The table is the single source of truth — it is the very list
/// `template::render` validates against — so the UI states exactly the tokens a
/// template may use and cannot drift from the renderer. Rows are rebuilt each
/// UI frame: 11 short strings, well under the §5 budget.
pub(crate) fn placeholder_reference_lines() -> Vec<(String, &'static str)> {
    TOKENS
        .iter()
        .map(|(name, description)| (format!("%{name}"), *description))
        .collect()
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

/// Honest filename for the Overlay tab's test chip (Stage 7): no capture and
/// no save happen, so the chip must not name a real file — the fixed name
/// says what it is a test of, matching the spec §4.3 success wording
/// (`Saved: <file name>`).
pub(crate) const OVERLAY_TEST_FILENAME: &str = "test.png";

/// Hover hint for the disabled test button (English UI, spec §4.4). The
/// button is gated here in the GUI because a direct `ToastHandle::show` does
/// not consult `overlay.enabled` — that gate lives in `capture::should_show`
/// and only applies to capture-driven chips.
pub(crate) const OVERLAY_TEST_DISABLED_HINT: &str = "Enable the overlay toast to test it";

/// Test-button gate (Stage 7): enabled exactly while the overlay toast
/// checkbox is checked.
pub(crate) fn overlay_test_button_enabled(enabled: bool) -> bool {
    enabled
}

/// Test chip text: the real save success wording (spec §4.3), built by the
/// very helper `capture::toast_from_outcome` uses for a real save, so the
/// button cannot drift from the `Saved: <file name>` format.
pub(crate) fn overlay_test_message(filename: &str) -> String {
    capture::saved_text(Path::new(filename))
}

/// The chip the Overlay tab publishes on a test click, built from the CURRENT
/// draft (spec §4.3): corner, monitor selection, and clamped hold mirror the
/// rules `capture::toast_from_outcome` applies to `config::Overlay` (those
/// helpers are private there — duplicated rather than widening capture's
/// interface, per the stage plan). Success color, text-only: no bytes and no
/// path, so nothing touches disk. The chip shows what the user just
/// configured, even while the draft is unsaved — that is the point of a test.
pub(crate) fn test_overlay_toast(overlay: &Overlay) -> Toast {
    Toast {
        monitor: if overlay.on_all_monitors {
            MonitorSel::All
        } else {
            MonitorSel::Index(0)
        },
        text: overlay_test_message(OVERLAY_TEST_FILENAME),
        image: None,
        color: ToastColor::Success,
        corner: match overlay.position {
            OverlayPosition::TopLeft => Corner::TopLeft,
            OverlayPosition::TopRight => Corner::TopRight,
            OverlayPosition::BottomLeft => Corner::BottomLeft,
            OverlayPosition::BottomRight => Corner::BottomRight,
        },
        duration_ms: u32::try_from(overlay.duration_ms).unwrap_or(u32::MAX),
    }
}

/// Key names the binding notation supports and bind-by-press accepts — the
/// CAPTURABLE set: keys egui 0.37 delivers as `Event::Key` (`egui-winit
/// 0.37.0/src/lib.rs:1575-1675` for `NamedKey`, `:1677-1846` for `KeyCode`,
/// logical characters via `Key::from_name` at `:1567-1573`) that
/// `global-hotkey` can also register on Windows
/// (`platform_impl/windows/mod.rs:205-351`). Anything else is ignored while
/// capturing.
///
/// It is a subset of the notation `bindings::parse_key` accepts. Codes egui
/// never delivers — `MediaPlay`, `MediaPause`, `MediaSelect`, `Convert`,
/// `KanaMode`, `NonConvert`, `IntlRo`, `IntlYen`, and the `Numpad*` codes
/// (egui collapses the numpad into digits, `Enter`, `Plus`, `Minus`, `Slash`)
/// — are config-only: editable in `config.toml`, registered at startup, but
/// not bindable by press. Excluded from both sets: `Escape`/`Backspace`
/// (reserved for the capture UI — Esc cancels, Backspace clears), modifier
/// keys as a main key, `Fn`/`FnLock`/`Eject`/`Power`/`Clear`/`Copy`/`Cut`/
/// `Paste`, `F25`..`F35` (egui delivers them, the Windows table has no virtual
/// key), and egui logical punctuation with no physical code (`Colon`, `Pipe`,
/// `Questionmark`, `Exclamationmark`, `OpenCurlyBracket`, `CloseCurlyBracket`).
/// `CapsLock`/`NumLock` are bindable but excluded: registering them bare would
/// swallow the lock toggle system-wide. `ScrollLock` IS listed (Stage 14): it is
/// a member of the Windows bind-probe set, where a bare registration happens
/// only while a capture is open, so the row must be able to render the name the
/// probe recorded.
///
/// Bare navigation keys (`Insert`, `Delete`, `Home`, `End`, `PageUp`,
/// `PageDown`, arrows) are consumed system-wide once registered, and the
/// clipboard chords `Ctrl+Insert`/`Shift+Insert`/`Shift+Delete` arrive as
/// clipboard commands (`egui-winit lib.rs:1538-1554`) that are swallowed
/// before any `Event::Key` (`:1129-1152`) — they cannot be captured.
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
    "F13",
    "F14",
    "F15",
    "F16",
    "F17",
    "F18",
    "F19",
    "F20",
    "F21",
    "F22",
    "F23",
    "F24",
    "ArrowDown",
    "ArrowLeft",
    "ArrowRight",
    "ArrowUp",
    "Space",
    "Enter",
    "Tab",
    "Insert",
    "Delete",
    "Home",
    "End",
    "PageUp",
    "PageDown",
    "Comma",
    "Period",
    "Minus",
    "Equal",
    "Semicolon",
    "Slash",
    "Backslash",
    "Quote",
    "Backquote",
    "BracketLeft",
    "BracketRight",
    "IntlBackslash",
    "ContextMenu",
    "Pause",
    "ScrollLock",
    "Help",
    "Sleep",
    "BrowserBack",
    "BrowserFavorites",
    "BrowserForward",
    "BrowserHome",
    "BrowserRefresh",
    "BrowserSearch",
    "BrowserStop",
    "MediaPlayPause",
    "MediaStop",
    "MediaTrackNext",
    "MediaTrackPrevious",
    "AudioVolumeDown",
    "AudioVolumeUp",
    "AudioVolumeMute",
    "LaunchMail",
    "LaunchApp1",
    "LaunchApp2",
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
/// click of the same field again, starting a keyboard capture, or Escape (§4.4,
/// v2), which ends it without changing the row.
const PAD_CAPTURE_BUTTON_LABEL: &str = "Press a button…";

/// Tooltip while the pad field is capturing — the same affordances as the
/// shared [`BIND_HINT`] line (§4.4, v2): Escape cancels, Backspace clears the
/// pad slot; clicking the field again cancels too.
const PAD_CAPTURE_TOOLTIP: &str =
    "Press a gamepad button to bind it — Esc to cancel, Backspace to clear; click again to cancel.";

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

/// Always-visible one-line hint at the start of the bind form (§4.4, v8):
/// Escape cancels a capture without changing the row and Backspace clears the
/// binding — for keyboard capture the keyboard half (the pad slot stays), for
/// pad capture the pad slot (the keyboard half stays). Shown whether or not a
/// capture is active, so the keys are discoverable before anyone clicks a
/// field. The wording is field-agnostic, so one line covers both fields.
const BIND_HINT: &str = "Esc to cancel, Backspace to clear";

/// Bind-button label of a row whose keyboard binding was cleared by Backspace.
/// The empty `key` is the unbound marker — §4.1 notation has no empty key, so
/// the button cannot show a binding and says the row needs a press instead.
const KEY_UNBOUND_LABEL: &str = "unbound — click to bind";

/// Tooltip for a disabled Apply/OK while some row is unbound: saving a cleared
/// binding writes a key `bindings::parse_key` rejects (§4.1), which costs the
/// app its hotkey registrations and its pad triggers, so saving waits until
/// the row is bound again.
const UNBOUND_SAVE_TOOLTIP: &str =
    "Bind a key on every row before saving — Backspace clears a binding";

/// Tooltip for a disabled Apply/OK while two rows share a trigger (§4.4,
/// Stage 3): one keyboard combo or pad button may trigger only one action,
/// so saving waits until the draft is unique again.
const DUPLICATE_SAVE_TOOLTIP: &str = "Duplicate binding: one combo or pad button must trigger only one action — re-bind one of the rows";

/// Status-icon colors (warning = red-orange, info = blue).
const WARNING_COLOR: egui::Color32 = egui::Color32::from_rgb(246, 118, 100);
const INFO_COLOR: egui::Color32 = egui::Color32::from_rgb(118, 176, 240);

/// Painted size of the warning triangle in a status row, in logical points.
/// The SAME slot is reserved for the ASCII `!` fallback so the label after
/// the icon never jumps between the texture and the fallback rendering.
const WARNING_ICON_SIZE: f32 = 16.0;

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

/// The config name to write for an egui key, only when it is in
/// [`SUPPORTED_KEY_NAMES`] (so `bindings::parse_key` accepts it); `None` for a
/// reserved, unmappable, or non-registrable key — the capture loop ignores
/// those and stays in capture mode.
///
/// egui's human-readable names differ from the crate/`config.toml` names for
/// some keys, so those are mapped explicitly (`egui::Key::name()` gives
/// `Down`/`Left`/`Right`/`Up`, `OpenBracket`, `CloseBracket`, `Backtick`,
/// `Equals`, `Plus`, `Menu`). A logical `+` (Shift+`=`) maps to `Equal`: the
/// physical code Windows registers for that key is VK_OEM_PLUS = `Equal`, so
/// the binding stays `Shift+Equal`.
pub(crate) fn supported_key_name(key: egui::Key) -> Option<&'static str> {
    let name = match key {
        egui::Key::ArrowDown => "ArrowDown",
        egui::Key::ArrowLeft => "ArrowLeft",
        egui::Key::ArrowRight => "ArrowRight",
        egui::Key::ArrowUp => "ArrowUp",
        egui::Key::OpenBracket => "BracketLeft",
        egui::Key::CloseBracket => "BracketRight",
        egui::Key::Backtick => "Backquote",
        egui::Key::Equals | egui::Key::Plus => "Equal",
        egui::Key::Menu => "ContextMenu",
        other => other.name(),
    };
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

/// Binding built from a WINDOWS BIND-PROBE press (Stage 14, spec §4.4): the same
/// commit semantics as [`binding_from_press`] — keyboard fields REPLACE, the
/// reserved `pad` slot stays — but the key comes from the crate `Code` (Windows
/// never delivers these keys to egui, so there is no `egui::Key::name()` to use)
/// and the modifiers come from the fired combo's `Modifiers`, mapped through
/// [`mods_from_press`] so the notation keeps `binding_label`'s order. Returns
/// `None` when the probe key has no bindable name (`SUPPORTED_KEY_NAMES`): the
/// caller then leaves the row untouched — an invalid binding is never written.
pub(crate) fn probe_binding_from_press(binding: &Binding, press: &ProbePress) -> Option<Binding> {
    let key = key_label(press.code);
    if !SUPPORTED_KEY_NAMES.contains(&key.as_str()) {
        return None;
    }
    Some(binding_from_press(
        &key,
        mods_from_press(press.mods.alt(), press.mods.ctrl(), press.mods.shift()),
        binding.pad.clone(),
    ))
}

/// Keyboard half cleared by Backspace during capture (§4.4): the key becomes
/// the empty unbound marker and the modifiers are emptied; the `pad` slot is
/// left untouched — the same field split as [`binding_from_press`], which
/// replaces only the keyboard fields. The row is rewritten in place and the
/// cleared value is returned so the caller can log it (pad included).
pub(crate) fn clear_key_binding(binding: &mut Binding) -> Binding {
    binding.key.clear();
    binding.mods.clear();
    binding.clone()
}

/// Pad half cleared by Backspace during pad capture (§4.4, v2): the `pad` slot
/// becomes `None` (no pad trigger) and the keyboard fields are left untouched —
/// the mirror of [`clear_key_binding`], which clears only the keyboard fields.
/// The row is rewritten in place and the cleared value is returned so the
/// caller can log it (keyboard half included).
pub(crate) fn clear_pad_binding(binding: &mut Binding) -> Binding {
    binding.pad = None;
    binding.clone()
}

/// Label of a keyboard bind button (§4.4): the capture prompt while a capture
/// is waiting, the unbound prompt after Backspace cleared the key, otherwise
/// the binding itself (`binding_label`).
pub(crate) fn bind_button_label(capturing: bool, key: &str, mods: &[String]) -> String {
    if capturing {
        CAPTURE_BUTTON_LABEL.to_owned()
    } else if key.is_empty() {
        KEY_UNBOUND_LABEL.to_owned()
    } else {
        binding_label(key, mods)
    }
}

/// Whether the draft may be saved (§4.1): every action needs a keyboard key.
/// An empty key is the cleared/unbound marker and `bindings::parse_key`
/// rejects it, so saving such a draft would write a config the parser refuses
/// — the app would lose its hotkey registrations AND its pad triggers at
/// startup (`gamepad::pad_triggers_now` parses the same controls). Apply/OK
/// stay disabled until the row is bound again.
pub(crate) fn draft_savable(draft: &Settings) -> bool {
    [
        &draft.controls.full_screen,
        &draft.controls.window,
        &draft.controls.rect,
    ]
    .iter()
    .all(|binding| !binding.key.is_empty())
}

/// Which trigger collides between two hotkey rows (§4.4, Stage 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DuplicateKind {
    /// The keyboard combo (key + modifiers) is identical on both rows.
    Keyboard,
    /// The pad notation string is identical on both rows.
    Pad,
}

/// One collision between two hotkey rows: the row (`action`) that collides
/// with the other one (`other`) on a `kind` of trigger, plus an English
/// tooltip naming the collision. A colliding pair yields TWO entries — one
/// per affected row — so every duplicated row can show its own warning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DuplicateBinding {
    pub(crate) action: Action,
    pub(crate) other: Action,
    pub(crate) kind: DuplicateKind,
    pub(crate) description: String,
}

/// Canonical comparison form of a keyboard binding: the key lowercased and
/// the modifiers lowercased, deduplicated, and sorted, so
/// `Ctrl+Shift+PrintScreen` equals `Shift+CTRL+ctrl+printscreen`. Unknown
/// modifier names participate verbatim — a config reject never reaches a
/// save anyway (the GUI only writes parsed names).
fn normalized_combo(binding: &Binding) -> (String, Vec<String>) {
    let mut mods: Vec<String> = binding
        .mods
        .iter()
        .map(|name| name.to_lowercase())
        .collect();
    mods.sort();
    mods.dedup();
    (binding.key.to_lowercase(), mods)
}

/// Duplicate triggers in the draft (spec §4.4, Stage 3): the same keyboard
/// combo (compared via [`normalized_combo`]) or the same pad notation string
/// (compared case-insensitively, so different spellings of one canonical name —
/// `XboxX` vs `xboxx` — dedupe, while an `unknown:<N>` fallback and a label
/// name do not) on two actions. Only the two active rows are
/// compared: the reserved `rect` row never registers or fires (§4.1), so it
/// never collides. The empty `key` is the unbound marker (§4.4) and an
/// empty/absent `pad` means no pad trigger — unbound slots never collide,
/// even against each other.
pub(crate) fn duplicate_bindings(draft: &Settings) -> Vec<DuplicateBinding> {
    let full = &draft.controls.full_screen;
    let window = &draft.controls.window;
    let mut duplicates = Vec::new();

    if !full.key.is_empty()
        && !window.key.is_empty()
        && normalized_combo(full) == normalized_combo(window)
    {
        duplicates.push(keyboard_duplicate(Action::FullScreen, Action::Window, full));
        duplicates.push(keyboard_duplicate(
            Action::Window,
            Action::FullScreen,
            window,
        ));
    }

    if let (Some(full_pad), Some(window_pad)) = (&full.pad, &window.pad)
        && !full_pad.is_empty()
        && !window_pad.is_empty()
        && full_pad.eq_ignore_ascii_case(window_pad)
    {
        duplicates.push(pad_duplicate(Action::FullScreen, Action::Window, full_pad));
        duplicates.push(pad_duplicate(
            Action::Window,
            Action::FullScreen,
            window_pad,
        ));
    }

    duplicates
}

/// One keyboard-collision entry: names the other row and shows this row's
/// own combo in canonical label form (§4.1 notation), e.g.
/// "Duplicates Window: Alt+PrintScreen".
fn keyboard_duplicate(action: Action, other: Action, binding: &Binding) -> DuplicateBinding {
    DuplicateBinding {
        action,
        other,
        kind: DuplicateKind::Keyboard,
        description: format!(
            "Duplicates {}: {}",
            row_label(other),
            binding_label(&binding.key, &binding.mods)
        ),
    }
}

/// One pad-collision entry, same shape as [`keyboard_duplicate`] but naming
/// the pad notation this row carries.
fn pad_duplicate(action: Action, other: Action, pad: &str) -> DuplicateBinding {
    DuplicateBinding {
        action,
        other,
        kind: DuplicateKind::Pad,
        description: format!("Duplicates {} pad: {pad}", row_label(other)),
    }
}

/// Warning tooltip for the KEYBOARD control of one row (§4.4, Stage 10): the
/// icon belongs to the bind button, so only keyboard signals raise it — this
/// row's keyboard collision or its own registration failure. A pad collision
/// never warns the bind button, and the reserved rect row never collides (§4.1).
/// A draft collision outranks the registration status: the draft is what Apply
/// would write and a collision blocks Apply, so it is the actionable state.
/// `registration` is the reason of this row's failed registration
/// (`Registration::Failed`); `None` means the row registered or is reserved.
/// The `clone` is the reason the seam returns an owned `String`: the tooltip
/// goes to the widget, which owns its text.
pub(crate) fn keyboard_warning(
    draft: &Settings,
    action: Action,
    registration: Option<&str>,
) -> Option<String> {
    duplicate_bindings(draft)
        .iter()
        .find(|duplicate| duplicate.action == action && duplicate.kind == DuplicateKind::Keyboard)
        .map(|duplicate| duplicate.description.clone())
        .or_else(|| registration.map(str::to_owned))
}

/// Warning tooltip for the PAD control of one row (§4.4, Stage 10): the icon
/// belongs to the pad field, so only pad signals raise it — this row's pad
/// collision. A keyboard collision never warns the pad field, and the reserved
/// rect row never collides (§4.1). An unresolved pad name is deliberately NOT
/// reported here: resolving a name needs the connected devices' label tables
/// (`gamepad::resolve_pad_binds`, Windows-only), so that warning stays a log
/// line (`gamepad::warn_unresolved_bind`) — there is no cheap GUI signal for it.
pub(crate) fn pad_warning(draft: &Settings, action: Action) -> Option<String> {
    duplicate_bindings(draft)
        .iter()
        .find(|duplicate| duplicate.action == action && duplicate.kind == DuplicateKind::Pad)
        .map(|duplicate| duplicate.description.clone())
}

/// Focus decision for the failed-registration retry (spec §4.1 v2, Stage 13).
/// Retry ONLY on the transition INTO focus, ONLY while some row actually
/// failed, and NEVER while a keyboard capture is running (live combos must stay
/// paused so a press is a bind candidate, §4.4 v5.5).
///
/// Why focus: on Windows the startup registration runs before any window exists
/// (`main` registers, then `gui::run`), and Microsoft's `RegisterHotKey` docs
/// say OS default hotkeys (PrintScreen → Snipping Tool) may be overridden when
/// an app window is FOREGROUND — so gaining foreground is the one moment the
/// combo can be taken. On Linux the combo is held by another client's X11 grab
/// this app cannot displace, so the retry re-reports the same failure honestly.
///
/// Both arguments are `Option<bool>` because egui reports the ROOT viewport's
/// focus that way (`ViewportInfo::focused`, filled by egui-winit from winit's
/// `WindowEvent::Focused`). `None` ("unknown yet") counts as not focused, so the
/// first pass in which the window IS focused is itself a transition — exactly
/// the startup case. Staying focused never retries: the pass is once per
/// transition, not per frame. The signal is the settings window (the root
/// viewport); toast/overlay CHILD viewports never change it, and the hidden
/// window's logic pass sees `Some(false)` until the tray shows it again.
pub(crate) fn should_retry_failed_registrations(
    previously_focused: Option<bool>,
    focused: Option<bool>,
    has_failed: bool,
    capture_active: bool,
) -> bool {
    has_failed && !capture_active && focused == Some(true) && previously_focused != Some(true)
}

/// Merge a focus-retry report into the row statuses (spec §4.1 v2): the retry
/// covers only previously failed rows, so an entry replaces the row of the same
/// action and nothing else in the report is touched. An entry for an action the
/// report does not have yet is appended rather than dropped.
pub(crate) fn merge_registration_statuses(
    status: &mut Vec<Registration>,
    retried: Vec<Registration>,
) {
    for registration in retried {
        match status
            .iter_mut()
            .find(|existing| existing.action() == registration.action())
        {
            Some(existing) => *existing = registration,
            None => status.push(registration),
        }
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
    /// Backspace pressed while capturing: clear the keyboard half of the row
    /// and end capture (§4.4) — an end, like Escape/Commit, not a new binding.
    Clear,
}

/// Pure decision mapping for capture-press events (§4.4 v5.5): idle-click
/// pauses the live hotkeys; any capture-end (reclick, Escape, commit, clear)
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
        CaptureInput::Escape | CaptureInput::Commit | CaptureInput::Clear => {
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

/// What a keyboard key does to an ACTIVE pad capture (§4.4, v2): Escape cancels
/// it and leaves the row unchanged, Backspace clears the row's pad slot,
/// anything else is ignored (capture stays open waiting for a pad press). Both
/// ends behave like the keyboard capture's Escape/Backspace pair; the pad field
/// never binds a keyboard key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PadCaptureChange {
    /// Escape: end capture, write nothing to the row.
    Cancel,
    /// Backspace: clear the pad slot via [`clear_pad_binding`] and end capture.
    Clear,
    /// Any other key: keep capture open, write nothing.
    Ignore,
}

/// Decision — and the single mutation — for a key press while one row's pad
/// field is capturing (§4.4, v2). Only the Backspace case touches the row
/// ([`clear_pad_binding`]); Escape and any other key leave it exactly as it was,
/// so the cancel-vs-clear split is testable without an egui context.
pub(crate) fn pad_capture_action(
    binding: &mut Binding,
    pressed_key: egui::Key,
) -> PadCaptureChange {
    match pressed_key {
        egui::Key::Escape => PadCaptureChange::Cancel,
        egui::Key::Backspace => {
            clear_pad_binding(binding);
            PadCaptureChange::Clear
        }
        _ => PadCaptureChange::Ignore,
    }
}

/// Pure pad-capture drain decision (§4.4, Stage 5): the press the pump committed
/// lands on the row that was capturing, as config notation (`gamepad::pad_notation`
/// — the device-info button name, `XboxX`, or the raw `unknown:<N>` fallback when
/// the device reports no label; `Guide`/`Share` for a system button). A press
/// with no capture target (cancelled mid-press) is dropped: no row is rewritten.
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
/// The warning icon renders as the embedded triangle texture when it loaded;
/// this glyph is its fallback.
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
fn warning_icon_texture(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    warning_color_image().map(|image| {
        ctx.load_texture(
            "virshot-warning-icon",
            image,
            egui::TextureOptions::default(),
        )
    })
}

/// Whether the draft differs from the applied settings (Apply button hint;
/// closing without Apply discards the draft — spec §4.4).
pub(crate) fn is_dirty(draft: &Settings, applied: &Settings) -> bool {
    draft != applied
}

/// Enabled state of the bottom-row buttons (spec §4.4). Apply and OK both run
/// `apply()`, so they are enabled only while the draft differs from what is
/// applied AND `savable`. The caller composes `savable` from BOTH save gates:
/// every row has a keyboard key (`draft_savable` — an unbound row is the
/// Backspace-cleared marker §4.1 cannot parse) AND no triggers collide
/// (`duplicate_bindings` — §4.4 Stage 3: one combo or pad must trigger one
/// action). Cancel saves nothing — it only discards the draft — so it stays
/// enabled in every state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BottomButtons {
    pub(crate) apply_enabled: bool,
    pub(crate) ok_enabled: bool,
    pub(crate) cancel_enabled: bool,
}

pub(crate) fn bottom_buttons_enabled(dirty: bool, savable: bool) -> BottomButtons {
    let save = dirty && savable;
    BottomButtons {
        apply_enabled: save,
        ok_enabled: save,
        cancel_enabled: true,
    }
}

/// Id salt of the docked bottom panel holding the `Apply | OK | Cancel` row
/// (spec §4.4). The row lives in a real `egui::Panel::bottom` rather than as a
/// trailing row after the tab content, so it hugs the window's bottom edge
/// instead of floating above it.
pub(crate) const BOTTOM_BAR_PANEL_ID: &str = "settings_bottom_bar";

/// Draft after Cancel (spec §4.4): a dirty draft is thrown away and the applied
/// settings come back; a clean draft already equals them, so it is returned as
/// the copy the caller assigns. Neither argument is rewritten — the app keeps
/// its own `applied` for the next Apply, and nothing is saved or published.
pub(crate) fn cancel_draft(draft: &Settings, applied: &Settings) -> Settings {
    if is_dirty(draft, applied) {
        applied.clone()
    } else {
        draft.clone()
    }
}

/// What a window close request means: `Exit` shuts the process down,
/// `HideToTray` hides the window and keeps the process resident.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CloseAction {
    Exit,
    HideToTray,
}

/// Pure close decision (spec §4.4 + §4.5): the tray `Exit` action and the
/// OK/Cancel buttons are explicit exits, so their close request is not
/// cancelled. A plain window close (the X button, no exit flag) hides the
/// window to the tray — exit is tray-menu-only.
pub(crate) fn close_decision(exit_requested: bool, closed_by_button: bool) -> CloseAction {
    if exit_requested || closed_by_button {
        CloseAction::Exit
    } else {
        CloseAction::HideToTray
    }
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
    /// Warning status icon: the embedded triangle texture uploaded once in
    /// `run`'s create closure (`warning_icon_texture`). `None` when the asset
    /// decode failed (logged at WARN) — warning rows then paint the ASCII
    /// `!` glyph in the same reserved slot.
    warning_icon: Option<egui::TextureHandle>,
    /// Set by the tray `Exit` action only (spec §4.5): the next close request
    /// is not cancelled, so eframe shuts down.
    exit_requested: bool,
    /// Set by the OK/Cancel buttons only (spec §4.4): those are explicit
    /// decisions, so their close request exits instead of hiding to the tray.
    /// The plain X button leaves it `false` and keeps the hide-to-tray
    /// behaviour (§4.5).
    close_for_exit: bool,
    /// Last root-viewport focus seen by `logic` (`None` = no pass yet), so the
    /// Stage 13 retry fires on the transition INTO focus only — see
    /// [`should_retry_failed_registrations`].
    viewport_focused: Option<bool>,
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
            toast_service,
            overlay_host,
            handoff,
            tray,
            warning_icon: None,
            exit_requested: false,
            close_for_exit: false,
            viewport_focused: None,
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

    /// Icon for a row's registration status (spec §4.4): info on the reserved
    /// slot. A failed registration is NOT an icon here — it belongs to the
    /// bind button (§4.4, Stage 10), see [`keyboard_warning`].
    fn status_icon_for(&self, action: Action) -> Option<StatusIcon> {
        self.status
            .iter()
            .find(|registration| registration.action() == action)
            .and_then(icon_for_status)
    }

    /// The registration failure reason of one row (§4.4, Stage 10): only
    /// `Registration::Failed` is a keyboard warning. `Registered` is healthy and
    /// `Reserved` is an info note in column 1 (§4.1), so neither warns the bind
    /// button.
    fn registration_failure(&self, action: Action) -> Option<&str> {
        self.status
            .iter()
            .find(|registration| registration.action() == action)
            .and_then(|registration| match registration {
                Registration::Failed { reason, .. } => Some(reason.as_str()),
                Registration::Registered { .. } | Registration::Reserved { .. } => None,
            })
    }

    /// Does ANY row carry a failed registration? That is the precondition for
    /// the Stage 13 focus retry — with a healthy report there is nothing to
    /// retry and the controller pass is not called at all.
    fn has_failed_registration(&self) -> bool {
        self.status
            .iter()
            .any(|registration| matches!(registration, Registration::Failed { .. }))
    }

    /// Reserve the status-icon slot immediately LEFT of a control and paint the
    /// warning there when that control has one (§4.4, Stage 10). The slot is
    /// allocated in every row — textured, ASCII `!` fallback, or empty — so the
    /// controls of a column stay aligned and the layout never jumps between the
    /// texture and the fallback. The embedded triangle texture is used when it
    /// loaded; otherwise the ASCII glyph is painted into the SAME slot (egui's
    /// bundled fonts have no ⚠, see [`icon_glyph`]). The tooltip stays on the
    /// icon: warnings only appear on active rows, where `on_hover_text` works.
    fn paint_warning_slot(&self, ui: &mut egui::Ui, tooltip: Option<&str>) {
        let (rect, response) =
            ui.allocate_exact_size(egui::Vec2::splat(WARNING_ICON_SIZE), egui::Sense::hover());
        let Some(tooltip) = tooltip else {
            return;
        };
        match &self.warning_icon {
            Some(texture) => {
                ui.painter().image(
                    texture.id(),
                    rect,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            None => {
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    icon_glyph(IconKind::Warning),
                    egui::FontId::default(),
                    icon_color(IconKind::Warning),
                );
            }
        }
        response.on_hover_text(tooltip);
    }

    /// Apply: save `config.toml` and re-register hotkeys live (spec §4.4).
    /// A draft with duplicate triggers is refused before anything else happens
    /// (§4.4, Stage 3): the bottom buttons already disable Apply/OK for it and
    /// `config::save` validates nothing, so this guard is defense in depth — a
    /// refused Apply logs the collisions and writes, publishes, and
    /// re-registers nothing. Other failures are logged; the app keeps running.
    fn apply(&mut self) {
        let duplicates = duplicate_bindings(&self.draft);
        if !duplicates.is_empty() {
            let summary = duplicates
                .iter()
                .map(|duplicate| duplicate.description.clone())
                .collect::<Vec<String>>()
                .join(", ");
            tracing::warn!("Apply refused: duplicate bindings — {summary}");
            return;
        }
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

    /// End pad capture (§4.4, v2): the mailbox returns to idle, so pad triggers
    /// fire again. Reached by clicking the field again, by starting a keyboard
    /// capture, and by Escape — none of which writes to the draft.
    fn cancel_pad_capture(&mut self) {
        self.capture_pad = None;
        set_capture_mode(&self.pad_capture, None);
    }

    /// Cancel any active bind capture — keyboard or pad — when the window
    /// closes via OK/Cancel (§4.4): a keyboard capture had paused the live
    /// registrations, so they are restored, and the shared pad mailbox returns
    /// to idle so pad triggers fire again. Nothing is committed to the draft:
    /// the caller discards it (`cancel_draft`).
    fn cancel_active_capture(&mut self) {
        let restoring = capture_transition(self.capture_press.is_some(), CaptureInput::Escape);
        // `set_keyboard_capture(None)` clears both fields — the keyboard
        // capture and the pad capture the mailbox carries.
        self.set_keyboard_capture(None);
        if let Some(transition) = restoring {
            self.apply_capture_transition(transition);
        }
    }

    /// Close the window for real (spec §4.4): OK and Cancel are explicit
    /// decisions, not a hide-to-tray. The flag is what `logic` reads — the
    /// close request lands on the next pass, and with it set the request is
    /// not cancelled, so eframe shuts down (the same exit path as the tray
    /// `Exit`, §4.5).
    fn request_window_exit(&mut self, ctx: &egui::Context) {
        self.close_for_exit = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        ctx.request_repaint();
    }

    /// Drain the press the pump committed into the draft (§4.4, Stage 5): the
    /// notation goes to the row that was capturing and capture state ends. The
    /// notation IS what the field shows and what config holds — the device-info
    /// label name (`XboxX`) or the raw `unknown:<N>` fallback (§9), so no
    /// separate per-row hint is kept. The draft is NOT
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
        tracing::info!("Pad bind set in draft: {} → {notation}", action.label());
    }

    /// Drive the hotkey pause/restore transition on the controller (Stage 18).
    /// The controller owns the manager created on the MAIN thread, and `ui`/
    /// `logic` passes run on that same thread (winit/eframe pump), so this
    /// satisfies the WM_HOTKEY thread affinity. No controller (host without
    /// hotkeys) → no-op.
    ///
    /// The bind probe (Stage 14) rides the same transition: it is registered
    /// when a capture opens and unregistered when it ends, so the temporary
    /// GrabPrinter-style hook — and with it the Snipping Tool block — only ever
    /// exists while a row is capturing. `Restore` stops the probes BEFORE
    /// restoring: a bare probe shares its crate id with a bare action binding of
    /// the same key, so a restore first would collide with our own registration.
    fn apply_capture_transition(&mut self, transition: CaptureTransition) {
        let Some(controller) = &mut self.controller else {
            return;
        };
        match transition {
            CaptureTransition::Pause => {
                controller.pause();
                controller.start_bind_probe();
            }
            CaptureTransition::Restore => {
                controller.stop_bind_probe();
                controller.restore();
            }
        }
    }

    /// Bind-by-press (spec §4.4): clicking the bind button records the next
    /// supported key press, REPLACING the draft binding's key+mods. Escape (or
    /// clicking the button again) cancels without changing the row; Backspace
    /// clears the keyboard half (key+mods emptied — the pad slot stays) and
    /// ends capture; unsupported keys are ignored and capture mode stays on.
    /// winit reports Alt/Ctrl/Shift as event modifiers;
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
                // Backspace clears the keyboard half of the row BEFORE any key
                // is committed (§4.4): it is a capture-END, like Escape, so the
                // paused registrations come back and capture exits. `Backspace`
                // is not in `SUPPORTED_KEY_NAMES`, so it can never become a
                // binding itself (reserved key, §4.1). The pad slot stays.
                if key == egui::Key::Backspace {
                    let cleared = clear_key_binding(self.binding_mut(action));
                    tracing::info!(
                        "Binding cleared for {}: key and mods dropped, pad {}",
                        action.label(),
                        cleared.pad.as_deref().unwrap_or("none")
                    );
                    self.set_keyboard_capture(None);
                    if let Some(transition) = capture_transition(true, CaptureInput::Clear) {
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

    /// Commit a recorded bind-probe press into the capturing row (Stage 14,
    /// spec §4.4) — the same semantics as an egui press in
    /// [`handle_capture_press`]: keyboard fields replace, `pad` stays, capture
    /// ends, and the transition restores (probes stopped first). The press comes
    /// from the listener thread, so this runs on the LOGIC pass: the row is
    /// written even while the settings window is hidden. A press with no keyboard
    /// capture open is dropped (DEBUG) — the probe only exists while a capture is
    /// active, so a stray press must never write a row; an unbindable probe key
    /// is dropped too, so no invalid binding is ever written.
    fn commit_probe_press(&mut self, press: ProbePress) {
        let Some(action) = self.capture_press else {
            tracing::debug!(
                "Bind probe press dropped: {} is not capturing",
                key_label(press.code)
            );
            return;
        };
        let Some(binding) = probe_binding_from_press(self.binding_mut(action), &press) else {
            tracing::debug!(
                "Bind probe press dropped: {} has no bindable name",
                key_label(press.code)
            );
            return;
        };
        tracing::info!(
            "Probe bind set for {}: {}",
            action.label(),
            binding_label(&binding.key, &binding.mods)
        );
        *self.binding_mut(action) = binding;
        self.set_keyboard_capture(None);
        if let Some(transition) = capture_transition(true, CaptureInput::Commit) {
            self.apply_capture_transition(transition);
        }
    }

    /// Pad bind-by-press keys (spec §4.4, v2): while one row's pad field is
    /// capturing, Escape ENDS the capture and writes nothing to the row, and
    /// Backspace clears that row's pad slot — the keyboard half stays, the
    /// mirror of [`clear_key_binding`] in [`handle_capture_press`]. Any other
    /// key is ignored and capture stays open: the pad slot is written only by
    /// the pump (`drain_pad_capture`), never by a keyboard press. Pad capture
    /// never paused the live hotkey registrations, so ending it needs no
    /// restore — the shared mailbox simply returns to idle
    /// ([`cancel_pad_capture`]), and pad triggers fire again.
    fn handle_pad_capture_press(&mut self, ui: &egui::Ui) {
        let Some(action) = self.capture_pad else {
            return;
        };
        for event in ui.input(|i| i.events.clone()) {
            if let egui::Event::Key {
                key,
                pressed: true,
                repeat: false,
                ..
            } = event
            {
                let change = pad_capture_action(self.binding_mut(action), key);
                match change {
                    PadCaptureChange::Ignore => continue, // keep waiting for a pad press
                    PadCaptureChange::Cancel => self.cancel_pad_capture(),
                    PadCaptureChange::Clear => {
                        let binding = self.binding(action);
                        tracing::info!(
                            "Pad binding cleared for {}: pad dropped, keyboard {}",
                            action.label(),
                            binding_label(&binding.key, &binding.mods)
                        );
                        self.cancel_pad_capture();
                    }
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

        // Placeholder reference (spec §4.2, Stage 6): the token table rendered
        // straight from `template::TOKENS`, so the tab states exactly what
        // `template::render` accepts and never duplicates the list. Collapsed
        // by default (a reference, not a control); it is part of the tab's
        // vertical layout — no extra frame, no scrolling.
        egui::CollapsingHeader::new("Placeholder reference")
            .id_salt("template_placeholder_reference")
            .default_open(false)
            .show(ui, |ui| {
                for (token, description) in placeholder_reference_lines() {
                    ui.horizontal(|ui| {
                        ui.monospace(token);
                        ui.label(format!("— {description}"));
                    });
                }
            });

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

    /// Hotkeys tab (spec §4.4 v5.0 + Stage 2 + Stage 10): ONE `egui::Grid` for
    /// the whole tab so the columns line up across rows — action name | keyboard
    /// bind button | gamepad slot. egui 0.37 has no `Table` widget (verified in
    /// the locked source: no table module, only `grid.rs`), so `Grid` is the
    /// alignment tool; a per-row grid sizes its own columns, which is why the
    /// old rows did not line up. A status icon belongs to the CONTROL it
    /// describes (§4.4, Stage 10): a keyboard warning is painted immediately
    /// LEFT of the bind button in column 2, a pad warning immediately LEFT of
    /// the pad field in column 3, and each is driven only by its own kind of
    /// problem ([`keyboard_warning`] vs [`pad_warning`]). Column 1 keeps just
    /// the action name; the reserved rect row has no bind button or pad field to
    /// attach an icon to, so its Reserved info icon stays there (§4.1). The icon
    /// slot is allocated in every row — empty when there is no warning — so the
    /// controls in a column stay aligned. One hint line at the form start states
    /// what Esc and Backspace do (§4.4, v2: both fields cancel on Esc and clear
    /// on Backspace); since v8 it is painted on every pass — capture active or
    /// not — so the affordance is discoverable before anyone starts a capture.
    /// No key
    /// dropdown, no modifier checkboxes: the button shows the binding and click
    /// → bind-by-press.
    fn hotkeys_tab(&mut self, ui: &mut egui::Ui) {
        // Always shown (§4.4, v8): the capture state does not gate it.
        ui.label(BIND_HINT);

        egui::Grid::new("hotkeys_grid")
            .num_columns(3)
            .show(ui, |ui| {
                for action in HOTKEY_ROWS {
                    let reserved = action == Action::Rect;
                    let capturing = self.capture_press == Some(action);

                    // Owned before the cell closures so the row can still
                    // toggle `capture_press` while rendering.
                    let label = bind_button_label(
                        capturing,
                        &self.binding(action).key,
                        &self.binding(action).mods,
                    );
                    // Per-control warnings (§4.4, Stage 10): keyboard signals
                    // warn the bind button, pad signals warn the pad field.
                    // Both are owned before the closures, which mutate `self`.
                    let keyboard =
                        keyboard_warning(&self.draft, action, self.registration_failure(action));
                    let pad = pad_warning(&self.draft, action);

                    // Column 1: the action name only. The reserved rect row
                    // renders disabled, which greys the label too; its Reserved
                    // info icon stays here because that row has no bind button
                    // or pad field of its own (§4.1) — the tooltip is what
                    // explains the greyed row.
                    ui.add_enabled_ui(!reserved, |ui| {
                        ui.horizontal(|ui| {
                            if reserved && let Some(icon) = self.status_icon_for(action) {
                                let response = ui.label(
                                    egui::RichText::new(icon_glyph(icon.kind))
                                        .color(icon_color(icon.kind))
                                        .strong(),
                                );
                                // `on_hover_text` is silent on disabled
                                // widgets (egui 0.37), and the reserved rect
                                // row renders disabled.
                                response.on_disabled_hover_text(icon.tooltip);
                            }
                            ui.label(row_label(action));
                        });
                    });

                    // Column 2: the keyboard bind button (§4.4), with this row's
                    // keyboard warning immediately to its LEFT (§4.4, Stage 10).
                    // Disabled on the reserved row, so `Rect` never enters
                    // capture — and therefore Backspace can never clear it; a
                    // reserved row also has no keyboard warning to paint.
                    ui.add_enabled_ui(!reserved, |ui| {
                        ui.horizontal(|ui| {
                            self.paint_warning_slot(ui, keyboard.as_deref());
                            let button = egui::Button::new(label)
                                .min_size(egui::Vec2::new(BIND_BUTTON_MIN_WIDTH, 0.0));
                            let response = if capturing {
                                ui.add(button).on_hover_text(CAPTURE_TOOLTIP)
                            } else {
                                ui.add(button)
                            };
                            // `on_hover_text` is silent on disabled widgets (egui
                            // 0.37), and both tooltip methods consume the
                            // response, so the result is rebound, not cloned.
                            let response = if reserved {
                                response.on_disabled_hover_text(RESERVED_TOOLTIP)
                            } else {
                                response
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
                        });
                    });

                    // Column 3: pad field (§4.1 + §4.4, Stage 5) with this row's
                    // pad warning immediately to its LEFT (§4.4, Stage 10): pad
                    // signals only, so a keyboard collision never marks this
                    // cell. On Windows the field is a real bind-by-press button —
                    // click starts capture, the pump commits the first DOWN event
                    // into the mailbox and `logic` writes the notation into the
                    // draft; a click while capturing cancels. On a host without
                    // GameInput it stays a disabled placeholder (Linux gilrs is
                    // out of scope) — the pad warning icon still renders next to
                    // it, because the icon itself is not disabled. Inside
                    // `ui.add_enabled_ui(!reserved)` the button is disabled on
                    // the rect row, so a reserved action never gets a pad bind
                    // from the UI either (and it never collides, §4.1).
                    ui.add_enabled_ui(!reserved, |ui| {
                        ui.horizontal(|ui| {
                            self.paint_warning_slot(ui, pad.as_deref());
                            let pad_capturing = self.capture_pad == Some(action);
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
                                // Same disabled-tooltip rule as the bind button:
                                // the tooltip methods consume the response.
                                let pad_response = if reserved {
                                    pad_response.on_disabled_hover_text(RESERVED_TOOLTIP)
                                } else {
                                    pad_response
                                };
                                if pad_response.clicked() {
                                    match pad_capture_toggle(pad_capturing, action) {
                                        Some(start) => self.start_pad_capture(start),
                                        None => self.cancel_pad_capture(),
                                    }
                                }
                            } else {
                                ui.add_enabled(false, egui::Button::new(GAMEPAD_PLACEHOLDER_LABEL));
                            }
                        });
                    });

                    ui.end_row();
                }
            });

        if self.controller.is_none() {
            ui.label("Hotkeys unavailable on this host (see log)");
        }
    }

    fn overlay_tab(&mut self, ui: &mut egui::Ui) {
        let overlay = &mut self.draft.overlay;
        // The test chip is published AFTER the tab body: the tab borrows
        // `draft.overlay` mutably and publishing reads `self.handoff` — the
        // same deferred-action pattern as Browse in the General tab.
        let mut test_chip: Option<Toast> = None;

        ui.horizontal(|ui| {
            ui.checkbox(&mut overlay.enabled, "Show overlay toast");
            let enabled = overlay_test_button_enabled(overlay.enabled);
            let test = ui.add_enabled(enabled, egui::Button::new("Test overlay"));
            if enabled {
                if test.clicked() {
                    test_chip = Some(test_overlay_toast(overlay));
                }
            } else {
                // `on_disabled_hover_text` consumes the response; a disabled
                // button can never be clicked, so the branches are exclusive.
                test.on_disabled_hover_text(OVERLAY_TEST_DISABLED_HINT);
            }
        });

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

        if let Some(chip) = test_chip {
            self.publish_overlay_test_chip(chip);
        }
    }

    /// Publish the Overlay tab's test chip through the SAME hand-off slot
    /// real saves use (spec §4.3): the handle never blocks, and the chip
    /// spawns on the next `overlay_host.update` pass in `logic`. No capture,
    /// no save, no config write, no hotkey re-registration — only the chip
    /// and one INFO line. A missing handle (no toast service) or a closed
    /// channel is WARNed, never fatal (spec §5).
    fn publish_overlay_test_chip(&self, chip: Toast) {
        let Some(handle) = capture::handoff_handle(&self.handoff) else {
            tracing::warn!("Overlay test chip skipped: no toast handle attached");
            return;
        };
        let text = chip.text.clone(); // `show` consumes the chip; the log quotes it
        if let Err(err) = handle.show(chip) {
            tracing::warn!("Overlay test chip failed: {err}");
            return;
        }
        tracing::info!("Overlay test toast shown: {text} (no file written)");
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
            match close_decision(self.exit_requested, self.close_for_exit) {
                CloseAction::Exit => {
                    // Tray `Exit` or an OK/Cancel button: no CancelClose this
                    // pass → eframe shuts down, `run_native` returns, the
                    // process exits 0.
                    tracing::info!(
                        "virshot exiting: {}",
                        if self.exit_requested {
                            "tray menu"
                        } else {
                            "settings window button"
                        }
                    );
                }
                CloseAction::HideToTray => {
                    // Plain window close (the X button): hide to the tray and
                    // discard the draft — the process stays resident (§4.5).
                    ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                    self.draft = self.applied.clone();
                }
            }
        }
        // Pad-capture mailbox (Stage 5): the pump committed a press on its own
        // thread; the newest-only drain writes it into the draft here, so a pad
        // press is recorded even while the settings window is hidden.
        self.drain_pad_capture();

        // Bind-probe press (Stage 14): Windows delivers PrintScreen/ScrollLock/
        // Pause to NO app key channel, so the temporary probe registration is the
        // only signal for those keys while a row is capturing. The listener
        // recorded it on its own thread; this latest-only drain commits it into
        // the draft here — same shape as the pad mailbox above, and it runs on
        // the logic-only pass too, so the row is written while hidden.
        if let Some(controller) = &mut self.controller
            && let Some(press) = controller.take_probe_press()
        {
            self.commit_probe_press(press);
        }

        // Toast chips (spec §4.3): the crate owns the chip lifetimes
        // (`toast::ToastService`); this line only drives the viewport backend —
        // queued overlays become child egui viewports HERE, so chips spawn,
        // paint, and fade on every pass, including the logic-only pass eframe
        // runs while the settings window is hidden.
        self.overlay_host.update(ctx);

        // Failed hotkeys are retried when the settings window GAINS focus
        // (spec §4.1 v2, Stage 13): on Windows an OS default hotkey may be
        // taken once this app is foreground, on Linux the combo is held by
        // another client and the retry just re-reports it. The signal is the
        // root viewport's focus; see [`should_retry_failed_registrations`] for
        // why that is the settings window and why `None` counts as unfocused.
        let focused = ctx.input(|i| i.viewport().focused);
        if should_retry_failed_registrations(
            self.viewport_focused,
            focused,
            self.has_failed_registration(),
            self.capture_press.is_some(),
        ) && let Some(controller) = &mut self.controller
        {
            merge_registration_statuses(&mut self.status, controller.retry_failed_registrations());
        }
        self.viewport_focused = focused;

        // Tray events arrive on crossbeam channels and never wake the winit
        // loop; schedule the next logic pass so polling stays alive while the
        // window is hidden. The toast service wakes the same context on its
        // fade ticks.
        ctx.request_repaint_after(TRAY_POLL_TICK);
    }

    /// Alpha-0 clear so transparent windows show through (eframe's glow backend
    /// uses ONE clear color for all viewports — including the toast child
    /// viewports the overlay host creates). The settings window stays fully
    /// opaque because both panels it uses — the docked bottom bar and the
    /// `CentralPanel` — paint their panel fill.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Color32::TRANSPARENT.to_normalized_gamma_f32()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.handle_capture_press(ui);
        // Pad capture keys (§4.4, v2) run in the same pass, before the tabs are
        // painted, so a cleared pad slot shows in this frame. The two captures
        // are mutually exclusive (`set_keyboard_capture` clears both), so one
        // key press can never be handled twice.
        self.handle_pad_capture_press(ui);

        // Bottom row (spec §4.4): `Apply | OK | Cancel` docked to the window's
        // BOTTOM EDGE as a real bottom panel, with the "Unsaved changes"
        // indicator on the left of the same row. Docking is what keeps the row
        // visible when the tab content grows (the placeholder accordion); the
        // content is the `CentralPanel` below, which shrinks instead.
        // Save (Apply/OK) is gated by TWO draft checks (§4.4): every row needs
        // a key (`draft_savable` — Stage 2) AND the triggers must be unique
        // (`duplicate_bindings` — Stage 3, one combo or pad must trigger one
        // action); Cancel discards, so it stays enabled. The disabled-button
        // tooltip names whichever save gate blocks.
        // `right_to_left` lays widgets out from the right edge, so they are
        // added rightmost-first: Cancel, OK, Apply. Clicks are collected in the
        // layout pass and acted on after it, so the draft borrow ends before
        // anything mutates it.
        // egui reserves panels outermost-first and the `CentralPanel` must be
        // added last, so the bottom panel comes before the tab content.
        egui::Panel::bottom(BOTTOM_BAR_PANEL_ID).show(ui, |ui| {
            let unbound = !draft_savable(&self.draft);
            let duplicates = duplicate_bindings(&self.draft);
            let savable = !unbound && duplicates.is_empty();
            let buttons = bottom_buttons_enabled(is_dirty(&self.draft, &self.applied), savable);
            let blocked_tooltip = if unbound {
                Some(UNBOUND_SAVE_TOOLTIP)
            } else if !duplicates.is_empty() {
                Some(DUPLICATE_SAVE_TOOLTIP)
            } else {
                None
            };
            let mut apply_clicked = false;
            let mut ok_clicked = false;
            let mut cancel_clicked = false;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                cancel_clicked = ui
                    .add_enabled(buttons.cancel_enabled, egui::Button::new("Cancel"))
                    .clicked();
                let ok_response = ui.add_enabled(buttons.ok_enabled, egui::Button::new("OK"));
                let apply_response =
                    ui.add_enabled(buttons.apply_enabled, egui::Button::new("Apply"));
                // Only a blocked save gets a hint: a clean draft is disabled
                // for dirtiness alone and needs no tooltip. The tooltip method
                // consumes the response, so it is rebound.
                let ok_response = if let Some(tooltip) = blocked_tooltip {
                    ok_response.on_disabled_hover_text(tooltip)
                } else {
                    ok_response
                };
                let apply_response = if let Some(tooltip) = blocked_tooltip {
                    apply_response.on_disabled_hover_text(tooltip)
                } else {
                    apply_response
                };
                ok_clicked = ok_response.clicked();
                apply_clicked = apply_response.clicked();
                if is_dirty(&self.draft, &self.applied) {
                    ui.label("Unsaved changes");
                }
            });

            if apply_clicked {
                // Apply: save + re-register + publish (§4.4); the window stays open.
                self.apply();
            }
            if ok_clicked {
                // OK: the same `apply()` as Apply, then a real close (§4.4) —
                // not the hide-to-tray the X button performs.
                self.apply();
                self.request_window_exit(ui.ctx());
            }
            if cancel_clicked {
                // Cancel: the draft is discarded — nothing is saved or
                // published — any bind capture is cancelled, and the window
                // closes the same way as OK (§4.4).
                self.draft = cancel_draft(&self.draft, &self.applied);
                self.cancel_active_capture();
                self.request_window_exit(ui.ctx());
            }
        });

        // Tab content fills whatever height the docked bottom bar left.
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
    let viewport = egui::ViewportBuilder::default()
        .with_title("virshot — Settings")
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
#[path = "gui.tests.rs"]
mod tests;
