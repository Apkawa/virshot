//! Unit tests for the gamepad trigger mapping: pad event → action matching
//! (controller index, Guide/Share system buttons, OR-fire tie-break) and the
//! shared debounce gate the hotkey listener and the pad pump fire through.
//! Linux-runnable: no GameInput, no hardware, no capture.

use super::*;

use chrono::DateTime;
use chrono::Local;

use crate::config::Binding;
use crate::config::Controls;

/// Fixed instant for deterministic gate tests: `ms` after the epoch, in the
/// local timezone (the type the trigger clock uses).
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

/// full_screen → `South` (controller index 2), window → `Guide`, rect → `Share`
/// (reserved action, must never fire from the pad either).
fn controls_south_guide_share() -> Controls {
    Controls {
        full_screen: binding("PrintScreen", &[], Some("South")),
        window: binding("PrintScreen", &["Alt"], Some("Guide")),
        rect: binding("PrintScreen", &["Ctrl"], Some("Share")),
    }
}

fn parsed(controls: &Controls) -> Vec<bindings::ActionBinding> {
    bindings::parse_controls(controls).expect("controls must parse")
}

#[test]
fn pad_triggers_keeps_active_bound_actions_and_drops_reserved_and_unbound() {
    let triggers = pad_triggers(&parsed(&controls_south_guide_share()));

    assert_eq!(
        triggers,
        vec![
            (Action::FullScreen, PadBind::Controller(2)),
            (Action::Window, PadBind::Guide),
        ]
    );
}

#[test]
fn pad_triggers_of_default_controls_is_empty() {
    assert!(pad_triggers(&parsed(&Controls::default())).is_empty());
}

#[test]
fn pad_triggers_now_invalid_binding_returns_none() {
    let settings = Settings {
        controls: Controls {
            full_screen: binding("PrintScreen", &[], Some("nosuchbutton")),
            window: binding("PrintScreen", &["Alt"], None),
            rect: binding("PrintScreen", &["Ctrl"], None),
        },
        ..Settings::default()
    };

    assert_eq!(pad_triggers_now(&Arc::new(Mutex::new(settings))), None);
}

#[test]
fn pad_triggers_now_follows_mailbox_updates_without_restart() {
    // Stage 5 defect fix: the pump reads the LIVE mailbox (UI Apply publishes
    // it), so a pad bind added after startup takes effect on the next event.
    let mailbox = Arc::new(Mutex::new(Settings::default()));

    assert!(pad_triggers_now(&mailbox).is_some_and(|triggers| triggers.is_empty()));

    *mailbox.lock().expect("settings mailbox") = Settings {
        controls: controls_south_guide_share(),
        ..Settings::default()
    };

    assert_eq!(
        pad_triggers_now(&mailbox),
        Some(vec![
            (Action::FullScreen, PadBind::Controller(2)),
            (Action::Window, PadBind::Guide),
        ])
    );
}

#[test]
fn newly_pressed_reports_only_zero_to_one_edges() {
    // 0→1 edges only: bit 1 was released, bit 2 was pressed.
    assert_eq!(newly_pressed(0b0010, 0b0100), vec![2]);
    assert_eq!(newly_pressed(0, 0b0011), vec![0, 1]);
}

#[test]
fn newly_pressed_release_only_change_has_no_edges() {
    // `Vec::<u8>::new()` instead of `vec![]`: once the crate links
    // `serde_json` (Stage 4 notes parsing), `u8: PartialEq<serde_json::Value>`
    // becomes a candidate, so the empty literal's element type stops inferring.
    assert_eq!(newly_pressed(0b0110, 0b0010), Vec::<u8>::new());
    assert_eq!(newly_pressed(0b0110, 0b0110), Vec::<u8>::new());
}

#[test]
fn newly_pressed_held_button_is_not_relisted() {
    // The library's event lists the CURRENTLY pressed indices, so holding South
    // (bit 2) and pressing West (bit 3) must not re-report bit 2 — otherwise a
    // held bind would re-fire on every later state change.
    assert_eq!(newly_pressed(0b000100, 0b001100), vec![3]);
}

#[test]
fn pad_notation_controller_is_raw_unknown_index() {
    // Without a device-info label (the Linux stub has no registry) the notation
    // falls back to the raw bit index — still accepted by `parse_pad` (§9).
    assert_eq!(
        pad_notation(PadDownEvent::Controller {
            device: 0,
            index: 3
        }),
        "unknown:3"
    );
}

#[test]
fn pad_notation_system_buttons_keep_names() {
    assert_eq!(pad_notation(PadDownEvent::Guide), "Guide");
    assert_eq!(pad_notation(PadDownEvent::Share), "Share");
}

#[test]
fn pad_notation_controller_index_parses_back_to_bind() {
    // The captured notation must round-trip through `bindings::parse_pad`.
    let notation = pad_notation(PadDownEvent::Controller {
        device: 1,
        index: 5,
    });
    let mut controls = Controls::default();
    controls.full_screen.pad = Some(notation);
    let parsed = bindings::parse_controls(&controls).expect("captured notation parses");

    assert_eq!(
        pad_triggers(&parsed),
        vec![(Action::FullScreen, PadBind::Controller(5))]
    );
}

#[test]
fn pad_button_label_without_windows_is_none() {
    // No GameInput registry on Linux, and system buttons have no label slot.
    assert_eq!(
        pad_button_label(PadDownEvent::Controller {
            device: 0,
            index: 3
        }),
        None
    );
    assert_eq!(pad_button_label(PadDownEvent::Guide), None);
}

#[test]
fn commit_pad_capture_first_press_records_and_ends_capture() {
    let mut capture = PadCapture {
        active: Some((CaptureField::Pad, Action::FullScreen)),
        captured: None,
    };

    assert_eq!(
        commit_pad_capture(
            &mut capture,
            PadDownEvent::Controller {
                device: 0,
                index: 3
            }
        ),
        Some(Action::FullScreen)
    );
    assert_eq!(
        capture.captured,
        Some(PadDownEvent::Controller {
            device: 0,
            index: 3
        })
    );
    assert_eq!(capture.active, None);
    // A second press after the capture ended commits nothing.
    assert_eq!(commit_pad_capture(&mut capture, PadDownEvent::Guide), None);
}

#[test]
fn commit_pad_capture_system_button_records_its_name() {
    let mut capture = PadCapture {
        active: Some((CaptureField::Pad, Action::Window)),
        captured: None,
    };

    assert_eq!(
        commit_pad_capture(&mut capture, PadDownEvent::Guide),
        Some(Action::Window)
    );
    assert_eq!(capture.captured, Some(PadDownEvent::Guide));
    assert_eq!(pad_notation(PadDownEvent::Guide), "Guide");
}

#[test]
fn commit_pad_capture_keyboard_capture_and_idle_commit_nothing() {
    let mut keyboard = PadCapture {
        active: Some((CaptureField::Keyboard, Action::FullScreen)),
        captured: None,
    };
    assert_eq!(commit_pad_capture(&mut keyboard, PadDownEvent::Guide), None);
    assert_eq!(keyboard.captured, None);

    let mut idle = PadCapture::default();
    assert_eq!(commit_pad_capture(&mut idle, PadDownEvent::Guide), None);
}

#[test]
fn decide_pad_shot_capture_active_suppresses_the_shot() {
    // Stage 5: a pad press while ANY field is capturing takes no screenshot.
    let triggers = pad_triggers(&parsed(&controls_south_guide_share()));
    let mut capture = PadCapture {
        active: Some((CaptureField::Pad, Action::Window)),
        captured: None,
    };

    assert_eq!(
        decide_pad_shot(
            &mut capture,
            PadDownEvent::Controller {
                device: 0,
                index: 2
            },
            &[2],
            &triggers
        ),
        None
    );
    assert_eq!(
        capture.captured,
        Some(PadDownEvent::Controller {
            device: 0,
            index: 2
        })
    );
    assert!(
        !pad_shots_suppressed(&capture),
        "capture ended on the commit"
    );
}

#[test]
fn decide_pad_shot_keyboard_capture_suppresses_without_commit() {
    let triggers = pad_triggers(&parsed(&controls_south_guide_share()));
    let mut capture = PadCapture {
        active: Some((CaptureField::Keyboard, Action::FullScreen)),
        captured: None,
    };

    assert_eq!(
        decide_pad_shot(
            &mut capture,
            PadDownEvent::Controller {
                device: 0,
                index: 2
            },
            &[2],
            &triggers
        ),
        None
    );
    assert_eq!(capture.captured, None);
    assert!(pad_shots_suppressed(&capture));
}

#[test]
fn decide_pad_shot_idle_capture_fires_matched_action() {
    // Capture OFF → the normal fire path: the DOWN edge matches its bind.
    let triggers = pad_triggers(&parsed(&controls_south_guide_share()));
    let mut capture = PadCapture::default();

    assert_eq!(
        decide_pad_shot(
            &mut capture,
            PadDownEvent::Controller {
                device: 0,
                index: 2
            },
            &[2],
            &triggers
        ),
        Some(Action::FullScreen)
    );
    assert_eq!(
        decide_pad_shot(&mut capture, PadDownEvent::Guide, &[], &triggers),
        Some(Action::Window)
    );
    assert_eq!(
        decide_pad_shot(&mut capture, PadDownEvent::Share, &[], &triggers),
        None,
        "Share is bound on the reserved rect action only"
    );
}

#[test]
fn decide_pad_shot_held_bind_with_release_fires_nothing() {
    // South is held (bit 2 stays set) and another button releases: no 0→1 edge,
    // so the held bind must not fire again.
    let triggers = pad_triggers(&parsed(&controls_south_guide_share()));
    let mut capture = PadCapture::default();
    let newly = newly_pressed(0b000101, 0b000100);

    assert_eq!(newly, Vec::<u8>::new()); // typed: see the `vec![]` note above
    assert_eq!(
        decide_pad_shot(
            &mut capture,
            PadDownEvent::Controller {
                device: 0,
                index: 2
            },
            &newly,
            &triggers
        ),
        None
    );
}

#[test]
fn capture_mailbox_set_mode_and_drain_hand_off() {
    // GUI sets the capture, pump commits, GUI drains exactly once.
    let mailbox = new_capture_mailbox();
    set_capture_mode(&mailbox, Some((CaptureField::Pad, Action::FullScreen)));

    let committed = {
        let mut capture = lock_capture(&mailbox);
        decide_pad_shot(
            &mut capture,
            PadDownEvent::Controller {
                device: 0,
                index: 3,
            },
            &[3],
            &[],
        )
    };
    assert_eq!(committed, None, "capture consumes the press, no shot");
    assert_eq!(active_capture(&mailbox), None);
    assert_eq!(
        take_captured(&mailbox),
        Some(PadDownEvent::Controller {
            device: 0,
            index: 3
        })
    );
    assert_eq!(take_captured(&mailbox), None);

    // Starting a new capture drops a leftover press so it cannot land on a
    // different row.
    set_capture_mode(&mailbox, None);
    lock_capture(&mailbox).captured = Some(PadDownEvent::Guide);
    set_capture_mode(&mailbox, Some((CaptureField::Pad, Action::FullScreen)));
    assert_eq!(take_captured(&mailbox), None);

    // Cancelling leaves the mailbox idle with nothing pending.
    set_capture_mode(&mailbox, None);
    assert_eq!(active_capture(&mailbox), None);
    assert!(!pad_shots_suppressed(&lock_capture(&mailbox)));
}

#[test]
fn match_controller_matching_index_fires_bound_action() {
    let triggers = pad_triggers(&parsed(&controls_south_guide_share()));

    assert_eq!(match_controller(&[2], &triggers), Some(Action::FullScreen));
}

#[test]
fn match_controller_non_matching_index_fires_nothing() {
    let triggers = pad_triggers(&parsed(&controls_south_guide_share()));

    assert_eq!(match_controller(&[0, 1], &triggers), None);
    assert_eq!(match_controller(&[], &triggers), None);
}

#[test]
fn match_controller_system_bind_never_matches_controller_indices() {
    let triggers = pad_triggers(&parsed(&controls_south_guide_share()));

    // Index 2 belongs to `South` (full_screen); `Guide` is a system button, so
    // a controller event must not fire the window action.
    assert_eq!(match_controller(&[2], &triggers), Some(Action::FullScreen));
    assert_eq!(match_controller(&[9], &triggers), None);
}

#[test]
fn match_controller_first_match_wins_when_two_actions_match() {
    // Documented tie-break (KISS): config order decides, first match fires.
    let triggers = vec![
        (Action::FullScreen, PadBind::Controller(3)),
        (Action::Window, PadBind::Controller(3)),
    ];

    assert_eq!(match_controller(&[3], &triggers), Some(Action::FullScreen));
}

#[test]
fn match_system_button_guide_down_fires_bound_action() {
    let triggers = pad_triggers(&parsed(&controls_south_guide_share()));

    assert_eq!(
        match_system_button(PadBind::Guide, true, &triggers),
        Some(Action::Window)
    );
}

#[test]
fn match_system_button_share_down_fires_bound_action() {
    let triggers = vec![(Action::FullScreen, PadBind::Share)];

    assert_eq!(
        match_system_button(PadBind::Share, true, &triggers),
        Some(Action::FullScreen)
    );
}

#[test]
fn match_system_button_down_without_matching_bind_fires_nothing() {
    let triggers = pad_triggers(&parsed(&controls_south_guide_share()));

    // `Share` is bound only on the reserved rect action, so a Share press
    // fires nothing.
    assert_eq!(match_system_button(PadBind::Share, true, &triggers), None);
}

#[test]
fn match_system_button_up_edge_fires_nothing() {
    let triggers = pad_triggers(&parsed(&controls_south_guide_share()));

    assert_eq!(match_system_button(PadBind::Guide, false, &triggers), None);
    assert_eq!(match_system_button(PadBind::Share, false, &triggers), None);
}

#[test]
fn shared_gate_blocks_second_fire_within_window_across_sources() {
    // One gate for both trigger sources (spec §4.1 OR-fire): a pad press and a
    // hotkey landing on the same action inside ~250 ms produce ONE shot.
    let gate = Arc::new(Mutex::new(Debounce::default()));

    assert!(bindings::try_fire_shared(
        &gate,
        Action::FullScreen,
        at_ms(0)
    ));
    assert!(!bindings::try_fire_shared(
        &gate,
        Action::FullScreen,
        at_ms(100)
    ));
    assert!(bindings::try_fire_shared(
        &gate,
        Action::FullScreen,
        at_ms(250)
    ));
}

#[test]
fn shared_gate_gates_actions_independently() {
    let gate = Arc::new(Mutex::new(Debounce::default()));

    assert!(bindings::try_fire_shared(
        &gate,
        Action::FullScreen,
        at_ms(0)
    ));
    assert!(bindings::try_fire_shared(&gate, Action::Window, at_ms(10)));
    assert!(!bindings::try_fire_shared(
        &gate,
        Action::FullScreen,
        at_ms(20)
    ));
}

#[test]
fn pad_notation_with_label_prefers_device_label_over_raw_index() {
    // Stage 5 (§4.1): the device-info label is the human-readable notation;
    // the raw index is only the fallback. An empty label is not a name.
    assert_eq!(
        pad_notation_with_label(
            PadDownEvent::Controller {
                device: 0,
                index: 7
            },
            Some("XboxX")
        ),
        "XboxX"
    );
    assert_eq!(
        pad_notation_with_label(
            PadDownEvent::Controller {
                device: 0,
                index: 7
            },
            Some("")
        ),
        "unknown:7"
    );
}

#[test]
fn pad_notation_with_label_falls_back_to_unknown_index_without_label() {
    assert_eq!(
        pad_notation_with_label(
            PadDownEvent::Controller {
                device: 1,
                index: 3
            },
            None
        ),
        "unknown:3"
    );
}

#[test]
fn pad_notation_with_label_keeps_guide_and_share_names() {
    // System buttons have no device-info label slot (§9): their names ARE the
    // notation, whatever label (if any) the caller hands in.
    assert_eq!(
        pad_notation_with_label(PadDownEvent::Guide, Some("XboxGuide")),
        "Guide"
    );
    assert_eq!(pad_notation_with_label(PadDownEvent::Share, None), "Share");
}

#[test]
fn pad_index_for_name_resolves_first_matching_device_label() {
    // Device-dependent mapping: index→name comes from device-info, never a
    // static table. Device order wins ties — device 0 labels XboxX at 3 even
    // though device 1 labels it at 9.
    let label_of = |device: usize, index: usize| match (device, index) {
        (0, 3) | (1, 9) => Some("XboxX".to_owned()),
        (1, 16) => Some("XboxLeftTrigger".to_owned()),
        _ => None,
    };
    assert_eq!(pad_index_for_name("XboxX", &[0, 1], label_of), Some(3));
    assert_eq!(
        pad_index_for_name("XboxLeftTrigger", &[0, 1], label_of),
        Some(16)
    );
    assert_eq!(pad_index_for_name("XboxX", &[1], label_of), Some(9));
    // Case-insensitive: config spelling may differ from the device's.
    assert_eq!(pad_index_for_name("xboxx", &[1], label_of), Some(9));
}

#[test]
fn pad_index_for_name_returns_none_without_any_matching_label() {
    // Unlabeled devices (v0, disconnected) and unknown names resolve to nothing.
    assert_eq!(
        pad_index_for_name("XboxX", &[0, 1], |_device: usize, _index: usize| None),
        None
    );
    assert_eq!(
        pad_index_for_name("XboxY", &[0], |device: usize, index: usize| (device == 0
            && index == 9)
            .then(|| "XboxX".to_owned())),
        None
    );
}

#[test]
fn pad_notation_named_round_trips_through_parse_pad() {
    // A label written by the capture path must parse back (the UI writes it
    // into config verbatim) and resolve back to the pressed index.
    let notation = pad_notation_with_label(
        PadDownEvent::Controller {
            device: 0,
            index: 9,
        },
        Some("XboxX"),
    );
    let mut controls = Controls::default();
    controls.full_screen.pad = Some(notation);
    let parsed = bindings::parse_controls(&controls).expect("label notation must parse");

    assert_eq!(
        pad_triggers(&parsed),
        vec![(Action::FullScreen, PadBind::Named("XboxX".to_owned()))]
    );
    assert_eq!(
        pad_index_for_name("XboxX", &[0], |device: usize, index: usize| (device == 0
            && index == 9)
            .then(|| "XboxX".to_owned())),
        Some(9)
    );
}

#[test]
fn resolve_pad_binds_named_bind_becomes_controller_index() {
    // Resolution maps a name to the live bit index through device-info; the
    // already-indexed and system binds pass through untouched, config order kept.
    let label_of =
        |device: usize, index: usize| (device == 0 && index == 9).then(|| "XboxX".to_owned());
    let triggers = vec![
        (Action::FullScreen, PadBind::Named("XboxX".to_owned())),
        (Action::Window, PadBind::Controller(4)),
        (Action::FullScreen, PadBind::Guide),
    ];
    let (resolved, unresolved) = resolve_pad_binds(&triggers, &[0], label_of);
    assert_eq!(
        resolved,
        vec![
            (Action::FullScreen, PadBind::Controller(9)),
            (Action::Window, PadBind::Controller(4)),
            (Action::FullScreen, PadBind::Guide),
        ]
    );
    assert_eq!(unresolved, Vec::<String>::new());
}

#[test]
fn resolve_pad_binds_unresolved_named_is_dropped_and_reported() {
    // A name no connected device reports fires nothing — the caller logs it.
    let label_of = |_device: usize, _index: usize| None;
    let triggers = vec![(Action::FullScreen, PadBind::Named("XboxX".to_owned()))];
    let (resolved, unresolved) = resolve_pad_binds(&triggers, &[0], label_of);
    assert_eq!(resolved, Vec::<(Action, PadBind)>::new());
    assert_eq!(unresolved, vec!["XboxX".to_owned()]);
}

#[test]
fn match_controller_ignores_unresolved_named_bind() {
    // Named binds are resolved to Controller before matching; a leftover Named
    // must never match a pressed index directly.
    assert_eq!(
        match_controller(
            &[9],
            &[(Action::FullScreen, PadBind::Named("XboxX".to_owned()))]
        ),
        None
    );
}
