//! Input binding model (spec §4.1): config `controls` → typed capture actions
//! with `global-hotkey` combos, plus the per-action debounce gate. Pure logic
//! only — registration, crate calls, and threads belong to `hotkeys`.
//!
//! MVP semantics: `full_screen` (PrintScreen) and `window` (Alt+PrintScreen)
//! are **active**; `rect` (Ctrl+PrintScreen) is typed as **reserved** — parsed
//! and reported, never registered (spec §4.1 reserves the rect-area action).
//! `pad` stays the reserved gamepad slot (integration deferred to Stage 7): it
//! is kept in the config model and ignored here — a filled `pad` never fails
//! parsing and never produces a trigger (spec §4.1 "action fires on any filled
//! field (OR)" applies when the gamepad slot lands, not in MVP).

use chrono::{DateTime, Local};
use std::collections::HashMap;
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

/// One parsed binding: which action it triggers, the combo to register, and
/// whether MVP registers it (`rect` → inactive).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ActionBinding {
    pub(crate) action: Action,
    pub(crate) hotkey: HotKey,
    pub(crate) active: bool,
}

/// Binding failures, typed per module (`thiserror` enum, no stringly
/// `Err(String)`). Both variants name the offending config section.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum Error {
    /// The `key` name is outside the supported MVP notation (see [`parse_key`]).
    #[error("unknown key \"{key}\" in binding [controls.{action}]")]
    UnknownKey { action: Action, key: String },

    /// The modifier name is outside the supported set (see [`parse_mod`]).
    #[error("unknown modifier \"{mod_name}\" in binding [controls.{action}]")]
    UnknownMod { action: Action, mod_name: String },
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

/// Parse all three control bindings (spec §4.1 order: full_screen, window,
/// rect). Fails on the first unknown key or modifier; the reserved `rect`
/// binding is parsed too (typed inactive) so a typo there is a visible config
/// error, not a silently dropped binding.
pub(crate) fn parse_controls(controls: &Controls) -> Result<Vec<ActionBinding>, Error> {
    Ok(vec![
        parse_binding(Action::FullScreen, &controls.full_screen)?,
        parse_binding(Action::Window, &controls.window)?,
        parse_binding(Action::Rect, &controls.rect)?,
    ])
}

/// Parse one binding. `pad` is read from the config model but ignored (Stage 7
/// slot), so it never contributes an error or a trigger.
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
    Ok(ActionBinding {
        action,
        hotkey: HotKey::new(Some(mods), key),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixed instant for deterministic debounce tests: `ms` after the epoch,
    /// in the local timezone (the type the trigger clock uses).
    fn at_ms(ms: i64) -> DateTime<Local> {
        (DateTime::UNIX_EPOCH + chrono::Duration::milliseconds(ms)).with_timezone(&Local)
    }

    fn binding(key: &str, mods: &[&str], pad: Option<&str>) -> Binding {
        Binding {
            key: key.to_owned(),
            mods: mods.iter().copied().map(str::to_owned).collect(),
            pad: pad.map(|name| name.to_owned()),
        }
    }

    #[test]
    fn parse_controls_defaults_returns_active_full_screen_and_window() {
        let parsed = parse_controls(&Controls::default()).expect("defaults must parse");

        assert_eq!(parsed.len(), 3);
        assert_eq!(
            parsed[0],
            ActionBinding {
                action: Action::FullScreen,
                hotkey: HotKey::new(None, Code::PrintScreen),
                active: true,
            }
        );
        assert_eq!(
            parsed[1],
            ActionBinding {
                action: Action::Window,
                hotkey: HotKey::new(Some(Modifiers::ALT), Code::PrintScreen),
                active: true,
            }
        );
    }

    #[test]
    fn parse_controls_rect_binding_is_reserved_inactive() {
        let parsed = parse_controls(&Controls::default()).expect("defaults must parse");

        assert_eq!(
            parsed[2],
            ActionBinding {
                action: Action::Rect,
                hotkey: HotKey::new(Some(Modifiers::CONTROL), Code::PrintScreen),
                active: false,
            }
        );
    }

    #[test]
    fn parse_controls_pad_value_is_accepted_and_ignored() {
        let controls = Controls {
            full_screen: binding("PrintScreen", &[], Some("South")),
            window: binding("PrintScreen", &["Alt"], Some("Start")),
            rect: binding("PrintScreen", &["Ctrl"], Some("Select")),
        };

        let parsed = parse_controls(&controls).expect("pad is a reserved slot, must not fail");

        assert_eq!(parsed[0].hotkey, HotKey::new(None, Code::PrintScreen));
        assert_eq!(
            parsed[1].hotkey,
            HotKey::new(Some(Modifiers::ALT), Code::PrintScreen)
        );
        assert!(!parsed[2].active);
    }

    #[test]
    fn parse_controls_unknown_key_returns_unknown_key_error() {
        let controls = Controls {
            full_screen: binding("PrintScreen", &[], None),
            window: binding("F13", &["Alt"], None),
            rect: Controls::default().rect,
        };

        let err = parse_controls(&controls).expect_err("F13 is outside MVP notation");

        assert_eq!(
            err,
            Error::UnknownKey {
                action: Action::Window,
                key: "F13".to_owned(),
            }
        );
    }

    #[test]
    fn parse_controls_unknown_mod_returns_unknown_mod_error() {
        let controls = Controls {
            full_screen: binding("PrintScreen", &[], None),
            window: binding("PrintScreen", &["Hyper-X"], None),
            rect: Controls::default().rect,
        };

        let err = parse_controls(&controls).expect_err("unknown modifier must fail");

        assert_eq!(
            err,
            Error::UnknownMod {
                action: Action::Window,
                mod_name: "Hyper-X".to_owned(),
            }
        );
    }

    #[test]
    fn parse_key_printscreen_returns_printscreen_code() {
        assert_eq!(parse_key("PrintScreen"), Some(Code::PrintScreen));
    }

    #[test]
    fn parse_key_single_letter_returns_letter_code() {
        assert_eq!(parse_key("A"), Some(Code::KeyA));
        assert_eq!(parse_key("s"), Some(Code::KeyS));
    }

    #[test]
    fn parse_key_digit_returns_digit_code() {
        assert_eq!(parse_key("0"), Some(Code::Digit0));
        assert_eq!(parse_key("7"), Some(Code::Digit7));
    }

    #[test]
    fn parse_key_function_key_returns_function_code() {
        assert_eq!(parse_key("F1"), Some(Code::F1));
        assert_eq!(parse_key("F12"), Some(Code::F12));
        assert_eq!(parse_key("f3"), Some(Code::F3));
    }

    #[test]
    fn parse_key_unsupported_names_return_none() {
        assert_eq!(parse_key("Pause"), None);
        assert_eq!(parse_key("F13"), None);
        assert_eq!(parse_key("AB"), None);
        assert_eq!(parse_key(""), None);
        assert_eq!(parse_key("Space"), None);
    }

    #[test]
    fn parse_mod_alt_ctrl_shift_return_crate_bits() {
        assert_eq!(parse_mod("Alt"), Some(Modifiers::ALT));
        assert_eq!(parse_mod("ctrl"), Some(Modifiers::CONTROL));
        assert_eq!(parse_mod("Control"), Some(Modifiers::CONTROL));
        assert_eq!(parse_mod("SHIFT"), Some(Modifiers::SHIFT));
    }

    #[test]
    fn parse_mod_win_and_command_return_meta_bit() {
        assert_eq!(parse_mod("Win"), Some(Modifiers::META));
        assert_eq!(parse_mod("Command"), Some(Modifiers::META));
        assert_eq!(parse_mod("windows"), Some(Modifiers::META));
    }

    #[test]
    fn parse_mod_unknown_name_returns_none() {
        assert_eq!(parse_mod("Hyper"), None);
        assert_eq!(parse_mod(""), None);
    }

    #[test]
    fn parse_binding_multi_mod_combo_matches_crate_hotkey() {
        let parsed = parse_binding(Action::Window, &binding("A", &["Alt", "Shift"], None))
            .expect("alt+shift+a must parse");

        assert_eq!(
            parsed.hotkey,
            HotKey::new(Some(Modifiers::ALT | Modifiers::SHIFT), Code::KeyA)
        );
        assert_eq!(
            parsed.hotkey.id(),
            (Modifiers::ALT.bits() << 16 | Modifiers::SHIFT.bits() << 16 | Code::KeyA as u32)
        );
        assert_eq!(parsed.hotkey.into_string(), "shift+alt+KeyA");
    }

    #[test]
    fn debounce_first_trigger_allows() {
        let mut debounce = Debounce::default();

        assert!(debounce.try_fire(Action::FullScreen, at_ms(0)));
    }

    #[test]
    fn debounce_repeat_within_interval_blocks() {
        let mut debounce = Debounce::default();

        assert!(debounce.try_fire(Action::FullScreen, at_ms(0)));
        assert!(!debounce.try_fire(Action::FullScreen, at_ms(100)));
    }

    #[test]
    fn debounce_repeat_at_exact_interval_allows() {
        let mut debounce = Debounce::default();

        assert!(debounce.try_fire(Action::FullScreen, at_ms(0)));
        assert!(debounce.try_fire(Action::FullScreen, at_ms(250)));
    }

    #[test]
    fn debounce_repeat_after_interval_allows() {
        let mut debounce = Debounce::default();

        assert!(debounce.try_fire(Action::FullScreen, at_ms(0)));
        assert!(debounce.try_fire(Action::FullScreen, at_ms(300)));
    }

    #[test]
    fn debounce_blocked_repeat_does_not_move_window() {
        let mut debounce = Debounce::default();

        assert!(debounce.try_fire(Action::FullScreen, at_ms(0)));
        assert!(!debounce.try_fire(Action::FullScreen, at_ms(100)));
        assert!(!debounce.try_fire(Action::FullScreen, at_ms(200)));
        assert!(debounce.try_fire(Action::FullScreen, at_ms(250)));
    }

    #[test]
    fn debounce_actions_gate_independently() {
        let mut debounce = Debounce::default();

        assert!(debounce.try_fire(Action::FullScreen, at_ms(0)));
        assert!(debounce.try_fire(Action::Window, at_ms(10)));
        assert!(!debounce.try_fire(Action::FullScreen, at_ms(20)));
    }

    #[test]
    fn debounce_custom_interval_blocks_within_custom_interval() {
        let mut debounce = Debounce::new(Duration::from_millis(600));

        assert!(debounce.try_fire(Action::Window, at_ms(0)));
        assert!(!debounce.try_fire(Action::Window, at_ms(500)));
        assert!(debounce.try_fire(Action::Window, at_ms(600)));
    }
}
