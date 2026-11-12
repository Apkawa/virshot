//! Input binding model (spec §4.1): config `controls` → typed capture actions
//! with `global-hotkey` combos, plus the per-action debounce gate (one shared
//! instance gates both trigger sources — keyboard hotkey and pad button, so an
//! action fires once per interval, not once per source). Pure logic
//! only — registration, crate calls, and threads belong to `hotkeys`.
//!
//! MVP semantics: `full_screen` (PrintScreen) and `window` (Alt+PrintScreen)
//! are **active**; `rect` (Ctrl+PrintScreen) is typed as **reserved** — parsed
//! and reported, never registered (spec §4.1 reserves the rect-area action).
//! `pad` is the gamepad slot: parsed here into [`PadBind`] — a canonical
//! GameInput button name (`XboxX`, `XboxLeftTrigger`, …) kept as a **named**
//! bind because its controller bit index is device-dependent and resolves at
//! runtime from device-info (`gamepad::pad_index_for_name`), legacy gilrs-style
//! names and `unknown:<N>` pinned to fixed indices, `Guide` / `Share` system
//! buttons. A filled `pad` parses alongside the keyboard
//! combo and is carried on [`ActionBinding`]; the gamepad module (Stage 4)
//! consumes it as the trigger OR-fire partner of the hotkey (spec §4.1 "action
//! fires on any filled field (OR)").

use chrono::{DateTime, Local};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use global_hotkey::hotkey::Code;
use global_hotkey::hotkey::HotKey;
use global_hotkey::hotkey::Modifiers;

use crate::config::Binding;
use crate::config::Controls;

/// Capture trigger action (spec §4.1): full screen, window, rect area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Action {
    FullScreen,
    Window,
    /// Reserved in MVP: parsed, never registered.
    Rect,
}

impl Action {
    /// Config section name, used in error messages and logs.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Action::FullScreen => "full_screen",
            Action::Window => "window",
            Action::Rect => "rect",
        }
    }
}

impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// One parsed binding: which action it triggers, the combo to register, the
/// gamepad trigger (if any), and whether MVP registers the hotkey (`rect` →
/// inactive). `Clone`, not `Copy`: `PadBind::Named` carries a String.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActionBinding {
    pub(crate) action: Action,
    pub(crate) hotkey: HotKey,
    /// Gamepad trigger parsed from the config `pad` slot (`None` = keyboard
    /// only). Consumed by the gamepad module, not by `hotkeys`.
    pub(crate) pad: Option<PadBind>,
    pub(crate) active: bool,
}

/// Gamepad trigger from the config `pad` notation (see [`parse_pad`]): a
/// controller button by its GameInput button index, a canonical GameInput
/// button NAME, or a system button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PadBind {
    /// Controller button index in GameInput button order (0..=63).
    Controller(u8),
    /// Canonical GameInput button name as written in config (the pad pump
    /// writes the device-info label verbatim, e.g. `XboxX`). The bit index of a
    /// button is device-dependent (§9), so the name resolves against the
    /// connected devices' label tables per event; an unresolved name fires
    /// nothing (logged once, not per event).
    Named(String),
    /// Xbox Guide button (system button, not a controller button).
    Guide,
    /// Xbox Share button (system button, not a controller button).
    Share,
}

/// Binding failures, typed per module (`thiserror` enum, no stringly
/// `Err(String)`). All variants name the offending config section.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
// The `Unknown*` prefix is meaningful: each variant names the notation it
// rejects (key, modifier, pad), and callers match on them by name.
#[allow(clippy::enum_variant_names)]
pub(crate) enum Error {
    /// The `key` name is outside the supported MVP notation (see [`parse_key`]).
    #[error("unknown key \"{key}\" in binding [controls.{action}]")]
    UnknownKey { action: Action, key: String },

    /// The `key` name is reserved for the settings UI (§4.4): `Escape` cancels
    /// a bind-by-press capture and `Backspace` clears a row, so neither may
    /// become a binding. A config that names one is a visible startup error,
    /// not a silently dropped binding.
    #[error(
        "reserved key \"{key}\" in binding [controls.{action}]: \
         Escape cancels capture, Backspace clears a binding"
    )]
    ReservedKey { action: Action, key: String },

    /// The modifier name is outside the supported set (see [`parse_mod`]).
    #[error("unknown modifier \"{mod_name}\" in binding [controls.{action}]")]
    UnknownMod { action: Action, mod_name: String },

    /// The `pad` name is outside the supported pad notation (see [`parse_pad`]).
    #[error("unknown pad \"{pad}\" in binding [controls.{action}]")]
    UnknownPad { action: Action, pad: String },
}

/// Minimum gap between two triggers of the same action (spec §4.1 debounce
/// ~250 ms).
const DEBOUNCE_INTERVAL: Duration = Duration::from_millis(250);

/// Per-action trigger gate. `now` is injected by the caller (the hotkey event
/// thread stamps the event), so unit tests use fixed `DateTime`s.
#[derive(Debug, Clone)]
pub(crate) struct Debounce {
    interval: Duration,
    last_fired: HashMap<Action, DateTime<Local>>,
}

impl Debounce {
    pub(crate) fn new(interval: Duration) -> Self {
        Self {
            interval,
            last_fired: HashMap::new(),
        }
    }

    /// Returns `true` when `action` has not fired in the last `interval` and
    /// records `now`. A repeat shorter than the interval returns `false` and
    /// leaves the recorded time untouched (a blocked trigger must not push the
    /// window forward).
    pub(crate) fn try_fire(&mut self, action: Action, now: DateTime<Local>) -> bool {
        let blocked = self
            .last_fired
            .get(&action)
            .map(|previous| (now - *previous).num_milliseconds() < self.interval.as_millis() as i64)
            .unwrap_or(false);
        if blocked {
            return false;
        }
        self.last_fired.insert(action, now);
        true
    }
}

impl Default for Debounce {
    fn default() -> Self {
        Self::new(DEBOUNCE_INTERVAL)
    }
}

/// Fire through a gate shared by BOTH trigger sources — the hotkey listener
/// thread and the gamepad pump thread (spec §4.1: an action fires on the
/// keyboard combo OR the pad button, so one shot per action, not one per
/// source). Same rule as [`Debounce::try_fire`]: a repeat shorter than the
/// interval is blocked and does not push the window forward. A poisoned lock
/// (a panicked trigger) is recovered rather than unwrapped, like the hotkey
/// settings mailbox.
pub(crate) fn try_fire_shared(
    gate: &Mutex<Debounce>,
    action: Action,
    now: DateTime<Local>,
) -> bool {
    let mut guard = gate.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.try_fire(action, now)
}

/// Parse all three control bindings (spec §4.1 order: full_screen, window,
/// rect). Fails on the first unknown key, modifier, or pad name; the reserved
/// `rect` binding is parsed too (typed inactive) so a typo there is a visible
/// config error, not a silently dropped binding.
pub(crate) fn parse_controls(controls: &Controls) -> Result<Vec<ActionBinding>, Error> {
    Ok(vec![
        parse_binding(Action::FullScreen, &controls.full_screen)?,
        parse_binding(Action::Window, &controls.window)?,
        parse_binding(Action::Rect, &controls.rect)?,
    ])
}

/// Parse one binding: keyboard combo plus the optional gamepad `pad` trigger.
/// The keyboard notation is checked first; a bad `pad` is its own error, so a
/// filled `pad` never changes how the keyboard fields fail.
fn parse_binding(action: Action, binding: &Binding) -> Result<ActionBinding, Error> {
    // Reserved keys are checked before the notation so a typo reports the
    // reservation reason instead of an "unknown key". Bind-by-press already
    // intercepts them in `src/gui/hotkeys.rs` (`handle_capture_press`); this is the config path.
    if reserved_key_name(&binding.key).is_some() {
        return Err(Error::ReservedKey {
            action,
            key: binding.key.clone(),
        });
    }
    let key = parse_key(&binding.key).ok_or_else(|| Error::UnknownKey {
        action,
        key: binding.key.clone(),
    })?;
    let mut mods = Modifiers::empty();
    for mod_name in &binding.mods {
        mods.insert(parse_mod(mod_name).ok_or_else(|| Error::UnknownMod {
            action,
            mod_name: mod_name.clone(),
        })?);
    }
    let pad = match binding.pad.as_deref() {
        Some(name) => Some(parse_pad(name).ok_or_else(|| Error::UnknownPad {
            action,
            pad: name.to_owned(),
        })?),
        None => None,
    };
    Ok(ActionBinding {
        action,
        hotkey: HotKey::new(Some(mods), key),
        pad,
        active: matches!(action, Action::FullScreen | Action::Window),
    })
}

/// Map a config key name onto `global_hotkey::hotkey::Code` (the crate
/// re-exports `keyboard_types::Code`; there is no `Key` type in its API).
///
/// Notation, case-insensitive. Canonical names are the crate's `Code`
/// spellings (`Code::to_string()`, so the config matches what the crate
/// registers); the legacy MVP spellings stay accepted and stable: a single
/// letter `A`–`Z` (= `KeyA`..`KeyZ`), a single digit `0`–`9` (=
/// `Digit0`..`Digit9`), `F1`..`F24`, `PrintScreen`, the egui-style arrow
/// aliases `Down`/`Left`/`Right`/`Up` (= `Arrow*`), and `Menu` (=
/// `ContextMenu`).
///
/// Accepted set = every code `global-hotkey 0.9.0` can register on Windows,
/// verified against its table `platform_impl/windows/mod.rs:205-351`
/// (`key_to_vk`, whose `_ => None` at `:349` is the cap). Linux translates
/// codes to keysyms (`platform_impl/x11/mod.rs:330`) with a wider table
/// (`F25`..`F35`, `Power`), so the shipped Windows target sets the cap: a name
/// past it is a config error on every platform, not a Windows-only failure.
///
/// Excluded deliberately: `Escape`/`Backspace` (reserved for the settings UI,
/// [`Error::ReservedKey`]); modifier keys as a main key (`ShiftLeft`,
/// `ControlLeft`, `AltLeft`, `Meta`, `Super*` — no virtual-key arm); the lock
/// toggles `CapsLock`/`NumLock`/`ScrollLock` (a bare registration would
/// swallow the toggle system-wide); `Fn`/`FnLock`/`Eject` (no virtual key);
/// and codes the table does not map (`Power`, `Clear`, `MediaRecord`,
/// `MediaRewind`, `MediaFastForward`, `NumpadBackspace`, …). `F25`..`F35` are
/// delivered by egui but have no Windows virtual key, so they stay rejected.
///
/// Codes that are bindable but never delivered by egui — `MediaPlay`,
/// `MediaPause`, `MediaSelect`, `Convert`, `KanaMode`, `NonConvert`, `IntlRo`,
/// `IntlYen`, and the numpad codes `NumpadDecimal`/`NumpadComma`/`NumpadEqual`/
/// `NumpadMultiply`/`NumpadClear` (egui collapses the numpad into digits,
/// `Enter`, `Plus`, `Minus`, `Slash`) — are writable in `config.toml` and
/// registered at startup/re-registration, but cannot be bound by a press in
/// the settings window: the UI allowlist is `gui::binding::SUPPORTED_KEY_NAMES`.
///
/// Once a bare navigation key (`Insert`, `Delete`, `Home`, `End`, `PageUp`,
/// `PageDown`, the arrows) is registered it is consumed system-wide, so other
/// applications lose it. The clipboard chords `Ctrl+Insert`, `Shift+Insert`
/// and `Shift+Delete` arrive as clipboard commands (`egui-winit
/// 0.37.0/src/lib.rs:1538-1554`) and are swallowed before any `Event::Key`
/// (`:1129-1152`), so they cannot be captured at all.
fn parse_key(name: &str) -> Option<Code> {
    let lowered = name.to_ascii_lowercase();
    let name = lowered.as_str();
    // Legacy notation: a lone letter, a lone digit, `F<n>`.
    if let Some(code) = letter_from(name) {
        return Some(code);
    }
    if let Some(code) = digit_from(name) {
        return Some(code);
    }
    if let Some(number) = name.strip_prefix('f') {
        let number = number.parse::<u8>().ok()?;
        return (1..=24).contains(&number).then_some(function_code(number));
    }
    // Crate spelling of the same two notation families; `letter_from`/
    // `digit_from` keep the remainder to exactly one character.
    if let Some(code) = name.strip_prefix("key").and_then(letter_from) {
        return Some(code);
    }
    if let Some(code) = name.strip_prefix("digit").and_then(digit_from) {
        return Some(code);
    }
    match name {
        "printscreen" => Some(Code::PrintScreen),
        "arrowdown" | "down" => Some(Code::ArrowDown),
        "arrowleft" | "left" => Some(Code::ArrowLeft),
        "arrowright" | "right" => Some(Code::ArrowRight),
        "arrowup" | "up" => Some(Code::ArrowUp),
        "space" => Some(Code::Space),
        "tab" => Some(Code::Tab),
        "enter" => Some(Code::Enter),
        "insert" => Some(Code::Insert),
        "delete" => Some(Code::Delete),
        "home" => Some(Code::Home),
        "end" => Some(Code::End),
        "pageup" => Some(Code::PageUp),
        "pagedown" => Some(Code::PageDown),
        "contextmenu" | "menu" => Some(Code::ContextMenu),
        "scrolllock" => Some(Code::ScrollLock),
        "pause" => Some(Code::Pause),
        "help" => Some(Code::Help),
        "sleep" => Some(Code::Sleep),
        "comma" => Some(Code::Comma),
        "period" => Some(Code::Period),
        "minus" => Some(Code::Minus),
        "equal" => Some(Code::Equal),
        "semicolon" => Some(Code::Semicolon),
        "slash" => Some(Code::Slash),
        "backslash" => Some(Code::Backslash),
        "quote" => Some(Code::Quote),
        "backquote" => Some(Code::Backquote),
        "bracketleft" => Some(Code::BracketLeft),
        "bracketright" => Some(Code::BracketRight),
        "intlbackslash" => Some(Code::IntlBackslash),
        "intlro" => Some(Code::IntlRo),
        "intlyen" => Some(Code::IntlYen),
        "convert" => Some(Code::Convert),
        "kanamode" => Some(Code::KanaMode),
        "nonconvert" => Some(Code::NonConvert),
        "browserback" => Some(Code::BrowserBack),
        "browserfavorites" => Some(Code::BrowserFavorites),
        "browserforward" => Some(Code::BrowserForward),
        "browserhome" => Some(Code::BrowserHome),
        "browserrefresh" => Some(Code::BrowserRefresh),
        "browsersearch" => Some(Code::BrowserSearch),
        "browserstop" => Some(Code::BrowserStop),
        "launchmail" => Some(Code::LaunchMail),
        "launchapp1" => Some(Code::LaunchApp1),
        "launchapp2" => Some(Code::LaunchApp2),
        "mediaselect" => Some(Code::MediaSelect),
        "mediaplay" => Some(Code::MediaPlay),
        "mediapause" => Some(Code::MediaPause),
        "mediaplaypause" => Some(Code::MediaPlayPause),
        "mediastop" => Some(Code::MediaStop),
        "mediatracknext" => Some(Code::MediaTrackNext),
        "mediatrackprevious" => Some(Code::MediaTrackPrevious),
        "audiovolumedown" => Some(Code::AudioVolumeDown),
        "audiovolumeup" => Some(Code::AudioVolumeUp),
        "audiovolumemute" => Some(Code::AudioVolumeMute),
        "numpad0" => Some(Code::Numpad0),
        "numpad1" => Some(Code::Numpad1),
        "numpad2" => Some(Code::Numpad2),
        "numpad3" => Some(Code::Numpad3),
        "numpad4" => Some(Code::Numpad4),
        "numpad5" => Some(Code::Numpad5),
        "numpad6" => Some(Code::Numpad6),
        "numpad7" => Some(Code::Numpad7),
        "numpad8" => Some(Code::Numpad8),
        "numpad9" => Some(Code::Numpad9),
        "numpadadd" => Some(Code::NumpadAdd),
        "numpadclear" => Some(Code::NumpadClear),
        "numpadcomma" => Some(Code::NumpadComma),
        "numpaddecimal" => Some(Code::NumpadDecimal),
        "numpaddivide" => Some(Code::NumpadDivide),
        "numpadenter" => Some(Code::NumpadEnter),
        "numpadequal" => Some(Code::NumpadEqual),
        "numpadmultiply" => Some(Code::NumpadMultiply),
        "numpadsubtract" => Some(Code::NumpadSubtract),
        _ => None,
    }
}

/// One ASCII letter as a `Code` (`KeyA`..`KeyZ`); anything longer than one
/// character is not a letter name.
fn letter_from(name: &str) -> Option<Code> {
    let letter = name.chars().next()?;
    (name.len() == 1 && letter.is_ascii_alphabetic())
        .then(|| letter_code(letter.to_ascii_uppercase()))
}

/// One ASCII digit as a `Code` (`Digit0`..`Digit9`).
fn digit_from(name: &str) -> Option<Code> {
    let digit = name.chars().next()?;
    (name.len() == 1 && digit.is_ascii_digit()).then(|| digit_code(digit))
}

/// Names reserved for the settings UI (§4.4), case-insensitive, mapped to the
/// canonical name the error message quotes. Checked by [`parse_binding`]
/// before [`parse_key`] so the reason is the reservation, not "unknown key".
fn reserved_key_name(name: &str) -> Option<&'static str> {
    match name.to_ascii_lowercase().as_str() {
        "escape" | "esc" => Some("Escape"),
        "backspace" => Some("Backspace"),
        _ => None,
    }
}

/// keyboard_types letter codes are `KeyA`..`KeyZ`; mapped explicitly (enum
/// order is not relied on).
fn letter_code(letter: char) -> Code {
    let offset = letter as u32 - 'A' as u32;
    match offset {
        0 => Code::KeyA,
        1 => Code::KeyB,
        2 => Code::KeyC,
        3 => Code::KeyD,
        4 => Code::KeyE,
        5 => Code::KeyF,
        6 => Code::KeyG,
        7 => Code::KeyH,
        8 => Code::KeyI,
        9 => Code::KeyJ,
        10 => Code::KeyK,
        11 => Code::KeyL,
        12 => Code::KeyM,
        13 => Code::KeyN,
        14 => Code::KeyO,
        15 => Code::KeyP,
        16 => Code::KeyQ,
        17 => Code::KeyR,
        18 => Code::KeyS,
        19 => Code::KeyT,
        20 => Code::KeyU,
        21 => Code::KeyV,
        22 => Code::KeyW,
        23 => Code::KeyX,
        24 => Code::KeyY,
        _ => Code::KeyZ,
    }
}

/// keyboard_types digit codes are `Digit0`..`Digit9` in ascending order.
fn digit_code(digit: char) -> Code {
    match digit {
        '0' => Code::Digit0,
        '1' => Code::Digit1,
        '2' => Code::Digit2,
        '3' => Code::Digit3,
        '4' => Code::Digit4,
        '5' => Code::Digit5,
        '6' => Code::Digit6,
        '7' => Code::Digit7,
        '8' => Code::Digit8,
        _ => Code::Digit9,
    }
}

/// keyboard_types function codes are `F1`..`F35`; the Windows table of
/// `global-hotkey` stops at `F24`, so that is the notation cap.
fn function_code(number: u8) -> Code {
    match number {
        1 => Code::F1,
        2 => Code::F2,
        3 => Code::F3,
        4 => Code::F4,
        5 => Code::F5,
        6 => Code::F6,
        7 => Code::F7,
        8 => Code::F8,
        9 => Code::F9,
        10 => Code::F10,
        11 => Code::F11,
        12 => Code::F12,
        13 => Code::F13,
        14 => Code::F14,
        15 => Code::F15,
        16 => Code::F16,
        17 => Code::F17,
        18 => Code::F18,
        19 => Code::F19,
        20 => Code::F20,
        21 => Code::F21,
        22 => Code::F22,
        23 => Code::F23,
        _ => Code::F24,
    }
}

/// Map a config modifier name onto `Modifiers` bits (keyboard_types bitflags;
/// the crate names the Win/Command modifier `META`, legacy `SUPER` is folded
/// onto `META` by `HotKey::new`). Accepted, case-insensitive: `Alt`, `Ctrl`,
/// `Control`, `Shift`, `Win`, `Windows`, `Command`, `Meta`, `Super`.
fn parse_mod(name: &str) -> Option<Modifiers> {
    match name.to_ascii_lowercase().as_str() {
        "alt" => Some(Modifiers::ALT),
        "ctrl" | "control" => Some(Modifiers::CONTROL),
        "shift" => Some(Modifiers::SHIFT),
        "win" | "windows" | "command" | "meta" | "super" => Some(Modifiers::META),
        _ => None,
    }
}

/// Canonical GameInput controller button names the config `pad` slot may name
/// — mirrored from `button_name` in `crates/gamepad/src/lib.rs` (the vendored
/// `GameInput.h` label enum is the authority; a `cfg(windows)` test pins the
/// two tables equal). The pad pump writes any device-info label into config via
/// the capture UI, so every name it can produce must parse back: a rejected
/// label would abort `parse_controls` and silently kill ALL pad triggers.
/// `Guide`/`Share` are absent — they parse as system buttons, not controller
/// labels; the header gap code `6` has no name.
const GAMEPAD_CANONICAL_NAMES: &[&str] = &[
    // Xbox console buttons (header codes 1..5).
    "XboxGuide",
    "XboxBack",
    "XboxStart",
    "XboxMenu",
    "XboxView",
    // Xbox controller buttons (7..24).
    "XboxA",
    "XboxB",
    "XboxX",
    "XboxY",
    "XboxDPadUp",
    "XboxDPadDown",
    "XboxDPadLeft",
    "XboxDPadRight",
    "XboxLeftShoulder",
    "XboxLeftTrigger",
    "XboxLeftStickButton",
    "XboxRightShoulder",
    "XboxRightTrigger",
    "XboxRightStickButton",
    "XboxPaddle1",
    "XboxPaddle2",
    "XboxPaddle3",
    "XboxPaddle4",
    // Letter/number buttons (25..60).
    "LetterA",
    "LetterB",
    "LetterC",
    "LetterD",
    "LetterE",
    "LetterF",
    "LetterG",
    "LetterH",
    "LetterI",
    "LetterJ",
    "LetterK",
    "LetterL",
    "LetterM",
    "LetterN",
    "LetterO",
    "LetterP",
    "LetterQ",
    "LetterR",
    "LetterS",
    "LetterT",
    "LetterU",
    "LetterV",
    "LetterW",
    "LetterX",
    "LetterY",
    "LetterZ",
    "Number0",
    "Number1",
    "Number2",
    "Number3",
    "Number4",
    "Number5",
    "Number6",
    "Number7",
    "Number8",
    "Number9",
    // Arrow buttons (61..74).
    "ArrowUp",
    "ArrowUpRight",
    "ArrowRight",
    "ArrowDownRight",
    "ArrowDown",
    "ArrowDownLLeft",
    "ArrowLeft",
    "ArrowUpLeft",
    "ArrowUpDown",
    "ArrowLeftRight",
    "ArrowUpDownLeftRight",
    "ArrowClockwise",
    "ArrowCounterClockwise",
    "ArrowReturn",
    // Icon buttons (75..94).
    "IconBranding",
    "IconHome",
    "IconMenu",
    "IconCross",
    "IconCircle",
    "IconSquare",
    "IconTriangle",
    "IconStar",
    "IconDPadUp",
    "IconDPadDown",
    "IconDPadLeft",
    "IconDPadRight",
    "IconDialClockwise",
    "IconDialCounterClockwise",
    "IconSliderLeftRight",
    "IconSliderUpDown",
    "IconWheelUpDown",
    "IconPlus",
    "IconMinus",
    "IconSuspension",
    // Generic console buttons (95, 97..103, 105..124).
    "Home",
    "Mode",
    "Select",
    "Menu",
    "View",
    "Back",
    "Start",
    "Options",
    "Up",
    "Down",
    "Left",
    "Right",
    "LB",
    "LT",
    "LSB",
    "L1",
    "L2",
    "L3",
    "RB",
    "RT",
    "RSB",
    "R1",
    "R2",
    "R3",
    "PaddleLeft1",
    "PaddleLeft2",
    "PaddleRight1",
    "PaddleRight2",
];

/// Map a config `pad` name onto a [`PadBind`], case-insensitive. Supported
/// notation: `unknown:<N>` with `N` a decimal button index 0..=63 (a raw index
/// from the GUI "press a button" capture on an unlabeled device), the system
/// buttons `Guide` and `Share`, the legacy gilrs-style names (fixed indices,
/// kept for pre-Stage-5 configs), and canonical GameInput controller button
/// names ([`GAMEPAD_CANONICAL_NAMES`]) → a [`PadBind::Named`] storing the
/// config's own spelling verbatim. Anything else returns `None` — a visible
/// config error, never a silently dropped bind.
fn parse_pad(name: &str) -> Option<PadBind> {
    let lowered = name.to_ascii_lowercase();
    if let Some(raw) = lowered.strip_prefix("unknown:") {
        let index = raw.parse::<u8>().ok()?;
        return (index <= 63).then_some(PadBind::Controller(index));
    }
    match lowered.as_str() {
        "guide" => Some(PadBind::Guide),
        "share" => Some(PadBind::Share),
        // Legacy names first: they pin fixed indices, so older configs keep
        // firing without a device-info lookup.
        other => match controller_index(other) {
            Some(index) => Some(PadBind::Controller(index)),
            None => GAMEPAD_CANONICAL_NAMES
                .iter()
                .any(|canonical| canonical.eq_ignore_ascii_case(other))
                .then(|| PadBind::Named(name.to_owned())),
        },
    }
}

/// Legacy gilrs-style names pinned to fixed GameInput button indices (the
/// PoC-era notation, kept for configs written before Stage 5). NOT the GameInput
/// label names — the header has `XboxA..Y`, no `South`/`North` — and the pin is
/// an assumption, not a device truth: the bit index of a button is
/// device-dependent (§9), which is why canonical names resolve at runtime
/// through device-info instead of through this table.
fn controller_index(name: &str) -> Option<u8> {
    match name {
        "north" => Some(0),
        "east" => Some(1),
        "south" => Some(2),
        "west" => Some(3),
        "leftshoulder" => Some(4),
        "rightshoulder" => Some(5),
        "lefttrigger" => Some(6),
        "righttrigger" => Some(7),
        "rightthumbstick" => Some(8),
        "leftthumbstick" => Some(9),
        "leftstick" => Some(10),
        "rightstick" => Some(11),
        _ => None,
    }
}

#[cfg(test)]
#[path = "bindings.tests.rs"]
mod tests;
