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
        window: binding("PrintScreen", &["Alt"], Some("SouthEast")),
        rect: Controls::default().rect,
    };

    let err = parse_controls(&controls).expect_err("names outside the notation must fail");

    assert_eq!(
        err.to_string(),
        "unknown pad \"SouthEast\" in binding [controls.window]"
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
fn parse_pad_legacy_gilrs_names_keep_their_pinned_indices() {
    // Pre-Stage-5 configs keep working: these names are the legacy gilrs-style
    // vocabulary, pinned to fixed button indices (§9), not GameInput labels.
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
fn parse_pad_accepts_canonical_gameinput_names() {
    // Stage 5 (§4.1): the captured device-info notation parses back. Names are
    // canonical GameInput label names, matched case-insensitively; the config's
    // own spelling is stored verbatim for display.
    assert_eq!(parse_pad("XboxX"), Some(PadBind::Named("XboxX".to_owned())));
    assert_eq!(
        parse_pad("XboxLeftTrigger"),
        Some(PadBind::Named("XboxLeftTrigger".to_owned()))
    );
    assert_eq!(
        parse_pad("XboxDPadUp"),
        Some(PadBind::Named("XboxDPadUp".to_owned()))
    );
    assert_eq!(
        parse_pad("XboxPaddle1"),
        Some(PadBind::Named("XboxPaddle1".to_owned()))
    );
    assert_eq!(parse_pad("xboxx"), Some(PadBind::Named("xboxx".to_owned())));
    // Generic console labels a non-Xbox device-info may report.
    assert_eq!(
        parse_pad("Select"),
        Some(PadBind::Named("Select".to_owned()))
    );
    assert_eq!(parse_pad("Home"), Some(PadBind::Named("Home".to_owned())));
    assert_eq!(
        parse_pad("PaddleRight2"),
        Some(PadBind::Named("PaddleRight2".to_owned()))
    );
}

#[test]
fn parse_pad_still_accepts_unknown_index_and_gilrs_names() {
    // Backwards compatibility (spec §9): raw-index notation and the legacy
    // gilrs-style names keep parsing to fixed controller indices.
    assert_eq!(parse_pad("unknown:7"), Some(PadBind::Controller(7)));
    assert_eq!(parse_pad("South"), Some(PadBind::Controller(2)));
    assert_eq!(parse_pad("RightStick"), Some(PadBind::Controller(11)));
}

#[test]
fn parse_pad_invalid_names_return_none() {
    // Names outside the GameInput label vocabulary (and typos of it) stay a
    // visible config error: a silently dropped bind would fire nothing.
    assert_eq!(parse_pad("South "), None);
    assert_eq!(parse_pad("Joystick"), None);
    assert_eq!(parse_pad("SouthEast"), None);
    assert_eq!(parse_pad("DPadUp"), None);
    assert_eq!(parse_pad("XboxXx"), None);
    assert_eq!(parse_pad("C"), None);
    assert_eq!(parse_pad("Z"), None);
    assert_eq!(parse_pad(""), None);
}

#[test]
fn parse_controls_unknown_key_returns_unknown_key_error() {
    let controls = Controls {
        full_screen: binding("PrintScreen", &[], None),
        // `F25` is delivered by egui but has no Windows virtual key in
        // `global_hotkey::key_to_vk`, so it is outside the notation.
        window: binding("F25", &["Alt"], None),
        rect: Controls::default().rect,
    };

    let err = parse_controls(&controls).expect_err("F25 cannot be registered on Windows");

    assert_eq!(
        err,
        Error::UnknownKey {
            action: Action::Window,
            key: "F25".to_owned(),
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
fn parse_key_accepts_the_bind_probe_keys_and_round_trips() {
    // Stage 14: a Windows bind-probe press commits these names into a row, so
    // every one must parse. `Code::to_string()` is what `gui` renders as the key
    // name (`hotkeys::key_label`), so it must parse back to the same code.
    assert_eq!(parse_key("ScrollLock"), Some(Code::ScrollLock));
    assert_eq!(parse_key("scrolllock"), Some(Code::ScrollLock));
    assert_eq!(parse_key("SCROLLLOCK"), Some(Code::ScrollLock));
    assert_eq!(parse_key("Pause"), Some(Code::Pause));
    for code in [Code::PrintScreen, Code::ScrollLock, Code::Pause] {
        assert_eq!(parse_key(&code.to_string()), Some(code));
    }
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
fn parse_key_accepts_navigation_and_function_keys() {
    assert_eq!(parse_key("Insert"), Some(Code::Insert));
    assert_eq!(parse_key("delete"), Some(Code::Delete));
    assert_eq!(parse_key("HOME"), Some(Code::Home));
    assert_eq!(parse_key("End"), Some(Code::End));
    assert_eq!(parse_key("PageUp"), Some(Code::PageUp));
    assert_eq!(parse_key("pagedown"), Some(Code::PageDown));
    // The crate's Windows table stops at F24 (`key_to_vk`), F25 has no VK.
    assert_eq!(parse_key("F13"), Some(Code::F13));
    assert_eq!(parse_key("f20"), Some(Code::F20));
    assert_eq!(parse_key("F24"), Some(Code::F24));
    assert_eq!(parse_key("F25"), None);
    assert_eq!(parse_key("F35"), None);
    assert_eq!(parse_key("F0"), None);
}

#[test]
fn parse_key_accepts_whitespace_and_enter() {
    assert_eq!(parse_key("Space"), Some(Code::Space));
    assert_eq!(parse_key("space"), Some(Code::Space));
    assert_eq!(parse_key("Enter"), Some(Code::Enter));
    assert_eq!(parse_key("Tab"), Some(Code::Tab));
    assert_eq!(parse_key("ContextMenu"), Some(Code::ContextMenu));
    assert_eq!(parse_key("Menu"), Some(Code::ContextMenu));
    assert_eq!(parse_key("Pause"), Some(Code::Pause));
    assert_eq!(parse_key("Help"), Some(Code::Help));
    assert_eq!(parse_key("Sleep"), Some(Code::Sleep));
}

#[test]
fn parse_key_arrow_names_are_canonical_and_aliasable() {
    assert_eq!(parse_key("ArrowDown"), Some(Code::ArrowDown));
    assert_eq!(parse_key("arrowleft"), Some(Code::ArrowLeft));
    assert_eq!(parse_key("ArrowRight"), Some(Code::ArrowRight));
    assert_eq!(parse_key("ARROWUP"), Some(Code::ArrowUp));
    // egui's short names are convenience aliases, canonical is `Arrow*`.
    assert_eq!(parse_key("Down"), Some(Code::ArrowDown));
    assert_eq!(parse_key("left"), Some(Code::ArrowLeft));
    assert_eq!(parse_key("Right"), Some(Code::ArrowRight));
    assert_eq!(parse_key("Up"), Some(Code::ArrowUp));
    assert_eq!(parse_key("Arrow"), None);
}

#[test]
fn parse_key_accepts_punctuation_names() {
    assert_eq!(parse_key("Comma"), Some(Code::Comma));
    assert_eq!(parse_key("Period"), Some(Code::Period));
    assert_eq!(parse_key("Minus"), Some(Code::Minus));
    assert_eq!(parse_key("Equal"), Some(Code::Equal));
    assert_eq!(parse_key("Semicolon"), Some(Code::Semicolon));
    assert_eq!(parse_key("Slash"), Some(Code::Slash));
    assert_eq!(parse_key("Backslash"), Some(Code::Backslash));
    assert_eq!(parse_key("Quote"), Some(Code::Quote));
    assert_eq!(parse_key("Backquote"), Some(Code::Backquote));
    assert_eq!(parse_key("BracketLeft"), Some(Code::BracketLeft));
    assert_eq!(parse_key("BracketRight"), Some(Code::BracketRight));
    assert_eq!(parse_key("IntlBackslash"), Some(Code::IntlBackslash));
    assert_eq!(parse_key("IntlRo"), Some(Code::IntlRo));
    assert_eq!(parse_key("IntlYen"), Some(Code::IntlYen));
    // Crate-style spellings of the legacy single-letter/digit notation.
    assert_eq!(parse_key("KeyA"), Some(Code::KeyA));
    assert_eq!(parse_key("keyz"), Some(Code::KeyZ));
    assert_eq!(parse_key("Digit7"), Some(Code::Digit7));
    assert_eq!(parse_key("digit0"), Some(Code::Digit0));
    // Parsing is case-insensitive, so `Keys` is the canonical `KeyS`
    // spelling; a multi-character remainder is not a key name.
    assert_eq!(parse_key("Keys"), Some(Code::KeyS));
    assert_eq!(parse_key("KeyAB"), None);
    assert_eq!(parse_key("Digits"), None);
    assert_eq!(parse_key("DigitX"), None);
}

#[test]
fn parse_key_accepts_media_browser_and_numpad_names() {
    assert_eq!(parse_key("BrowserHome"), Some(Code::BrowserHome));
    assert_eq!(parse_key("browserSearch"), Some(Code::BrowserSearch));
    assert_eq!(parse_key("AudioVolumeMute"), Some(Code::AudioVolumeMute));
    assert_eq!(parse_key("AudioVolumeDown"), Some(Code::AudioVolumeDown));
    assert_eq!(parse_key("AudioVolumeUp"), Some(Code::AudioVolumeUp));
    assert_eq!(parse_key("MediaPlayPause"), Some(Code::MediaPlayPause));
    assert_eq!(parse_key("MediaTrackNext"), Some(Code::MediaTrackNext));
    assert_eq!(
        parse_key("MediaTrackPrevious"),
        Some(Code::MediaTrackPrevious)
    );
    assert_eq!(parse_key("MediaStop"), Some(Code::MediaStop));
    // Bindable, but egui never delivers them — config-only notation.
    assert_eq!(parse_key("MediaPlay"), Some(Code::MediaPlay));
    assert_eq!(parse_key("MediaPause"), Some(Code::MediaPause));
    assert_eq!(parse_key("MediaSelect"), Some(Code::MediaSelect));
    assert_eq!(parse_key("LaunchMail"), Some(Code::LaunchMail));
    assert_eq!(parse_key("LaunchApp1"), Some(Code::LaunchApp1));
    assert_eq!(parse_key("LaunchApp2"), Some(Code::LaunchApp2));
    assert_eq!(parse_key("Convert"), Some(Code::Convert));
    assert_eq!(parse_key("KanaMode"), Some(Code::KanaMode));
    assert_eq!(parse_key("NonConvert"), Some(Code::NonConvert));
    assert_eq!(parse_key("Numpad5"), Some(Code::Numpad5));
    assert_eq!(parse_key("NumpadEnter"), Some(Code::NumpadEnter));
    assert_eq!(parse_key("numpadadd"), Some(Code::NumpadAdd));
    assert_eq!(parse_key("NumpadSubtract"), Some(Code::NumpadSubtract));
    assert_eq!(parse_key("NumpadMultiply"), Some(Code::NumpadMultiply));
    assert_eq!(parse_key("NumpadDivide"), Some(Code::NumpadDivide));
    assert_eq!(parse_key("NumpadDecimal"), Some(Code::NumpadDecimal));
    assert_eq!(parse_key("NumpadComma"), Some(Code::NumpadComma));
    assert_eq!(parse_key("NumpadEqual"), Some(Code::NumpadEqual));
    assert_eq!(parse_key("NumpadClear"), Some(Code::NumpadClear));
    assert_eq!(parse_key("Numpad"), None);
    // Codes the Windows table does not map.
    assert_eq!(parse_key("MediaRecord"), None);
    assert_eq!(parse_key("MediaRewind"), None);
    assert_eq!(parse_key("NumpadBackspace"), None);
}

#[test]
fn parse_key_rejects_reserved_escape_and_backspace() {
    assert_eq!(parse_key("Escape"), None);
    assert_eq!(parse_key("esc"), None);
    assert_eq!(parse_key("ESCAPE"), None);
    assert_eq!(parse_key("Backspace"), None);
    assert_eq!(parse_key("backspace"), None);
}

#[test]
fn parse_key_rejects_modifiers_lock_keys_and_unmappable_names() {
    // Modifier keys cannot be the main key of a Windows hotkey (`key_to_vk`
    // has no arm for them); `CapsLock`/`NumLock` are excluded on purpose.
    // `ScrollLock` is NOT excluded (Stage 14): it is a bind-probe key, and a
    // binding naming it must parse so the committed row is registrable.
    assert_eq!(parse_key("ShiftLeft"), None);
    assert_eq!(parse_key("ShiftRight"), None);
    assert_eq!(parse_key("ControlLeft"), None);
    assert_eq!(parse_key("AltRight"), None);
    assert_eq!(parse_key("Meta"), None);
    assert_eq!(parse_key("SuperLeft"), None);
    assert_eq!(parse_key("CapsLock"), None);
    assert_eq!(parse_key("NumLock"), None);
    assert_eq!(parse_key("Fn"), None);
    assert_eq!(parse_key("FnLock"), None);
    assert_eq!(parse_key("Eject"), None);
    assert_eq!(parse_key("Power"), None);
    assert_eq!(parse_key("Clear"), None);
    assert_eq!(parse_key("Copy"), None);
    assert_eq!(parse_key("Cut"), None);
    assert_eq!(parse_key("Paste"), None);
    // Shift-modified characters have no physical code to register.
    assert_eq!(parse_key("Colon"), None);
    assert_eq!(parse_key("Pipe"), None);
    assert_eq!(parse_key("Questionmark"), None);
    assert_eq!(parse_key("Exclamationmark"), None);
    assert_eq!(parse_key("OpenCurlyBracket"), None);
    assert_eq!(parse_key("CloseCurlyBracket"), None);
    assert_eq!(parse_key("Plus"), None);
    assert_eq!(parse_key("OpenBracket"), None);
    assert_eq!(parse_key("Backtick"), None);
    assert_eq!(parse_key("Equals"), None);
}

#[test]
fn reserved_key_name_escape_and_backspace_are_canonical() {
    assert_eq!(reserved_key_name("Escape"), Some("Escape"));
    assert_eq!(reserved_key_name("esc"), Some("Escape"));
    assert_eq!(reserved_key_name("BACKSPACE"), Some("Backspace"));
    assert_eq!(reserved_key_name(""), None);
    assert_eq!(reserved_key_name("PrintScreen"), None);
}

#[test]
fn parse_controls_reserved_key_error_names_action_and_key() {
    let controls = Controls {
        full_screen: binding("PrintScreen", &[], None),
        window: binding("Esc", &["Ctrl"], None),
        rect: Controls::default().rect,
    };

    let err = parse_controls(&controls).expect_err("Escape is reserved for the capture UI");

    assert_eq!(
        err,
        Error::ReservedKey {
            action: Action::Window,
            key: "Esc".to_owned(),
        }
    );
    assert_eq!(
        err.to_string(),
        "reserved key \"Esc\" in binding [controls.window]: \
         Escape cancels capture, Backspace clears a binding"
    );
}

#[test]
fn parse_controls_backspace_reserved_error_reports_the_rect_action() {
    let controls = Controls {
        full_screen: Controls::default().full_screen,
        window: Controls::default().window,
        rect: binding("Backspace", &[], None),
    };

    let err = parse_controls(&controls).expect_err("Backspace is reserved for the capture UI");

    assert_eq!(
        err,
        Error::ReservedKey {
            action: Action::Rect,
            key: "Backspace".to_owned(),
        }
    );
    assert_eq!(
        err.to_string(),
        "reserved key \"Backspace\" in binding [controls.rect]: \
         Escape cancels capture, Backspace clears a binding"
    );
}

#[test]
fn parse_key_unsupported_names_return_none() {
    assert_eq!(parse_key("F25"), None);
    assert_eq!(parse_key("AB"), None);
    assert_eq!(parse_key(""), None);
    assert_eq!(parse_key("Hyper"), None);
    assert_eq!(parse_key("NumpadHash"), None);
    assert_eq!(parse_key("Space "), None);
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

#[cfg(windows)]
#[test]
fn canonical_pad_names_match_the_gamepad_label_table() {
    // The notation vocabulary mirrors `button_name` in crates/gamepad (the
    // vendored GameInput header): every name the UI can write must parse back.
    // Guide/Share are system binds, not controller labels, so they are excluded.
    let mut expected: Vec<&str> = (1..=124).filter_map(gamepad::button_name).collect();
    expected.retain(|name| *name != "Guide" && *name != "Share");
    let mut mirrored = GAMEPAD_CANONICAL_NAMES.to_vec();
    expected.sort_unstable();
    mirrored.sort_unstable();
    assert_eq!(mirrored, expected);
}
