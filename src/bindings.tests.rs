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
