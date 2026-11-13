//! Autostart entry wiring (task `2026-11-13_autostart`, docs/spec.md §4.4):
//! the Windows-only config flag `general.autostart` decides whether the HKCU
//! `Software\Microsoft\Windows\CurrentVersion\Run` value `virshot` exists, and
//! [`sync`] performs that decision — write, delete, or skip.
//!
//! Shape follows the `gamepad` precedent: the pure logic (value names, payload
//! quoting, the write/delete/skip decision table) is `#[cfg(any(test,
//! windows))]` so Linux unit tests pin it without a registry — except the
//! scratch `TEST_VALUE_NAME`, which is `#[cfg(test)]`-only because no shipped
//! build references it; the registry
//! calls are `#[cfg(windows)]` through the target-scoped `winreg` dependency
//! (approved by the user — pure Rust, compiles under MinGW), and off Windows
//! [`sync`] is a no-op `Ok(())`. A Linux production build compiles none of
//! this module: nothing there reads `general.autostart` (the field is
//! `#[cfg(windows)]` in `config`), so gating also keeps clippy's dead_code
//! quiet on Linux.
//!
//! Test safety: under `cfg(test)` [`sync`] operates on `TEST_VALUE_NAME`,
//! never `VALUE_NAME`, so a test run on a Windows host can never enable or
//! disable the user's real `virshot` autostart entry.
//!
//! Errors are values (AGENTS.md): every registry or path-resolution failure
//! maps to a typed [`Error`] variant; [`sync`] never panics, and callers log
//! the failure without failing the settings save.

/// Run value name in shipped builds — the product name (spec §4.4).
#[cfg(any(test, windows))]
pub(crate) const VALUE_NAME: &str = "virshot";

/// Scratch Run value name used by the Windows integration tests: under
/// `cfg(test)` [`sync`] writes and deletes THIS name only, so a test run
/// never touches the host user's real entry. `#[cfg(test)]`-gated (not
/// `any(test, windows)`): a shipped Windows build never references it, so
/// `dead_code` stays quiet there.
#[cfg(test)]
pub(crate) const TEST_VALUE_NAME: &str = "virshot_autostart_test";

/// What [`sync`] must do with the Run value (decision table output).
#[derive(Debug, PartialEq, Eq)]
#[cfg(any(test, windows))]
pub(crate) enum Action {
    /// Write (or rewrite) the value with the payload.
    Register,
    /// Remove the value.
    Unregister,
    /// Leave the registry untouched.
    Unchanged,
}

/// Decision behind the sync: an enabled flag writes the payload when the
/// stored value is absent or differs (a moved exe re-registers); a disabled
/// flag removes a present value; matching/paired states skip the registry.
#[cfg(any(test, windows))]
pub(crate) fn decide_action(enabled: bool, current: Option<&str>, payload: &str) -> Action {
    match (enabled, current) {
        (true, Some(current)) if current == payload => Action::Unchanged,
        (true, _) => Action::Register,
        (false, Some(_)) => Action::Unregister,
        (false, None) => Action::Unchanged,
    }
}

/// Windows Run payload: the exe path wrapped in double quotes (the shell
/// resolves the value, so paths with spaces need quoting — spec §4.4). Plain
/// `"` characters are stripped from the path defensively; Windows paths
/// cannot contain them anyway, and a stray quote would otherwise break the
/// quoting around the whole path.
#[cfg(any(test, windows))]
pub(crate) fn run_value_payload(exe_path: &str) -> String {
    format!("\"{}\"", exe_path.replace('"', ""))
}

/// Typed registry/path failures surfaced by [`sync`]. Callers log and keep
/// running — an autostart failure must not fail a settings save (spec §5).
#[derive(Debug, thiserror::Error)]
#[allow(dead_code)] // Variants are constructed by the Windows sync only.
#[cfg(any(test, windows))]
pub(crate) enum Error {
    #[error("failed to resolve the executable path for the autostart entry: {0}")]
    CurrentExe(std::io::Error),
    #[error("failed to create or open the autostart registry key: {0}")]
    RegistryOpen(std::io::Error),
    #[error("failed to read the autostart registry value: {0}")]
    RegistryRead(std::io::Error),
    #[error("failed to write the autostart registry value: {0}")]
    RegistryWrite(std::io::Error),
    #[error("failed to delete the autostart registry value: {0}")]
    RegistryDelete(std::io::Error),
}

/// Sync the HKCU Run entry with the requested `enabled` state.
///
/// Windows: resolves `current_exe()`, builds [`run_value_payload`], creates
/// or opens the Run subkey with `KEY_ALL_ACCESS` (`RegCreateKeyEx` semantics:
/// a clean Windows profile may have no HKCU `Run` key at all, and enabling
/// autostart must create it), reads the stored value (absent —
/// `ErrorKind::NotFound` — is `None`), applies [`decide_action`), and only
/// then writes (`set_value`), deletes (`delete_value`; an already-absent
/// value counts as `Unchanged`), or skips. Under `cfg(test)` the value name
/// is `TEST_VALUE_NAME`; shipped builds use `VALUE_NAME`. RegKey handles are
/// neither `Sync` nor `Clone`, so one fresh handle is opened per call.
/// Off Windows: no-op `Ok(())`.
///
/// Failure modes: executable path resolution, key create-or-open, value
/// read, value write, and value delete failures return the typed [`Error`]
/// variants — never a panic.
#[cfg(any(test, windows))]
pub(crate) fn sync(enabled: bool) -> Result<(), Error> {
    #[cfg(windows)]
    {
        use winreg::HKCU;
        use winreg::enums::KEY_ALL_ACCESS;

        const RUN_KEY_PATH: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

        #[cfg(test)]
        let value_name = TEST_VALUE_NAME;
        #[cfg(not(test))]
        let value_name = VALUE_NAME;

        let exe = std::env::current_exe().map_err(Error::CurrentExe)?;
        let payload = run_value_payload(&exe.to_string_lossy());
        let (key, _disposition) = HKCU
            .create_subkey_with_flags(RUN_KEY_PATH, KEY_ALL_ACCESS)
            .map_err(Error::RegistryOpen)?;
        let current = match key.get_value::<String, _>(value_name) {
            Ok(value) => Some(value),
            // ERROR_FILE_NOT_FOUND maps to `ErrorKind::NotFound`: absence is
            // a state, not a failure.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => return Err(Error::RegistryRead(err)),
        };
        match decide_action(enabled, current.as_deref(), &payload) {
            Action::Register => {
                key.set_value(value_name, &payload)
                    .map_err(Error::RegistryWrite)?;
            }
            Action::Unregister => {
                match key.delete_value(value_name) {
                    Ok(()) => {}
                    // The value disappeared between the read and the delete:
                    // the requested end state (absent) already holds.
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                    Err(err) => return Err(Error::RegistryDelete(err)),
                }
            }
            Action::Unchanged => {}
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = enabled;
        Ok(())
    }
}
