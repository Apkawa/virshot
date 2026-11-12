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
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::Duration;
use std::time::Instant;

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
use crate::fonts;
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
use crate::update;
use crate::update_install;
use crate::update_notes;

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

/// Catalog keys for the placeholder descriptions (`general.placeholder_desc_*`).
/// Token names and key suffixes differ (`t`→`title`, `n`→`counter`,
/// `y`→`year_short`), so the mapping is spelled out; the `%token` notation
/// itself is config notation (spec §4.2) and stays untranslated.
const PLACEHOLDER_DESC_KEYS: [(&str, &str); 11] = [
    ("Y", "general.placeholder_desc_year"),
    ("y", "general.placeholder_desc_year_short"),
    ("mo", "general.placeholder_desc_month"),
    ("d", "general.placeholder_desc_day"),
    ("H", "general.placeholder_desc_hour"),
    ("M", "general.placeholder_desc_minute"),
    ("S", "general.placeholder_desc_second"),
    ("iso", "general.placeholder_desc_iso"),
    ("t", "general.placeholder_desc_title"),
    ("n", "general.placeholder_desc_counter"),
    ("ext", "general.placeholder_desc_ext"),
];

/// One token's localized description; an unlisted token falls back to the
/// English `template::TOKENS` text rather than rendering a dotted key.
fn placeholder_description(token: &str, english: &str) -> String {
    match PLACEHOLDER_DESC_KEYS
        .iter()
        .find(|(name, _)| *name == token)
    {
        Some((_, key)) => rust_i18n::t!(*key).into_owned(),
        None => english.to_owned(),
    }
}

/// Placeholder reference rows for the General tab accordion (spec §4.2): one
/// `(token, description)` line per `template::TOKENS` entry, token spelled with
/// its `%`. The table is the single source of truth — it is the very list
/// `template::render` validates against — so the UI states exactly the tokens a
/// template may use and cannot drift from the renderer. Descriptions are
/// catalog lookups (spec §2); tokens stay config notation. Rows are rebuilt
/// each UI frame: 11 short strings, well under the §5 budget.
pub(crate) fn placeholder_reference_lines() -> Vec<(String, String)> {
    TOKENS
        .iter()
        .map(|(name, description)| {
            (
                format!("%{name}"),
                placeholder_description(name, description),
            )
        })
        .collect()
}

/// Hotkeys tab rows in spec §4.4 order.
pub(crate) const HOTKEY_ROWS: [Action; 3] = [Action::FullScreen, Action::Window, Action::Rect];

/// UI labels for the action rows (catalog `hotkey.row_*`, spec §2).
fn row_label(action: Action) -> String {
    match action {
        Action::FullScreen => rust_i18n::t!("hotkey.row_full_screen").into_owned(),
        Action::Window => rust_i18n::t!("hotkey.row_window").into_owned(),
        Action::Rect => rust_i18n::t!("hotkey.row_rect").into_owned(),
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

/// Language dropdown options (spec §2, Stage 2b): config code + NATIVE name.
/// The array order IS the dropdown order, and `en` is first because it is the
/// spec §7 default. Native names are endonyms — they are never localized
/// ("Русский" stays Russian in every locale), so they are constants here,
/// not catalog keys; only the row label is (`general.language_label`).
pub(crate) const LANGUAGE_OPTIONS: [(&str, &str); 6] = [
    ("en", "English"),
    ("ru", "Русский"),
    ("zh-CN", "简体中文"),
    ("es", "Español"),
    ("ja", "日本語"),
    ("ko", "한국어"),
];

/// Native display name of a config language code. An unknown or empty code
/// mirrors `config::language_code`'s lenient normalization by showing the
/// English endonym — the code normalizes to `en` when the draft is applied.
fn native_name(code: &str) -> &str {
    LANGUAGE_OPTIONS
        .iter()
        .find(|(option_code, _)| *option_code == code)
        .map(|(_, native)| *native)
        .unwrap_or(LANGUAGE_OPTIONS[0].1)
}

/// Locale code for `rust_i18n::set_locale` from a draft: a thin wrapper over
/// `config::language_code`, the same EXACT-match normalization
/// `config::load_or_default` applies on load (spec §7), so a dropdown value
/// and a hand-edited config switch identically on Apply.
pub(crate) fn locale_code_of(settings: &Settings) -> &'static str {
    config::language_code(&settings.general.language)
}

/// Honest filename for the Overlay tab's test chip (Stage 7): no capture and
/// no save happen, so the chip must not name a real file — the fixed name
/// says what it is a test of, matching the spec §4.3 success wording
/// (`Saved: <file name>`).
pub(crate) const OVERLAY_TEST_FILENAME: &str = "test.png";

/// Hover hint for the disabled test button (catalog `overlay.test_disabled_hint`,
/// spec §4.4). The button is gated here in the GUI because a direct
/// `ToastHandle::show` does not consult `overlay.enabled` — that gate lives in
/// `capture::should_show` and only applies to capture-driven chips.
pub(crate) fn overlay_test_disabled_hint() -> String {
    rust_i18n::t!("overlay.test_disabled_hint").into_owned()
}

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

/// Bind-button label while bind-by-press is waiting for a key (§4.4,
/// catalog `hotkey.capture_button_label`).
fn capture_button_label() -> String {
    rust_i18n::t!("hotkey.capture_button_label").into_owned()
}

/// Tooltip while capturing: Escape cancels (catalog `hotkey.capture_tooltip`).
/// Live hotkeys are PAUSED during capture (Stage 18, spec §4.4 v5.5), so a
/// press cannot fire a registered combo (the old PrintScreen quirk note is
/// gone).
fn capture_tooltip() -> String {
    rust_i18n::t!("hotkey.capture_tooltip").into_owned()
}

/// Tooltip for the reserved-slot info icon (§4.4: rect is out of MVP scope,
/// catalog `hotkey.reserved_tooltip`).
fn reserved_tooltip() -> String {
    rust_i18n::t!("hotkey.reserved_tooltip").into_owned()
}

/// Pad-field label while it is capturing a pad press (§4.4, Stage 5, catalog
/// `gamepad.capture_button_label`). The pump commits the first DOWN event,
/// which also ends the capture; cancelling is a click of the same field again,
/// starting a keyboard capture, or Escape (§4.4, v2), which ends it without
/// changing the row.
fn pad_capture_button_label() -> String {
    rust_i18n::t!("gamepad.capture_button_label").into_owned()
}

/// Tooltip while the pad field is capturing — the same affordances as the
/// shared [`bind_hint`] line (§4.4, v2): Escape cancels, Backspace clears the
/// pad slot; clicking the field again cancels too (catalog
/// `gamepad.capture_tooltip`).
fn pad_capture_tooltip() -> String {
    rust_i18n::t!("gamepad.capture_tooltip").into_owned()
}

/// Tooltip for a Windows pad button (spec §4.1: the pad trigger ORs with the
/// keyboard combo through one shared debounce gate; catalog
/// `gamepad.bind_tooltip`).
fn pad_bind_tooltip() -> String {
    rust_i18n::t!("gamepad.bind_tooltip").into_owned()
}

/// Empty pad field on Windows, ready to capture (catalog `hotkey.pad_label`).
fn pad_unbound_label() -> String {
    rust_i18n::t!("hotkey.pad_label").into_owned()
}

/// Pad field on a host with no GameInput backend (catalog
/// `gamepad.windows_only_note`). Linux gilrs is out of scope (AGENTS.md
/// hardware finding), so the field stays a disabled placeholder there; the pad
/// trigger still works from `config.toml` notation once a Windows pump
/// delivers it.
fn gamepad_placeholder_label() -> String {
    rust_i18n::t!("gamepad.windows_only_note").into_owned()
}

/// Bind-button minimum width so the three rows stay visually aligned.
const BIND_BUTTON_MIN_WIDTH: f32 = 170.0;

/// Always-visible one-line hint at the start of the bind form (§4.4, v8,
/// catalog `hotkey.bind_hint`): Escape cancels a capture without changing the
/// row and Backspace clears the binding — for keyboard capture the keyboard
/// half (the pad slot stays), for pad capture the pad slot (the keyboard half
/// stays). Shown whether or not a capture is active, so the keys are
/// discoverable before anyone clicks a field. The wording is field-agnostic,
/// so one line covers both fields.
fn bind_hint() -> String {
    rust_i18n::t!("hotkey.bind_hint").into_owned()
}

/// Bind-button label of a row whose keyboard binding was cleared by Backspace
/// (catalog `hotkey.unassigned`). The empty `key` is the unbound marker —
/// §4.1 notation has no empty key, so the button cannot show a binding and
/// says the row needs a press instead.
fn key_unbound_label() -> String {
    rust_i18n::t!("hotkey.unassigned").into_owned()
}

/// Tooltip for a disabled Apply/OK while some row is unbound (catalog
/// `hotkey.bind_all_hint`): saving a cleared binding writes a key
/// `bindings::parse_key` rejects (§4.1), which costs the app its hotkey
/// registrations and its pad triggers, so saving waits until the row is bound
/// again.
fn unbound_save_tooltip() -> String {
    rust_i18n::t!("hotkey.bind_all_hint").into_owned()
}

/// Tooltip for a disabled Apply/OK while two rows share a trigger (§4.4,
/// Stage 3, catalog `hotkey.duplicate_tooltip`): one keyboard combo or pad
/// button may trigger only one action, so saving waits until the draft is
/// unique again.
fn duplicate_save_tooltip() -> String {
    rust_i18n::t!("hotkey.duplicate_tooltip").into_owned()
}

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
    fn label(self) -> String {
        match self {
            Tab::General => rust_i18n::t!("general.tab_general").into_owned(),
            Tab::Hotkeys => rust_i18n::t!("general.tab_hotkeys").into_owned(),
            Tab::Overlay => rust_i18n::t!("general.tab_overlay").into_owned(),
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
        capture_button_label()
    } else if key.is_empty() {
        key_unbound_label()
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
/// own combo in canonical label form (§4.1 notation), localized via catalog
/// `hotkey.duplicate_error`, e.g. "Duplicates Window: Alt+PrintScreen".
fn keyboard_duplicate(action: Action, other: Action, binding: &Binding) -> DuplicateBinding {
    DuplicateBinding {
        action,
        other,
        kind: DuplicateKind::Keyboard,
        description: rust_i18n::t!(
            "hotkey.duplicate_error",
            row = row_label(other),
            combo = binding_label(&binding.key, &binding.mods)
        )
        .into_owned(),
    }
}

/// One pad-collision entry, same shape as [`keyboard_duplicate`] but naming
/// the pad notation this row carries (catalog `hotkey.duplicate_pad_error`).
fn pad_duplicate(action: Action, other: Action, pad: &str) -> DuplicateBinding {
    DuplicateBinding {
        action,
        other,
        kind: DuplicateKind::Pad,
        description: rust_i18n::t!(
            "hotkey.duplicate_pad_error",
            row = row_label(other),
            pad = pad
        )
        .into_owned(),
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
        gamepad_placeholder_label()
    } else if capturing {
        pad_capture_button_label()
    } else {
        pad.map_or_else(pad_unbound_label, |notation| notation.to_owned())
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
            tooltip: reserved_tooltip(),
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

/// Decode the embedded app-icon asset and upload it as a texture for the
/// About header, mirroring [`warning_icon_texture`]: egui 0.37 core has no
/// PNG loader and the `egui_extras` loaders are OFF (no Uri/Bytes image
/// path), so the bytes go through `icons::rgba` → `ColorImage::
/// from_rgba_unmultiplied` → `Context::load_texture` — texture route only.
/// Wrapped in `Arc` so the viewport callback can share the handle without
/// re-uploading per paint (`TextureHandle::drop` frees the texture id).
///
/// Failure mode: [`icons::IconsError`] on invalid or corrupt PNG bytes —
/// logged at WARN and `None`, so the About header paints its text without
/// an icon and never panics.
fn about_icon_texture(ctx: &egui::Context) -> Option<Arc<egui::TextureHandle>> {
    match icons::rgba(icons::APP_ICON_PNG) {
        Ok((rgba, width, height)) => Some(Arc::new(ctx.load_texture(
            "virshot-about-icon",
            egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &rgba),
            egui::TextureOptions::default(),
        ))),
        Err(err) => {
            tracing::warn!(
                "About app icon decode failed: {err}; the About header paints text only"
            );
            None
        }
    }
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

/// Install lifecycle state for the update row and dialog (task
/// 2026-11-11_auto-update Stage 6, spec §4.7). `Clone` is justified: the
/// deferred viewport callback is `'static` and needs an owned snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UpdateStatus {
    /// No install running; the install action is offered.
    Idle,
    /// The install thread is running; the button is disabled.
    Installing,
    /// The install failed; the row shows the verbatim English message.
    Failed(String),
    /// The binary was replaced; the process must restart to run it. Stage 6
    /// only RECORDS this — the restart wiring lands in Stage 7 (`main.rs`).
    RestartPending,
}

/// Which dialog button the paint pass recorded; `logic` gates the effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UpdateDialogButton {
    Install,
    Cancel,
}

/// The gated result of a dialog button press. No `PartialEq`: `UpdateInfo`
/// has no equality derive (Stage 3 shipped it as `Debug, Clone`), so tests
/// match on the variant and compare the short fields.
#[derive(Debug, Clone)]
pub(crate) enum UpdateDialogOutcome {
    /// Hand the discovered update to the install path (owned: the install
    /// thread needs it across the thread boundary).
    Install(update::UpdateInfo),
    /// Close the dialog, install nothing.
    Dismiss,
}

/// Visibility of the update row in the General tab (spec §4.7): a held
/// discovery is enough. Neither the interval nor dev-ness gates it — a
/// discovery may come from the forced «Проверить обновления» button, which
/// works with ANY interval including `Never`, and a `-dev` build checks
/// updates on the same terms as a release build.
pub(crate) fn update_row_visible(info: Option<&update::UpdateInfo>) -> bool {
    info.is_some()
}

/// Enabled state of the forced-check button (spec §4.7): exactly ONE check may
/// be in flight, so the button is disabled while `check_in_flight` is set.
pub(crate) fn check_now_button_enabled(check_in_flight: bool) -> bool {
    !check_in_flight
}

/// Whether the ONE background-check slot is occupied after a logic pass has
/// armed the interval deadline (if it had to) and drained the check outcomes
/// (spec §4.7: exactly one check in flight at a time, and the button gate
/// reads this flag).
///
/// - `slot_busy`: the slot was already occupied at the start of the pass — a
///   scheduled or forced check spawned by an earlier pass, whose deadline was
///   consumed at spawn (`next_check = None`).
/// - `outcome_drained`: this pass drained an outcome, which ENDS the check that
///   produced it, so the slot is free afterwards whatever was armed.
/// - `startup_deadline`: the deadline armed on the FIRST pass. It represents
///   the startup check `main.rs` spawned before the event loop (both are the
///   same interval gate: a `Never` interval arms nothing and spawns nothing),
///   so it means "in flight" ONLY on that pass — an arm of a later pass is a
///   FUTURE check, not a running one.
///
/// The pass-1 order matters: arming AFTER the drain put `check_in_flight` back
/// ON after the startup outcome had already drained (a fast failure such as an
/// HTTP 404, or a slow window init), and nothing ever cleared it again — no
/// scheduled check fired and «Проверить обновления» stayed disabled for the
/// whole session.
pub(crate) fn check_in_flight_after(
    slot_busy: bool,
    outcome_drained: bool,
    startup_deadline: Option<Instant>,
) -> bool {
    if outcome_drained {
        false
    } else {
        slot_busy || startup_deadline.is_some()
    }
}

/// Whether a button press starts a forced check (spec §4.7): the interval
/// setting is IGNORED — `Never` still runs a check — while an in-flight check
/// blocks a second one. `interval` is passed for the record, not for the
/// decision; the scheduled deadline stays untouched either way (`logic` never
/// re-arms it on a forced outcome).
pub(crate) fn force_check_requested(_interval: config::UpdateCheck, check_in_flight: bool) -> bool {
    !check_in_flight
}

/// Whether a drained outcome may re-arm the interval deadline (spec §4.7): only
/// a SCHEDULED outcome moves the deadline; a FORCED outcome leaves it exactly
/// where it was, so a manual «Проверить обновления» never shifts the background
/// schedule.
pub(crate) fn deadline_rearmed_by_outcome(forced_check_in_flight: bool) -> bool {
    !forced_check_in_flight
}

/// What one logic pass does with the forced-check press the paint recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ForcedCheckDecision {
    /// Spawn the forced check on this pass and clear the press record.
    pub(crate) spawn: bool,
    /// Keep the press recorded for a later pass: the single slot is busy — a
    /// scheduled check may have taken it on THIS very pass — so the press must
    /// not be dropped; it runs as soon as the slot frees.
    pub(crate) pending: bool,
}

/// Forced-check press decision (spec §4.7): honored while the single slot is
/// free (the interval is ignored — see [`force_check_requested`]) and kept
/// PENDING while the slot is busy, so a press recorded during paint survives a
/// pass in which the scheduled spawn consumed the slot.
pub(crate) fn forced_check_decision(
    interval: config::UpdateCheck,
    pressed: bool,
    check_in_flight: bool,
) -> ForcedCheckDecision {
    let honored = pressed && force_check_requested(interval, check_in_flight);
    ForcedCheckDecision {
        spawn: honored,
        pending: pressed && !honored,
    }
}

/// The background-check deadline after `apply()` (spec §4.7): the APPLIED
/// interval governs background checks, so saving settings moves the schedule —
/// a scheduled interval arms `now + interval` (an interval switch such as
/// 1h→1d re-arms to the NEW interval, and `никогда`→scheduled arms the
/// schedule without a restart), and `Never` clears the deadline so no extra
/// check fires after the user turned checks off. A check already in flight is
/// never cancelled: this decides the deadline only — the caller leaves
/// `check_in_flight`/`forced_check_in_flight` untouched.
pub(crate) fn schedule_after_apply(now: Instant, interval: config::UpdateCheck) -> Option<Instant> {
    update::next_check_deadline(now, interval.interval())
}

/// The install action is offered in exactly ONE state: Idle. `Installing`
/// must not start a second install thread; `Failed` keeps the button hidden
/// so the row shows the error text instead (spec §4.7).
pub(crate) fn install_button_enabled(status: &UpdateStatus) -> bool {
    matches!(status, UpdateStatus::Idle)
}

/// The failure text the row shows INSTEAD of the button: the verbatim
/// English `InstallError` display, no locale key — technical error text
/// stays English (precedent: `capture::chip_text`). `None` outside Failed.
pub(crate) fn install_failure_hint(status: &UpdateStatus) -> Option<&str> {
    match status {
        UpdateStatus::Failed(message) => Some(message.as_str()),
        UpdateStatus::Idle | UpdateStatus::Installing | UpdateStatus::RestartPending => None,
    }
}

/// The install request behind an Install press. The owned `UpdateInfo` is a
/// justified clone: the install thread moves it across the thread boundary
/// while app state keeps the discovery for the row. Nothing is requested
/// while `Installing`/`Failed`/`RestartPending` or without a held discovery.
pub(crate) fn install_request(
    status: &UpdateStatus,
    info: Option<&update::UpdateInfo>,
) -> Option<update::UpdateInfo> {
    if install_button_enabled(status) {
        info.cloned()
    } else {
        None
    }
}

/// Gate a dialog button press into its effect. Presses land in the outcome
/// slot during the CHILD paint pass; `logic` gates them in the root pass —
/// the same one-frame ordering as the About close flag. Cancel always
/// dismisses; Install only requests while Idle with a held discovery (the
/// button is disabled in other states; this gate is the second defense).
pub(crate) fn dialog_outcome(
    button: UpdateDialogButton,
    status: &UpdateStatus,
    info: Option<&update::UpdateInfo>,
) -> Option<UpdateDialogOutcome> {
    match button {
        UpdateDialogButton::Cancel => Some(UpdateDialogOutcome::Dismiss),
        UpdateDialogButton::Install => {
            install_request(status, info).map(UpdateDialogOutcome::Install)
        }
    }
}

/// Auto-install path (spec §4.7): with `auto_install` ON, a discovery starts
/// the install immediately — no dialog, no row. The caller passes the
/// APPLIED flag (§4.4: published settings drive behavior).
pub(crate) fn auto_install_on_discovery(info_present: bool, auto_install: bool) -> bool {
    info_present && auto_install
}

/// Gate for the discovery chip, mirroring `capture::should_show` (§4.3):
/// success-style chips show only while the overlay is enabled. Failure
/// chips are ALWAYS shown and never pass through this gate.
pub(crate) fn update_toast_enabled(
    info: Option<&update::UpdateInfo>,
    overlay_enabled: bool,
) -> bool {
    info.is_some() && overlay_enabled
}

/// State transition from the install thread's single outcome. `Ok` lands in
/// `RestartPending`: the binary IS replaced and the process must restart to
/// run it — Stage 6 only RECORDS that; Stage 7 wires it: `App::logic` sets
/// `restart_requested` and sends `ViewportCommand::Close`, and `main.rs`
/// drops the instance lock, spawns the new binary, and exits 0 (plus startup
/// `update_install::cleanup_leftovers` on every start).
/// `Err` keeps the verbatim message for the row and the danger chip.
pub(crate) fn install_finished(outcome: &Result<(), String>) -> UpdateStatus {
    match outcome {
        Ok(()) => UpdateStatus::RestartPending,
        Err(message) => UpdateStatus::Failed(message.clone()),
    }
}

/// Discovery after an install result: success clears it (the row disappears;
/// the restart banner is Stage 7's job), failure keeps it so the row can
/// show the error. Takes the discovery by value — returned or dropped,
/// never borrowed-and-cloned.
pub(crate) fn update_info_after_install(
    info: Option<update::UpdateInfo>,
    ok: bool,
) -> Option<update::UpdateInfo> {
    if ok { None } else { info }
}

/// State transition when a NEW discovery arrives (spec §4.7, Stage 7): a
/// discovery that lands while an install FAILURE is displayed re-opens the
/// install action — the failure text belongs to the attempt that is over, so
/// the state returns to `Idle` and the row offers Install again. `Installing`
/// is untouched (a running install must not be interrupted), `Idle` stays
/// `Idle`, and `RestartPending` stays: the binary is already replaced, so a
/// discovery there describes the very bytes the restart is about to run.
pub(crate) fn status_after_new_discovery(status: UpdateStatus) -> UpdateStatus {
    match status {
        UpdateStatus::Failed(_) => UpdateStatus::Idle,
        other => other,
    }
}

/// The result line painted next to «Проверить обновления» (task
/// 2026-11-11_auto-update Stage 11 sub-item 2, spec §4.7): ONLY forced-check
/// outcomes land here — scheduled/background results keep reporting through
/// the discovery chip and the General row as before. `None` before the first
/// forced check means no status text at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ForcedCheckStatus {
    /// The last forced check found nothing newer than the running build —
    /// the plain localized «Обновлений нет» (`update.no_updates_available`).
    NoUpdates,
    /// The last forced check failed: the verbatim English `UpdateError`
    /// Display, never localized (precedent: `capture::error_display`), drawn
    /// in the same warning styling as the install-failure hint.
    Failed(String),
}

/// The result line after ONE drained check outcome (sub-item 2): a
/// SCHEDULED outcome leaves the slot exactly as it was; a FORCED outcome
/// REPLACES it — `UpToDate` → «Обновлений нет», `Failed` → the verbatim
/// `UpdateError` Display, `Updated` → cleared, because the discovery row
/// shows that result and a stale line must never sit beside a fresh
/// «Доступно обновление». Takes the slot by value — returned or dropped,
/// never borrowed-and-cloned.
pub(crate) fn forced_check_status_after(
    slot: Option<ForcedCheckStatus>,
    outcome: &update::CheckOutcome,
    forced: bool,
) -> Option<ForcedCheckStatus> {
    if !forced {
        return slot;
    }
    match outcome {
        update::CheckOutcome::UpToDate => Some(ForcedCheckStatus::NoUpdates),
        update::CheckOutcome::Failed(err) => Some(ForcedCheckStatus::Failed(err.to_string())),
        update::CheckOutcome::Updated(_) => None,
    }
}

/// What the paint pass renders in the slot: NOTHING while a FORCED check is
/// in flight (the slot stays untouched until its outcome drains — no
/// half-result while the button is disabled), otherwise the held line.
pub(crate) fn forced_check_status_shown(
    slot: Option<&ForcedCheckStatus>,
    forced_check_in_flight: bool,
) -> Option<&ForcedCheckStatus> {
    if forced_check_in_flight { None } else { slot }
}

/// Stale-discovery reset (sub-item 4, spec §4.7): a check outcome REPLACES
/// the previous result. An up-to-date outcome — FORCED or scheduled — clears
/// the held discovery (nothing newer exists anymore); a failed FORCED check
/// clears it too (the user's bug: a nightly discovery kept «Доступно
/// обновление» after switching the channel to `stable` and the forced check
/// answering 404); a failed SCHEDULED check KEEPS it (a transient
/// network/rate-limit blip must not erase an actionable update — the next
/// interval retries). `Updated` never clears here: the drain replaces the
/// discovery with the new one (or auto-install consumes it). While an
/// install is in progress NOTHING ever clears: the running install owns its
/// discovery, and a failure that lands afterwards keeps the row for the
/// retry (`update_info_after_install`).
pub(crate) fn discovery_cleared_by_check_outcome(
    outcome: &update::CheckOutcome,
    forced: bool,
    install_status: &UpdateStatus,
) -> bool {
    if matches!(install_status, UpdateStatus::Installing) {
        return false;
    }
    match outcome {
        update::CheckOutcome::UpToDate => true,
        update::CheckOutcome::Failed(_) => forced,
        update::CheckOutcome::Updated(_) => false,
    }
}

/// Lifecycle of the update child viewport — the exact shape of
/// `about::AboutWindow` (task 2026-11-10 stage 10): an OS close request is
/// visible ONLY in the child viewport's input pass, so the paint records it
/// into a shared flag; the next logic pass sees the flag, stops
/// registration, egui GCs the viewport, and eframe drops the `Arc<Window>`
/// (`crates/overlay/src/host.rs:313-323`). Not reusing `about::AboutWindow`:
/// its name and tray semantics belong to the About module (SRP), and this
/// dialog additionally needs the Cancel-button close path.
pub(crate) struct UpdateDialogWindow {
    open: bool,
    close_flag: Arc<AtomicBool>,
}

impl UpdateDialogWindow {
    /// Closed state with an inert flag; opening replaces the flag, so a
    /// stale flag is never handed to a paint callback.
    fn new() -> Self {
        Self {
            open: false,
            close_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Open with a FRESH close flag; returns whether this call newly opened
    /// (a re-open of an already-open viewport must not inherit state — same
    /// rule as `about::AboutWindow::open`).
    fn open(&mut self) -> bool {
        if self.open {
            return false;
        }
        self.close_flag = Arc::new(AtomicBool::new(false));
        self.open = true;
        true
    }

    /// Cancel-button close: no OS request happened, but the next pass must
    /// stop registration like one did (egui GCs the viewport).
    fn close(&mut self) {
        self.open = false;
    }

    /// Observe the shared flag: true (and now closed) when the child pass
    /// recorded an OS close request since the last observation.
    fn observe_close(&mut self) -> bool {
        if self.open && self.close_flag.load(Ordering::Relaxed) {
            self.open = false;
            return true;
        }
        false
    }

    /// Whether the logic pass must call `show_viewport_deferred` for this
    /// viewport (stopping registration = GC, see the struct doc).
    fn wants_registration(&self) -> bool {
        self.open
    }

    /// Close-flag handle for the paint callback, `None` when closed. The
    /// `Arc` clone is O(1) — one refcount bump, shared with app state.
    fn close_flag(&self) -> Option<Arc<AtomicBool>> {
        if self.open {
            Some(Arc::clone(&self.close_flag))
        } else {
            None
        }
    }
}

/// What the update dialog paints on a pass, built from the snapshots the
/// deferred callback owns, so the callback stays `'static`.
struct UpdateDialogView<'a> {
    /// `None` = fetch pending (loading hint); `Some` = arrived result.
    notes: Option<&'a Result<Arc<update_notes::ReleaseNotes>, update_notes::NotesError>>,
    /// Held discovery: version line + fallback summary. `None` only in the
    /// auto-install path, which never opens the dialog.
    info: Option<&'a update::UpdateInfo>,
    /// APPLIED channel, for the release-page fallback URL.
    channel: config::UpdateChannel,
    /// Install-state snapshot driving the button's enabled flag.
    status: &'a UpdateStatus,
}

/// Paint the update dialog in its deferred child viewport — the exact About
/// pattern (task 2026-11-10 stage 10, verified against the locked egui 0.37
/// sources): record an OS close request into the shared flag (only the child
/// pass sees it), fill an OPAQUE background (the app's clear color is alpha-0
/// and `Frame::central_panel` covers the whole viewport), render notes with
/// the scrollable `egui_commonmark` viewer (`viewport_cache(true)`, viewer
/// `!Send` → constructed per paint), and end with the install/cancel buttons
/// (spec §4.7). NOT modal: the settings window keeps working while the
/// dialog is open.
fn paint_update_dialog(
    ui: &mut egui::Ui,
    cache: &mut egui_commonmark::CommonMarkCache,
    close_flag: &AtomicBool,
    slot: &Mutex<Option<UpdateDialogButton>>,
    view: &UpdateDialogView<'_>,
) {
    if ui.input(|i| i.viewport().close_requested()) {
        close_flag.store(true, Ordering::Relaxed);
    }
    let frame = egui::Frame::central_panel(ui.style()).inner_margin(egui::Margin::symmetric(16, 8));
    egui::CentralPanel::default().frame(frame).show(ui, |ui| {
        if let Some(info) = view.info {
            // The discovered version is a technical value kept literal
            // (About build-line convention), so no locale key here.
            ui.label(
                egui::RichText::new(info.version.as_str())
                    .size(18.0)
                    .strong(),
            );
        }
        ui.separator();
        match view.notes {
            None => {
                ui.label(rust_i18n::t!("update.notes_loading"));
            }
            Some(Ok(notes)) if !notes.body.is_empty() => {
                egui_commonmark::CommonMarkViewer::new().show_scrollable(
                    egui::Id::unique("virshot-update-notes"),
                    ui,
                    cache,
                    &egui_commonmark::CommonMarkScrollOptions::default().viewport_cache(true),
                    &notes.body,
                );
            }
            // Fallback (spec §4.7): an empty body or a fetch failure shows
            // the release.yaml summary plus the release-page link — the
            // technical values stay literal and the URL is verbatim (About
            // repo-link precedent).
            Some(Ok(_)) | Some(Err(_)) => {
                ui.label(rust_i18n::t!("update.notes_unavailable"));
                if let Some(info) = view.info {
                    ui.label(format!(
                        "{} · {} · {}",
                        info.version, info.release_date, info.commit_sha
                    ));
                }
                let url = view
                    .notes
                    .and_then(|result| result.as_ref().ok())
                    .map(|notes| notes.html_url.as_str())
                    .unwrap_or_else(|| update_notes::release_page_url(view.channel));
                ui.hyperlink(url);
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
            let install_clicked = ui
                .add_enabled(
                    install_button_enabled(view.status),
                    egui::Button::new(rust_i18n::t!("general.install_update_button")),
                )
                .clicked();
            let cancel_clicked = ui.button(rust_i18n::t!("bottom.cancel")).clicked();
            if install_clicked || cancel_clicked {
                // RECORDED, not acted on: the root logic pass gates the press
                // against current state (`dialog_outcome`).
                let button = if install_clicked {
                    UpdateDialogButton::Install
                } else {
                    UpdateDialogButton::Cancel
                };
                *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(button);
            }
        });
    });
}

/// Selected text of the interval dropdown: `never` is a decision word —
/// localized (spec §4.4); the intervals are config literals shown verbatim
/// (`1h`/`1d`/`1w`), exactly like the Format dropdown's `png`/`jpg`/`webp`
/// option labels — technical values stay unlocalized.
fn update_check_selected_text(check: config::UpdateCheck) -> String {
    match check {
        config::UpdateCheck::Never => rust_i18n::t!("general.update_check_never").into_owned(),
        config::UpdateCheck::Hour => "1h".to_owned(),
        config::UpdateCheck::Day => "1d".to_owned(),
        config::UpdateCheck::Week => "1w".to_owned(),
    }
}

/// Chip geometry from the APPLIED overlay settings: the same monitor /
/// corner / clamped-hold mapping `capture::toast_from_outcome` applies to
/// real chips. Those helpers are private in capture, so this duplicates the
/// three-line mapping locally instead of widening capture's interface
/// (precedent: `test_overlay_toast`).
fn update_chip_geometry(overlay: &Overlay) -> (MonitorSel, Corner, u32) {
    let monitor = if overlay.on_all_monitors {
        MonitorSel::All
    } else {
        MonitorSel::Index(0)
    };
    let corner = match overlay.position {
        OverlayPosition::TopLeft => Corner::TopLeft,
        OverlayPosition::TopRight => Corner::TopRight,
        OverlayPosition::BottomLeft => Corner::BottomLeft,
        OverlayPosition::BottomRight => Corner::BottomRight,
    };
    let duration_ms = u32::try_from(overlay.duration_ms).unwrap_or(u32::MAX);
    (monitor, corner, duration_ms)
}

/// Discovery chip (success style — gated by `update_toast_enabled` BEFORE
/// this is built): localized announcement with the version interpolated, the
/// same `%{...}` mechanism as the save chip's file name.
fn update_discovery_chip(overlay: &Overlay, info: &update::UpdateInfo) -> Toast {
    let (monitor, corner, duration_ms) = update_chip_geometry(overlay);
    Toast {
        monitor,
        text: rust_i18n::t!("update.discovered_message", version = info.version.as_str())
            .into_owned(),
        image: None,
        color: ToastColor::Success,
        corner,
        duration_ms,
    }
}

/// Install-failure chip: ALWAYS shown (danger chips ignore the overlay
/// toggle, §4.3 `should_show`) with the verbatim English error text — no
/// locale key for technical failures (capture::chip_text precedent).
fn update_failure_chip(overlay: &Overlay, message: &str) -> Toast {
    let (monitor, corner, duration_ms) = update_chip_geometry(overlay);
    Toast {
        monitor,
        text: message.to_owned(),
        image: None,
        color: ToastColor::Danger,
        corner,
        duration_ms,
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
    /// About child viewport (task 2026-11-10_about-dialog, stage 10):
    /// lifecycle state (`about::AboutWindow`). Tray `About` opens a closed
    /// window and RAISES (Focus) an open one; the child paint pass records an
    /// OS close request into the shared flag, and the next logic pass stops
    /// registration — egui GCs the viewport and eframe drops the
    /// `Arc<Window>`, destroying the window (`crates/overlay/src/host.rs:313-323`).
    about_window: crate::about::AboutWindow,
    /// Markdown cache for the About changelog. The viewport callback is
    /// `Send + Sync + 'static` (egui 0.37), so the cache lives behind an Arc
    /// the closure can own; the viewer itself is created per paint in
    /// `about::paint`.
    about_changelog_cache: Arc<Mutex<egui_commonmark::CommonMarkCache>>,
    /// About header app icon: uploaded ONCE when About first opens
    /// (`about_icon_texture`, lazily in the tray handler). Behind an `Arc`
    /// because the viewport callback is `'static` and must share the handle
    /// with app state — rebuilding it per paint pass would drop the previous
    /// `TextureHandle` and free its texture id every frame. `None` when the
    /// decode failed (logged at WARN) or About has never opened: the header
    /// then paints text only.
    about_icon: Option<Arc<egui::TextureHandle>>,
    /// Held update discovery (task 2026-11-11_auto-update Stage 6, spec §4.7):
    /// the newest `CheckOutcome::Updated` drained in `logic`; `None` until a
    /// check finds something newer than the running build. Cleared by a
    /// successful install (`update_info_after_install`); kept on failure so
    /// the row can show the error.
    update: Option<update::UpdateInfo>,
    /// Background-check hand-off receiver (task 2026-11-11_auto-update
    /// Stage 7, spec §4.7): `main.rs` creates the channel and moves BOTH
    /// halves into `gui::run` — the GUI does not create its own. Contract:
    /// drained ONLY in `App::logic`; draining a closed channel is a cheap
    /// no-op and never blocks. `Option` so the drain can take it by reference
    /// while app state stays borrowable.
    update_rx: Option<mpsc::Receiver<update::CheckOutcome>>,
    /// Sender half for every check this app spawns (Stage 7): scheduled and
    /// forced checks send on this clone. A `Sender` clone is O(1) — a refcount
    /// bump on the shared channel — so it is never wrapped in `Arc`
    /// (AGENTS.md: `Sender` already clones in O(1)).
    update_tx: mpsc::Sender<update::CheckOutcome>,
    /// Next periodic-check instant (Stage 7, spec §4.7): `None` when nothing is
    /// pending — a `Never` interval, or the scheduled check that just fired.
    /// Re-armed after a drained SCHEDULED outcome only; the forced button
    /// never moves it (§4.7). `apply()` re-arms it from the APPLIED interval
    /// (`schedule_after_apply`), so an interval switch takes effect live.
    /// `logic` spawns the next check when the clock passes it and requests a
    /// repaint no later than this instant.
    next_check: Option<Instant>,
    /// Whether the FIRST `logic` pass has armed the interval deadline: the
    /// startup check is spawned by `main.rs` BEFORE the event loop, so the
    /// interval clock starts on the first pass, not at app construction. The
    /// arm runs BEFORE the outcome drain, so a startup outcome that drains on
    /// that same pass still frees the slot (see [`check_in_flight_after`]).
    deadline_armed: bool,
    /// Whether ONE check is in flight (Stage 9, spec §4.7): scheduled or
    /// forced. Set on spawn and recomputed once per pass by
    /// [`check_in_flight_after`] — the drained outcome in `logic` is the only
    /// thing that frees the slot. Gates the forced-check button
    /// ([`check_now_button_enabled`]) and the scheduled spawn, so exactly one
    /// check thread is live at a time.
    check_in_flight: bool,
    /// Whether the check currently in flight is the FORCED one: its drained
    /// outcome must NOT re-arm or move the interval deadline (§4.7). The two
    /// flags describe one slot — `check_in_flight` picks the gate, this one
    /// picks the deadline rule.
    forced_check_in_flight: bool,
    /// Forced-check press recorded by the paint pass; `logic` spawns the check
    /// and clears it. Paint never spawns threads, so the press is a record.
    /// A press the busy slot cannot honor stays recorded (`forced_check_decision`)
    /// and runs on the next pass, so a press is never silently dropped.
    check_now_requested: bool,
    /// The result line shown beside «Проверить обновления» (Stage 11
    /// sub-item 2, spec §4.7): ONLY forced-check outcomes land here
    /// (`forced_check_status_after`); scheduled results never touch it —
    /// they keep using the chip and the General row. `None` before the
    /// first forced check; while a forced check is in flight the paint
    /// renders nothing (`forced_check_status_shown`) but keeps the slot.
    forced_check_status: Option<ForcedCheckStatus>,
    /// Shared install→restart flag (Stage 7, spec §5): created in `main.rs`
    /// and set here when an install succeeds, together with
    /// `ViewportCommand::Close`; `main.rs` drops the instance lock and spawns
    /// the new binary once `run_native` returns.
    restart_requested: Arc<AtomicBool>,
    /// Install lifecycle shown by the General row and the dialog buttons
    /// (see [`UpdateStatus`]).
    update_status: UpdateStatus,
    /// Install-thread result: exactly one `Result<(), String>` (verbatim
    /// English `InstallError` text), drained once in `logic`; `None` while no
    /// install thread is live.
    install_rx: Option<mpsc::Receiver<Result<(), String>>>,
    /// Update-dialog child viewport lifecycle (see [`UpdateDialogWindow`],
    /// the exact About shape). Opened from the General row; NOT modal.
    update_dialog: UpdateDialogWindow,
    /// Slot the dialog paint records button presses into; `logic` gates them
    /// with `dialog_outcome`. A fresh slot per open (like About's fresh close
    /// flag) so a re-open never inherits a stale press.
    dialog_outcome_slot: Arc<Mutex<Option<UpdateDialogButton>>>,
    /// Release-notes hand-off, created ONLY when the dialog opens (lazy
    /// fetch, spec §4.7) and dropped on close: a pending fetch thread's send
    /// then lands on a closed receiver and is ignored (module contract), and
    /// a re-open fetches fresh.
    notes_rx: Option<mpsc::Receiver<Result<update_notes::ReleaseNotes, update_notes::NotesError>>>,
    /// Arrived notes: `None` = fetch still pending (dialog paints the loading
    /// hint). The `Arc` wrap is for the `'static` viewport callback, which
    /// needs an owned snapshot — an O(1) refcount bump, not a deep copy.
    notes: Option<Result<Arc<update_notes::ReleaseNotes>, update_notes::NotesError>>,
    /// Markdown cache for the dialog notes — same Arc pattern as
    /// `about_changelog_cache`: the viewport callback is `Send + Sync +
    /// 'static`, and the `!Send` viewer is constructed per paint.
    update_notes_cache: Arc<Mutex<egui_commonmark::CommonMarkCache>>,
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
        update_rx: mpsc::Receiver<update::CheckOutcome>,
        update_tx: mpsc::Sender<update::CheckOutcome>,
        restart_requested: Arc<AtomicBool>,
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
            about_window: crate::about::AboutWindow::new(),
            about_changelog_cache: Arc::new(
                Mutex::new(egui_commonmark::CommonMarkCache::default()),
            ),
            about_icon: None,
            update: None,
            update_rx: Some(update_rx),
            update_tx,
            next_check: None,
            deadline_armed: false,
            check_in_flight: false,
            forced_check_in_flight: false,
            check_now_requested: false,
            forced_check_status: None,
            restart_requested,
            update_status: UpdateStatus::Idle,
            install_rx: None,
            update_dialog: UpdateDialogWindow::new(),
            dialog_outcome_slot: Arc::new(Mutex::new(None)),
            notes_rx: None,
            notes: None,
            update_notes_cache: Arc::new(Mutex::new(egui_commonmark::CommonMarkCache::default())),
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
            Ok(()) => {
                tracing::info!("Config saved: {}", self.config_path.display());
                // Live locale switch (spec §2, Stage 2b): the SAVED draft is
                // the source of truth, so switch only after the save succeeds
                // (a failed save leaves the current locale alone), BEFORE the
                // tray rebuild, so the rebuilt menu labels resolve in the new
                // language — no restart needed.
                rust_i18n::set_locale(locale_code_of(&self.draft));
                if let Some(tray) = &self.tray {
                    // Best-effort: a refused rebuild keeps the old-language
                    // menu, which is cosmetic; the app stays alive (spec §5).
                    if let Err(err) = tray.refresh_menu() {
                        tracing::warn!("Tray menu rebuild failed: {err}");
                    }
                }
            }
            Err(err) => tracing::error!("Config save failed: {err}"),
        }
        self.applied = self.draft.clone();

        // Background-check schedule (spec §4.7): the APPLIED interval governs
        // the scheduled checks, so saving settings moves the deadline —
        // `никогда` → a scheduled interval arms the schedule right away (no
        // restart needed), an interval switch (1h→1d) re-arms to the NEW
        // interval, and a switch to `никогда` clears an armed deadline so no
        // extra check fires after checks were turned off. A check already in
        // flight is left running: only the deadline changes here, never
        // `check_in_flight`/`forced_check_in_flight` (`schedule_after_apply`).
        self.next_check = schedule_after_apply(Instant::now(), self.applied.general.update_check);

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
        // Language dropdown (spec §2 live locale switch, Stage 2b): FIRST row
        // of the tab. The draft stores the canonical config code (Apply
        // persists it and switches the locale — see `apply`), so `is_dirty`
        // sees the change like any other draft field. Selected text and items
        // show NATIVE names (`LANGUAGE_OPTIONS`), the row label is a catalog
        // lookup.
        egui::ComboBox::from_label(rust_i18n::t!("general.language_label"))
            .selected_text(native_name(&self.draft.general.language))
            .show_ui(ui, |ui| {
                for (code, native) in LANGUAGE_OPTIONS {
                    if ui
                        .selectable_label(self.draft.general.language == code, native)
                        .clicked()
                    {
                        self.draft.general.language = code.to_owned();
                    }
                }
            });

        // The save-path row is rendered before the draft borrow is taken for
        // the rest of the tab: Browse rewrites `draft.general.save_path`
        // after the dialog returns (spec §4.4 — Apply persists it as usual).
        ui.label(rust_i18n::t!("general.save_path_label"));
        let mut browse_clicked = false;
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.draft.general.save_path);
            browse_clicked = ui.button(rust_i18n::t!("general.browse_button")).clicked();
        });
        if browse_clicked {
            self.browse_save_path();
        }

        let general = &mut self.draft.general;

        ui.label(rust_i18n::t!("general.full_screen_label"));
        ui.text_edit_singleline(&mut general.file_full_screen);

        ui.label(rust_i18n::t!("general.window_label"));
        ui.text_edit_singleline(&mut general.file_window);

        // Placeholder reference (spec §4.2, Stage 6): the token table rendered
        // straight from `template::TOKENS`, so the tab states exactly what
        // `template::render` accepts and never duplicates the list. Collapsed
        // by default (a reference, not a control); it is part of the tab's
        // vertical layout — no extra frame, no scrolling.
        egui::CollapsingHeader::new(rust_i18n::t!("general.placeholder_reference_header"))
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

        egui::ComboBox::from_label(rust_i18n::t!("general.format_label"))
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
            ui.label(rust_i18n::t!("general.jpeg_quality_label"));
        });

        ui.horizontal(|ui| {
            ui.add_enabled(
                general.format == Format::Webp,
                egui::DragValue::new(&mut general.webp_quality).range(1..=100),
            );
            ui.label(rust_i18n::t!("general.webp_quality_label"));
        });

        // ── «Обновление» group (spec §4.4/§4.7, task 2026-11-11 Stages 6-9) ──
        // Draft edits participate in `is_dirty` (Settings derives PartialEq,
        // so the General fields added by Stage 2 are covered by the existing
        // machinery) and persist ONLY through `apply()` — no bypass. Behavior
        // reads APPLIED settings (§4.4): an unsaved interval/channel switch
        // cannot change checks, the row, or the install path until Apply.
        ui.separator();
        ui.label(rust_i18n::t!("update.section_title"));

        egui::ComboBox::from_label(rust_i18n::t!("update.check_label"))
            .selected_text(update_check_selected_text(general.update_check))
            .show_ui(ui, |ui| {
                // `Never` is a decision word — localized; the intervals are
                // config literals shown verbatim, exactly like the Format
                // dropdown's `png`/`jpg`/`webp` option labels.
                ui.selectable_value(
                    &mut general.update_check,
                    config::UpdateCheck::Never,
                    rust_i18n::t!("general.update_check_never"),
                );
                ui.selectable_value(&mut general.update_check, config::UpdateCheck::Hour, "1h");
                ui.selectable_value(&mut general.update_check, config::UpdateCheck::Day, "1d");
                ui.selectable_value(&mut general.update_check, config::UpdateCheck::Week, "1w");
            });

        // Forced check (spec §4.7), under the interval dropdown: it ignores the
        // interval — `Never` still checks — and never moves or re-arms the
        // scheduled deadline. Paint records the press only; `logic` spawns the
        // check with the APPLIED channel, so an unsaved channel switch cannot
        // redirect it (§4.4). The button is disabled while a check is in flight.
        // Stage 11 sub-item 2: the button and its RESULT line share one
        // horizontal row — the line shows ONLY the last forced outcome: the
        // localized «Обновлений нет» after an up-to-date check, or the
        // verbatim English `UpdateError` Display after a failure (never
        // localized, `capture::error_display` precedent, in the same warning
        // styling as the install-failure hint). Scheduled results keep using
        // the chip and the General row; nothing renders while a forced check
        // is in flight (`forced_check_status_shown`).
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    check_now_button_enabled(self.check_in_flight),
                    egui::Button::new(rust_i18n::t!("update.check_now_button")),
                )
                .clicked()
            {
                self.check_now_requested = true;
            }
            if let Some(status) = forced_check_status_shown(
                self.forced_check_status.as_ref(),
                self.forced_check_in_flight,
            ) {
                match status {
                    // Technical failure text stays English verbatim (no
                    // locale key) and shares the warning styling of
                    // `install_failure_hint` below.
                    ForcedCheckStatus::Failed(message) => {
                        ui.label(
                            egui::RichText::new(message.as_str())
                                .color(WARNING_COLOR)
                                .strong(),
                        );
                    }
                    // The success line is a plain localized result, not an error.
                    ForcedCheckStatus::NoUpdates => {
                        ui.label(rust_i18n::t!("update.no_updates_available"));
                    }
                }
            }
        });

        egui::ComboBox::from_label(rust_i18n::t!("general.update_channel_label"))
            .selected_text(match general.update_channel {
                config::UpdateChannel::Stable => "stable",
                config::UpdateChannel::Nightly => "nightly",
            })
            .show_ui(ui, |ui| {
                // Channel values are release-tag names — config literals, not
                // decision words, so `stable`/`nightly` stay unlocalized.
                ui.selectable_value(
                    &mut general.update_channel,
                    config::UpdateChannel::Stable,
                    "stable",
                );
                ui.selectable_value(
                    &mut general.update_channel,
                    config::UpdateChannel::Nightly,
                    "nightly",
                );
            });

        ui.checkbox(
            &mut general.auto_install,
            rust_i18n::t!("general.auto_install_label"),
        );

        // «Update available» row (spec §4.7): the dialog-path UI. With
        // auto-install ON a discovery never sits held — install starts at
        // discovery — so this row is what the OFF path offers: open the
        // update dialog while Idle, read the verbatim failure text after a
        // failed install, and stay disabled while Installing. Visibility is
        // `update_row_visible` (a held discovery is enough): a discovery can
        // come from the forced check, which works with ANY interval (§4.7), so
        // neither the interval nor dev-ness gates the row anymore.
        if update_row_visible(self.update.as_ref()) {
            let version = self
                .update
                .as_ref()
                .map(|info| info.version.as_str())
                .unwrap_or("");
            let failure = install_failure_hint(&self.update_status).map(str::to_owned);
            let enabled = install_button_enabled(&self.update_status);
            let mut open_dialog = false;
            ui.horizontal(|ui| {
                ui.label(rust_i18n::t!("general.update_available_label"));
                ui.label(version);
                match failure {
                    // A failed install replaces the button with the verbatim
                    // English error text (no locale key for technical
                    // failures) in the warning color already used for status
                    // icons — the message itself is the evidence.
                    Some(message) => {
                        ui.label(egui::RichText::new(message).color(WARNING_COLOR).strong());
                    }
                    None => {
                        open_dialog = ui
                            .add_enabled(
                                enabled,
                                egui::Button::new(rust_i18n::t!("general.install_update_button")),
                            )
                            .clicked();
                    }
                }
            });
            if open_dialog {
                self.open_update_dialog();
            }
        }
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
        ui.label(bind_hint());

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
                                ui.add(button).on_hover_text(capture_tooltip())
                            } else {
                                ui.add(button)
                            };
                            // `on_hover_text` is silent on disabled widgets (egui
                            // 0.37), and both tooltip methods consume the
                            // response, so the result is rebound, not cloned.
                            let response = if reserved {
                                response.on_disabled_hover_text(reserved_tooltip())
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
                                    ui.add(pad_button).on_hover_text(pad_capture_tooltip())
                                } else {
                                    ui.add(pad_button).on_hover_text(pad_bind_tooltip())
                                };
                                // Same disabled-tooltip rule as the bind button:
                                // the tooltip methods consume the response.
                                let pad_response = if reserved {
                                    pad_response.on_disabled_hover_text(reserved_tooltip())
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
                                ui.add_enabled(
                                    false,
                                    egui::Button::new(gamepad_placeholder_label()),
                                );
                            }
                        });
                    });

                    ui.end_row();
                }
            });

        if self.controller.is_none() {
            ui.label(rust_i18n::t!("hotkey.unavailable_label"));
        }
    }

    fn overlay_tab(&mut self, ui: &mut egui::Ui) {
        let overlay = &mut self.draft.overlay;
        // The test chip is published AFTER the tab body: the tab borrows
        // `draft.overlay` mutably and publishing reads `self.handoff` — the
        // same deferred-action pattern as Browse in the General tab.
        let mut test_chip: Option<Toast> = None;

        ui.horizontal(|ui| {
            ui.checkbox(&mut overlay.enabled, rust_i18n::t!("overlay.enabled_label"));
            let enabled = overlay_test_button_enabled(overlay.enabled);
            let test = ui.add_enabled(
                enabled,
                egui::Button::new(rust_i18n::t!("overlay.test_button")),
            );
            if enabled {
                if test.clicked() {
                    test_chip = Some(test_overlay_toast(overlay));
                }
            } else {
                // `on_disabled_hover_text` consumes the response; a disabled
                // button can never be clicked, so the branches are exclusive.
                test.on_disabled_hover_text(overlay_test_disabled_hint());
            }
        });

        ui.horizontal(|ui| {
            // Catalog `overlay.duration_suffix` is `" ms"` — the leading space
            // is part of the text, so the suffix renders byte-for-byte like
            // the old literal.
            let duration_suffix = rust_i18n::t!("overlay.duration_suffix").into_owned();
            ui.add(
                egui::DragValue::new(&mut overlay.duration_ms)
                    .range(250..=10_000)
                    .suffix(duration_suffix.as_str()),
            );
            ui.label(rust_i18n::t!("overlay.duration_label"));
        });

        egui::ComboBox::from_label(rust_i18n::t!("overlay.position_label"))
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

        ui.checkbox(
            &mut overlay.on_all_monitors,
            rust_i18n::t!("overlay.on_all_monitors_label"),
        );
        ui.label(rust_i18n::t!("overlay.all_monitors_hint"));

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

    /// Open the update dialog from the General row (spec §4.7): exactly the
    /// About lazy pattern — fresh close flag, fresh outcome slot, and the
    /// notes fetch thread is spawned HERE and nowhere else (a background
    /// check never pays for notes nobody reads). Re-opening an open dialog
    /// is a no-op (`UpdateDialogWindow::open` returns false).
    fn open_update_dialog(&mut self) {
        if !self.update_dialog.open() {
            return;
        }
        self.dialog_outcome_slot = Arc::new(Mutex::new(None));
        self.notes = None;
        let (tx, rx) =
            mpsc::channel::<Result<update_notes::ReleaseNotes, update_notes::NotesError>>();
        self.notes_rx = Some(rx);
        // Notes fetch (Stage 7 seam re-point, spec §4.7): the seam is
        // `Fn(&str) -> Result<ReleaseNotes, NotesError>`, so the production
        // fetcher is `update_notes::fetch_release_notes` itself — fetch AND
        // parse belong to `update_notes`, and this wiring point passes the fn
        // item with no error mapping (SRP: `update` owns the `release.yaml`
        // contract, `update_notes` owns the GitHub Releases API contract).
        update_notes::spawn_notes_fetch(
            self.applied.general.update_channel,
            tx,
            update_notes::fetch_release_notes,
        );
    }

    /// Close the dialog from the Cancel button (an OS close goes through
    /// `observe_close` in `logic` instead): stop registration next pass —
    /// egui GCs the viewport and eframe drops the `Arc<Window>` — and drop
    /// the notes state, so a still-pending fetch send is ignored by the
    /// closed receiver (module contract) and a re-open fetches fresh.
    fn dismiss_update_dialog(&mut self) {
        self.update_dialog.close();
        self.notes_rx = None;
        self.notes = None;
    }

    /// Start the install path (spec §4.7 — auto-install on discovery or a
    /// dialog Install press): ONE plain std thread (repo thread model, no
    /// async) running `update_install::install` with
    /// `self_replace::self_replace` as the replace seam. The single result
    /// arrives over a dedicated channel drained in `logic`; nothing here
    /// restarts or exits — `RestartPending` is the Stage 7 seam (see
    /// [`install_finished`]).
    fn start_install(&mut self, info: update::UpdateInfo) {
        let (tx, rx) = mpsc::channel::<Result<(), String>>();
        self.install_rx = Some(rx);
        self.update_status = UpdateStatus::Installing;
        std::thread::spawn(move || {
            // `InstallError`'s Display text travels verbatim: the row and
            // the danger chip show the real OS/network failure wording,
            // exactly as `capture::chip_text` does for capture failures.
            // A closure (not the bare fn item) is required here: `install`'s
            // replace seam is higher-ranked (`for<'a> Fn(&'a Path)`), and a
            // generic fn item instantiated at the call site is specialized to
            // one lifetime — the closure stays generic over the argument.
            let outcome = update_install::install(&info, |path| self_replace::self_replace(path))
                .map_err(|err| err.to_string());
            let _ = tx.send(outcome);
        });
    }

    /// Publish an update chip through the SAME hand-off slot real saves use
    /// (spec §4.3): the handle never blocks, and the chip spawns on the next
    /// `overlay_host.update` pass in `logic`. A missing handle (no toast
    /// service) or a closed channel is WARNed, never fatal (spec §5) — the
    /// update result itself is what app state keeps.
    fn publish_update_chip(&self, chip: Toast) {
        let Some(handle) = capture::handoff_handle(&self.handoff) else {
            tracing::warn!("Update chip skipped: no toast handle attached");
            return;
        };
        let text = chip.text.clone(); // `show` consumes the chip; the log quotes it
        if let Err(err) = handle.show(chip) {
            tracing::warn!("Update chip failed: {err}");
            return;
        }
        tracing::info!("Update chip shown: {text}");
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
    /// `About` opens the About child viewport if closed and RAISES it (Focus)
    /// if already open (task 2026-11-10_about-dialog),
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
                    TrayAction::About => {
                        // Tray select (user decision 2026-11-11): a CLOSED
                        // About opens — the first logic pass registers it, the
                        // first paint shows it (deferred viewports start
                        // hidden, like toast chips). An OPEN About is RAISED,
                        // never closed: `Focus` raises the window and
                        // registration stays untouched.
                        if self.about_window.select_from_tray() {
                            // Upload the header icon ONCE on first open (lazy:
                            // no GPU texture while About is never used). A
                            // decode failure is logged and stays `None` — the
                            // header paints text only.
                            if self.about_icon.is_none() {
                                self.about_icon = about_icon_texture(ctx);
                            }
                        } else {
                            ctx.send_viewport_cmd_to(
                                egui::ViewportId::from_hash_of("virshot-about"),
                                egui::ViewportCommand::Focus,
                            );
                        }
                        ctx.request_repaint();
                    }
                    TrayAction::Exit => {
                        self.exit_requested = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        ctx.request_repaint();
                    }
                }
            }
        }

        // About child viewport (task 2026-11-10_about-dialog, stage 10):
        // registered on EVERY pass while open (egui closes an unregistered
        // child viewport — `crates/overlay/src/host.rs:314-323` precedent).
        // An OS close CANNOT be observed here: eframe pushes
        // `ViewportEvent::Close` into the CHILD's `ViewportInfo`
        // (`glow_integration.rs:1075`), the child pass runs before this root
        // pass (`run.rs:147-152`), and eframe clears the child's info after
        // its pass (`glow_integration.rs:853`), so the old `raw.viewports…
        // close_requested()` read was dead. `paint` records the request into
        // the shared flag inside the CHILD pass, and `observe_close()` turns
        // it into "closed" here. Stopping registration destroys the window:
        // egui GCs the viewport and eframe drops the `Arc<Window>` (R10).
        // No `ViewportCommand::Close`, no `CancelClose` (root-only, ignored
        // for children), no `Visible(false)` (phantom hidden window). An open
        // About never blocks tray Exit: Exit closes the ROOT viewport and
        // eframe tears down child viewports on shutdown (spec open question 5).
        self.about_window.observe_close();
        if self.about_window.wants_registration() {
            let about_viewport = egui::ViewportId::from_hash_of("virshot-about");
            if let Some(close_flag) = self.about_window.close_flag() {
                let cache = Arc::clone(&self.about_changelog_cache);
                // Arc clone is O(1) — one reference-count bump. The callback
                // is `Send + Sync + 'static` (egui 0.37), so the closure owns
                // a shared handle to the app-state texture instead of
                // uploading a new one per paint pass.
                let about_icon = self.about_icon.clone();
                // Chrome policy from `about::window_chrome()` (stage 9): title
                // bar, close button and OS resize border stay; minimize and
                // maximize buttons are off. The flags fold into winit's
                // `WindowAttributes::with_enabled_buttons` at window creation;
                // honored on Windows/macOS, inert on X11/Wayland. The builder
                // stays constant per viewport id — egui applies builder
                // changes by recreating the OS window.
                let chrome = crate::about::window_chrome();
                ctx.show_viewport_deferred(
                    about_viewport,
                    egui::ViewportBuilder::default()
                        .with_title(rust_i18n::t!("about.title"))
                        .with_inner_size([480.0, 360.0])
                        .with_decorations(chrome.decorations)
                        .with_resizable(chrome.resizable)
                        .with_close_button(chrome.close_button)
                        .with_minimize_button(chrome.minimize_button)
                        .with_maximize_button(chrome.maximize_button),
                    move |ui, _class| {
                        crate::about::paint(
                            ui,
                            &mut cache
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner()),
                            about_icon.as_deref(),
                            &close_flag,
                        );
                    },
                );
                ctx.request_repaint_of(about_viewport);
            }
        }

        // ── Auto-update (task 2026-11-11_auto-update, Stages 6-9, spec §4.7) ──
        // ONE clock read per pass: the deadline arming below, the drain, the
        // deadline comparison and the repaint request all share it.
        let now = Instant::now();
        // Interval scheduling (Stage 7, spec §4.7): the deadline is armed on the
        // FIRST pass — the startup check was spawned by `main.rs` before the
        // event loop — and re-armed after every drained SCHEDULED outcome, so
        // the next check follows the interval the settings publish rather than
        // the request that produced the outcome. Dev builds schedule on the
        // same terms as release builds; only `Never` yields `None` from
        // `update::next_check_deadline`.
        //
        // Arming runs BEFORE the drain: on the first pass an armed deadline
        // means the startup check is in flight (both are the same interval
        // gate), and a startup outcome that drains on that same pass — a fast
        // failure such as an HTTP 404, or a slow window init — must free the
        // slot. Arming after the drain put the flag back ON after the outcome
        // was already drained and nothing cleared it again: no scheduled check
        // ever fired and «Проверить обновления» stayed disabled all session.
        let armed_this_pass = !self.deadline_armed;
        if armed_this_pass {
            self.deadline_armed = true;
            self.next_check =
                update::next_check_deadline(now, self.applied.general.update_check.interval());
        }

        // Drain background-check outcomes: they are the source state for the
        // row, the dialog, and the install path. A FAILED scheduled CHECK is a
        // log line only — the scheduled check below retries it on the next
        // interval. An UP-TO-DATE outcome RESETS the held discovery (Stage 11
        // sub-item 4: a check outcome replaces the previous result — see
        // `discovery_cleared_by_check_outcome`). The latest held discovery
        // wins if several arrive in one pass.
        let mut discovery: Option<update::UpdateInfo> = None;
        let mut outcome_drained = false;
        // The slot holds exactly one in-flight check, so its forced flag
        // decides the result line AND the reset rule for every outcome this
        // pass drains (sub-items 2+4).
        let forced_check = self.forced_check_in_flight;
        if let Some(rx) = &self.update_rx {
            while let Ok(outcome) = rx.try_recv() {
                outcome_drained = true;
                // Sub-item 2: a FORCED outcome REPLACES the beside-button
                // line; a scheduled outcome leaves the slot untouched.
                self.forced_check_status = forced_check_status_after(
                    self.forced_check_status.take(),
                    &outcome,
                    forced_check,
                );
                // Sub-item 4: stale-discovery reset, wired through the pure
                // helper so the rule is unit-testable without a Context.
                if discovery_cleared_by_check_outcome(&outcome, forced_check, &self.update_status) {
                    self.update = None;
                }
                match outcome {
                    update::CheckOutcome::Updated(info) => discovery = Some(info),
                    update::CheckOutcome::UpToDate => {}
                    update::CheckOutcome::Failed(err) => {
                        tracing::warn!("update check failed: {err}");
                    }
                }
            }
        }
        if let Some(info) = discovery {
            // Stage 7 (§4.7): a NEW discovery while an install FAILURE is
            // displayed puts the row back to Idle — `status_after_new_discovery`
            // — so Install is offered again instead of keeping the stale
            // failure error text. One clone: the state is read again by the
            // row and dialog passes later in this function.
            self.update_status = status_after_new_discovery(self.update_status.clone());
            // Success-style chip, overlay-enabled gate only (§4.3 — the same
            // suppression save toasts use). The chip text is built before
            // `info` moves into the install/discovery path.
            if update_toast_enabled(Some(&info), self.applied.overlay.enabled) {
                let chip = update_discovery_chip(&self.applied.overlay, &info);
                self.publish_update_chip(chip);
            }
            if auto_install_on_discovery(true, self.applied.general.auto_install) {
                // Auto-install ON (§4.7): straight to the install path — no
                // dialog, no row; the discovery is never held.
                self.start_install(info);
            } else {
                // Dialog path: the General row shows the discovery until an
                // install succeeds (or checks are switched off).
                self.update = Some(info);
            }
            ctx.request_repaint();
        }
        // Any drained outcome — Updated, UpToDate, or Failed — ends the check
        // that produced it, so the forced-check button is enabled again (§4.7).
        // Only a SCHEDULED outcome re-arms the interval deadline; a FORCED
        // outcome must not move it (`deadline_rearmed_by_outcome`, §4.7).
        if outcome_drained {
            if deadline_rearmed_by_outcome(self.forced_check_in_flight) {
                self.next_check =
                    update::next_check_deadline(now, self.applied.general.update_check.interval());
            }
            self.forced_check_in_flight = false;
        }
        // The single in-flight slot is decided in ONE place, after the arm and
        // the drain, so the first-pass arm cannot re-occupy a slot the drain
        // just freed (see [`check_in_flight_after`]). Exactly one check is in
        // flight at a time, and the deadline consumes only while it is free.
        self.check_in_flight = check_in_flight_after(
            self.check_in_flight,
            outcome_drained,
            if armed_this_pass {
                self.next_check
            } else {
                None
            },
        );
        // Scheduled check. A deadline that arrives while a FORCED check is
        // running is not consumed: it fires on the next pass, once the forced
        // outcome drains, so exactly one check thread is live at a time.
        if let Some(deadline) = self.next_check
            && now >= deadline
            && !self.check_in_flight
        {
            self.next_check = None;
            self.check_in_flight = true;
            // The Sender clone is the thread boundary: `spawn_check` moves an
            // owned sender into its thread while app state keeps its own for
            // the next deadline (O(1) clone, no `Arc`).
            update::spawn_check(
                self.applied.general.update_channel,
                update::current_build(),
                self.update_tx.clone(),
                update::fetch_manifest_text,
            );
        }
        // Forced check (Stage 9, spec §4.7): the press the paint pass recorded.
        // The interval is IGNORED — `Never` checks too — and the scheduled
        // deadline is neither moved nor re-armed. `Never` schedules nothing, so
        // this button is the only check path that setting has. A press the busy
        // slot cannot honor stays RECORDED rather than dropped: a scheduled
        // spawn that took the slot on this very pass must not swallow it — the
        // forced check runs on the next pass, once the slot frees (the tray tick
        // keeps logic passes coming, so no extra repaint is needed).
        let forced = forced_check_decision(
            self.applied.general.update_check,
            self.check_now_requested,
            self.check_in_flight,
        );
        if forced.spawn {
            self.check_now_requested = false;
            self.check_in_flight = true;
            self.forced_check_in_flight = true;
            update::spawn_check(
                self.applied.general.update_channel,
                update::current_build(),
                self.update_tx.clone(),
                update::fetch_manifest_text,
            );
        }
        if forced.pending {
            tracing::debug!("forced check deferred: a check is already in flight");
        }

        // Install-thread result (exactly one per install): transition the
        // shared state through the pure decision functions so the row and
        // dialog reflect it.
        if let Some(rx) = &self.install_rx
            && let Ok(outcome) = rx.try_recv()
        {
            self.install_rx = None;
            self.update_status = install_finished(&outcome);
            self.update = update_info_after_install(self.update.take(), outcome.is_ok());
            match &outcome {
                Ok(()) => {
                    // Stage 7 (spec §5): the binary IS replaced, so this
                    // process must run the new one. `restart_requested` marks
                    // the shutdown as an explicit EXIT (the close flow below
                    // never cancels it), and the viewport close makes
                    // `run_native` return; `main.rs` drops the instance lock,
                    // spawns the new binary, and exits 0. Startup cleanup of
                    // self-replace leftovers happens on EVERY start there.
                    tracing::info!("update installed: restart pending");
                    self.restart_requested.store(true, Ordering::Relaxed);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Err(message) => {
                    tracing::warn!("update install failed: {message}");
                    // Danger chip ALWAYS shown (§4.3: failures ignore the
                    // overlay toggle), with the verbatim English text.
                    self.publish_update_chip(update_failure_chip(&self.applied.overlay, message));
                }
            }
            ctx.request_repaint();
        }

        // Notes hand-off, drained only while the dialog is open (the
        // receiver is created on open and dropped on close); exactly one
        // value ever arrives.
        if let Some(rx) = &self.notes_rx
            && let Ok(outcome) = rx.try_recv()
        {
            self.notes_rx = None;
            self.notes = Some(match outcome {
                Ok(notes) => Ok(Arc::new(notes)),
                Err(err) => Err(err),
            });
        }

        // Update dialog: the exact About child-viewport lifecycle — an OS
        // close request is recorded by the CHILD pass into the shared flag,
        // this pass stops registration, and egui GCs the viewport (eframe
        // drops the `Arc<Window>`). The Cancel button reaches the same end
        // via `dismiss_update_dialog`. Closing also drops the notes state
        // (a pending fetch send is ignored by the closed receiver).
        if self.update_dialog.observe_close() {
            self.dismiss_update_dialog();
        }
        if self.update_dialog.wants_registration() {
            let update_viewport = egui::ViewportId::from_hash_of("virshot-update");
            if let Some(close_flag) = self.update_dialog.close_flag() {
                // Owned snapshots for the `'static` callback: two O(1) Arc
                // bumps (cache, outcome slot), one Option clone of notes
                // (Arc bump inside) and one of the small discovery summary
                // and install status — the callback cannot borrow app state.
                let cache = Arc::clone(&self.update_notes_cache);
                let slot = Arc::clone(&self.dialog_outcome_slot);
                let notes = self.notes.clone();
                let info = self.update.clone();
                let channel = self.applied.general.update_channel; // Copy
                let status = self.update_status.clone();
                // Same chrome policy as About (task 2026-11-10): title bar +
                // close button + resizable, NO minimize/maximize.
                let chrome = crate::about::window_chrome();
                ctx.show_viewport_deferred(
                    update_viewport,
                    egui::ViewportBuilder::default()
                        .with_title(rust_i18n::t!("update.dialog_title"))
                        .with_inner_size([480.0, 360.0])
                        .with_decorations(chrome.decorations)
                        .with_resizable(chrome.resizable)
                        .with_close_button(chrome.close_button)
                        .with_minimize_button(chrome.minimize_button)
                        .with_maximize_button(chrome.maximize_button),
                    move |ui, _class| {
                        paint_update_dialog(
                            ui,
                            &mut cache
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner()),
                            &close_flag,
                            &slot,
                            &UpdateDialogView {
                                notes: notes.as_ref(),
                                info: info.as_ref(),
                                channel,
                                status: &status,
                            },
                        );
                    },
                );
                // A hidden settings window still repaints this viewport on
                // its own schedule; nudge it so notes arrival lands promptly.
                ctx.request_repaint_of(update_viewport);
            }
            // Presses recorded by the CHILD pass of the previous frame (the
            // child pass runs BEFORE the root pass — same one-frame ordering
            // as the About close flag): gate them against CURRENT state.
            let pressed = self
                .dialog_outcome_slot
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take();
            if let Some(button) = pressed {
                match dialog_outcome(button, &self.update_status, self.update.as_ref()) {
                    Some(UpdateDialogOutcome::Dismiss) => {
                        self.dismiss_update_dialog();
                    }
                    Some(UpdateDialogOutcome::Install(info)) => {
                        self.dismiss_update_dialog();
                        self.start_install(info);
                    }
                    // `None`: an Install press the current state rejects (the
                    // button is disabled anyway) — dialog stays open, effect
                    // nothing.
                    None => {}
                }
            }
        }

        if ctx.input(|i| i.viewport().close_requested()) {
            if self.restart_requested.load(Ordering::Relaxed) {
                // Stage 7 (spec §5): this Close request is the one the install
                // success sent above — an explicit EXIT, never cancelled, so
                // `run_native` returns and `main.rs` drops the instance lock,
                // spawns the new binary, and exits 0. An X-button close while
                // a restart is pending exits too: the installed binary has to
                // run either way, and hiding would keep the old one alive.
                tracing::info!("virshot exiting: update installed, restarting");
            } else {
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
        // A pending check deadline needs a pass AT the deadline instant, even
        // while the window is hidden: requesting the shorter of the two waits
        // keeps the tray tick alive and bounds the check drift to this tick
        // (`egui` keeps the earliest scheduled repaint, so this is additive).
        if let Some(deadline) = self.next_check {
            ctx.request_repaint_after(deadline.saturating_duration_since(now).min(TRAY_POLL_TICK));
        }
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
                Some(unbound_save_tooltip())
            } else if !duplicates.is_empty() {
                Some(duplicate_save_tooltip())
            } else {
                None
            };
            let mut apply_clicked = false;
            let mut ok_clicked = false;
            let mut cancel_clicked = false;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                cancel_clicked = ui
                    .add_enabled(
                        buttons.cancel_enabled,
                        egui::Button::new(rust_i18n::t!("bottom.cancel")),
                    )
                    .clicked();
                let ok_response = ui.add_enabled(
                    buttons.ok_enabled,
                    egui::Button::new(rust_i18n::t!("bottom.ok")),
                );
                let apply_response = ui.add_enabled(
                    buttons.apply_enabled,
                    egui::Button::new(rust_i18n::t!("bottom.apply")),
                );
                // Only a blocked save gets a hint: a clean draft is disabled
                // for dirtiness alone and needs no tooltip. The tooltip method
                // consumes the response, so it is rebound.
                let ok_response = if let Some(tooltip) = &blocked_tooltip {
                    ok_response.on_disabled_hover_text(tooltip)
                } else {
                    ok_response
                };
                let apply_response = if let Some(tooltip) = &blocked_tooltip {
                    apply_response.on_disabled_hover_text(tooltip)
                } else {
                    apply_response
                };
                ok_clicked = ok_response.clicked();
                apply_clicked = apply_response.clicked();
                if is_dirty(&self.draft, &self.applied) {
                    ui.label(rust_i18n::t!("bottom.unsaved_changes"));
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
#[path = "gui.tests.rs"]
mod tests;
