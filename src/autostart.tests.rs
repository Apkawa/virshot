//! Unit tests for the `autostart` module (task 2026-11-13_autostart): the
//! HKCU Run decision table (`decide_action`), the quoted Run payload shape
//! (`run_value_payload`), the Run value names, and the `sync` behavior off
//! Windows.
//!
//! RED by design: `src/autostart.rs` does not exist yet, so the
//! `pub(crate) mod autostart;` declaration in `main.rs` fails with
//! "couldn't read src/autostart.rs: no such file or directory" on both
//! targets. The Windows integration test additionally needs the `winreg`
//! dependency (added by the next step); `#[cfg(windows)]` code is not
//! compiled on Linux, so Linux red evidence is the missing module file only.
//!
//! The Windows integration test drives `sync` against the real HKCU Run key
//! through `TEST_VALUE_NAME` ONLY: under `cfg(test)` `sync` must register and
//! unregister the SCRATCH name, never `VALUE_NAME`, so a test run can never
//! enable or disable autostart for the host user's real `virshot` entry.

use super::*;

/// Payload used by the decision-table tests; the same quoted shape
/// `run_value_payload` produces, so `decide_action` inputs are realistic.
const PAYLOAD: &str = "\"C:\\Program Files\\virshot\\virshot.exe\"";

#[test]
fn decide_action_enabled_missing_registers() {
    // Autostart requested, no Run value yet: the value must be written.
    assert_eq!(
        autostart::decide_action(true, None, PAYLOAD),
        autostart::Action::Register,
        "enabled + absent must Register"
    );
}

#[test]
fn decide_action_enabled_changed_registers() {
    // Autostart requested and the stored payload differs (moved exe, renamed
    // path): the value must be rewritten.
    assert_eq!(
        autostart::decide_action(true, Some("\"C:\\Other\\virshot.exe\""), PAYLOAD),
        autostart::Action::Register,
        "enabled + differing current must Register"
    );
}

#[test]
fn decide_action_enabled_unchanged_skips() {
    // Autostart requested and the stored payload already matches: no registry
    // write must happen.
    assert_eq!(
        autostart::decide_action(true, Some(PAYLOAD), PAYLOAD),
        autostart::Action::Unchanged,
        "enabled + matching current must skip"
    );
}

#[test]
fn decide_action_disabled_present_unregisters() {
    // Autostart off and the Run value exists: it must be removed.
    assert_eq!(
        autostart::decide_action(false, Some(PAYLOAD), PAYLOAD),
        autostart::Action::Unregister,
        "disabled + present must Unregister"
    );
}

#[test]
fn decide_action_disabled_absent_skips() {
    // Autostart off and nothing is registered: nothing to remove.
    assert_eq!(
        autostart::decide_action(false, None, PAYLOAD),
        autostart::Action::Unchanged,
        "disabled + absent must skip"
    );
}

#[test]
fn run_value_payload_quotes_path() {
    // Windows runs the Run value through the shell, so the path must be
    // quoted verbatim — spaces included (spec §4 autostart payload).
    assert_eq!(
        autostart::run_value_payload("C:\\Users\\Test User\\virshot.exe"),
        "\"C:\\Users\\Test User\\virshot.exe\"",
        "the exe path must be wrapped in double quotes"
    );
}

#[test]
fn run_value_payload_strips_embedded_quotes() {
    // Windows paths cannot contain `"` anyway, but a defensive strip keeps a
    // malformed input from breaking the quoting around the whole path.
    assert_eq!(
        autostart::run_value_payload("C:\\Users\\Test\"User\\virshot.exe"),
        "\"C:\\Users\\TestUser\\virshot.exe\"",
        "plain quote characters must be removed from the path"
    );
}

#[test]
fn value_name_is_virshot() {
    // The Run value name is the product name; the scratch name used by
    // integration tests must be a different, clearly-marked value.
    assert_eq!(autostart::VALUE_NAME, "virshot");
    assert_eq!(autostart::TEST_VALUE_NAME, "virshot_autostart_test");
    assert_ne!(
        autostart::TEST_VALUE_NAME,
        autostart::VALUE_NAME,
        "tests must never touch the real Run value name"
    );
}

#[cfg(not(windows))]
#[test]
fn sync_off_windows_is_noop_ok() {
    // Linux/macOS have no HKCU Run key: `sync` must do nothing and report
    // success rather than failing the settings save.
    assert!(
        autostart::sync(true).is_ok(),
        "sync(true) off Windows must be a no-op Ok(())"
    );
    assert!(
        autostart::sync(false).is_ok(),
        "sync(false) off Windows must be a no-op Ok(())"
    );
}

#[cfg(windows)]
#[test]
fn sync_registers_and_unregisters_scratch_value() {
    use std::io::ErrorKind;
    use winreg::HKCU;
    use winreg::enums::KEY_ALL_ACCESS;

    const RUN_KEY_PATH: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

    // Drop guard: removes the scratch value even if an assertion above fails.
    // `VALUE_NAME` is deliberately never touched (see the header note).
    struct ScratchGuard;
    impl Drop for ScratchGuard {
        fn drop(&mut self) {
            let _ = HKCU
                .open_subkey_with_flags(RUN_KEY_PATH, KEY_ALL_ACCESS)
                .and_then(|key| key.delete_value(autostart::TEST_VALUE_NAME));
        }
    }

    // Create-or-open, mirroring `sync`: a clean profile (and GitHub Actions
    // Windows runners) may have no HKCU `Run` subkey at all, so the test must
    // not assume it exists.
    let (run, _disposition) = HKCU
        .create_subkey_with_flags(RUN_KEY_PATH, KEY_ALL_ACCESS)
        .expect("HKCU Run key must be create-or-openable");
    let _guard = ScratchGuard;

    let exe = std::env::current_exe().expect("current_exe must be available");
    let payload = autostart::run_value_payload(&exe.to_string_lossy());

    // Absence is proven by ERROR_FILE_NOT_FOUND (winreg maps it to
    // `io::ErrorKind::NotFound`).
    fn scratch_absent(key: &winreg::RegKey) -> bool {
        match key.get_value::<String, _>(autostart::TEST_VALUE_NAME) {
            Err(err) => err.kind() == ErrorKind::NotFound,
            Ok(_) => false,
        }
    }

    // Phase 0: scratch plumbing against `TEST_VALUE_NAME` directly through
    // winreg, proving the key handle supports write/read/delete without
    // involving the real Run entry.
    let _ = run.delete_value(autostart::TEST_VALUE_NAME);
    run.set_value(autostart::TEST_VALUE_NAME, &payload)
        .expect("scratch value must be writable");
    assert_eq!(
        run.get_value::<String, _>(autostart::TEST_VALUE_NAME)
            .expect("scratch value must be readable"),
        payload,
        "scratch value data must round-trip"
    );
    run.delete_value(autostart::TEST_VALUE_NAME)
        .expect("scratch value must be deletable");
    assert!(
        scratch_absent(&run),
        "scratch value must be gone after delete_value"
    );

    // Phase 1: `sync(true)` creates the scratch value whose data equals the
    // quoted current exe path. The value stays for phase 2.
    autostart::sync(true).expect("sync(true) must succeed");
    assert_eq!(
        run.get_value::<String, _>(autostart::TEST_VALUE_NAME)
            .expect("sync(true) must create the scratch value"),
        payload,
        "registered data must equal run_value_payload(current_exe())"
    );

    // Phase 2: `sync(false)` removes the value the phase-1 call left behind.
    autostart::sync(false).expect("sync(false) must succeed");
    assert!(
        scratch_absent(&run),
        "sync(false) must delete the scratch value (ERROR_FILE_NOT_FOUND)"
    );

    // Phase 3: `sync(true)` twice — the second call sees the matching payload
    // (`decide_action` -> `Unchanged`) and must leave the value intact.
    autostart::sync(true).expect("first sync(true) must succeed");
    assert_eq!(
        run.get_value::<String, _>(autostart::TEST_VALUE_NAME)
            .expect("first sync(true) must create the scratch value"),
        payload
    );
    autostart::sync(true).expect("second sync(true) must succeed");
    assert_eq!(
        run.get_value::<String, _>(autostart::TEST_VALUE_NAME)
            .expect("scratch value must survive the second sync(true)"),
        payload,
        "a second sync(true) must leave the value intact"
    );
    // Explicit cleanup after the last phase (the guard covers failure paths).
    let _ = run.delete_value(autostart::TEST_VALUE_NAME);
}
