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
//! Combos the platform REFUSED (another client already holds them) are retried
//! when the settings window gains focus (Stage 13, spec §4.1 v2), and their
//! failure reason is readable UI text, not the crate's `Debug` dump.
//!
//! While a keyboard bind-capture is active, the module ALSO registers a
//! temporary PROBE set of combos for keys Windows never delivers to egui
//! (Stage 14): Windows sends no `WM_CHAR`/`WM_DEADCHAR` for `VK_SNAPSHOT`, and
//! winit 0.30.13 defers a keydown until a char message arrives
//! (`platform_impl/windows/keyboard.rs:106-155`) and drops the stored
//! `PartialKeyEventInfo` on the next keydown (`:124-125`), so `PrintScreen`
//! (and the other lock/media keys in [`PROBE_KEYS`]) never becomes a bind
//! candidate — the row would sit on "Press keys…" until Esc. A fired probe
//! combo must NOT take a screenshot: the listener recognises it by id from the
//! shared [`ProbeSink`] slot BEFORE the action map, records the press
//! (key + mods) and wakes the eframe loop, and the GUI writes it into the
//! capturing row exactly like a normal egui press. Probes exist only on
//! Windows (`cfg(windows)` gates the registration); on Linux these keys arrive
//! through egui normally and the probe path is a no-op. Registration of a probe
//! is best-effort like everything else here — a refusal is a WARN, never a
//! crash — and the whole set is unregistered when the capture ends, BEFORE
//! [`HotkeysController::restore`] re-registers the live combos (a bare probe
//! shares its id with a bare action binding, so restoring first would collide
//! with ourselves).
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
/// Registration failures are TYPED ([`RegistrationFailure`]) so the UI and the
/// log can say WHY a combo is dead (spec §4.1 v2) instead of dumping the
/// crate's `Debug` of `Error::AlreadyRegistered`; unregister errors stay owned
/// strings (`HotKey`/`Settings` stay the shared types). Neither is propagated:
/// both are reported per combo.
pub(crate) trait Registrar {
    fn register(&self, hotkey: HotKey) -> Result<(), RegistrationFailure>;
    fn unregister(&self, hotkey: HotKey) -> Result<(), String>;
}

impl Registrar for GlobalHotKeyManager {
    fn register(&self, hotkey: HotKey) -> Result<(), RegistrationFailure> {
        GlobalHotKeyManager::register(self, hotkey).map_err(RegistrationFailure::from)
    }

    fn unregister(&self, hotkey: HotKey) -> Result<(), String> {
        GlobalHotKeyManager::unregister(self, hotkey).map_err(|err| err.to_string())
    }
}

/// Why one registration attempt failed. `global_hotkey::Error` is
/// `#[non_exhaustive]`; the one variant worth special-casing is
/// `AlreadyRegistered`, which BOTH backends produce for an occupied combo:
/// Windows `RegisterHotKey` failure (`platform_impl/windows/mod.rs:105`) and
/// X11 `BadAccess` on `GrabKey` (`platform_impl/x11/mod.rs:157-164`). That is
/// the Stage 13 case: another client (Windows OS default hotkey such as
/// PrintScreen→Snipping Tool; Linux window manager or user app grab) already
/// holds the combo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RegistrationFailure {
    /// The combo is held by another client, so this app cannot take it.
    AlreadyRegistered,

    /// Any other backend failure; carries the crate's own message.
    Other { message: String },
}

impl From<global_hotkey::Error> for RegistrationFailure {
    fn from(err: global_hotkey::Error) -> Self {
        match err {
            global_hotkey::Error::AlreadyRegistered(_) => Self::AlreadyRegistered,
            other => Self::Other {
                message: other.to_string(),
            },
        }
    }
}

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;

use chrono::Local;
use egui::Context;
use global_hotkey::GlobalHotKeyEvent;
use global_hotkey::GlobalHotKeyManager;
use global_hotkey::HotKeyState;
use global_hotkey::hotkey::Code;
use global_hotkey::hotkey::HotKey;
use global_hotkey::hotkey::Modifiers;

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

/// Keys Windows never delivers to an egui window while a bind-capture is open,
/// probed through a temporary hotkey registration instead (Stage 14, spec §4.4).
/// The list is deliberately small and extensible: `PrintScreen` (no `WM_CHAR`
/// for `VK_SNAPSHOT` — the reported bug), `ScrollLock` and `Pause` (system keys
/// whose keydown is likewise deferred or swallowed by the OS). Add a code here
/// when the same shape shows up for another key; nothing else needs to change.
pub(crate) const PROBE_KEYS: &[Code] = &[Code::PrintScreen, Code::ScrollLock, Code::Pause];

/// Modifier sets a probe is registered with: bare plus every Alt/Ctrl/Shift
/// combination. `Win`/`META` is absent on purpose — winit never reports Super as
/// a modifier, so a Win combo could never be captured by press anyway. Built per
/// call rather than in a `const` array: keyboard-types' modifier bitflags have no
/// `const` `|`, so the Alt+Shift / Ctrl+Shift chords cannot be const-folded, and
/// this runs once per capture start.
fn probe_mods() -> Vec<Option<Modifiers>> {
    let shift = Modifiers::SHIFT;
    vec![
        None,
        Some(Modifiers::ALT),
        Some(Modifiers::CONTROL),
        Some(shift),
        Some(Modifiers::ALT | shift),
        Some(Modifiers::CONTROL | shift),
    ]
}

/// The probe combos: [`PROBE_KEYS`] × [`probe_mods`], deterministic order, one
/// per id. A bare probe shares its id with a bare action binding of the same key
/// (`id == (mods.bits() << 16) | code`), which is safe because the live combos
/// are unregistered while a capture is paused — the listener consults the probe
/// map FIRST, so a probe id is never dispatched as an action.
fn probe_combos() -> Vec<HotKey> {
    let mods = probe_mods();
    PROBE_KEYS
        .iter()
        .flat_map(|key| mods.iter().map(move |mods| HotKey::new(*mods, *key)))
        .collect()
}

/// A probe press the listener recorded: the key and the modifiers the probe
/// combo was registered with (a fired hotkey carries no modifiers of its own).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProbePress {
    pub(crate) code: Code,
    pub(crate) mods: Modifiers,
}

/// Shared probe slot — the same latest-only shape as `capture::ToastHandoff`:
/// the listener thread writes a press, the main-thread GUI drains it. The combo
/// map is what makes a fired id recognisable as a probe; `wake` repaints the
/// eframe loop so a press lands in the row within one frame.
#[derive(Debug, Default)]
pub(crate) struct ProbeState {
    /// Live probe combos by crate id — empty while no capture is active.
    combos: HashMap<u32, (Code, Modifiers)>,
    /// Newest-only recorded press (drained by `take_probe_press`).
    press: Option<ProbePress>,
    /// Repaint wake for the settings-window loop, attached once by the GUI.
    wake: Option<Context>,
}

pub(crate) type ProbeSink = Arc<Mutex<ProbeState>>;

/// Empty probe slot, created by [`HotkeysController::start`] and shared with the
/// listener thread.
pub(crate) fn new_probe_sink() -> ProbeSink {
    Arc::new(Mutex::new(ProbeState::default()))
}

/// Read/write access to the probe slot; a poisoned lock (a panic elsewhere) is
/// recovered rather than unwrapped, like every other mailbox here.
fn probe_state(sink: &ProbeSink) -> MutexGuard<'_, ProbeState> {
    sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Is this fired id a LIVE probe combo? `Some` with the key+mods of that combo;
/// `None` for anything else, so the caller falls through to the action dispatch.
/// Consulted BEFORE the action map — that order is what keeps a probe (which can
/// share an id with a live action combo) from firing a screenshot.
fn probe_press_for(state: &ProbeState, event_id: u32) -> Option<ProbePress> {
    state.combos.get(&event_id).map(|(code, mods)| ProbePress {
        code: *code,
        mods: *mods,
    })
}

/// Record a probe press into the slot and wake the GUI loop. Returns `false`
/// when the id is not a live probe, so the listener keeps dispatching normally.
/// The lock is held only for the map lookup, the press write and the cheap
/// `request_repaint` call.
fn record_probe_press(sink: &ProbeSink, event_id: u32) -> bool {
    let mut state = probe_state(sink);
    let Some(press) = probe_press_for(&state, event_id) else {
        return false;
    };
    state.press = Some(press);
    if let Some(wake) = state.wake.clone() {
        wake.request_repaint();
    }
    tracing::debug!(
        "Bind probe press recorded: {}",
        combo_label(&HotKey::new(Some(press.mods), press.code))
    );
    true
}

/// Register the probe combos and publish the ones that took as live. A refused
/// combo (another client holds it) is a WARN and stays out of the map, so it is
/// never treated as a probe and never unregistered later.
fn register_probe_combos(
    registrar: &dyn Registrar,
    combos: &[HotKey],
    sink: &ProbeSink,
) -> Vec<HotKey> {
    let mut live = Vec::new();
    {
        let mut state = probe_state(sink);
        for hotkey in combos {
            match registrar.register(*hotkey) {
                Ok(()) => {
                    state.combos.insert(hotkey.id(), (hotkey.key, hotkey.mods));
                    live.push(*hotkey);
                }
                Err(failure) => tracing::warn!(
                    "Bind probe registration failed ({}): {}",
                    combo_label(hotkey),
                    failure_reason(&failure, hotkey)
                ),
            }
        }
    }
    live
}

/// Live hotkey wiring. The manager lives on the MAIN thread (Windows
/// WM_HOTKEY thread affinity — winit/eframe pump Win32 messages there);
/// registration and re-registration must therefore run on the main thread.
/// The listener thread only reads the settings mailbox and dispatches events.
pub(crate) struct HotkeysController {
    /// Crate manager behind the injectable [`Registrar`] surface (unit tests
    /// drive the pause/restore state machine with a mock).
    registrar: Box<dyn Registrar>,
    /// Active combos as handed to the backend — unregistered on Apply before
    /// re-register, and on [`HotkeysController::pause`] for bind-by-press. A
    /// combo whose registration FAILED is listed too (shipped behavior):
    /// pause/restore still attempts it, and a successful focus retry therefore
    /// needs no extra bookkeeping here.
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
    /// Combos whose LAST registration pass failed (spec §4.1 v2, Stage 13) —
    /// the retry candidates for the settings-window focus transition. Rebuilt
    /// by every full pass (`start`, `apply`, `restore`); `retry_failed_registrations`
    /// drops the combos it managed to register and keeps the rest, so a combo
    /// that is already live is never re-registered.
    failed: Vec<(Action, HotKey)>,
    /// Shared probe slot for the temporary bind probe (Stage 14): the listener
    /// thread recognises fired probe ids here and records a press instead of
    /// firing a capture; the GUI drains it on its logic pass.
    probe: ProbeSink,
    /// Probe combos currently registered — empty on non-Windows hosts and
    /// whenever no capture is active, so [`HotkeysController::stop_bind_probe`]
    /// is a no-op there. Only the combos that actually registered are listed.
    probe_hotkeys: Vec<HotKey>,
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
        let probe = new_probe_sink();

        spawn_listener(
            Arc::clone(&mailbox),
            Arc::clone(&handoff),
            Arc::clone(&debounce),
            Arc::clone(&probe),
        );

        Ok((
            Self {
                registrar,
                registered,
                mailbox,
                paused: false,
                failed: failed_combos(&parsed, &registrations),
                probe,
                probe_hotkeys: Vec::new(),
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

        // The probe exists only while a capture is paused; Apply ends that state
        // (new combos are registered below), so the probe set goes with it — a
        // leftover probe would shadow a live combo of the same id (Stage 14).
        self.stop_bind_probe();

        for hotkey in &self.registered {
            if let Err(reason) = self.registrar.unregister(*hotkey) {
                tracing::warn!("Hotkey unregister failed ({hotkey:?}): {reason}");
            }
        }

        let registrations = register(self.registrar.as_ref(), &parsed);
        self.registered = active_hotkeys(&parsed);
        self.failed = failed_combos(&parsed, &registrations);
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
        self.failed = failed_combos(&parsed, &registrations);
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

    /// Register the temporary bind probe for keys Windows never delivers to
    /// egui (Stage 14, spec §4.4). Called right after [`HotkeysController::pause`]
    /// so the live combos are already gone — a probe may then take a combo the OS
    /// default client holds (PrintScreen → Snipping Tool) for the duration of the
    /// capture only. Windows-only: on Linux these keys arrive through egui as
    /// ordinary key events, so a probe there would swallow them system-wide for
    /// nothing — no registrations, empty count. Best-effort: a refused combo is a
    /// WARN and stays out of the live map; the capture UI keeps working with
    /// whatever egui does deliver. Never panics. Returns how many probe combos
    /// are live (0 off Windows, 0 while no capture is active).
    pub(crate) fn start_bind_probe(&mut self) -> usize {
        if !cfg!(windows) || !self.paused {
            return 0;
        }
        self.probe_hotkeys =
            register_probe_combos(self.registrar.as_ref(), &probe_combos(), &self.probe);
        tracing::info!(
            "Bind probe active ({} combo(s)) for {}",
            self.probe_hotkeys.len(),
            PROBE_KEYS
                .iter()
                .map(|key| key_label(*key))
                .collect::<Vec<String>>()
                .join(", ")
        );
        self.probe_hotkeys.len()
    }

    /// Unregister the temporary bind probe when a bind capture ends (Stage 14).
    /// Must run BEFORE [`HotkeysController::restore`]: a bare probe shares its id
    /// with a bare action binding of the same key (`id == (mods.bits() << 16) |
    /// code`), so restoring first would collide with our own still-registered
    /// combo and report a bogus failure. Best-effort per combo (a failed
    /// unregister is a WARN); the combo map and any pending press are cleared
    /// either way, so a press recorded after the capture ended can never commit
    /// into a later capture. No-op off Windows, where nothing was registered.
    pub(crate) fn stop_bind_probe(&mut self) {
        let hotkeys = std::mem::take(&mut self.probe_hotkeys);
        for hotkey in &hotkeys {
            if let Err(reason) = self.registrar.unregister(*hotkey) {
                tracing::warn!(
                    "Bind probe unregister failed ({}): {reason}",
                    combo_label(hotkey)
                );
            }
        }
        let mut state = probe_state(&self.probe);
        state.combos.clear();
        state.press = None;
        if !hotkeys.is_empty() {
            tracing::info!(
                "Bind probe stopped ({} combo(s) unregistered)",
                hotkeys.len()
            );
        }
    }

    /// Drain the recorded probe press — latest-only, exactly one per drain,
    /// `None` when nothing was recorded since the last one. The GUI commits it
    /// into the capturing row; a press with no active capture is dropped there.
    pub(crate) fn take_probe_press(&mut self) -> Option<ProbePress> {
        probe_state(&self.probe).press.take()
    }

    /// Attach the repaint wake of the settings-window loop (Stage 14), so a probe
    /// press recorded on the listener thread gets a logic pass instead of waiting
    /// for the tray tick. Called once at window creation; a later attach replaces
    /// the earlier one, like every latest-only slot here (`capture::ToastHandoff`).
    pub(crate) fn attach_probe_wake(&mut self, ctx: Context) {
        probe_state(&self.probe).wake = Some(ctx);
    }

    /// Retry the combos the LAST pass failed (spec §4.1 v2, Stage 13), called
    /// by the GUI when the settings window GAINS focus. Why focus is the right
    /// moment: on Windows the startup registration runs before any window
    /// exists (`main`), and Microsoft's `RegisterHotKey` docs state that OS
    /// default hotkeys (PrintScreen → Snipping Tool) may be overridden when an
    /// app window is FOREGROUND — so becoming foreground is the one point where
    /// an occupied default combo can be taken. On Linux the combo is held by
    /// another client's X11 grab, which this app cannot displace; the retry
    /// then simply re-reports the same failure honestly (WARN, row stays
    /// warning).
    ///
    /// Only the recorded failures are re-attempted — a combo that is already
    /// live is never registered twice. A combo that succeeds drops out of the
    /// retry list (`registered` already lists every active combo, and the
    /// listener maps ids from the settings mailbox per event, so dispatch works
    /// with no extra bookkeeping). No-op while capture PAUSED: a press must
    /// reach the egui window as a bind candidate (§4.4 v5.5), and `restore`
    /// re-registers everything at capture end anyway. Runs on the manager
    /// thread; returns the updated statuses of the retried combos only (the GUI
    /// merges them into its row report). INFO per success, WARN per combo that
    /// is still occupied — once per focus transition, never per frame.
    pub(crate) fn retry_failed_registrations(&mut self) -> Vec<Registration> {
        if self.paused {
            return Vec::new();
        }
        let candidates = std::mem::take(&mut self.failed);
        if candidates.is_empty() {
            return Vec::new();
        }

        let mut retried = Vec::new();
        for (action, hotkey) in candidates {
            match self.registrar.register(hotkey) {
                Ok(()) => {
                    tracing::info!(
                        "Hotkey registered after window focus: {} ({})",
                        combo_label(&hotkey),
                        action.label()
                    );
                    retried.push(Registration::Registered {
                        action,
                        combo: hotkey.to_string(),
                    });
                }
                Err(failure) => {
                    let reason = failure_reason(&failure, &hotkey);
                    tracing::warn!(
                        "Hotkey still not registered after window focus: {} ({}): {}",
                        combo_label(&hotkey),
                        action.label(),
                        reason
                    );
                    self.failed.push((action, hotkey));
                    retried.push(Registration::Failed {
                        action,
                        combo: hotkey.to_string(),
                        reason,
                    });
                }
            }
        }
        retried
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
                Err(failure) => Registration::Failed {
                    action: binding.action,
                    combo: binding.hotkey.to_string(),
                    reason: failure_reason(&failure, &binding.hotkey),
                },
            }
        })
        .collect()
}

/// The combos to retry on the next focus transition: the `Failed` entries of a
/// registration pass, in the pass order. `register` maps `parsed` 1:1, so
/// zip-pairing the two lists is the identity, not a lookup.
fn failed_combos(
    parsed: &[ActionBinding],
    registrations: &[Registration],
) -> Vec<(Action, HotKey)> {
    parsed
        .iter()
        .zip(registrations)
        .filter(|(_, registration)| matches!(registration, Registration::Failed { .. }))
        .map(|(binding, _)| (binding.action, binding.hotkey))
        .collect()
}

/// Combo label for UI and log text: modifiers in the UI canonical order
/// (`Alt`, `Ctrl`, `Shift`, `Win` — the same order `gui::binding_label` prints
/// for config bindings), then the key in UI/config notation, so a failure
/// reason names the combo exactly like the row shows it. The crate's own
/// `Display` is lowercase, in a different order (`alt+KeyS`, shift first), and
/// spells letters/digits with a prefix — which is why this module renders its
/// own label instead of reusing it; duplicating the 4-name order locally is
/// cheaper than making `hotkeys` import `gui` (a wrong layering).
fn combo_label(hotkey: &HotKey) -> String {
    let mods = hotkey.mods;
    let mut parts: Vec<&str> = Vec::new();
    if mods.alt() {
        parts.push("Alt");
    }
    if mods.ctrl() {
        parts.push("Ctrl");
    }
    if mods.shift() {
        parts.push("Shift");
    }
    if mods.meta() {
        parts.push("Win");
    }
    let key = key_label(hotkey.key);
    if parts.is_empty() {
        key
    } else {
        format!("{}+{}", parts.join("+"), key)
    }
}

/// Key name in UI/config notation. Config `key` and `gui::binding_label` use
/// the bare letter/digit (`S`, `0`); the crate's `Code` spelling carries a
/// prefix (`KeyS`, `Digit0`), so it is dropped for those two families. Every
/// other code already matches config notation (`PrintScreen`, `F1`,
/// `ArrowDown`, `BracketLeft`, …). `pub(crate)` because `gui` renders the
/// notation of a recorded probe press (Stage 14) — a `Code` never arrives as an
/// egui key on Windows, so there is no `egui::Key::name()` to use there.
pub(crate) fn key_label(key: Code) -> String {
    let spelled = key.to_string();
    let stripped = spelled
        .strip_prefix("Key")
        .or_else(|| spelled.strip_prefix("Digit"))
        .filter(|rest| rest.chars().count() == 1);
    stripped.unwrap_or(&spelled).to_owned()
}

/// Readable failure reason (spec §4.1 v2): names the combo and says what to do
/// instead of the crate's `Debug` dump (`HotKey already registered: HotKey {
/// mods: Modifiers(0x0), key: PrintScreen, id: 117 }`). The wording is UI text
/// — it lands in the row tooltip and in the startup/restore ERROR log, so it
/// comes from the catalog (`hotkey.already_registered`, `hotkey.registration_failed`).
fn failure_reason(failure: &RegistrationFailure, hotkey: &HotKey) -> String {
    match failure {
        RegistrationFailure::AlreadyRegistered => {
            rust_i18n::t!("hotkey.already_registered", combo = combo_label(hotkey)).into_owned()
        }
        RegistrationFailure::Other { message } => {
            rust_i18n::t!("hotkey.registration_failed", reason = message).into_owned()
        }
    }
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
/// A fired id that matches a LIVE probe combo (Stage 14) is handled BEFORE the
/// action map: it records a bind candidate in the probe slot and wakes the GUI
/// instead of firing a screenshot, so a probe that shares an id with a live
/// action combo can never take a shot.
fn spawn_listener(
    mailbox: Arc<Mutex<Settings>>,
    handoff: ToastHandoff,
    debounce: Arc<Mutex<Debounce>>,
    probe: ProbeSink,
) {
    std::thread::spawn(move || {
        for event in GlobalHotKeyEvent::receiver() {
            if event.state != HotKeyState::Pressed {
                continue;
            }

            if record_probe_press(&probe, event.id) {
                continue; // bind probe press — never a capture command
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
