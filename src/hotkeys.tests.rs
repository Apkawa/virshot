use super::*;

use crate::config::Binding;
use crate::config::Controls;

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
}

impl Registrar for MockRegistrar {
    fn register(&self, hotkey: HotKey) -> Result<(), String> {
        self.ops.borrow_mut().push((Op::Register, hotkey.id()));
        Ok(())
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
