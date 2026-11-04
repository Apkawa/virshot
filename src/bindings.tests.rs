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
            pad: None,
            active: true,
        }
    );
    assert_eq!(
        parsed[1],
        ActionBinding {
            action: Action::Window,
            hotkey: HotKey::new(Some(Modifiers::ALT), Code::PrintScreen),
            pad: None,
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
            pad: None,
            active: false,
        }
    );
}

#[test]
fn parse_controls_filled_pad_yields_parsed_pad_bind_and_hotkey() {
    let controls = Controls {
        full_screen: binding("PrintScreen", &[], Some("South")),
        window: Controls::default().window,
        rect: Controls::default().rect,
    };

    let parsed = parse_controls(&controls).expect("pad notation must parse");

    assert_eq!(
        parsed[0],
        ActionBinding {
            action: Action::FullScreen,
            hotkey: HotKey::new(None, Code::PrintScreen),
            pad: Some(PadBind::Controller(2)),
            active: true,
        }
    );
}

#[test]
fn parse_controls_system_pad_bind_yields_share() {
    let controls = Controls {
        full_screen: Controls::default().full_screen,
        window: binding("PrintScreen", &["Alt"], Some("Share")),
        rect: Controls::default().rect,
    };

    let parsed = parse_controls(&controls).expect("system pad buttons must parse");

    assert_eq!(parsed[1].pad, Some(PadBind::Share));
    assert_eq!(
        parsed[1].hotkey,
        HotKey::new(Some(Modifiers::ALT), Code::PrintScreen)
    );
}

#[test]
fn parse_controls_invalid_pad_returns_unknown_pad_error() {
    let controls = Controls {
        full_screen: Controls::default().full_screen,
        window: binding("PrintScreen", &["Alt"], Some("nope")),
        rect: Controls::default().rect,
    };

    let err = parse_controls(&controls).expect_err("bad pad notation must fail");

    assert_eq!(
        err,
        Error::UnknownPad {
            action: Action::Window,
            pad: "nope".to_owned(),
        }
    );
}

#[test]
fn parse_controls_invalid_pad_error_names_action_and_pad() {
    let controls = Controls {
        full_screen: Controls::default().full_screen,
        window: binding("PrintScreen", &["Alt"], Some("Select")),
        rect: Controls::default().rect,
    };

    let err = parse_controls(&controls).expect_err("gilrs-only names are out of notation");

    assert_eq!(
        err.to_string(),
        "unknown pad \"Select\" in binding [controls.window]"
    );
}

#[test]
fn parse_controls_missing_pad_field_is_none() {
    let parsed = parse_controls(&Controls::default()).expect("defaults must parse");

    assert_eq!(parsed[0].pad, None);
    assert_eq!(parsed[1].pad, None);
    assert_eq!(parsed[2].pad, None);
}

#[test]
fn parse_pad_canonical_names_map_to_gameinput_indices() {
    assert_eq!(parse_pad("North"), Some(PadBind::Controller(0)));
    assert_eq!(parse_pad("East"), Some(PadBind::Controller(1)));
    assert_eq!(parse_pad("South"), Some(PadBind::Controller(2)));
    assert_eq!(parse_pad("West"), Some(PadBind::Controller(3)));
    assert_eq!(parse_pad("LeftShoulder"), Some(PadBind::Controller(4)));
    assert_eq!(parse_pad("RightShoulder"), Some(PadBind::Controller(5)));
    assert_eq!(parse_pad("LeftTrigger"), Some(PadBind::Controller(6)));
    assert_eq!(parse_pad("RightTrigger"), Some(PadBind::Controller(7)));
    assert_eq!(parse_pad("RightThumbstick"), Some(PadBind::Controller(8)));
    assert_eq!(parse_pad("LeftThumbstick"), Some(PadBind::Controller(9)));
    assert_eq!(parse_pad("LeftStick"), Some(PadBind::Controller(10)));
    assert_eq!(parse_pad("RightStick"), Some(PadBind::Controller(11)));
}

#[test]
fn parse_pad_names_are_case_insensitive() {
    assert_eq!(parse_pad("south"), Some(PadBind::Controller(2)));
    assert_eq!(parse_pad("RIGHTSHOULDER"), Some(PadBind::Controller(5)));
    assert_eq!(parse_pad("UNKNOWN:7"), Some(PadBind::Controller(7)));
    assert_eq!(parse_pad("guide"), Some(PadBind::Guide));
    assert_eq!(parse_pad("SHARE"), Some(PadBind::Share));
}

#[test]
fn parse_pad_unknown_prefix_returns_raw_controller_index() {
    assert_eq!(parse_pad("unknown:0"), Some(PadBind::Controller(0)));
    assert_eq!(parse_pad("unknown:5"), Some(PadBind::Controller(5)));
    assert_eq!(parse_pad("unknown:63"), Some(PadBind::Controller(63)));
}

#[test]
fn parse_pad_unknown_index_out_of_range_returns_none() {
    assert_eq!(parse_pad("unknown:64"), None);
    assert_eq!(parse_pad("unknown:255"), None);
    assert_eq!(parse_pad("unknown:-1"), None);
}

#[test]
fn parse_pad_unknown_malformed_suffix_returns_none() {
    assert_eq!(parse_pad("unknown:x"), None);
    assert_eq!(parse_pad("unknown:"), None);
    assert_eq!(parse_pad("unknown:0x1"), None);
}

#[test]
fn parse_pad_system_buttons_return_guide_and_share() {
    assert_eq!(parse_pad("Guide"), Some(PadBind::Guide));
    assert_eq!(parse_pad("Share"), Some(PadBind::Share));
}

#[test]
fn parse_pad_invalid_names_return_none() {
    assert_eq!(parse_pad("South "), None);
    assert_eq!(parse_pad("Joystick"), None);
    assert_eq!(parse_pad("Select"), None);
    assert_eq!(parse_pad("Start"), None);
    assert_eq!(parse_pad("DPadUp"), None);
    assert_eq!(parse_pad("Mode"), None);
    assert_eq!(parse_pad("C"), None);
    assert_eq!(parse_pad("Z"), None);
    assert_eq!(parse_pad(""), None);
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
