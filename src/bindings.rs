//! Input binding model (spec §4.1): config `controls` → typed capture actions
//! with `global-hotkey` combos, plus the per-action debounce gate (one shared
//! instance gates both trigger sources — keyboard hotkey and pad button, so an
//! action fires once per interval, not once per source). Pure logic
//! only — registration, crate calls, and threads belong to `hotkeys`.
//!
//! MVP semantics: `full_screen` (PrintScreen) and `window` (Alt+PrintScreen)
//! are **active**; `rect` (Ctrl+PrintScreen) is typed as **reserved** — parsed
//! and reported, never registered (spec §4.1 reserves the rect-area action).
//! `pad` is the gamepad slot: parsed here into [`PadBind`] (canonical GameInput
//! button name → controller button index, `unknown:<N>` → raw index, `Guide` /
//! `Share` → system button). A filled `pad` parses alongside the keyboard
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
/// inactive).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ActionBinding {
    pub(crate) action: Action,
    pub(crate) hotkey: HotKey,
    /// Gamepad trigger parsed from the config `pad` slot (`None` = keyboard
    /// only). Consumed by the gamepad module, not by `hotkeys`.
    pub(crate) pad: Option<PadBind>,
    pub(crate) active: bool,
}

/// Gamepad trigger from the config `pad` notation (see [`parse_pad`]): a
/// controller button by its GameInput button index, or a system button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PadBind {
    /// Controller button index in GameInput button order (0..=63).
    Controller(u8),
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
/// Supported MVP notation, case-insensitive: `PrintScreen`, a single letter
/// `A`–`Z`, a single digit `0`–`9`, and `F1`–`F12`.
fn parse_key(name: &str) -> Option<Code> {
    if name.eq_ignore_ascii_case("printscreen") {
        return Some(Code::PrintScreen);
    }
    if let Some(letter) = name.chars().next()
        && name.len() == 1
        && letter.is_ascii_alphabetic()
    {
        return Some(letter_code(letter.to_ascii_uppercase()));
    }
    if let Some(digit) = name.chars().next()
        && name.len() == 1
        && digit.is_ascii_digit()
    {
        return Some(digit_code(digit));
    }
    let number = name
        .get(1..)?
        .parse::<u8>()
        .ok()
        .filter(|value| (1..=12).contains(value));
    let function = number?;
    if !name.starts_with('f') && !name.starts_with('F') {
        return None;
    }
    Some(function_code(function))
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

/// keyboard_types function codes are `F1`..`F12`.
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
        _ => Code::F12,
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

/// Map a config `pad` name onto a [`PadBind`], case-insensitive. Supported
/// notation: a canonical GameInput controller button name (see
/// [`controller_index`]), `unknown:<N>` with `N` a decimal button index
/// 0..=63 (a raw index from the GUI "press a button" capture), and the system
/// buttons `Guide` and `Share`. Anything else returns `None`.
fn parse_pad(name: &str) -> Option<PadBind> {
    let lowered = name.to_ascii_lowercase();
    if let Some(raw) = lowered.strip_prefix("unknown:") {
        let index = raw.parse::<u8>().ok()?;
        return (index <= 63).then_some(PadBind::Controller(index));
    }
    match lowered.as_str() {
        "guide" => Some(PadBind::Guide),
        "share" => Some(PadBind::Share),
        other => controller_index(other).map(PadBind::Controller),
    }
}

/// Canonical GameInput controller button names mapped to GameInput button
/// indices (the vendored header order, authoritative). Legacy gilrs names
/// (`Select`, `Start`, `Mode`, `C`, `Z`, `DPad*`) are not part of the notation.
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
