//! Global hotkey wiring over the `global-hotkey` crate (spec §4.1 + §6 Input
//! Thread): create the manager, register the active MVP combos (full screen,
//! window — the rect action stays reserved and is never registered), report
//! typed per-hotkey registration status (`Registration` — the GUI renders it as
//! an icon per spec §4.4 v5.0, `main` logs it), re-register live on Apply
//! (Stage 8), dispatch debounced triggers to `capture::spawn_capture`, and
//! pause/restore registrations around bind-by-press capture (Stage 18, spec
//! §4.4 v5.5) so a press of a live combo — notably PrintScreen, whose
//! WM_KEYDOWN is stolen by the registered WM_HOTKEY hook on Windows — reaches
//! the egui window as a bind candidate instead of firing the screenshot action.
//!
//! Crate facts verified in the locked `global-hotkey` 0.9.0 source, not from
//! memory:
//! - `GlobalHotKeyManager::new() -> Result<Self, global_hotkey::Error>`,
//!   `register(HotKey) -> Result<()>` (`lib.rs:146`), `unregister(HotKey) ->
//!   Result<()>` (`lib.rs:150`), `unregister_all(&[HotKey]) -> Result<()>`
//!   (`lib.rs:159`; there is no `HotKeysManager` / `set_shortcuts` in 0.9.0).
//!   `unregister_all` aborts at the first failure (`windows/mod.rs:140-145`
//!   loops with `?`), so this module loops `unregister` per combo and keeps
//!   going with a WARN — pause must remove as much as possible.
//! - The manager is neither `Send` nor `Clone`: the Windows impl holds an
//!   `HWND` (`platform_impl/windows/mod.rs:22-24`, destroyed only on `Drop`,
//!   `:26-30`), the X11 impl holds a channel sender to its pump thread
//!   (`platform_impl/x11/mod.rs:26-28`). [`HotkeysController`] owns it on one
//!   thread and never moves it across threads, so no `Send` bound is needed.
//! - Windows: `new()` creates a hidden window **on the calling thread** and
//!   `WM_HOTKEY` is delivered to that thread's window procedure, so a Win32
//!   message loop must run on the same thread (crate docs; Stage 6 finding).
//!   winit/eframe pumps Win32 messages on the MAIN thread, so the manager is
//!   created and re-registered on the main thread, inside
//!   [`HotkeysController`] — the crate's `wnd_proc` runs inside the winit
//!   pump; no unsafe in our code.
//! - Linux (X11 only): `new()` spawns the crate's own X11 event-pump thread,
//!   so this module pumps nothing; `register()`/`unregister()` hand requests
//!   to that thread over a channel. The manager object stays on the main
//!   thread too — nothing needs to move it across threads. Quirk: if the pump
//!   thread died (no X server), `register()` returns `Ok(())` because the
//!   crate ignores the disconnected reply channel — hence the X11 probe in
//!   [`check_backend`].
//! - macOS: the crate requires the manager on the main thread with a Cocoa
//!   run loop; macOS is out of MVP scope (spec §2: Windows target, Linux
//!   best-effort).
//! - Types: `global_hotkey::hotkey::{HotKey, Code, Modifiers}` re-export
//!   `keyboard_types` (`Code`, not `Key`); modifiers are `ALT`/`CONTROL`/
//!   `SHIFT`/`META` (legacy `SUPER` is folded onto `META` by `HotKey::new`).
//! - Events: `GlobalHotKeyEvent::receiver()` (blocking `recv`) yields
//!   `GlobalHotKeyEvent { id, state }` where `id == HotKey::id()`
//!   (`(mods.bits() << 16) | Code as u32`) and `state` is
//!   `HotKeyState::Pressed`/`Released`. The alternative `set_event_handler`
//!   is unused: the receiver keeps the debounce state thread-local (no shared
//!   `Mutex` for trigger state, per the repo thread-boundary rule). The
//!   receiver is a reference to a static channel (`GLOBAL_HOTKEY_CHANNEL`,
//!   `lib.rs:87-88`, `receiver()` at `lib.rs:108-110`) fed by
//!   `GlobalHotKeyEvent::send` (`lib.rs:126-132`); unregistering touches only
//!   the platform registration (`UnregisterHotKey`, `windows/mod.rs:125-131`;
//!   `ungrab_key` keeps the X11 pump loop alive until `DropThread`,
//!   `x11/mod.rs:205-217`), so pause/restore never breaks the listener's
//!   receiver — events simply stop, then resume.
//!
//! Thread shape (Stage 8): the manager lives on the MAIN thread inside
//! [`HotkeysController`] — registration and live re-registration (Apply) run
//! there, which is mandatory on Windows (WM_HOTKEY thread affinity) and
//! equally correct on Linux. A dedicated listener OS thread blocks on the
//! global event receiver, maps `id` → action, fires through the SHARED
//! debounce gate (the same `Mutex<bindings::Debounce>` instance the pad pump
//! uses, spec §4.1 OR-fire — one shot per action per interval regardless of
//! the trigger source), and spawns the capture. Latest settings reach the listener via a
//! `Mutex<Settings>` mailbox used ONLY as a latest-config mailbox (an owned
//! clone is taken per trigger — no shared capture state). Per-trigger parsing
//! means a re-registered combo is honored by captures immediately, with no
//! stale id map. Registration status is produced synchronously on the main
//! thread (both `start` and `apply`) and handed to the GUI; no status channel
//! is needed because registration never happens off the main thread.
//! [`HotkeysController`] is owned by `gui::SettingsApp` (Stage 8 wiring), so
//! bind-by-press pause/restore (Stage 18) is called from the GUI logic pass —
//! the same main thread the manager was created on.

/// Platform registration surface of the crate manager, injected so the
/// pause/restore state machine is unit-testable without a live backend.
/// Errors are owned strings (`HotKey`/`Settings` stay the shared types);
/// registration failures are reported per combo, never propagated.
pub(crate) trait Registrar {
    fn register(&self, hotkey: HotKey) -> Result<(), String>;
    fn unregister(&self, hotkey: HotKey) -> Result<(), String>;
}

impl Registrar for GlobalHotKeyManager {
    fn register(&self, hotkey: HotKey) -> Result<(), String> {
        GlobalHotKeyManager::register(self, hotkey).map_err(|err| err.to_string())
    }

    fn unregister(&self, hotkey: HotKey) -> Result<(), String> {
        GlobalHotKeyManager::unregister(self, hotkey).map_err(|err| err.to_string())
    }
}

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;

use chrono::Local;
use global_hotkey::GlobalHotKeyEvent;
use global_hotkey::GlobalHotKeyManager;
use global_hotkey::HotKeyState;
use global_hotkey::hotkey::HotKey;

use crate::bindings;
use crate::bindings::Action;
use crate::bindings::ActionBinding;
use crate::bindings::Debounce;
use crate::capture::CaptureCommand;
use crate::capture::CaptureMode;
use crate::capture::ToastHandoff;
use crate::capture::handoff_handle;
use crate::capture::spawn_capture;
use crate::config::Settings;

/// Hotkey wiring failures, typed per module (`thiserror` enum). Registration
/// failures are *not* errors here — they are reported per hotkey inside
/// [`StartStatus`] so the app keeps running (spec §4.1).
#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    /// The platform backend cannot be used on this host (Linux without an X11
    /// server). The app logs it and stays resident. Constructed by the Linux
    /// backend probe only; other platforms report real failures via
    /// [`Error::Manager`].
    #[cfg(target_os = "linux")]
    #[error("global hotkeys are unavailable on this host: {reason}")]
    Unavailable { reason: String },

    /// The crate failed to create the manager (Windows: window/class creation
    /// failure; unified channel failure on Linux) or the event loop failed.
    #[error("global hotkey manager failed: {source}")]
    Manager {
        #[from]
        source: global_hotkey::Error,
    },

    /// A config binding could not be parsed into a crate combo; captured as a
    /// stringly-typed owned message so `Error` stays `Debug` without
    /// depending on the `bindings::Error` `Clone` story.
    #[error("hotkey bindings invalid: {message}")]
    Binding { message: String },
}

/// Per-hotkey registration outcome (spec §4.1: status in UI + log, no crash).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Registration {
    /// The combo is live.
    Registered { action: Action, combo: String },

    /// Registration failed (occupied id, backend error); typed reason kept.
    Failed {
        action: Action,
        combo: String,
        reason: String,
    },

    /// The rect action is reserved in MVP — parsed, never registered.
    Reserved { action: Action, combo: String },
}

/// Startup report returned by [`HotkeysController::start`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StartStatus {
    pub(crate) registrations: Vec<Registration>,
}

/// Live hotkey wiring. The manager lives on the MAIN thread (Windows
/// WM_HOTKEY thread affinity — winit/eframe pump Win32 messages there);
/// registration and re-registration must therefore run on the main thread.
/// The listener thread only reads the settings mailbox and dispatches events.
pub(crate) struct HotkeysController {
    /// Crate manager behind the injectable [`Registrar`] surface (unit tests
    /// drive the pause/restore state machine with a mock).
    registrar: Box<dyn Registrar>,
    /// Combos currently registered — unregistered on Apply before re-register,
    /// and on [`HotkeysController::pause`] for bind-by-press.
    registered: Vec<HotKey>,
    /// Latest-config mailbox for the listener thread AND the pad pump
    /// (`gamepad`) — the same Arc `main` created and hands to both, so UI Apply
    /// reaches every trigger source (owned clone per trigger; no shared capture
    /// state, per the repo thread-boundary rule). Also the source of truth for
    /// `restore`: it holds the settings published by the last `apply`, never the
    /// GUI draft.
    mailbox: Arc<Mutex<Settings>>,
    /// Set by `pause` (capture mode active), cleared by `restore`/`apply`.
    paused: bool,
}

impl HotkeysController {
    /// Parse controls, probe the backend, create the manager **on the calling
    /// (main) thread**, register the active combos, and spawn the event
    /// listener thread. `mailbox` is the settings Arc `main` created — the
    /// listener clones it per trigger, the pad pump reads the same Arc, and
    /// [`HotkeysController::apply`] publishes it. `handoff` is the toast-handle
    /// slot the listener reads per capture command (`capture::handoff_handle`)
    /// — empty until the GUI attaches the `ToastService` handle, so a trigger
    /// before the GUI simply captures without a chip; `debounce` is
    /// the trigger gate shared with the pad pump (`gamepad`), so keyboard and
    /// pad triggers of one action share one ~250 ms window.
    ///
    /// Fails with `Error::Unavailable` (Linux without an X11 server) or
    /// [`Error::Manager`] (manager creation failed); a bindings parse failure
    /// returns [`Error::Binding`]. Per-hotkey registration failures are NOT
    /// errors — they come back in [`StartStatus`]. The mailbox is the caller's
    /// on every failure path, so the pad pump and the GUI keep working without
    /// a controller.
    pub(crate) fn start(
        mailbox: Arc<Mutex<Settings>>,
        handoff: ToastHandoff,
        debounce: Arc<Mutex<Debounce>>,
    ) -> Result<(Self, StartStatus), Error> {
        let settings = mailbox_settings(&mailbox).clone();
        let parsed =
            bindings::parse_controls(&settings.controls).map_err(|err| Error::Binding {
                message: err.to_string(),
            })?;

        check_backend()?;

        let manager = GlobalHotKeyManager::new().map_err(|source| Error::Manager { source })?;

        let registrar: Box<dyn Registrar> = Box::new(manager);
        let registrations = register(registrar.as_ref(), &parsed);
        let registered = active_hotkeys(&parsed);

        spawn_listener(
            Arc::clone(&mailbox),
            Arc::clone(&handoff),
            Arc::clone(&debounce),
        );

        Ok((
            Self {
                registrar,
                registered,
                mailbox,
                paused: false,
            },
            StartStatus { registrations },
        ))
    }

    /// Apply edited settings live (spec §4.4): unregister the current combos,
    /// register the new ones, and publish the settings to the listener mailbox
    /// so subsequent captures use the new templates/format. Runs on the main
    /// thread (manager thread affinity). Per-hotkey failures are reported in
    /// the returned status; a bindings parse failure returns
    /// [`Error::Binding`] and leaves the previous registrations and settings
    /// untouched.
    pub(crate) fn apply(&mut self, settings: Settings) -> Result<StartStatus, Error> {
        let parsed =
            bindings::parse_controls(&settings.controls).map_err(|err| Error::Binding {
                message: err.to_string(),
            })?;

        for hotkey in &self.registered {
            if let Err(reason) = self.registrar.unregister(*hotkey) {
                tracing::warn!("Hotkey unregister failed ({hotkey:?}): {reason}");
            }
        }

        let registrations = register(self.registrar.as_ref(), &parsed);
        self.registered = active_hotkeys(&parsed);
        *mailbox_settings(&self.mailbox) = settings;
        // Apply re-registers live combos, so the pause flag must not survive
        // it: a capture-mode Apply (button pressed while a row is capturing)
        // leaves the new combos registered and later restores must be no-ops.
        self.paused = false;

        Ok(StartStatus { registrations })
    }

    /// Pause registrations for bind-by-press (spec §4.4 v5.5, Stage 18):
    /// unregister every live combo so a press reaches the egui window as a
    /// bind candidate instead of being stolen by the platform hotkey hook
    /// (Windows WM_KEYDOWN→WM_HOTKEY) and firing the screenshot action.
    /// Best-effort per combo (a failed unregister is a WARN, the loop keeps
    /// going). No-op while already paused. Runs on the manager thread — the
    /// GUI calls it from the main-thread logic/render pass.
    pub(crate) fn pause(&mut self) {
        if self.paused {
            return;
        }
        let removed = self.registered.len();
        for hotkey in &self.registered {
            if let Err(reason) = self.registrar.unregister(*hotkey) {
                tracing::warn!(
                    "Hotkey unregister failed during rebind pause ({hotkey:?}): {reason}"
                );
            }
        }
        self.registered.clear();
        self.paused = true;
        tracing::info!("Hotkeys paused for rebind ({removed} combo(s) unregistered)");
    }

    /// Restore registrations after bind-by-press ends (commit or cancel):
    /// re-register the LAST APPLIED settings from the mailbox — the GUI draft
    /// is deliberately not applied here (Apply semantics unchanged, §4.4), so
    /// cancel keeps the old bindings live. No-op while not paused. Runs on the
    /// manager thread.
    pub(crate) fn restore(&mut self) {
        if !self.paused {
            return;
        }
        let settings = mailbox_settings(&self.mailbox).clone();
        let parsed = match bindings::parse_controls(&settings.controls) {
            Ok(parsed) => parsed,
            Err(err) => {
                // Cannot happen for mailbox settings (only parsed settings are
                // published), but stay paused rather than panic.
                tracing::error!("Hotkeys not restored (invalid binding): {err}");
                return;
            }
        };
        let registrations = register(self.registrar.as_ref(), &parsed);
        self.registered = active_hotkeys(&parsed);
        self.paused = false;
        for registration in &registrations {
            if let Registration::Failed {
                action,
                combo,
                reason,
            } = registration
            {
                tracing::error!(
                    "Hotkey not restored: {combo} ({}): {reason}",
                    action.label()
                );
            }
        }
        tracing::info!(
            "Hotkeys restored ({} combo(s) re-registered from applied settings)",
            self.registered.len()
        );
    }
}

/// Active actions become crate hotkeys; reserved (`rect`) is never registered.
fn active_hotkeys(parsed: &[ActionBinding]) -> Vec<HotKey> {
    parsed
        .iter()
        .filter(|binding| binding.active)
        .map(|binding| binding.hotkey)
        .collect()
}

/// Register the parsed bindings and build the status report. Reserved actions
/// report [`Registration::Reserved`] without touching the registrar; each
/// active registration reports [`Registration::Registered`] or
/// [`Registration::Failed`] with the crate's typed reason.
fn register(registrar: &dyn Registrar, parsed: &[ActionBinding]) -> Vec<Registration> {
    parsed
        .iter()
        .map(|binding| {
            if !binding.active {
                return Registration::Reserved {
                    action: binding.action,
                    combo: binding.hotkey.to_string(),
                };
            }
            match registrar.register(binding.hotkey) {
                Ok(()) => Registration::Registered {
                    action: binding.action,
                    combo: binding.hotkey.to_string(),
                },
                Err(err) => Registration::Failed {
                    action: binding.action,
                    combo: binding.hotkey.to_string(),
                    reason: err.to_string(),
                },
            }
        })
        .collect()
}

/// Crate event id → active action only; reserved ids are absent, so a stray
/// event can never trigger a rect capture.
fn active_ids(parsed: &[ActionBinding]) -> HashMap<u32, Action> {
    parsed
        .iter()
        .filter(|binding| binding.active)
        .map(|binding| (binding.hotkey.id(), binding.action))
        .collect()
}

/// Action → capture mode; `None` for the reserved rect action. Consumed by
/// this module's listener and by `gamepad` (the pad trigger fires the same
/// shot path as the keyboard trigger).
pub(crate) fn capture_mode(action: Action) -> Option<CaptureMode> {
    match action {
        Action::FullScreen => Some(CaptureMode::FullScreen),
        Action::Window => Some(CaptureMode::Window),
        Action::Rect => None,
    }
}

impl Registration {
    /// Which action a status entry refers to (GUI row lookup).
    pub(crate) fn action(&self) -> Action {
        match self {
            Registration::Registered { action, .. } => *action,
            Registration::Failed { action, .. } => *action,
            Registration::Reserved { action, .. } => *action,
        }
    }
}

/// Clone the latest settings out of the mailbox (owned value across the
/// thread boundary). A poisoned lock (listener panic) is recovered rather
/// than unwrapped: the settings are still valid inside the poison guard.
fn mailbox_settings(mailbox: &Arc<Mutex<Settings>>) -> MutexGuard<'_, Settings> {
    mailbox
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The listener thread (spec §6 Input Thread): blocks on the crate's global
/// event receiver, dispatches debounced `Pressed` events into
/// `capture::spawn_capture` with a fresh clone of the mailbox settings and the
/// toast handle read from the shared hand-off slot. The trigger gate is the
/// SHARED `bindings::Debounce`
/// (same instance the pad pump fires through, spec §4.1 OR-fire), the real
/// clock is stamped at the module boundary (`chrono::Local::now`).
fn spawn_listener(
    mailbox: Arc<Mutex<Settings>>,
    handoff: ToastHandoff,
    debounce: Arc<Mutex<Debounce>>,
) {
    std::thread::spawn(move || {
        for event in GlobalHotKeyEvent::receiver() {
            if event.state != HotKeyState::Pressed {
                continue;
            }

            let settings = mailbox_settings(&mailbox).clone();
            let parsed = match bindings::parse_controls(&settings.controls) {
                Ok(parsed) => parsed,
                Err(err) => {
                    tracing::error!("Hotkey event ignored (invalid binding): {err}");
                    continue;
                }
            };
            let ids = active_ids(&parsed);

            let Some(action) = ids.get(&event.id).copied() else {
                continue; // unknown id (stale combo after Apply) — ignore
            };
            let Some(mode) = capture_mode(action) else {
                continue; // reserved action — never triggers
            };

            if bindings::try_fire_shared(&debounce, action, Local::now()) {
                spawn_capture(CaptureCommand { mode, settings }, handoff_handle(&handoff));
            }
        }
    });
}

/// Linux only: the crate supports X11 hotkeys via its own pump thread; a
/// Wayland-only host cannot deliver them (best-effort, spec §9). On Windows
/// (and other unsupported platforms) the backend is assumed available —
/// manager creation reports real failures.
#[cfg(target_os = "linux")]
#[derive(Debug, PartialEq, Eq)]
enum BackendStatus {
    /// The crate can be used; registration will be attempted.
    Usable,

    /// No X11 server reachable on this Linux host.
    Unavailable { reason: String },
}

/// X11 display availability check, factored out for unit tests
/// (`display_env` / `socket_exists` injected). `None` display means the
/// variable is absent.
#[cfg(target_os = "linux")]
fn backend_status(
    display_env: Option<String>,
    mut socket_exists: impl FnMut(&str) -> bool,
) -> BackendStatus {
    match display_env.as_deref() {
        Some(display) if display.starts_with(':') => {
            // Local display ":N" needs /tmp/.X11-unix/XN; remote "host:N" is attempted.
            let short = display
                .trim_start_matches(':')
                .split('.')
                .next()
                .unwrap_or("");
            if let Ok(number) = short.parse::<u32>() {
                let socket = format!("/tmp/.X11-unix/X{number}");
                if !socket_exists(&socket) {
                    return BackendStatus::Unavailable {
                        reason: format!("X11 display {display} has no socket file ({socket})"),
                    };
                }
            }
            BackendStatus::Usable
        }
        Some(_) => BackendStatus::Usable, // remote/host display: let the crate try
        None => BackendStatus::Unavailable {
            reason: "DISPLAY is not set (Wayland-only host; hotkeys are X11-only)".to_owned(),
        },
    }
}

#[cfg(target_os = "linux")]
fn check_backend() -> Result<(), Error> {
    let status = backend_status(std::env::var("DISPLAY").ok(), |path| {
        std::path::Path::new(path).exists()
    });
    match status {
        BackendStatus::Usable => Ok(()),
        BackendStatus::Unavailable { reason } => Err(Error::Unavailable { reason }),
    }
}

#[cfg(not(target_os = "linux"))]
fn check_backend() -> Result<(), Error> {
    Ok(())
}

#[cfg(test)]
#[path = "hotkeys.tests.rs"]
mod tests;
