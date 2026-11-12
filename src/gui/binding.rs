//! Pure binding logic for the Hotkeys tab (spec §4.4): the key-name tables,
//! binding construction and labels, the duplicate/save gate, the registration
//! retry decision, and the capture state machine — no egui painting and no
//! app state; the `SettingsApp` wiring calls these through `binding::`.

use crate::bindings::Action;
use crate::config::Binding;
use crate::config::Settings;
use crate::gamepad::PadDownEvent;
use crate::gamepad::pad_notation;
use crate::hotkeys::ProbePress;
use crate::hotkeys::Registration;
use crate::hotkeys::key_label;

use super::hotkeys::row_label;

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

/// Tooltip for the reserved-slot info icon (§4.4: rect is out of MVP scope,
/// catalog `hotkey.reserved_tooltip`).
pub(crate) fn reserved_tooltip() -> String {
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

/// Empty pad field on Windows, ready to capture (catalog `hotkey.pad_label`).
fn pad_unbound_label() -> String {
    rust_i18n::t!("hotkey.pad_label").into_owned()
}

/// Pad field on a host with no GameInput backend (catalog
/// `gamepad.windows_only_note`). Linux gilrs is out of scope (AGENTS.md
/// hardware finding), so the field stays a disabled placeholder there; the pad
/// trigger still works from `config.toml` notation once a Windows pump
/// delivers it.
pub(crate) fn gamepad_placeholder_label() -> String {
    rust_i18n::t!("gamepad.windows_only_note").into_owned()
}

/// Bind-button label of a row whose keyboard binding was cleared by Backspace
/// (catalog `hotkey.unassigned`). The empty `key` is the unbound marker —
/// §4.1 notation has no empty key, so the button cannot show a binding and
/// says the row needs a press instead.
fn key_unbound_label() -> String {
    rust_i18n::t!("hotkey.unassigned").into_owned()
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
pub(crate) fn binding_from_press(key: &str, mods: Vec<String>, pad: Option<String>) -> Binding {
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

/// Tooltip for a disabled Apply/OK while some row is unbound (catalog
/// `hotkey.bind_all_hint`): saving a cleared binding writes a key
/// `bindings::parse_key` rejects (§4.1), which costs the app its hotkey
/// registrations and its pad triggers, so saving waits until the row is bound
/// again.
pub(crate) fn unbound_save_tooltip() -> String {
    rust_i18n::t!("hotkey.bind_all_hint").into_owned()
}

/// Tooltip for a disabled Apply/OK while two rows share a trigger (§4.4,
/// Stage 3, catalog `hotkey.duplicate_tooltip`): one keyboard combo or pad
/// button may trigger only one action, so saving waits until the draft is
/// unique again.
pub(crate) fn duplicate_save_tooltip() -> String {
    rust_i18n::t!("hotkey.duplicate_tooltip").into_owned()
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

#[cfg(test)]
#[path = "binding.tests.rs"]
mod tests;
