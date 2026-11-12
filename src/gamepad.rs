//! Gamepad trigger wiring over the `gamepad` crate: the Windows GameInput pump
//! thread, pad-bind matching, shot dispatch that mirrors the hotkey listener
//! exactly, and the pad-capture hand-off the settings UI reads (docs/spec.md
//! §4.1 pad field, §4.4 bind-by-press, §6 gamepad poll thread, plan Stages 4-5).
//!
//! The library (`crates/gamepad`) is a **target-scoped path dependency**
//! (`[target."cfg(windows)".dependencies]` in the root manifest), so nothing in
//! this module references it on Linux: the mapping and capture rules below are
//! pure and cfg-free (Linux-runnable unit tests), the pump startup is
//! `#[cfg(windows)]`, and the non-Windows [`spawn_pump`] logs and returns
//! without spawning (Linux gilrs is out of scope — hardware finding: the wgi
//! backend gates reads on the controller timestamp, generic HID pads report a
//! constant one).
//!
//! Fire semantics (docs/spec.md §4.1 "action fires on any filled field (OR)"):
//! the keyboard half of the OR is owned by `hotkeys`, this module owns the pad
//! half. Both sources fire through **one shared debounce gate**
//! (`bindings::try_fire_shared`, ~250 ms per action) so a pad press and a
//! hotkey landing on the same action inside the window produce one shot, not
//! two. A pad bind is a single button (pad combos are out of scope); when one
//! event matches several actions, the FIRST match in config order wins — a
//! documented tie-break, not a feature (KISS).
//!
//! Reserved (`rect`) actions never fire from the pad either: parity with
//! `hotkeys`, which never registers them. The action→mode map and the capture
//! call are reused from `hotkeys`/`capture`, so the shot path is identical to
//! the keyboard path (`capture::spawn_capture(CaptureCommand { mode, settings,
//! toast })` — owned values plus the toast handle read from the shared
//! hand-off slot, save on its own thread, spec §4.2).
//!
//! LIVE settings (Stage 5 defect fix): the pump holds the **same settings
//! mailbox Arc** the hotkey listener reads (created in `main`, published by UI
//! Apply), and re-parses the pad triggers per event. Parsing three config rows
//! per event is cheap next to a capture, and it is what makes a pad bind
//! captured in the UI (or edited in `config.toml`) fire without a restart —
//! the startup `Settings` snapshot it replaced could not see a re-bind.
//!
//! DOWN edges (Stage 5 defect fix): the library's `buttons_changed` delivers a
//! STATE change — `bits` is the new mask and `pressed_indices` lists the
//! CURRENTLY pressed indices of that mask (`crates/gamepad/src/gameinput.rs`
//! builds `pressed` from the new mask, not from the diff), so a bind held while
//! another button changes re-appears in the list. This module keeps a per-device
//! previous mask and fires only on bits newly SET (`bits & !previous`) — a
//! press edge, not a state re-listing. System buttons already arrive as
//! DOWN/UP edges and fire on DOWN only.
//!
//! Capture mode (Stage 5, spec §4.4): while the UI is binding a field, the pad
//! must not take a screenshot — the pump consults [`PadCapture`] (shared with
//! the GUI by Arc) before the trigger path, and a PAD capture commits the first
//! DOWN event instead of firing. The GUI never receives a callback (`emit` holds
//! the observer mutex — callbacks must not re-emit), so it polls this mailbox in
//! its logic pass, the same latest-only shape `capture` and `hotkeys` use.
//!
//! Observer lifetime hazard (accepted library defect, plan Stage 2 note (a)):
//! `gamepad::run` erases the observer reference into a process-wide static slot
//! and callbacks can arrive on GameInput's threads, so the observer and the
//! stop flag are **owned by the pump thread** and stay alive for the thread's
//! whole lifetime. `run` pumps forever — the stop flag is never set (app-exit
//! parity with the hotkey listener thread: the process is the lifecycle
//! boundary, the thread is detached). On an init failure `run` returns before
//! any registration or pump starts, so nothing can emit into the erased
//! reference afterwards. Callbacks must NOT re-emit into the library (`emit`
//! holds the observer mutex), so this observer only reads the shared mailboxes,
//! logs, and calls `capture::spawn_capture`; `gamepad::button_label` takes the
//! registry lock, which is safe here because the library delivers
//! `buttons_changed` AFTER releasing that lock (crate Observer doc), and the
//! name→index scan runs in the same callback — never inside `emit`.
//!
//! Logging: `INFO` on device connect and disconnect (name + registry index —
//! spec §4.1 connect/disconnect INFO; the library emits a disconnect once per
//! registry entry, whose index stays stable), `INFO` when a shot fires from the
//! pad, `INFO` when a pad capture commits, `INFO` when the pump cannot start
//! (errors are values — the app keeps running), `DEBUG` on button/system events
//! and the heartbeat. The observer prints nothing.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;

#[cfg(windows)]
use std::collections::HashMap;
#[cfg(windows)]
use std::sync::atomic::AtomicBool;

use crate::bindings;
use crate::bindings::Action;
use crate::bindings::Debounce;
use crate::bindings::PadBind;
use crate::capture::ToastHandoff;
use crate::config::Settings;

// The shot path is Windows-only (there is no pad trigger to fire elsewhere),
// so these imports are Windows-scoped too.
#[cfg(windows)]
use crate::capture::CaptureCommand;
#[cfg(windows)]
use crate::capture::handoff_handle;
#[cfg(windows)]
use crate::capture::spawn_capture;
#[cfg(windows)]
use crate::hotkeys::capture_mode;

/// Which bind field is waiting for a press (spec §4.4 bind-by-press). The GUI
/// owns this state; the pad pump reads it to decide whether a pad event is a
/// trigger or a capture commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CaptureField {
    /// The keyboard bind button is capturing a key combo (`gui::SettingsApp::capture_press`).
    Keyboard,
    /// The pad bind button is capturing a pad press (Windows only).
    Pad,
}

/// A pad DOWN event, classified for the trigger and capture paths. A controller
/// event carries the registry index (for `gamepad::button_label`) and the GameInput
/// button index (the config notation). Constructed by the Windows pump (and by
/// unit tests on Linux); on Linux the non-Windows `spawn_pump` never runs, so
/// the variants have no constructor there — same allow pattern as
/// `capture::spawn_capture`.
#[allow(dead_code)] // Constructed by the Windows pump only; tests build them on Linux.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PadDownEvent {
    Controller { device: usize, index: u8 },
    Guide,
    Share,
}

/// Config notation of a captured pad press (spec §4.1, Stage 5): the device-info
/// label name when the device names the button (`XboxX`), the raw `unknown:<N>`
/// fallback when it does not (v0 devices expose no labels, the Linux stub has no
/// registry, §9), and the system-button notation names as-is. `label` is
/// injected so the notation rule is unit-testable without GameInput.
pub(crate) fn pad_notation_with_label(event: PadDownEvent, label: Option<&str>) -> String {
    match event {
        PadDownEvent::Controller { index, .. } => match label {
            Some(label) if !label.is_empty() => label.to_owned(),
            _ => format!("unknown:{index}"),
        },
        // System buttons have no device-info label slot (§9): the name IS the
        // notation, whatever label a caller hands in.
        PadDownEvent::Guide => "Guide".to_owned(),
        PadDownEvent::Share => "Share".to_owned(),
    }
}

/// [`pad_notation_with_label`] with the live device-info label looked up through
/// the library — `None` off Windows, so the raw-index fallback stays. Safe on
/// the GUI thread and in `buttons_changed` callbacks (the crate emits after
/// releasing the registry lock).
pub(crate) fn pad_notation(event: PadDownEvent) -> String {
    pad_notation_with_label(event, pad_button_label(event).as_deref())
}

/// Live device-info label of a captured press (`gamepad::button_label`).
/// `None` for system buttons, on Linux, and for unlabeled devices (v0 carries
/// no button labels; a disconnected device has no entry to look up).
fn pad_button_label(event: PadDownEvent) -> Option<String> {
    #[cfg(windows)]
    match event {
        // The library already returns an owned label (`Option<String>`).
        PadDownEvent::Controller { device, index } => gamepad::button_label(device, index),
        PadDownEvent::Guide | PadDownEvent::Share => None,
    }
    #[cfg(not(windows))]
    {
        let _ = event;
        None
    }
}

/// Pad-capture hand-off state between the GUI (main thread — it sets the active
/// capture) and the pump observer (GameInput threads — it commits a press).
#[derive(Debug, Default)]
pub(crate) struct PadCapture {
    /// Field currently waiting for a press; `None` = idle. ANY active capture
    /// suppresses the pad shot path (spec §4.4 v5.5 rationale: a press while
    /// binding a field must not reach the action — for keyboard capture the
    /// registrations are additionally paused by `hotkeys`).
    active: Option<(CaptureField, Action)>,
    /// The first pad DOWN event committed by the pump, waiting for the GUI to
    /// write it into the draft. Latest-only (one press binds one field).
    captured: Option<PadDownEvent>,
}

/// The mailbox shape `hotkeys` (settings) and `capture` (toast hand-off) use:
/// an `Arc<Mutex<...>>` both sides consult, never a callback.
pub(crate) type PadCaptureMailbox = Arc<Mutex<PadCapture>>;

/// An empty capture mailbox, created once per resident process by `main`.
pub(crate) fn new_capture_mailbox() -> PadCaptureMailbox {
    Arc::new(Mutex::new(PadCapture::default()))
}

/// Poisoned-lock recovery (same policy as `hotkeys`/`capture`): a panicking
/// side leaves the stored state intact, so the hand-off keeps working.
fn lock_capture(mailbox: &PadCaptureMailbox) -> MutexGuard<'_, PadCapture> {
    mailbox
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Clone the live settings out of the shared mailbox (same poison-recovery
/// policy as `hotkeys::mailbox_settings`).
fn settings_now(mailbox: &Arc<Mutex<Settings>>) -> Settings {
    mailbox
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// Enter or leave bind-by-press (GUI side). Starting a capture drops a
/// leftover `captured` value so a cancelled capture cannot write an old press
/// into a different row.
pub(crate) fn set_capture_mode(
    mailbox: &PadCaptureMailbox,
    active: Option<(CaptureField, Action)>,
) {
    let mut capture = lock_capture(mailbox);
    if active.is_some() {
        capture.captured = None;
    }
    capture.active = active;
}

/// The field the pump is still waiting on (GUI keeps its own copy in sync).
pub(crate) fn active_capture(mailbox: &PadCaptureMailbox) -> Option<(CaptureField, Action)> {
    lock_capture(mailbox).active
}

/// Drain the committed press (GUI polling side): each press is read once.
pub(crate) fn take_captured(mailbox: &PadCaptureMailbox) -> Option<PadDownEvent> {
    lock_capture(mailbox).captured.take()
}

/// True while ANY bind field is capturing — the pad shot path stays silent
/// (spec §4.4, Stage 5). Consumed by `decide_pad_shot`, so it shares that
/// function's Windows/test gating: on Linux the pad shot path never runs.
#[cfg(any(test, windows))]
pub(crate) fn pad_shots_suppressed(capture: &PadCapture) -> bool {
    capture.active.is_some()
}

/// Commit the first pad DOWN event of an active PAD capture: record the press,
/// end the capture (one press binds one field). A keyboard capture or an idle
/// mailbox commits nothing — the event is a trigger candidate there.
#[cfg(any(test, windows))]
pub(crate) fn commit_pad_capture(capture: &mut PadCapture, event: PadDownEvent) -> Option<Action> {
    match capture.active {
        Some((CaptureField::Pad, action)) => {
            capture.captured = Some(event);
            capture.active = None;
            Some(action)
        }
        Some((CaptureField::Keyboard, _)) | None => None,
    }
}

/// Pad triggers derived from parsed controls, in config order (`full_screen` →
/// `window` → `rect`): active actions that carry a `pad` bind. Keyboard-only
/// bindings and the reserved `rect` action are absent. `Named` binds come out
/// unresolved — [`resolve_pad_binds`] turns them into `Controller` indices
/// against the live devices before matching.
pub(crate) fn pad_triggers(parsed: &[bindings::ActionBinding]) -> Vec<(Action, PadBind)> {
    parsed
        .iter()
        .filter(|binding| binding.active)
        // `binding` is borrowed from the parsed rows and the trigger list owns
        // its binds (`Named` carries a String), so clone is the only move here.
        .filter_map(|binding| binding.pad.clone().map(|pad| (binding.action, pad)))
        .collect()
}

/// Button indices the device-info label scan walks: controller state is a u64
/// mask, so 0..64 — the same width the library clamps device button counts to
/// (`MAX_BUTTON_COUNT` in `crates/gamepad/src/gameinput.rs`), and the same
/// range [`newly_pressed`] reports.
#[cfg(any(test, windows))]
const PAD_BUTTON_INDEXES: usize = 64;

/// Resolve a canonical GameInput button name to its controller bit index by
/// scanning the device-info label tables of the connected devices. The mapping
/// is device-dependent (§9 — the header has no static name→index route: the
/// press PoC saw South at index 2 while `XboxA` is label 7), so device-info is
/// the only truth. First device whose table carries the name wins; matching is
/// case-insensitive. `label_of` is injected so the scan rule is unit-testable
/// without GameInput; production passes `gamepad::button_label`.
#[cfg(any(test, windows))]
pub(crate) fn pad_index_for_name(
    name: &str,
    devices: &[usize],
    label_of: impl Fn(usize, usize) -> Option<String>,
) -> Option<u8> {
    devices.iter().find_map(|device| {
        (0..PAD_BUTTON_INDEXES).find_map(|index| {
            label_of(*device, index)
                .is_some_and(|label| label.eq_ignore_ascii_case(name))
                .then_some(index as u8)
        })
    })
}

/// Resolve `Named` binds against the connected devices: resolved ones become
/// `Controller(index)` (config order preserved), unresolved names are dropped
/// and returned so the caller warns once per name — a name no device reports
/// fires nothing rather than aborting the whole trigger list.
#[cfg(any(test, windows))]
pub(crate) fn resolve_pad_binds(
    triggers: &[(Action, PadBind)],
    devices: &[usize],
    label_of: impl Fn(usize, usize) -> Option<String>,
) -> (Vec<(Action, PadBind)>, Vec<String>) {
    let mut resolved = Vec::with_capacity(triggers.len());
    let mut unresolved = Vec::new();
    for (action, bind) in triggers {
        match bind {
            PadBind::Named(name) => match pad_index_for_name(name, devices, &label_of) {
                Some(index) => resolved.push((*action, PadBind::Controller(index))),
                None => unresolved.push(name.clone()),
            },
            // The resolved list owns its binds; the input rows stay borrowed.
            bind => resolved.push((*action, bind.clone())),
        }
    }
    (resolved, unresolved)
}

/// Controller-button match: the action whose `Controller(index)` bind appears
/// in `indices`. Callers pass the DOWN-edge indices ([`newly_pressed`]), not the
/// library's currently-pressed list, so a held button cannot re-match. System
/// binds (`Guide`/`Share`) never match controller indices: they arrive via
/// `system_button`. A `Named` bind matches nothing directly — resolution is the
/// only route from a name to an index, so an unresolved name never fires.
#[cfg(any(test, windows))]
pub(crate) fn match_controller(
    pressed_indices: &[u8],
    triggers: &[(Action, PadBind)],
) -> Option<Action> {
    triggers.iter().find_map(|(action, bind)| match bind {
        PadBind::Controller(index) => pressed_indices.contains(index).then_some(*action),
        PadBind::Named(_) | PadBind::Guide | PadBind::Share => None,
    })
}

/// System-button match (Guide/Share): fires on the DOWN edge only, for the
/// action whose bind names that button. An UP edge, or a DOWN of a button no
/// bind names, fires nothing.
#[cfg(any(test, windows))]
pub(crate) fn match_system_button(
    button: PadBind,
    down: bool,
    triggers: &[(Action, PadBind)],
) -> Option<Action> {
    if !down {
        return None;
    }
    triggers
        .iter()
        .find_map(|(action, bind)| (*bind == button).then_some(*action))
}

/// Indices with a 0→1 edge between the previous state mask and the new one:
/// `bits & !previous`, ascending. The library's event is a STATE change whose
/// `pressed_indices` lists the currently pressed buttons (see the module doc),
/// so the DOWN edges are computed here — a button held across another button's
/// change is NOT re-reported.
#[cfg(any(test, windows))]
pub(crate) fn newly_pressed(previous: u64, current: u64) -> Vec<u8> {
    let newly = current & !previous;
    (0..64)
        .filter(|index| newly & (1 << index) != 0)
        .map(|index| index as u8)
        .collect()
}

/// One pad event → shot-or-capture decision (spec §4.1 OR-fire + §4.4 capture
/// mode), pure so it is unit-testable on Linux:
/// - capture active → NO shot. A PAD capture commits this DOWN event (ending
///   the capture); a keyboard capture ignores the pad event.
/// - idle → a controller event matches the newly pressed indices, a system
///   event matches its button name; config order wins ties.
#[cfg(any(test, windows))]
pub(crate) fn decide_pad_shot(
    capture: &mut PadCapture,
    event: PadDownEvent,
    newly: &[u8],
    triggers: &[(Action, PadBind)],
) -> Option<Action> {
    if pad_shots_suppressed(capture) {
        match commit_pad_capture(capture, event) {
            Some(action) => tracing::info!(
                "Pad bind captured: {} → {}",
                action.label(),
                pad_notation(event)
            ),
            None => tracing::debug!("gameinput: pad event ignored (capture mode)"),
        }
        return None;
    }
    match event {
        PadDownEvent::Controller { .. } => match_controller(newly, triggers),
        PadDownEvent::Guide => match_system_button(PadBind::Guide, true, triggers),
        PadDownEvent::Share => match_system_button(PadBind::Share, true, triggers),
    }
}

/// Live pad triggers from the settings mailbox, parsed per event (cheap: three
/// config rows) so a UI Apply or a captured bind fires without a restart. A
/// parse failure is logged and yields `None` (errors are values); the event
/// fires nothing rather than trusting stale bindings. cfg-free so the mailbox
/// behavior is unit-testable on Linux.
fn pad_triggers_now(mailbox: &Arc<Mutex<Settings>>) -> Option<Vec<(Action, PadBind)>> {
    match bindings::parse_controls(&settings_now(mailbox).controls) {
        Ok(parsed) => Some(pad_triggers(&parsed)),
        Err(err) => {
            tracing::error!("Gamepad bindings invalid: {err}");
            None
        }
    }
}

/// Pad-bind count for the non-Windows startup log line. A parse failure is
/// logged; the count is 0, and on Windows the pump starts with 0 binds too
/// because the capture UI needs pump events before any bind exists.
#[cfg(not(windows))]
fn pad_bind_count(mailbox: &Arc<Mutex<Settings>>) -> usize {
    pad_triggers_now(mailbox).map_or(0, |triggers| triggers.len())
}

/// Start the gamepad trigger. Called from `main` after the hotkey listener is
/// up, with the shared settings mailbox (UI Apply publishes it), the shared
/// debounce gate both trigger sources fire through, and the shared pad-capture
/// mailbox the settings UI drives.
///
/// Windows: spawns the pump thread (spec §6 gamepad poll thread). It starts
/// with zero pad binds too — the UI needs pump events to capture a bind.
/// Linux/macOS: logs the platform has no GameInput backend and spawns nothing.
#[cfg(windows)]
pub(crate) fn spawn_pump(
    settings: Arc<Mutex<Settings>>,
    handoff: ToastHandoff,
    gate: Arc<Mutex<Debounce>>,
    capture: PadCaptureMailbox,
) {
    let configured = pad_triggers_now(&settings).map_or(0, |triggers| triggers.len());
    tracing::info!("Gamepad pump starting ({configured} pad bind(s) configured)");

    std::thread::spawn(move || {
        // Ownership contract (observer lifetime hazard above): `observer` and
        // `stop` live in this closure for the thread's whole lifetime, which is
        // the process lifetime — `run` never returns because `stop` is never
        // set. Nothing else holds a reference to the observer.
        let observer = PadObserver {
            settings,
            previous: Mutex::new(HashMap::new()),
            devices: Mutex::new(Vec::new()),
            warned_named_binds: Mutex::new(Vec::new()),
            capture,
            gate,
            handoff,
        };
        let stop = AtomicBool::new(false);
        match gamepad::run(&observer, &stop) {
            Ok(()) => tracing::info!("Gamepad pump stopped"),
            Err(err) => tracing::info!("Gamepad pump not running: {err}"),
        }
    });
}

/// Non-Windows: no GameInput backend (Linux gilrs is out of scope, spec §9 +
/// AGENTS.md hardware finding). The pad binds are parsed for the log line so a
/// misconfigured `pad` is visible on this host too; nothing is spawned, so the
/// capture mailbox stays idle (the UI renders the pad field as a disabled
/// placeholder).
#[cfg(not(windows))]
pub(crate) fn spawn_pump(
    settings: Arc<Mutex<Settings>>,
    handoff: ToastHandoff,
    gate: Arc<Mutex<Debounce>>,
    capture: PadCaptureMailbox,
) {
    let _ = (handoff, gate, capture);
    let configured = pad_bind_count(&settings);
    tracing::info!(
        "gameinput: not supported on this platform ({} pad bind(s) configured)",
        configured,
    );
}

/// Event sink handed to `gamepad::run`: logs the GameInput events and fires the
/// shot. Callbacks run on GameInput's threads or inside `Dispatch`, so they
/// must return quickly: the body is a mask diff, one capture lock, one gate
/// lock, and `spawn_capture` (which spawns a detached thread and returns). No
/// re-emit.
#[cfg(windows)]
struct PadObserver {
    /// The LIVE settings mailbox — the same Arc the hotkey listener reads,
    /// published by UI Apply. Triggers are re-parsed per event from it.
    settings: Arc<Mutex<Settings>>,
    /// Per-device previous button-state mask, the source of the 0→1 edges
    /// ([`newly_pressed`]). Registry entries are never removed, so an index is
    /// stable; a disconnect drops the entry (a reconnect is a new index).
    previous: Mutex<HashMap<usize, u64>>,
    /// Registry indices of the connected devices, for resolving `Named` binds
    /// through device-info labels ([`resolve_pad_binds`]). `device_connected`
    /// adds, `device_disconnected` removes: a disconnected entry exposes no
    /// labels, so scanning it would be wasted work.
    devices: Mutex<Vec<usize>>,
    /// `Named` bind names already warned about — the trigger list re-parses per
    /// event, so an unresolved name would otherwise WARN on every button press.
    warned_named_binds: Mutex<Vec<String>>,
    /// Shared with the GUI: capture mode in, committed press out.
    capture: PadCaptureMailbox,
    /// Shared with the hotkey listener (spec §4.1 OR-fire, one gate).
    gate: Arc<Mutex<Debounce>>,
    /// Latest-only toast-handle slot: read per shot, empty until the GUI
    /// attaches the service handle (spec §4.3 hand-off).
    handoff: ToastHandoff,
}

#[cfg(windows)]
impl gamepad::Observer for PadObserver {
    fn device_connected(&self, device: &gamepad::DeviceInfo) {
        tracing::info!(
            "Gamepad connected: {} (vid=0x{:04x} pid=0x{:04x}, registry index {})",
            device.name,
            device.vendor_id,
            device.product_id,
            device.index,
        );
        // Remember the registry index for `Named`-bind resolution. A poisoned
        // registry skips the push — logging must not fail the callback.
        if let Ok(mut devices) = self.devices.lock() {
            devices.push(device.index);
        }
    }

    fn buttons_changed(&self, device_index: usize, bits: u64, _pressed_indices: &[u8]) {
        // `Debug` of the index list is rendered by `format!`: a slice is not a
        // `tracing::Value`, so it cannot be captured directly by the macro.
        let previous = self.previous_mask(device_index, bits);
        let newly = newly_pressed(previous, bits);
        tracing::debug!(
            "gameinput: device {} button state changed bits=0x{:016x} down={}",
            device_index,
            bits,
            format!("{newly:?}"),
        );
        // A release-only change has no DOWN edge: nothing to fire, nothing to
        // capture. The notation records the LOWEST newly pressed index when one
        // poll catches several presses — a documented tie-break for capture.
        let Some(&index) = newly.first() else {
            return;
        };
        let event = PadDownEvent::Controller {
            device: device_index,
            index,
        };
        let triggers = self.triggers_now();
        let action = {
            let mut capture = lock_capture(&self.capture);
            decide_pad_shot(&mut capture, event, &newly, &triggers)
        };
        let Some(action) = action else { return };
        self.fire(action);
    }

    fn system_button(&self, button: gamepad::SystemButton, down: bool, device_name: &str) {
        let edge = if down { "DOWN" } else { "UP" };
        tracing::debug!(
            "gameinput: {} {} (device {})",
            system_name(button),
            edge,
            device_name
        );
        if !down {
            return;
        }
        let event = system_event(button);
        let triggers = self.triggers_now();
        let action = {
            let mut capture = lock_capture(&self.capture);
            decide_pad_shot(&mut capture, event, &[], &triggers)
        };
        let Some(action) = action else { return };
        self.fire(action);
    }

    fn device_disconnected(&self, index: usize, name: &str) {
        tracing::info!("Gamepad disconnected: {name} (registry index {index})");
        // Drop the remembered mask: the registry entry stays (index stable) but
        // a reconnect registers as a NEW entry, so a stale mask must never be
        // diffed against a different device. A poisoned tracker is skipped —
        // logging must not fail the callback.
        if let Ok(mut previous) = self.previous.lock() {
            previous.remove(&index);
        }
        // Drop the registry index too: `Named` binds must only resolve against
        // connected devices (§9), so a gone device can no longer supply a
        // label→index mapping. A poisoned registry skips the removal — logging
        // must not fail the callback.
        if let Ok(mut devices) = self.devices.lock() {
            devices.retain(|device| *device != index);
        }
    }

    fn heartbeat(&self, devices: usize, dispatches: u64) {
        tracing::debug!(
            "gameinput: alive devices={} dispatches={}",
            devices,
            dispatches
        );
    }
}

#[cfg(windows)]
impl PadObserver {
    /// Live pad triggers, re-parsed from the settings mailbox per event (cheap:
    /// three config rows) so a UI Apply or a captured bind fires without a
    /// restart. `Named` binds resolve to bit indices through the connected
    /// devices' label tables ([`resolve_pad_binds`] — the name→index mapping is
    /// device-dependent, §9); a name no device reports drops out and warns once
    /// instead of aborting the whole trigger list. An invalid config fires
    /// nothing for this event (logged).
    fn triggers_now(&self) -> Vec<(Action, PadBind)> {
        let Some(parsed) = pad_triggers_now(&self.settings) else {
            return Vec::new();
        };
        let devices = self.connected_devices();
        let (resolved, unresolved) = resolve_pad_binds(&parsed, &devices, |device, index| {
            gamepad::button_label(device, index as u8)
        });
        for name in unresolved {
            self.warn_unresolved_bind(&name);
        }
        resolved
    }

    /// Snapshot of the connected device indices. Clone, not a borrow: the label
    /// scan calls `gamepad::button_label` (the library registry lock), and our
    /// own mutex must not be held while taking it — one lock order, no deadlock.
    fn connected_devices(&self) -> Vec<usize> {
        self.devices
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// WARN once per unresolved name (the trigger list re-parses every event):
    /// a bind naming a button no connected device labels stays silent, but the
    /// reason is visible in the log without spamming each button press.
    fn warn_unresolved_bind(&self, name: &str) {
        let mut warned = self
            .warned_named_binds
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if warned
            .iter()
            .any(|warned| warned.eq_ignore_ascii_case(name))
        {
            return;
        }
        tracing::warn!(
            "Pad bind \"{name}\" is not labeled by any connected device — its trigger stays silent"
        );
        warned.push(name.to_owned());
    }

    /// Previous mask of one device, then record the new one. A poisoned tracker
    /// reads the previous mask as 0 — every bit then counts as a DOWN edge, the
    /// conservative direction for capture, and the debounce gate bounds re-fires.
    fn previous_mask(&self, device_index: usize, current: u64) -> u64 {
        let mut previous = self
            .previous
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let last = previous.get(&device_index).copied().unwrap_or(0);
        previous.insert(device_index, current);
        last
    }

    /// The shot path, identical to the hotkey listener's: reserved action or a
    /// blocked trigger fires nothing; otherwise the same `spawn_capture` call
    /// (`hotkeys.rs` listener) with the CURRENT mailbox settings and the toast
    /// handle read from the shared slot.
    fn fire(&self, action: Action) {
        let Some(mode) = capture_mode(action) else {
            return;
        };
        if !bindings::try_fire_shared(&self.gate, action, chrono::Local::now()) {
            return;
        }
        tracing::info!("Screenshot fired from gamepad: {}", action.label());
        spawn_capture(
            CaptureCommand {
                mode,
                settings: settings_now(&self.settings),
            },
            handoff_handle(&self.handoff),
        );
    }
}

/// Library `SystemButton` → the DOWN event classification shared with capture.
/// The bind comparison itself happens inside `decide_pad_shot`, which matches
/// the event against the same two `bindings::parse_pad` names, so no separate
/// `PadBind` conversion is needed here.
#[cfg(windows)]
fn system_event(button: gamepad::SystemButton) -> PadDownEvent {
    match button {
        gamepad::SystemButton::Guide => PadDownEvent::Guide,
        gamepad::SystemButton::Share => PadDownEvent::Share,
    }
}

/// Log name of a system button.
#[cfg(windows)]
fn system_name(button: gamepad::SystemButton) -> &'static str {
    match button {
        gamepad::SystemButton::Guide => "guide",
        gamepad::SystemButton::Share => "share",
    }
}

#[cfg(test)]
#[path = "gamepad.tests.rs"]
mod tests;
