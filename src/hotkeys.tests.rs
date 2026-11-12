use super::*;

use crate::config::Binding;
use crate::config::Controls;
use global_hotkey::hotkey::Code;
use global_hotkey::hotkey::Modifiers;

fn controls_full_window_rect() -> Controls {
    Controls {
        full_screen: Binding {
            key: "PrintScreen".to_owned(),
            mods: Vec::new(),
            pad: None,
        },
        window: Binding {
            key: "PrintScreen".to_owned(),
            mods: vec!["Alt".to_owned()],
            pad: None,
        },
        rect: Binding {
            key: "PrintScreen".to_owned(),
            mods: vec!["Ctrl".to_owned()],
            pad: None,
        },
    }
}

#[test]
fn capture_mode_maps_each_action() {
    assert_eq!(
        capture_mode(Action::FullScreen),
        Some(CaptureMode::FullScreen)
    );
    assert_eq!(capture_mode(Action::Window), Some(CaptureMode::Window));
    assert_eq!(capture_mode(Action::Rect), None);
}

#[test]
fn active_ids_maps_active_actions_only() {
    let parsed = bindings::parse_controls(&controls_full_window_rect()).expect("parsed");

    let ids = active_ids(&parsed);

    assert_eq!(ids.len(), 2);
    assert_eq!(ids.get(&parsed[0].hotkey.id()), Some(&Action::FullScreen));
    assert_eq!(ids.get(&parsed[1].hotkey.id()), Some(&Action::Window));
    assert!(!ids.contains_key(&parsed[2].hotkey.id()));
}

#[test]
fn active_hotkeys_lists_only_registered_combos() {
    let parsed = bindings::parse_controls(&controls_full_window_rect()).expect("parsed");

    let hotkeys = active_hotkeys(&parsed);

    assert_eq!(hotkeys.len(), 2);
    assert_eq!(hotkeys[0].id(), parsed[0].hotkey.id());
    assert_eq!(hotkeys[1].id(), parsed[1].hotkey.id());
}

#[test]
fn registration_reports_active_and_reserved_actions() {
    // Real manager on the test host (Linux without X11 still returns Ok
    // from register per the crate quirk; Windows headless CI is out of
    // scope for this unit). This asserts report SHAPE, not success.
    let Ok(manager) = GlobalHotKeyManager::new() else {
        return; // manager creation can fail on a headless host — not the point
    };
    let parsed = bindings::parse_controls(&controls_full_window_rect()).expect("parsed");

    let registrations = register(&manager, &parsed);

    assert_eq!(registrations.len(), 3);
    assert!(matches!(registrations[2], Registration::Reserved { .. }));
    // Actions 0 and 1 are either Registered or Failed, never Reserved.
    assert!(!matches!(registrations[0], Registration::Reserved { .. }));
    assert!(!matches!(registrations[1], Registration::Reserved { .. }));
}

#[test]
fn registration_fails_with_typed_reason_for_reserved_rect() {
    // The reserved action must never be handed to the manager; the report
    // says Reserved even without a manager (verified via register_all path
    // shape: we assert the non-active branch directly).
    let parsed = bindings::parse_controls(&controls_full_window_rect()).expect("parsed");
    let rect = &parsed[2];
    assert!(!rect.active);
    assert!(matches!(
        Registration::Reserved {
            action: rect.action,
            combo: rect.hotkey.to_string(),
        },
        Registration::Reserved { action, .. } if action == Action::Rect
    ));
}

#[test]
fn parse_failure_is_reported_as_binding_error() {
    let mut controls = controls_full_window_rect();
    controls.window.key = "NotAKey".to_owned();

    let err = bindings::parse_controls(&controls).expect_err("must fail");

    assert!(matches!(err, bindings::Error::UnknownKey { .. }));
}

#[cfg(target_os = "linux")]
#[test]
fn linux_backend_status_without_display_is_unavailable() {
    assert_eq!(
        backend_status(None, |_| true),
        BackendStatus::Unavailable {
            reason: "DISPLAY is not set (Wayland-only host; hotkeys are X11-only)".to_owned()
        }
    );
}

#[cfg(target_os = "linux")]
#[test]
fn x11_socket_path_matches_display_number() {
    let mut seen = Vec::new();
    backend_status(Some(":1".to_owned()), |path| {
        seen.push(path.to_owned());
        true
    });
    assert_eq!(seen, vec!["/tmp/.X11-unix/X1"]);
}

#[cfg(target_os = "linux")]
#[test]
fn backend_status_socket_missing_reports_unavailable() {
    let outcome = backend_status(Some(":0".to_owned()), |_| false);

    match outcome {
        BackendStatus::Unavailable { reason } => {
            assert!(reason.contains("/tmp/.X11-unix/X0"));
            assert!(reason.contains(":0"));
        }
        BackendStatus::Usable => panic!("expected typed unavailable status, got usable"),
    }
}

#[cfg(target_os = "linux")]
#[test]
fn backend_status_socket_present_allows_registration() {
    assert_eq!(
        backend_status(Some(":0".to_owned()), |path| path == "/tmp/.X11-unix/X0"),
        BackendStatus::Usable
    );
}

#[cfg(target_os = "linux")]
#[test]
fn backend_status_remote_display_attempts_registration() {
    assert!(matches!(
        backend_status(Some("192.168.1.5:0".to_owned()), |_| false),
        BackendStatus::Usable
    ));
}

#[cfg(target_os = "linux")]
#[test]
fn backend_status_malformed_display_attempts_registration() {
    assert!(matches!(
        backend_status(Some("nonsense".to_owned()), |_| false),
        BackendStatus::Usable
    ));
}

// Pause/restore state machine (Stage 18) — driven through the injected
// `Registrar` so no real backend or GUI is involved.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
    Register,
    Unregister,
}

#[derive(Clone, Default)]
struct MockRegistrar {
    ops: Arc<std::cell::RefCell<Vec<(Op, u32)>>>,
    /// Combo ids held by "another client": every `register` of such an id is
    /// rejected as occupied (Stage 13 — Windows `RegisterHotKey` failure /
    /// X11 `BadAccess`, both `Error::AlreadyRegistered`). `release_all` models
    /// that client letting go of the combo.
    occupied: Arc<std::cell::RefCell<Vec<u32>>>,
}

impl MockRegistrar {
    fn ops(&self) -> Vec<(Op, u32)> {
        self.ops.borrow().clone()
    }

    fn clear(&self) {
        self.ops.borrow_mut().clear();
    }

    fn ids(&self, op: Op) -> Vec<u32> {
        self.ops()
            .into_iter()
            .filter(|(kind, _)| *kind == op)
            .map(|(_, id)| id)
            .collect()
    }

    fn occupy(&self, ids: &[u32]) {
        self.occupied.borrow_mut().extend_from_slice(ids);
    }

    fn release_all(&self) {
        self.occupied.borrow_mut().clear();
    }
}

impl Registrar for MockRegistrar {
    fn register(&self, hotkey: HotKey) -> Result<(), RegistrationFailure> {
        self.ops.borrow_mut().push((Op::Register, hotkey.id()));
        if self.occupied.borrow().contains(&hotkey.id()) {
            Err(RegistrationFailure::AlreadyRegistered)
        } else {
            Ok(())
        }
    }

    fn unregister(&self, hotkey: HotKey) -> Result<(), String> {
        self.ops.borrow_mut().push((Op::Unregister, hotkey.id()));
        Ok(())
    }
}

fn settings_default_controls() -> Settings {
    Settings {
        controls: controls_full_window_rect(),
        ..Settings::default()
    }
}

fn settings_rebound() -> Settings {
    let mut controls = controls_full_window_rect();
    controls.full_screen = Binding {
        key: "S".to_owned(),
        mods: Vec::new(),
        pad: None,
    };
    controls.window = Binding {
        key: "S".to_owned(),
        mods: vec!["Ctrl".to_owned()],
        pad: None,
    };
    Settings {
        controls,
        ..Settings::default()
    }
}

fn controller_with(mock: MockRegistrar, settings: Settings) -> HotkeysController {
    let parsed = bindings::parse_controls(&settings.controls).expect("fixture parses");
    HotkeysController {
        registrar: Box::new(mock),
        registered: active_hotkeys(&parsed),
        mailbox: Arc::new(Mutex::new(settings)),
        paused: false,
        failed: Vec::new(),
        probe: new_probe_sink(),
        probe_hotkeys: Vec::new(),
    }
}

/// Controller as it exists after a registration pass in which some combos were
/// refused — the state the focus retry is designed for (`start`/`apply`/`restore`
/// build exactly this).
fn controller_after_failed_pass(mock: MockRegistrar, settings: Settings) -> HotkeysController {
    let parsed = bindings::parse_controls(&settings.controls).expect("fixture parses");
    let registrations = register(&mock, &parsed);
    HotkeysController {
        registrar: Box::new(mock),
        registered: active_hotkeys(&parsed),
        mailbox: Arc::new(Mutex::new(settings)),
        paused: false,
        failed: failed_combos(&parsed, &registrations),
        probe: new_probe_sink(),
        probe_hotkeys: Vec::new(),
    }
}

#[test]
fn pause_unregisters_all() {
    let mock = MockRegistrar::default();
    let mut controller = controller_with(mock.clone(), settings_default_controls());

    controller.pause();

    let parsed = bindings::parse_controls(&controls_full_window_rect()).expect("fixture parses");
    let active_ids = vec![parsed[0].hotkey.id(), parsed[1].hotkey.id()];
    assert_eq!(mock.ids(Op::Unregister), active_ids);
    assert!(mock.ids(Op::Register).is_empty());
}

#[test]
fn restore_registers_applied_settings() {
    let mock = MockRegistrar::default();
    let mut controller = controller_with(mock.clone(), settings_default_controls());

    controller.pause();
    mock.clear();
    controller.restore();

    let parsed = bindings::parse_controls(&controls_full_window_rect()).expect("fixture parses");
    assert_eq!(
        mock.ids(Op::Register),
        vec![parsed[0].hotkey.id(), parsed[1].hotkey.id()]
    );
    assert!(mock.ids(Op::Unregister).is_empty());
}

#[test]
fn double_pause_is_noop() {
    let mock = MockRegistrar::default();
    let mut controller = controller_with(mock.clone(), settings_default_controls());

    controller.pause();
    controller.pause();

    assert_eq!(mock.ops().len(), 2); // exactly one unregister pass, no re-register
    assert_eq!(mock.ids(Op::Register), Vec::<u32>::new());
}

#[test]
fn restore_without_pause_is_noop() {
    let mock = MockRegistrar::default();
    let mut controller = controller_with(mock.clone(), settings_default_controls());

    controller.restore();

    assert!(mock.ops().is_empty());
}

#[test]
fn restore_uses_last_applied_not_draft() {
    // The controller only ever sees settings through `apply` (the GUI keeps
    // the draft private until Apply), so restore must re-register the
    // settings published by the LAST apply, not the startup settings.
    let mock = MockRegistrar::default();
    let mut controller = controller_with(mock.clone(), settings_default_controls());

    controller
        .apply(settings_rebound())
        .expect("rebound settings apply");
    mock.clear();

    controller.pause();
    controller.restore();

    let rebound = bindings::parse_controls(&settings_rebound().controls).expect("fixture parses");
    assert_eq!(
        mock.ids(Op::Register),
        vec![rebound[0].hotkey.id(), rebound[1].hotkey.id()]
    );
}

// Readable failure reason and the focus-transition retry (Stage 13, spec
// §4.1 v2). The occupied combo is simulated by `MockRegistrar::occupy`, which
// reproduces the real backend behavior: both platforms report the crate's
// `Error::AlreadyRegistered` for a combo another client already holds.
#[test]
fn failure_mapping_from_crate_error_keeps_already_registered_typed() {
    // The crate's error enum is `#[non_exhaustive]`; the occupied-combo variant
    // must stay typed (it drives the readable reason), everything else keeps
    // the crate's own message.
    assert_eq!(
        RegistrationFailure::from(global_hotkey::Error::AlreadyRegistered(HotKey::new(
            Some(Modifiers::ALT),
            Code::PrintScreen
        ))),
        RegistrationFailure::AlreadyRegistered
    );
    assert_eq!(
        RegistrationFailure::from(global_hotkey::Error::HotKeyParseError(
            "Shift+Shift+PrintScreen".to_owned()
        )),
        RegistrationFailure::Other {
            message: "Shift+Shift+PrintScreen".to_owned()
        }
    );
}

#[test]
fn combo_label_matches_ui_binding_notation() {
    // The reason must name the combo exactly like the row shows it: canonical
    // modifier order (Alt, Ctrl, Shift, Win) and canonical key spelling.
    assert_eq!(
        combo_label(&HotKey::new(None, Code::PrintScreen)),
        "PrintScreen"
    );
    assert_eq!(
        combo_label(&HotKey::new(Some(Modifiers::ALT), Code::PrintScreen)),
        "Alt+PrintScreen"
    );
    // Config notation for letters/digits drops the crate's `Key`/`Digit`
    // prefix, so the reason names the combo exactly like the row does.
    assert_eq!(combo_label(&HotKey::new(None, Code::KeyS)), "S");
    assert_eq!(
        combo_label(&HotKey::new(Some(Modifiers::CONTROL), Code::Digit0)),
        "Ctrl+0"
    );
    assert_eq!(
        combo_label(&HotKey::new(
            Some(Modifiers::META | Modifiers::SHIFT | Modifiers::CONTROL),
            Code::KeyS,
        )),
        "Ctrl+Shift+Win+S"
    );

    // Consistency with the UI label of the same config binding, both bare and
    // modified — the reason text and the row must never disagree.
    let controls = controls_full_window_rect();
    let parsed = bindings::parse_controls(&controls).expect("fixture parses");
    assert_eq!(
        combo_label(&parsed[0].hotkey),
        crate::gui::binding::binding_label(&controls.full_screen.key, &controls.full_screen.mods)
    );
    assert_eq!(
        combo_label(&parsed[1].hotkey),
        crate::gui::binding::binding_label(&controls.window.key, &controls.window.mods)
    );
}

#[test]
fn already_registered_failure_reason_names_the_key_and_a_hint() {
    // The shipped wording was the crate's Debug dump (`HotKey already
    // registered: HotKey { mods: Modifiers(0x0), key: PrintScreen, id: 117 }`)
    // — unreadable in a tooltip and in the ERROR log.
    let reason = failure_reason(
        &RegistrationFailure::AlreadyRegistered,
        &HotKey::new(None, Code::PrintScreen),
    );

    assert!(reason.contains("already registered by another application"));
    assert!(reason.contains("PrintScreen"));
    assert!(reason.contains("free that key or pick another"));
    assert!(
        !reason.contains("Modifiers(") && !reason.contains("id:"),
        "reason must not leak the crate's Debug representation: {reason}"
    );
}

#[test]
fn other_registration_failure_reason_keeps_backend_message() {
    let reason = failure_reason(
        &RegistrationFailure::Other {
            message: "Unable to register hotkey.".to_owned(),
        },
        &HotKey::new(None, Code::PrintScreen),
    );

    assert_eq!(reason, "registration failed: Unable to register hotkey.");
}

#[test]
fn failed_combos_lists_failed_registrations_only() {
    let mock = MockRegistrar::default();
    let controls = controls_full_window_rect();
    let parsed = bindings::parse_controls(&controls).expect("fixture parses");
    mock.occupy(&[parsed[0].hotkey.id()]);

    let registrations = register(&mock, &parsed);
    let failed = failed_combos(&parsed, &registrations);

    assert_eq!(failed, vec![(Action::FullScreen, parsed[0].hotkey)]);
}

#[test]
fn retry_failed_registrations_keeps_only_still_failing() {
    // Startup pass: both active combos refused. Then the other client releases
    // ONE combo; the focus retry registers it and drops it from the retry list,
    // keeping the combo that is still held.
    let mock = MockRegistrar::default();
    let settings = settings_default_controls();
    let parsed = bindings::parse_controls(&settings.controls).expect("fixture parses");
    mock.occupy(&[parsed[0].hotkey.id(), parsed[1].hotkey.id()]);
    let mut controller = controller_after_failed_pass(mock.clone(), settings.clone());
    mock.clear();
    mock.release_all();
    mock.occupy(&[parsed[0].hotkey.id()]);

    let retried = controller.retry_failed_registrations();

    assert_eq!(
        retried
            .iter()
            .map(|registration| registration.action())
            .collect::<Vec<_>>(),
        vec![Action::FullScreen, Action::Window]
    );
    assert!(matches!(
        retried[1],
        Registration::Registered {
            action: Action::Window,
            combo: _
        }
    ));
    assert!(matches!(retried[0], Registration::Failed { .. }));
    assert_eq!(
        controller.failed,
        vec![(Action::FullScreen, parsed[0].hotkey)]
    );
    assert_eq!(
        mock.ids(Op::Register),
        vec![parsed[0].hotkey.id(), parsed[1].hotkey.id()]
    );
}

#[test]
fn retry_failed_registrations_retries_each_failed_combo_once_per_transition() {
    // Still occupied: the retry runs once per focus transition, one register op
    // per failed combo, never a duplicate — and stays retryable on the next
    // transition.
    let mock = MockRegistrar::default();
    let settings = settings_default_controls();
    let parsed = bindings::parse_controls(&settings.controls).expect("fixture parses");
    mock.occupy(&[parsed[0].hotkey.id(), parsed[1].hotkey.id()]);
    let mut controller = controller_after_failed_pass(mock.clone(), settings.clone());
    mock.clear();

    let first = controller.retry_failed_registrations();
    let second = controller.retry_failed_registrations();

    assert_eq!(first.len(), 2);
    assert!(
        first
            .iter()
            .all(|registration| matches!(registration, Registration::Failed { .. }))
    );
    assert!(
        second
            .iter()
            .all(|registration| matches!(registration, Registration::Failed { .. }))
    );
    assert_eq!(controller.failed.len(), 2);
    assert_eq!(
        mock.ids(Op::Register),
        vec![
            parsed[0].hotkey.id(),
            parsed[1].hotkey.id(),
            parsed[0].hotkey.id(),
            parsed[1].hotkey.id(),
        ]
    );
}

#[test]
fn retry_failed_registrations_does_not_re_register_live_combos() {
    // Only the refused combo is re-attempted; a combo that is already live is
    // left alone (a second `register` of the same combo would fail as
    // `AlreadyRegistered` against ourselves).
    let mock = MockRegistrar::default();
    let settings = settings_default_controls();
    let parsed = bindings::parse_controls(&settings.controls).expect("fixture parses");
    mock.occupy(&[parsed[0].hotkey.id()]);
    let mut controller = controller_after_failed_pass(mock.clone(), settings.clone());
    mock.clear();

    let retried = controller.retry_failed_registrations();

    assert_eq!(retried.len(), 1);
    assert_eq!(mock.ids(Op::Register), vec![parsed[0].hotkey.id()]);
    assert!(mock.ids(Op::Unregister).is_empty());
}

#[test]
fn retry_failed_registrations_skipped_while_capture_is_paused() {
    // While bind-by-press capture runs, the combos are deliberately unregistered
    // (§4.4 v5.5) — a focus change must not re-register them mid-capture.
    let mock = MockRegistrar::default();
    let settings = settings_default_controls();
    let parsed = bindings::parse_controls(&settings.controls).expect("fixture parses");
    mock.occupy(&[parsed[0].hotkey.id(), parsed[1].hotkey.id()]);
    let mut controller = controller_after_failed_pass(mock.clone(), settings.clone());
    mock.clear();

    controller.pause();
    let retried = controller.retry_failed_registrations();

    assert!(retried.is_empty());
    assert!(mock.ids(Op::Register).is_empty());
    // The candidates survive the pause, so `restore` is the next full pass.
    assert_eq!(controller.failed.len(), 2);
}

#[test]
fn retry_failed_registrations_after_release_reports_registered_and_stops() {
    // The success path: once the other client lets go of the combo, every
    // candidate registers, reports `Registered` with the UI combo label, and
    // there is nothing left to retry on a later transition.
    let mock = MockRegistrar::default();
    let settings = settings_default_controls();
    let parsed = bindings::parse_controls(&settings.controls).expect("fixture parses");
    mock.occupy(&[parsed[0].hotkey.id(), parsed[1].hotkey.id()]);
    let mut controller = controller_after_failed_pass(mock.clone(), settings.clone());
    mock.clear();
    mock.release_all();

    let retried = controller.retry_failed_registrations();
    let again = controller.retry_failed_registrations();

    assert_eq!(
        retried
            .iter()
            .map(|registration| registration.action())
            .collect::<Vec<_>>(),
        vec![Action::FullScreen, Action::Window]
    );
    assert!(
        retried
            .iter()
            .all(|registration| matches!(registration, Registration::Registered { .. }))
    );
    // `combo` stays the crate's own `Display` (shipped log wording, lowercase
    // modifiers); the readable REASON and the retry log lines use the UI label.
    assert_eq!(
        retried
            .iter()
            .map(|registration| match registration {
                Registration::Registered { combo, .. } => combo.clone(),
                other => panic!("expected a registered entry, got {other:?}"),
            })
            .collect::<Vec<_>>(),
        vec!["PrintScreen", "alt+PrintScreen"]
    );
    assert!(controller.failed.is_empty());
    assert!(again.is_empty());
    assert_eq!(mock.ids(Op::Register).len(), 2);
}

#[test]
fn apply_rebuilds_retry_candidates() {
    // Apply re-registers everything (shipped behavior), so the retry list must
    // reflect the NEW pass, not the startup one.
    let mock = MockRegistrar::default();
    mock.occupy(&[HotKey::new(None, Code::KeyS).id()]);
    let mut controller = controller_with(mock.clone(), settings_default_controls());

    controller
        .apply(settings_rebound())
        .expect("rebound settings apply");

    let rebound = bindings::parse_controls(&settings_rebound().controls).expect("fixture parses");
    assert_eq!(
        controller.failed,
        vec![(Action::FullScreen, rebound[0].hotkey)]
    );
}

#[test]
fn restore_rebuilds_retry_candidates() {
    // Restore re-registers the applied settings; combos refused there become the
    // retry candidates for the next focus transition.
    let mock = MockRegistrar::default();
    let settings = settings_default_controls();
    let parsed = bindings::parse_controls(&settings.controls).expect("fixture parses");
    let mut controller = controller_with(mock.clone(), settings.clone());

    controller.pause();
    mock.clear();
    mock.occupy(&[parsed[1].hotkey.id()]);
    controller.restore();

    assert_eq!(controller.failed, vec![(Action::Window, parsed[1].hotkey)]);
}

// Bind probe (Stage 14): temporary registrations for keys Windows never delivers
// to egui (`PrintScreen`, `ScrollLock`, `Pause`). The registration itself needs a
// Windows host, so the state machine is driven through the mock registrar here;
// the Windows runtime behavior is verified on the Windows rig.
#[test]
fn probe_combos_cover_every_probe_key_with_each_mod_set_once() {
    let combos = probe_combos();

    assert_eq!(combos.len(), PROBE_KEYS.len() * probe_mods().len());
    let unique: std::collections::HashSet<u32> = combos.iter().map(|hotkey| hotkey.id()).collect();
    assert_eq!(unique.len(), combos.len(), "one probe per id");
    // Deterministic order: the first combo is the bare probe of the first key.
    assert_eq!(combos[0], HotKey::new(None, PROBE_KEYS[0]));
    assert_eq!(combos[6], HotKey::new(None, PROBE_KEYS[1]));
    // No Win/META combo: such a press can never arrive as a bind candidate.
    assert!(combos.iter().all(|hotkey| !hotkey.mods.meta()));
    assert_eq!(
        probe_combos(),
        combos,
        "the probe set is stable across calls"
    );
}

#[test]
fn key_label_names_the_probe_keys_as_the_ui_notation() {
    // The row renders `key_label` as a config key name, so the probe keys must
    // spell exactly what `bindings::parse_key` accepts.
    assert_eq!(key_label(Code::PrintScreen), "PrintScreen");
    assert_eq!(key_label(Code::ScrollLock), "ScrollLock");
    assert_eq!(key_label(Code::Pause), "Pause");
    for name in PROBE_KEYS.iter().map(|key| key_label(*key)) {
        let mut settings = Settings::default();
        settings.controls.full_screen.key = name.clone();
        settings.controls.full_screen.mods.clear();
        assert!(
            bindings::parse_controls(&settings.controls).is_ok(),
            "probe key {name} must parse as a hotkey combo"
        );
    }
}

#[test]
fn probe_press_for_ignores_ids_that_are_not_live_probes() {
    let sink = new_probe_sink();
    let live = register_probe_combos(&MockRegistrar::default(), &probe_combos(), &sink);

    let state = probe_state(&sink);
    assert_eq!(probe_press_for(&state, 99_999), None);
    assert_eq!(
        probe_press_for(&state, live[0].id()),
        Some(ProbePress {
            code: Code::PrintScreen,
            mods: Modifiers::empty(),
        })
    );
    assert_eq!(
        probe_press_for(&state, live[5].id()),
        Some(ProbePress {
            code: Code::PrintScreen,
            mods: Modifiers::CONTROL | Modifiers::SHIFT,
        })
    );
    assert_eq!(
        probe_press_for(&state, live[7].id()),
        Some(ProbePress {
            code: Code::ScrollLock,
            mods: Modifiers::ALT,
        })
    );
    assert_eq!(
        probe_press_for(&state, live[12].id()),
        Some(ProbePress {
            code: Code::Pause,
            mods: Modifiers::empty(),
        })
    );
}

#[test]
fn probe_lookup_precedes_the_action_map_on_a_bare_id_collision() {
    // The shipped default is a BARE PrintScreen action binding, and the bare
    // probe of the same key has the SAME crate id (`id == (mods << 16) | code`).
    // The listener consults the probe map FIRST, so a fired bare PrintScreen
    // while a capture is open is recorded as a bind candidate — never a shot.
    let parsed =
        bindings::parse_controls(&settings_default_controls().controls).expect("fixture parses");
    let action_id = parsed[0].hotkey.id();
    assert_eq!(action_id, HotKey::new(None, Code::PrintScreen).id());

    let sink = new_probe_sink();
    register_probe_combos(&MockRegistrar::default(), &probe_combos(), &sink);

    assert_eq!(
        probe_press_for(&probe_state(&sink), action_id),
        Some(ProbePress {
            code: Code::PrintScreen,
            mods: Modifiers::empty(),
        })
    );
    assert!(record_probe_press(&sink, action_id));
    assert_eq!(
        probe_state(&sink).press.map(|press| press.code),
        Some(Code::PrintScreen)
    );
}

#[test]
fn record_probe_press_leaves_unknown_ids_to_the_action_dispatch() {
    let sink = new_probe_sink();
    register_probe_combos(&MockRegistrar::default(), &probe_combos(), &sink);

    assert!(
        !record_probe_press(&sink, HotKey::new(None, Code::KeyS).id()),
        "a combo outside the probe set must stay an action dispatch"
    );
    assert!(
        probe_state(&sink).press.is_none(),
        "no press may be recorded"
    );

    assert!(record_probe_press(
        &sink,
        HotKey::new(None, Code::Pause).id()
    ));
    assert_eq!(
        probe_state(&sink).press.map(|press| press.code),
        Some(Code::Pause)
    );
}

#[test]
fn register_probe_combos_keeps_refused_combos_out_of_the_live_map() {
    // Snipping Tool holding bare PrintScreen is the reported case: that combo is
    // refused (WARN), the rest take. A refused combo must never be treated as a
    // probe or unregistered later.
    let mock = MockRegistrar::default();
    let refused = HotKey::new(None, Code::PrintScreen).id();
    mock.occupy(&[refused]);
    let sink = new_probe_sink();

    let live = register_probe_combos(&mock, &probe_combos(), &sink);

    assert_eq!(live.len(), probe_combos().len() - 1);
    assert!(!live.iter().any(|hotkey| hotkey.id() == refused));
    assert_eq!(probe_state(&sink).combos.len(), live.len());
    assert_eq!(mock.ids(Op::Register).len(), probe_combos().len());
    assert_eq!(probe_press_for(&probe_state(&sink), refused), None);
}

#[test]
fn take_probe_press_drains_the_latest_press_once() {
    let mock = MockRegistrar::default();
    let mut controller = controller_with(mock.clone(), settings_default_controls());
    let live = register_probe_combos(&mock, &probe_combos(), &controller.probe);

    record_probe_press(&controller.probe, live[1].id());
    record_probe_press(&controller.probe, live[4].id());

    assert_eq!(
        controller.take_probe_press(),
        Some(ProbePress {
            code: Code::PrintScreen,
            mods: Modifiers::ALT | Modifiers::SHIFT,
        }),
        "only the newest press survives"
    );
    assert_eq!(
        controller.take_probe_press(),
        None,
        "a drained slot is empty"
    );
}

#[test]
fn attach_probe_wake_keeps_a_repaint_context_for_the_listener() {
    let mut controller = controller_with(MockRegistrar::default(), settings_default_controls());

    controller.attach_probe_wake(egui::Context::default());

    assert!(probe_state(&controller.probe).wake.is_some());
    let live = register_probe_combos(
        &MockRegistrar::default(),
        &probe_combos(),
        &controller.probe,
    );
    // A press is recorded with the wake attached: the slot holds it, the GUI
    // drains it on its next logic pass.
    assert!(record_probe_press(&controller.probe, live[0].id()));
    assert_eq!(
        controller.take_probe_press().map(|press| press.code),
        Some(Code::PrintScreen)
    );
}

#[test]
fn start_bind_probe_does_nothing_without_a_paused_capture() {
    let mock = MockRegistrar::default();
    let mut controller = controller_with(mock.clone(), settings_default_controls());

    assert_eq!(controller.start_bind_probe(), 0);
    assert!(mock.ids(Op::Register).is_empty());
    assert!(controller.probe_hotkeys.is_empty());
    assert!(controller.take_probe_press().is_none());
}

#[cfg(not(windows))]
#[test]
fn start_bind_probe_registers_nothing_off_windows() {
    // Linux delivers PrintScreen/ScrollLock/Pause to egui itself, so a probe
    // there would swallow those keys system-wide for nothing — the capture flow
    // must stay byte-for-byte unchanged on this host.
    let mock = MockRegistrar::default();
    let mut controller = controller_with(mock.clone(), settings_default_controls());

    controller.pause();

    assert_eq!(controller.start_bind_probe(), 0);
    assert!(mock.ids(Op::Register).is_empty());
    assert!(probe_state(&controller.probe).combos.is_empty());
    assert!(controller.probe_hotkeys.is_empty());
}

#[test]
fn stop_bind_probe_unregisters_live_probes_and_clears_the_slot() {
    // The Windows registration path cannot run on this host, so the live state is
    // filled exactly as `start_bind_probe` fills it on Windows.
    let mock = MockRegistrar::default();
    let mut controller = controller_with(mock.clone(), settings_default_controls());
    controller.pause();
    let live = register_probe_combos(&mock, &probe_combos(), &controller.probe);
    controller.probe_hotkeys = live.clone();
    record_probe_press(&controller.probe, live[0].id());
    mock.clear();

    controller.stop_bind_probe();

    assert_eq!(
        mock.ids(Op::Unregister),
        live.iter().map(|hotkey| hotkey.id()).collect::<Vec<_>>()
    );
    assert!(controller.probe_hotkeys.is_empty());
    let state = probe_state(&controller.probe);
    assert!(state.combos.is_empty(), "no id may stay a probe");
    assert!(
        state.press.is_none(),
        "a stale press must not commit into a later capture"
    );
}

#[test]
fn stop_bind_probe_without_probes_unregisters_nothing() {
    let mock = MockRegistrar::default();
    let mut controller = controller_with(mock.clone(), settings_default_controls());

    controller.stop_bind_probe();

    assert!(mock.ids(Op::Unregister).is_empty());
    assert!(controller.probe_hotkeys.is_empty());
}

#[test]
fn apply_stops_a_live_probe_before_re_registering() {
    // Apply ends the paused capture and registers the new combos; a leftover bare
    // probe would share an id with a live action combo, so it must be gone first.
    let mock = MockRegistrar::default();
    let mut controller = controller_with(mock.clone(), settings_default_controls());
    controller.pause();
    let live = register_probe_combos(&mock, &probe_combos(), &controller.probe);
    controller.probe_hotkeys = live.clone();
    mock.clear();

    controller
        .apply(settings_rebound())
        .expect("rebound settings apply");

    assert!(controller.probe_hotkeys.is_empty());
    assert!(probe_state(&controller.probe).combos.is_empty());
    let probes_off = mock
        .ids(Op::Unregister)
        .into_iter()
        .filter(|id| live.iter().any(|hotkey| hotkey.id() == *id))
        .collect::<Vec<_>>();
    assert_eq!(
        probes_off.len(),
        live.len(),
        "every live probe was unregistered"
    );
}
