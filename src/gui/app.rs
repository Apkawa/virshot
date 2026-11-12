//! App state and the event-loop wiring of the settings window (spec §4.4,
//! §4.5, §4.3, §4.7): the `SettingsApp` struct owns the draft settings, the
//! hotkey controller, the tray, the toast stack, the About/update child
//! viewports, bind-capture state and the auto-update schedule; `apply()`
//! saves, switches the locale, rebuilds the tray menu, re-arms the check
//! schedule and re-registers hotkeys live; the eframe `App` pass runs the
//! pump — `logic` drains tray actions and the pad-capture / probe / check /
//! install / notes mailboxes, drives the overlay host, decides close-vs-exit
//! and schedules repaints, while `ui` docks the bottom bar and dispatches
//! the tabs. Tab painting itself lives in the sibling modules.
//!
//! eframe 0.37 API facts verified in the locked source, not from memory:
//! - `App` trait: `logic(&mut self, ctx, frame)` + `ui(&mut self, ui, frame)`
//!   (no `bool` return; exit is controlled by viewport close commands).
//! - Window X click: winit `CloseRequested` → egui `RequestClose`; eframe
//!   shuts down UNLESS the app emits `ViewportCommand::CancelClose` in the
//!   frame that observed `close_requested()` (eframe `epi_integration` +
//!   `glow_integration`). We cancel the close and send `Visible(false)`:
//!   the window hides, the event loop keeps running, the process stays
//!   resident. Tray (Stage 10, spec §4.5): `logic` polls
//!   `tray::TrayController::poll()` every frame — tray events do not wake
//!   the winit loop, so `request_repaint_after(TRAY_POLL_TICK)` keeps the
//!   polling alive while the window is hidden (logic-only pass, negligible
//!   CPU). While hidden, eframe runs `logic` only, no UI pass.
//! - Exit is the tray menu or the bottom OK/Cancel buttons (eframe 0.37 has
//!   NO `App::should_exit` — verified in the locked source): tray `Exit` sets
//!   `exit_requested`, OK/Cancel set `close_for_exit` and send
//!   `ViewportCommand::Close`; the pass that sees `close_requested()` decides
//!   through `close_decision` — an exit does NOT cancel, eframe shuts down,
//!   `run_native` returns, the process exits 0. X-close still hides only.
//!
//! Toast plumbing (spec §4.3, Stage 4): this window OWNS the toast stack —
//! `overlay::OverlayHost` (crates/overlay host-loop backend) and
//! `toast::ToastService` (crates/toast timer thread) are created in the eframe
//! `create` closure of `run`; trigger threads reach the chips through the
//! handle slot (`capture::ToastHandoff`) filled here. The host paints each chip
//! as a child egui viewport on this loop: transparent, undecorated,
//! always-on-top, mouse-passthrough, content-protected, hidden until its first
//! paint, faded out at the end — the crate builds and commands the viewports,
//! this module no longer reimplements any of it. Verified against the locked
//! egui/eframe 0.37 sources, not from memory:
//! - `Context::show_viewport_deferred` only writes viewport output, so
//!   `OverlayHost::update(&ctx)` is valid from `App::logic` on BOTH passes:
//!   a visible root runs `logic` inside the ui pass, and a hidden/minimized
//!   root runs `Context::run_logic` (`eframe epi_integration`), whose
//!   `LogicOutput` carries platform output and viewport commands for ALL
//!   viewports — so chips still spawn, paint, and fade while the settings
//!   window is hidden.
//! - Monitor rects are PHYSICAL desktop pixels with each monitor's DPI scale
//!   attached (`overlay::MonitorRect`), enumerated once through
//!   `overlay::monitors` — the crate's single source of truth for monitor
//!   geometry; this module only wires it and logs — because `OverlayHost`
//!   converts a rect with the CHILD window's own pixels-per-point
//!   (`crates/overlay/src/host.rs`) — the root window's scale belongs to the
//!   settings window, not to a chip. `xcap` stays capture-only. The PRIMARY
//!   monitor comes FIRST — toast `MonitorSel::Index(0)` means primary (spec
//!   §4.3); `overlay::monitors` flags the OS primary monitor. A monitor with
//!   no usable size is skipped (WARN); with no monitor geometry at all the
//!   app falls back to the host window's own physical rect, and an empty list
//!   makes the service log and skip chips (the app stays alive).
//! - `ToastService::spawn(..., wake = Some(ctx.clone()))` wakes this loop for
//!   fade frames; `TRAY_POLL_TICK` repaints keep the host loop fed while idle.
//! - Drop order: `toast_service` is declared BEFORE `overlay_host`, so the
//!   service's `Drop` (Quit + join, hides chips) runs while the host is alive.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::Duration;
use std::time::Instant;

// The `gui::overlay` child module shadows the `overlay` path crate inside this
// file, so the crate imports are qualified with the `::` prefix. Likewise the
// `gui::update` child module shadows the crate module `crate::update`, so
// backend paths here are spelled `crate::update::...` and the GUI update items
// are spelled `update::...`.
use ::overlay::OverlayHost;
use toast::ToastService;

use super::binding;
use super::chrome;
use super::general;
use super::icons;
use super::update;
use crate::bindings::Action;
use crate::capture;
use crate::capture::ToastHandoff;
use crate::config;
use crate::config::Settings;
use crate::gamepad::PadCaptureMailbox;
use crate::hotkeys::HotkeysController;
use crate::hotkeys::Registration;
use crate::tray::TrayAction;
use crate::tray::TrayController;
use crate::update::notes;

/// Tray poll cadence: tray events do not wake the winit event loop, so the
/// logic pass schedules a repaint to drain them promptly even while the
/// window is hidden. 500 ms of logic-only ticks keeps idle CPU far under the
/// §5 budget.
const TRAY_POLL_TICK: Duration = Duration::from_millis(500);

/// Tab identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tab {
    General,
    Hotkeys,
    Overlay,
}

impl Tab {
    fn label(self) -> String {
        match self {
            Tab::General => rust_i18n::t!("general.tab_general").into_owned(),
            Tab::Hotkeys => rust_i18n::t!("general.tab_hotkeys").into_owned(),
            Tab::Overlay => rust_i18n::t!("general.tab_overlay").into_owned(),
        }
    }
}

/// The settings-window app. Owns the draft settings, the hotkey controller
/// (main-thread manager), the toast stack (`ToastService` + `OverlayHost`,
/// spec §4.3), and bind-by-press state.
pub(crate) struct SettingsApp {
    config_path: PathBuf,
    /// Last applied settings — closing without Apply resets the draft to it.
    pub(crate) applied: Settings,
    pub(crate) draft: Settings,
    pub(crate) controller: Option<HotkeysController>,
    pub(crate) status: Vec<Registration>,
    tab: Tab,
    /// Row whose "Capture press" is waiting for a key event (§4.4 bind-by-press).
    pub(crate) capture_press: Option<Action>,
    /// Row whose pad field is waiting for a pad DOWN event (§4.4, Stage 5).
    /// Windows only — the GameInput pump is what commits it; elsewhere the pad
    /// field renders as a disabled placeholder and this stays `None`.
    pub(crate) capture_pad: Option<Action>,
    /// Pad-capture mailbox shared with the pump (`gamepad::PadCapture`): the
    /// GUI sets the capture, the pump commits a press, `logic` drains it into
    /// the draft — no callback crosses the thread boundary.
    pub(crate) pad_capture: PadCaptureMailbox,
    /// Settings mailbox shared with the hotkey listener and the pad pump
    /// (`main` created it). Published by `apply`; needed here as the fallback
    /// publisher when the hotkey controller is unavailable.
    settings_mailbox: Arc<Mutex<Settings>>,
    /// Toast timer thread (spec §4.3): owns chip lifetimes — latest wins,
    /// fade-out at the end, errors held longer. Declared BEFORE `overlay_host`
    /// so its `Drop` (Quit + join, hiding chips) runs while the host backend
    /// is still alive. Read by nothing: this field is the owner, and ownership
    /// is the whole reason it exists.
    #[allow(dead_code)] // Owned for its lifetime only; the service is driven
    // through the handle the triggers hold, and it shuts down on drop.
    toast_service: ToastService,
    /// Toast viewport backend (crates/overlay host loop): chips are child egui
    /// viewports spawned, painted, and faded here; `logic` drives it every
    /// pass (spec §4.3).
    overlay_host: OverlayHost,
    /// Latest-only toast-handle slot shared with the hotkey listener and the
    /// pad pump (`main` created it); `run`'s create closure fills it, so
    /// triggers can attach a chip to their capture (spec §4.3 hand-off).
    pub(crate) handoff: ToastHandoff,
    /// Tray controller (spec §4.5, Stage 10); `None` when the tray failed to
    /// start (typed error logged by main). Polled in `logic`.
    tray: Option<TrayController>,
    /// Warning status icon: the embedded triangle texture uploaded once in
    /// `run`'s create closure (`icons::warning_icon_texture`). `None` when the asset
    /// decode failed (logged at WARN) — warning rows then paint the ASCII
    /// `!` glyph in the same reserved slot.
    pub(crate) warning_icon: Option<egui::TextureHandle>,
    /// Set by the tray `Exit` action only (spec §4.5): the next close request
    /// is not cancelled, so eframe shuts down.
    exit_requested: bool,
    /// Set by the OK/Cancel buttons only (spec §4.4): those are explicit
    /// decisions, so their close request exits instead of hiding to the tray.
    /// The plain X button leaves it `false` and keeps the hide-to-tray
    /// behaviour (§4.5).
    close_for_exit: bool,
    /// Last root-viewport focus seen by `logic` (`None` = no pass yet), so the
    /// Stage 13 retry fires on the transition INTO focus only — see
    /// [`binding::should_retry_failed_registrations`].
    viewport_focused: Option<bool>,
    /// About child viewport (task 2026-11-10_about-dialog, stage 10):
    /// lifecycle state (`about::AboutWindow`). Tray `About` opens a closed
    /// window and RAISES (Focus) an open one; the child paint pass records an
    /// OS close request into the shared flag, and the next logic pass stops
    /// registration — egui GCs the viewport and eframe drops the
    /// `Arc<Window>`, destroying the window (`crates/overlay/src/host.rs:313-323`).
    about_window: crate::about::AboutWindow,
    /// Markdown cache for the About changelog. The viewport callback is
    /// `Send + Sync + 'static` (egui 0.37), so the cache lives behind an Arc
    /// the closure can own; the viewer itself is created per paint in
    /// `about::paint`.
    about_changelog_cache: Arc<Mutex<egui_commonmark::CommonMarkCache>>,
    /// About header app icon: uploaded ONCE when About first opens
    /// (`icons::about_icon_texture`, lazily in the tray handler). Behind an `Arc`
    /// because the viewport callback is `'static` and must share the handle
    /// with app state — rebuilding it per paint pass would drop the previous
    /// `TextureHandle` and free its texture id every frame. `None` when the
    /// decode failed (logged at WARN) or About has never opened: the header
    /// then paints text only.
    about_icon: Option<Arc<egui::TextureHandle>>,
    /// Held update discovery (task 2026-11-11_auto-update Stage 6, spec §4.7):
    /// the newest `CheckOutcome::Updated` drained in `logic`; `None` until a
    /// check finds something newer than the running build. Cleared by a
    /// successful install (`update_info_after_install`); kept on failure so
    /// the row can show the error.
    pub(crate) update: Option<crate::update::UpdateInfo>,
    /// Background-check hand-off receiver (task 2026-11-11_auto-update
    /// Stage 7, spec §4.7): `main.rs` creates the channel and moves BOTH
    /// halves into `gui::run` — the GUI does not create its own. Contract:
    /// drained ONLY in `App::logic`; draining a closed channel is a cheap
    /// no-op and never blocks. `Option` so the drain can take it by reference
    /// while app state stays borrowable.
    update_rx: Option<mpsc::Receiver<crate::update::CheckOutcome>>,
    /// Sender half for every check this app spawns (Stage 7): scheduled and
    /// forced checks send on this clone. A `Sender` clone is O(1) — a refcount
    /// bump on the shared channel — so it is never wrapped in `Arc`
    /// (AGENTS.md: `Sender` already clones in O(1)).
    update_tx: mpsc::Sender<crate::update::CheckOutcome>,
    /// Next periodic-check instant (Stage 7, spec §4.7): `None` when nothing is
    /// pending — a `Never` interval, or the scheduled check that just fired.
    /// Re-armed after a drained SCHEDULED outcome only; the forced button
    /// never moves it (§4.7). `apply()` re-arms it from the APPLIED interval
    /// (`schedule_after_apply`), so an interval switch takes effect live.
    /// `logic` spawns the next check when the clock passes it and requests a
    /// repaint no later than this instant.
    next_check: Option<Instant>,
    /// Whether the FIRST `logic` pass has armed the interval deadline: the
    /// startup check is spawned by `main.rs` BEFORE the event loop, so the
    /// interval clock starts on the first pass, not at app construction. The
    /// arm runs BEFORE the outcome drain, so a startup outcome that drains on
    /// that same pass still frees the slot (see [`update::check_in_flight_after`]).
    deadline_armed: bool,
    /// Whether ONE check is in flight (Stage 9, spec §4.7): scheduled or
    /// forced. Set on spawn and recomputed once per pass by
    /// [`update::check_in_flight_after`] — the drained outcome in `logic` is the only
    /// thing that frees the slot. Gates the forced-check button
    /// ([`update::check_now_button_enabled`]) and the scheduled spawn, so exactly one
    /// check thread is live at a time.
    pub(crate) check_in_flight: bool,
    /// Whether the check currently in flight is the FORCED one: its drained
    /// outcome must NOT re-arm or move the interval deadline (§4.7). The two
    /// flags describe one slot — `check_in_flight` picks the gate, this one
    /// picks the deadline rule.
    pub(crate) forced_check_in_flight: bool,
    /// Forced-check press recorded by the paint pass; `logic` spawns the check
    /// and clears it. Paint never spawns threads, so the press is a record.
    /// A press the busy slot cannot honor stays recorded (`forced_check_decision`)
    /// and runs on the next pass, so a press is never silently dropped.
    pub(crate) check_now_requested: bool,
    /// The result line shown beside «Проверить обновления» (Stage 11
    /// sub-item 2, spec §4.7): ONLY forced-check outcomes land here
    /// (`forced_check_status_after`); scheduled results never touch it —
    /// they keep using the chip and the General row. `None` before the
    /// first forced check; while a forced check is in flight the paint
    /// renders nothing (`forced_check_status_shown`) but keeps the slot.
    pub(crate) forced_check_status: Option<update::ForcedCheckStatus>,
    /// Shared install→restart flag (Stage 7, spec §5): created in `main.rs`
    /// and set here when an install succeeds, together with
    /// `ViewportCommand::Close`; `main.rs` drops the instance lock and spawns
    /// the new binary once `run_native` returns.
    restart_requested: Arc<AtomicBool>,
    /// Install lifecycle shown by the General row and the dialog buttons
    /// (see [`update::UpdateStatus`]).
    pub(crate) update_status: update::UpdateStatus,
    /// Install-thread result: exactly one `Result<(), String>` (verbatim
    /// English `InstallError` text), drained once in `logic`; `None` while no
    /// install thread is live.
    pub(crate) install_rx: Option<mpsc::Receiver<Result<(), String>>>,
    /// Update-dialog child viewport lifecycle (see [`update::UpdateDialogWindow`],
    /// the exact About shape). Opened from the General row; NOT modal.
    pub(crate) update_dialog: update::UpdateDialogWindow,
    /// Slot the dialog paint records button presses into; `logic` gates them
    /// with `dialog_outcome`. A fresh slot per open (like About's fresh close
    /// flag) so a re-open never inherits a stale press.
    pub(crate) dialog_outcome_slot: Arc<Mutex<Option<update::UpdateDialogButton>>>,
    /// Release-notes hand-off, created ONLY when the dialog opens (lazy
    /// fetch, spec §4.7) and dropped on close: a pending fetch thread's send
    /// then lands on a closed receiver and is ignored (module contract), and
    /// a re-open fetches fresh.
    pub(crate) notes_rx: Option<mpsc::Receiver<Result<notes::ReleaseNotes, notes::NotesError>>>,
    /// Arrived notes: `None` = fetch still pending (dialog paints the loading
    /// hint). The `Arc` wrap is for the `'static` viewport callback, which
    /// needs an owned snapshot — an O(1) refcount bump, not a deep copy.
    pub(crate) notes: Option<Result<Arc<notes::ReleaseNotes>, notes::NotesError>>,
    /// Markdown cache for the dialog notes — same Arc pattern as
    /// `about_changelog_cache`: the viewport callback is `Send + Sync +
    /// 'static`, and the `!Send` viewer is constructed per paint.
    update_notes_cache: Arc<Mutex<egui_commonmark::CommonMarkCache>>,
}

impl SettingsApp {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        config_path: PathBuf,
        settings: Settings,
        controller: Option<HotkeysController>,
        status: Vec<Registration>,
        toast_service: ToastService,
        overlay_host: OverlayHost,
        handoff: ToastHandoff,
        tray: Option<TrayController>,
        settings_mailbox: Arc<Mutex<Settings>>,
        pad_capture: PadCaptureMailbox,
        update_rx: mpsc::Receiver<crate::update::CheckOutcome>,
        update_tx: mpsc::Sender<crate::update::CheckOutcome>,
        restart_requested: Arc<AtomicBool>,
    ) -> Self {
        Self {
            config_path,
            applied: settings.clone(),
            draft: settings,
            controller,
            status,
            tab: Tab::General,
            capture_press: None,
            capture_pad: None,
            pad_capture,
            settings_mailbox,
            toast_service,
            overlay_host,
            handoff,
            tray,
            warning_icon: None,
            exit_requested: false,
            close_for_exit: false,
            viewport_focused: None,
            about_window: crate::about::AboutWindow::new(),
            about_changelog_cache: Arc::new(
                Mutex::new(egui_commonmark::CommonMarkCache::default()),
            ),
            about_icon: None,
            update: None,
            update_rx: Some(update_rx),
            update_tx,
            next_check: None,
            deadline_armed: false,
            check_in_flight: false,
            forced_check_in_flight: false,
            check_now_requested: false,
            forced_check_status: None,
            restart_requested,
            update_status: update::UpdateStatus::Idle,
            install_rx: None,
            update_dialog: update::UpdateDialogWindow::new(),
            dialog_outcome_slot: Arc::new(Mutex::new(None)),
            notes_rx: None,
            notes: None,
            update_notes_cache: Arc::new(Mutex::new(egui_commonmark::CommonMarkCache::default())),
        }
    }

    /// Apply: save `config.toml` and re-register hotkeys live (spec §4.4).
    /// A draft with duplicate triggers is refused before anything else happens
    /// (§4.4, Stage 3): the bottom buttons already disable Apply/OK for it and
    /// `config::save` validates nothing, so this guard is defense in depth — a
    /// refused Apply logs the collisions and writes, publishes, and
    /// re-registers nothing. Other failures are logged; the app keeps running.
    pub(crate) fn apply(&mut self) {
        let duplicates = binding::duplicate_bindings(&self.draft);
        if !duplicates.is_empty() {
            let summary = duplicates
                .iter()
                .map(|duplicate| duplicate.description.clone())
                .collect::<Vec<String>>()
                .join(", ");
            tracing::warn!("Apply refused: duplicate bindings — {summary}");
            return;
        }
        match config::save(&self.draft, &self.config_path) {
            Ok(()) => {
                tracing::info!("Config saved: {}", self.config_path.display());
                // Live locale switch (spec §2, Stage 2b): the SAVED draft is
                // the source of truth, so switch only after the save succeeds
                // (a failed save leaves the current locale alone), BEFORE the
                // tray rebuild, so the rebuilt menu labels resolve in the new
                // language — no restart needed.
                rust_i18n::set_locale(general::locale_code_of(&self.draft));
                if let Some(tray) = &self.tray {
                    // Best-effort: a refused rebuild keeps the old-language
                    // menu, which is cosmetic; the app stays alive (spec §5).
                    if let Err(err) = tray.refresh_menu() {
                        tracing::warn!("Tray menu rebuild failed: {err}");
                    }
                }
            }
            Err(err) => tracing::error!("Config save failed: {err}"),
        }
        self.applied = self.draft.clone();

        // Background-check schedule (spec §4.7): the APPLIED interval governs
        // the scheduled checks, so saving settings moves the deadline —
        // `никогда` → a scheduled interval arms the schedule right away (no
        // restart needed), an interval switch (1h→1d) re-arms to the NEW
        // interval, and a switch to `никогда` clears an armed deadline so no
        // extra check fires after checks were turned off. A check already in
        // flight is left running: only the deadline changes here, never
        // `check_in_flight`/`forced_check_in_flight` (`schedule_after_apply`).
        self.next_check =
            update::schedule_after_apply(Instant::now(), self.applied.general.update_check);

        match &mut self.controller {
            Some(controller) => match controller.apply(self.draft.clone()) {
                Ok(status) => self.status = status.registrations,
                Err(err) => tracing::error!("Hotkey re-registration failed: {err}"),
            },
            None => {
                // No hotkey controller (Linux without X11, or a Windows manager
                // failure): the pad pump reads the same settings mailbox, so it
                // is published here instead. With a controller, `apply` publishes
                // it (one publisher per Apply).
                let mut mailbox = self
                    .settings_mailbox
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                *mailbox = self.draft.clone();
            }
        }
    }

    /// Close the window for real (spec §4.4): OK and Cancel are explicit
    /// decisions, not a hide-to-tray. The flag is what `logic` reads — the
    /// close request lands on the next pass, and with it set the request is
    /// not cancelled, so eframe shuts down (the same exit path as the tray
    /// `Exit`, §4.5).
    pub(crate) fn request_window_exit(&mut self, ctx: &egui::Context) {
        self.close_for_exit = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        ctx.request_repaint();
    }
}

impl eframe::App for SettingsApp {
    /// Runs even while the window is hidden (no UI pass then). Keeps the
    /// process resident after a window close (spec §4.4): cancel the close
    /// request and hide the window instead — the tray (Stage 10) can reopen
    /// it. Closing discards unsaved draft edits (spec §4.4). The toast
    /// lifecycle runs here too, so a hotkey fired while the window is hidden
    /// still shows its toast (spec §4.3). Tray actions are polled every pass:
    /// left click / `Settings` re-shows the window, `Take screenshot` spawns
    /// a full-screen capture with the applied settings + the shared mailbox,
    /// `About` opens the About child viewport if closed and RAISES it (Focus)
    /// if already open (task 2026-11-10_about-dialog),
    /// `Exit` requests a real shutdown (spec §4.5 — exit is tray-menu-only).
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(tray) = &self.tray {
            for action in tray.poll() {
                match action {
                    TrayAction::ShowSettings => {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        ctx.request_repaint();
                    }
                    TrayAction::TakeScreenshot => {
                        capture::spawn_capture(
                            capture::CaptureCommand {
                                mode: capture::CaptureMode::FullScreen,
                                // Owned settings for the capture thread (spec
                                // §4.2); `applied` stays valid for the UI.
                                settings: self.applied.clone(),
                            },
                            capture::handoff_handle(&self.handoff),
                        );
                    }
                    TrayAction::About => {
                        // Tray select (user decision 2026-11-11): a CLOSED
                        // About opens — the first logic pass registers it, the
                        // first paint shows it (deferred viewports start
                        // hidden, like toast chips). An OPEN About is RAISED,
                        // never closed: `Focus` raises the window and
                        // registration stays untouched.
                        if self.about_window.select_from_tray() {
                            // Upload the header icon ONCE on first open (lazy:
                            // no GPU texture while About is never used). A
                            // decode failure is logged and stays `None` — the
                            // header paints text only.
                            if self.about_icon.is_none() {
                                self.about_icon = icons::about_icon_texture(ctx);
                            }
                        } else {
                            ctx.send_viewport_cmd_to(
                                egui::ViewportId::from_hash_of("virshot-about"),
                                egui::ViewportCommand::Focus,
                            );
                        }
                        ctx.request_repaint();
                    }
                    TrayAction::Exit => {
                        self.exit_requested = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        ctx.request_repaint();
                    }
                }
            }
        }

        // About child viewport (task 2026-11-10_about-dialog, stage 10):
        // registered on EVERY pass while open (egui closes an unregistered
        // child viewport — `crates/overlay/src/host.rs:314-323` precedent).
        // An OS close CANNOT be observed here: eframe pushes
        // `ViewportEvent::Close` into the CHILD's `ViewportInfo`
        // (`glow_integration.rs:1075`), the child pass runs before this root
        // pass (`run.rs:147-152`), and eframe clears the child's info after
        // its pass (`glow_integration.rs:853`), so the old `raw.viewports…
        // close_requested()` read was dead. `paint` records the request into
        // the shared flag inside the CHILD pass, and `observe_close()` turns
        // it into "closed" here. Stopping registration destroys the window:
        // egui GCs the viewport and eframe drops the `Arc<Window>` (R10).
        // No `ViewportCommand::Close`, no `CancelClose` (root-only, ignored
        // for children), no `Visible(false)` (phantom hidden window). An open
        // About never blocks tray Exit: Exit closes the ROOT viewport and
        // eframe tears down child viewports on shutdown (spec open question 5).
        self.about_window.observe_close();
        if self.about_window.wants_registration() {
            let about_viewport = egui::ViewportId::from_hash_of("virshot-about");
            if let Some(close_flag) = self.about_window.close_flag() {
                let cache = Arc::clone(&self.about_changelog_cache);
                // Arc clone is O(1) — one reference-count bump. The callback
                // is `Send + Sync + 'static` (egui 0.37), so the closure owns
                // a shared handle to the app-state texture instead of
                // uploading a new one per paint pass.
                let about_icon = self.about_icon.clone();
                // Chrome policy from `about::window_chrome()` (stage 9): title
                // bar, close button and OS resize border stay; minimize and
                // maximize buttons are off. The flags fold into winit's
                // `WindowAttributes::with_enabled_buttons` at window creation;
                // honored on Windows/macOS, inert on X11/Wayland. The builder
                // stays constant per viewport id — egui applies builder
                // changes by recreating the OS window.
                let chrome = crate::about::window_chrome();
                ctx.show_viewport_deferred(
                    about_viewport,
                    egui::ViewportBuilder::default()
                        .with_title(rust_i18n::t!("about.title"))
                        .with_inner_size([480.0, 360.0])
                        .with_decorations(chrome.decorations)
                        .with_resizable(chrome.resizable)
                        .with_close_button(chrome.close_button)
                        .with_minimize_button(chrome.minimize_button)
                        .with_maximize_button(chrome.maximize_button),
                    move |ui, _class| {
                        crate::about::paint(
                            ui,
                            &mut cache
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner()),
                            about_icon.as_deref(),
                            &close_flag,
                        );
                    },
                );
                ctx.request_repaint_of(about_viewport);
            }
        }

        // ── Auto-update (task 2026-11-11_auto-update, Stages 6-9, spec §4.7) ──
        // ONE clock read per pass: the deadline arming below, the drain, the
        // deadline comparison and the repaint request all share it.
        let now = Instant::now();
        // Interval scheduling (Stage 7, spec §4.7): the deadline is armed on the
        // FIRST pass — the startup check was spawned by `main.rs` before the
        // event loop — and re-armed after every drained SCHEDULED outcome, so
        // the next check follows the interval the settings publish rather than
        // the request that produced the outcome. Dev builds schedule on the
        // same terms as release builds; only `Never` yields `None` from
        // `crate::update::next_check_deadline`.
        //
        // Arming runs BEFORE the drain: on the first pass an armed deadline
        // means the startup check is in flight (both are the same interval
        // gate), and a startup outcome that drains on that same pass — a fast
        // failure such as an HTTP 404, or a slow window init — must free the
        // slot. Arming after the drain put the flag back ON after the outcome
        // was already drained and nothing cleared it again: no scheduled check
        // ever fired and «Проверить обновления» stayed disabled all session.
        let armed_this_pass = !self.deadline_armed;
        if armed_this_pass {
            self.deadline_armed = true;
            self.next_check = crate::update::next_check_deadline(
                now,
                self.applied.general.update_check.interval(),
            );
        }

        // Drain background-check outcomes: they are the source state for the
        // row, the dialog, and the install path. A FAILED scheduled CHECK is a
        // log line only — the scheduled check below retries it on the next
        // interval. An UP-TO-DATE outcome RESETS the held discovery (Stage 11
        // sub-item 4: a check outcome replaces the previous result — see
        // `discovery_cleared_by_check_outcome`). The latest held discovery
        // wins if several arrive in one pass.
        let mut discovery: Option<crate::update::UpdateInfo> = None;
        let mut outcome_drained = false;
        // The slot holds exactly one in-flight check, so its forced flag
        // decides the result line AND the reset rule for every outcome this
        // pass drains (sub-items 2+4).
        let forced_check = self.forced_check_in_flight;
        if let Some(rx) = &self.update_rx {
            while let Ok(outcome) = rx.try_recv() {
                outcome_drained = true;
                // Sub-item 2: a FORCED outcome REPLACES the beside-button
                // line; a scheduled outcome leaves the slot untouched.
                self.forced_check_status = update::forced_check_status_after(
                    self.forced_check_status.take(),
                    &outcome,
                    forced_check,
                );
                // Sub-item 4: stale-discovery reset, wired through the pure
                // helper so the rule is unit-testable without a Context.
                if update::discovery_cleared_by_check_outcome(
                    &outcome,
                    forced_check,
                    &self.update_status,
                ) {
                    self.update = None;
                }
                match outcome {
                    crate::update::CheckOutcome::Updated(info) => discovery = Some(info),
                    crate::update::CheckOutcome::UpToDate => {}
                    crate::update::CheckOutcome::Failed(err) => {
                        tracing::warn!("update check failed: {err}");
                    }
                }
            }
        }
        if let Some(info) = discovery {
            // Stage 7 (§4.7): a NEW discovery while an install FAILURE is
            // displayed puts the row back to Idle — `status_after_new_discovery`
            // — so Install is offered again instead of keeping the stale
            // failure error text. One clone: the state is read again by the
            // row and dialog passes later in this function.
            self.update_status = update::status_after_new_discovery(self.update_status.clone());
            // Success-style chip, overlay-enabled gate only (§4.3 — the same
            // suppression save toasts use). The chip text is built before
            // `info` moves into the install/discovery path.
            if update::update_toast_enabled(Some(&info), self.applied.overlay.enabled) {
                let chip = update::update_discovery_chip(&self.applied.overlay, &info);
                self.publish_update_chip(chip);
            }
            if update::auto_install_on_discovery(true, self.applied.general.auto_install) {
                // Auto-install ON (§4.7): straight to the install path — no
                // dialog, no row; the discovery is never held.
                self.start_install(info);
            } else {
                // Dialog path: the General row shows the discovery until an
                // install succeeds (or checks are switched off).
                self.update = Some(info);
            }
            ctx.request_repaint();
        }
        // Any drained outcome — Updated, UpToDate, or Failed — ends the check
        // that produced it, so the forced-check button is enabled again (§4.7).
        // Only a SCHEDULED outcome re-arms the interval deadline; a FORCED
        // outcome must not move it (`deadline_rearmed_by_outcome`, §4.7).
        if outcome_drained {
            if update::deadline_rearmed_by_outcome(self.forced_check_in_flight) {
                self.next_check = crate::update::next_check_deadline(
                    now,
                    self.applied.general.update_check.interval(),
                );
            }
            self.forced_check_in_flight = false;
        }
        // The single in-flight slot is decided in ONE place, after the arm and
        // the drain, so the first-pass arm cannot re-occupy a slot the drain
        // just freed (see [`update::check_in_flight_after`]). Exactly one check is in
        // flight at a time, and the deadline consumes only while it is free.
        self.check_in_flight = update::check_in_flight_after(
            self.check_in_flight,
            outcome_drained,
            if armed_this_pass {
                self.next_check
            } else {
                None
            },
        );
        // Scheduled check. A deadline that arrives while a FORCED check is
        // running is not consumed: it fires on the next pass, once the forced
        // outcome drains, so exactly one check thread is live at a time.
        if let Some(deadline) = self.next_check
            && now >= deadline
            && !self.check_in_flight
        {
            self.next_check = None;
            self.check_in_flight = true;
            // The Sender clone is the thread boundary: `spawn_check` moves an
            // owned sender into its thread while app state keeps its own for
            // the next deadline (O(1) clone, no `Arc`).
            crate::update::spawn_check(
                self.applied.general.update_channel,
                crate::update::current_build(),
                self.update_tx.clone(),
                crate::update::fetch_manifest_text,
            );
        }
        // Forced check (Stage 9, spec §4.7): the press the paint pass recorded.
        // The interval is IGNORED — `Never` checks too — and the scheduled
        // deadline is neither moved nor re-armed. `Never` schedules nothing, so
        // this button is the only check path that setting has. A press the busy
        // slot cannot honor stays RECORDED rather than dropped: a scheduled
        // spawn that took the slot on this very pass must not swallow it — the
        // forced check runs on the next pass, once the slot frees (the tray tick
        // keeps logic passes coming, so no extra repaint is needed).
        let forced = update::forced_check_decision(
            self.applied.general.update_check,
            self.check_now_requested,
            self.check_in_flight,
        );
        if forced.spawn {
            self.check_now_requested = false;
            self.check_in_flight = true;
            self.forced_check_in_flight = true;
            crate::update::spawn_check(
                self.applied.general.update_channel,
                crate::update::current_build(),
                self.update_tx.clone(),
                crate::update::fetch_manifest_text,
            );
        }
        if forced.pending {
            tracing::debug!("forced check deferred: a check is already in flight");
        }

        // Install-thread result (exactly one per install): transition the
        // shared state through the pure decision functions so the row and
        // dialog reflect it.
        if let Some(rx) = &self.install_rx
            && let Ok(outcome) = rx.try_recv()
        {
            self.install_rx = None;
            self.update_status = update::install_finished(&outcome);
            self.update = update::update_info_after_install(self.update.take(), outcome.is_ok());
            match &outcome {
                Ok(()) => {
                    // Stage 7 (spec §5): the binary IS replaced, so this
                    // process must run the new one. `restart_requested` marks
                    // the shutdown as an explicit EXIT (the close flow below
                    // never cancels it), and the viewport close makes
                    // `run_native` return; `main.rs` drops the instance lock,
                    // spawns the new binary, and exits 0. Startup cleanup of
                    // self-replace leftovers happens on EVERY start there.
                    tracing::info!("update installed: restart pending");
                    self.restart_requested.store(true, Ordering::Relaxed);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Err(message) => {
                    tracing::warn!("update install failed: {message}");
                    // Danger chip ALWAYS shown (§4.3: failures ignore the
                    // overlay toggle), with the verbatim English text.
                    self.publish_update_chip(update::update_failure_chip(
                        &self.applied.overlay,
                        message,
                    ));
                }
            }
            ctx.request_repaint();
        }

        // Notes hand-off, drained only while the dialog is open (the
        // receiver is created on open and dropped on close); exactly one
        // value ever arrives.
        if let Some(rx) = &self.notes_rx
            && let Ok(outcome) = rx.try_recv()
        {
            self.notes_rx = None;
            self.notes = Some(match outcome {
                Ok(notes) => Ok(Arc::new(notes)),
                Err(err) => Err(err),
            });
        }

        // Update dialog: the exact About child-viewport lifecycle — an OS
        // close request is recorded by the CHILD pass into the shared flag,
        // this pass stops registration, and egui GCs the viewport (eframe
        // drops the `Arc<Window>`). The Cancel button reaches the same end
        // via `dismiss_update_dialog`. Closing also drops the notes state
        // (a pending fetch send is ignored by the closed receiver).
        if self.update_dialog.observe_close() {
            self.dismiss_update_dialog();
        }
        if self.update_dialog.wants_registration() {
            let update_viewport = egui::ViewportId::from_hash_of("virshot-update");
            if let Some(close_flag) = self.update_dialog.close_flag() {
                // Owned snapshots for the `'static` callback: two O(1) Arc
                // bumps (cache, outcome slot), one Option clone of notes
                // (Arc bump inside) and one of the small discovery summary
                // and install status — the callback cannot borrow app state.
                let cache = Arc::clone(&self.update_notes_cache);
                let slot = Arc::clone(&self.dialog_outcome_slot);
                let notes = self.notes.clone();
                let info = self.update.clone();
                let channel = self.applied.general.update_channel; // Copy
                let status = self.update_status.clone();
                // Same chrome policy as About (task 2026-11-10): title bar +
                // close button + resizable, NO minimize/maximize.
                let chrome = crate::about::window_chrome();
                ctx.show_viewport_deferred(
                    update_viewport,
                    egui::ViewportBuilder::default()
                        .with_title(rust_i18n::t!("update.dialog_title"))
                        .with_inner_size([480.0, 360.0])
                        .with_decorations(chrome.decorations)
                        .with_resizable(chrome.resizable)
                        .with_close_button(chrome.close_button)
                        .with_minimize_button(chrome.minimize_button)
                        .with_maximize_button(chrome.maximize_button),
                    move |ui, _class| {
                        update::paint_update_dialog(
                            ui,
                            &mut cache
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner()),
                            &close_flag,
                            &slot,
                            &update::UpdateDialogView {
                                notes: notes.as_ref(),
                                info: info.as_ref(),
                                channel,
                                status: &status,
                            },
                        );
                    },
                );
                // A hidden settings window still repaints this viewport on
                // its own schedule; nudge it so notes arrival lands promptly.
                ctx.request_repaint_of(update_viewport);
            }
            // Presses recorded by the CHILD pass of the previous frame (the
            // child pass runs BEFORE the root pass — same one-frame ordering
            // as the About close flag): gate them against CURRENT state.
            let pressed = self
                .dialog_outcome_slot
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take();
            if let Some(button) = pressed {
                match update::dialog_outcome(button, &self.update_status, self.update.as_ref()) {
                    Some(update::UpdateDialogOutcome::Dismiss) => {
                        self.dismiss_update_dialog();
                    }
                    Some(update::UpdateDialogOutcome::Install(info)) => {
                        self.dismiss_update_dialog();
                        self.start_install(info);
                    }
                    // `None`: an Install press the current state rejects (the
                    // button is disabled anyway) — dialog stays open, effect
                    // nothing.
                    None => {}
                }
            }
        }

        if ctx.input(|i| i.viewport().close_requested()) {
            if self.restart_requested.load(Ordering::Relaxed) {
                // Stage 7 (spec §5): this Close request is the one the install
                // success sent above — an explicit EXIT, never cancelled, so
                // `run_native` returns and `main.rs` drops the instance lock,
                // spawns the new binary, and exits 0. An X-button close while
                // a restart is pending exits too: the installed binary has to
                // run either way, and hiding would keep the old one alive.
                tracing::info!("virshot exiting: update installed, restarting");
            } else {
                match chrome::close_decision(self.exit_requested, self.close_for_exit) {
                    chrome::CloseAction::Exit => {
                        // Tray `Exit` or an OK/Cancel button: no CancelClose this
                        // pass → eframe shuts down, `run_native` returns, the
                        // process exits 0.
                        tracing::info!(
                            "virshot exiting: {}",
                            if self.exit_requested {
                                "tray menu"
                            } else {
                                "settings window button"
                            }
                        );
                    }
                    chrome::CloseAction::HideToTray => {
                        // Plain window close (the X button): hide to the tray and
                        // discard the draft — the process stays resident (§4.5).
                        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                        self.draft = self.applied.clone();
                    }
                }
            }
        }
        // Pad-capture mailbox (Stage 5): the pump committed a press on its own
        // thread; the newest-only drain writes it into the draft here, so a pad
        // press is recorded even while the settings window is hidden.
        self.drain_pad_capture();

        // Bind-probe press (Stage 14): Windows delivers PrintScreen/ScrollLock/
        // Pause to NO app key channel, so the temporary probe registration is the
        // only signal for those keys while a row is capturing. The listener
        // recorded it on its own thread; this latest-only drain commits it into
        // the draft here — same shape as the pad mailbox above, and it runs on
        // the logic-only pass too, so the row is written while hidden.
        if let Some(controller) = &mut self.controller
            && let Some(press) = controller.take_probe_press()
        {
            self.commit_probe_press(press);
        }

        // Toast chips (spec §4.3): the crate owns the chip lifetimes
        // (`toast::ToastService`); this line only drives the viewport backend —
        // queued overlays become child egui viewports HERE, so chips spawn,
        // paint, and fade on every pass, including the logic-only pass eframe
        // runs while the settings window is hidden.
        self.overlay_host.update(ctx);

        // Failed hotkeys are retried when the settings window GAINS focus
        // (spec §4.1 v2, Stage 13): on Windows an OS default hotkey may be
        // taken once this app is foreground, on Linux the combo is held by
        // another client and the retry just re-reports it. The signal is the
        // root viewport's focus; see [`binding::should_retry_failed_registrations`] for
        // why that is the settings window and why `None` counts as unfocused.
        let focused = ctx.input(|i| i.viewport().focused);
        if binding::should_retry_failed_registrations(
            self.viewport_focused,
            focused,
            self.has_failed_registration(),
            self.capture_press.is_some(),
        ) && let Some(controller) = &mut self.controller
        {
            binding::merge_registration_statuses(
                &mut self.status,
                controller.retry_failed_registrations(),
            );
        }
        self.viewport_focused = focused;

        // Tray events arrive on crossbeam channels and never wake the winit
        // loop; schedule the next logic pass so polling stays alive while the
        // window is hidden. The toast service wakes the same context on its
        // fade ticks.
        ctx.request_repaint_after(TRAY_POLL_TICK);
        // A pending check deadline needs a pass AT the deadline instant, even
        // while the window is hidden: requesting the shorter of the two waits
        // keeps the tray tick alive and bounds the check drift to this tick
        // (`egui` keeps the earliest scheduled repaint, so this is additive).
        if let Some(deadline) = self.next_check {
            ctx.request_repaint_after(deadline.saturating_duration_since(now).min(TRAY_POLL_TICK));
        }
    }

    /// Alpha-0 clear so transparent windows show through (eframe's glow backend
    /// uses ONE clear color for all viewports — including the toast child
    /// viewports the overlay host creates). The settings window stays fully
    /// opaque because both panels it uses — the docked bottom bar and the
    /// `CentralPanel` — paint their panel fill.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Color32::TRANSPARENT.to_normalized_gamma_f32()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.handle_capture_press(ui);
        // Pad capture keys (§4.4, v2) run in the same pass, before the tabs are
        // painted, so a cleared pad slot shows in this frame. The two captures
        // are mutually exclusive (`set_keyboard_capture` clears both), so one
        // key press can never be handled twice.
        self.handle_pad_capture_press(ui);

        // Bottom row (spec §4.4): `Apply | OK | Cancel` docked to the window's
        // BOTTOM EDGE as a real bottom panel, with the "Unsaved changes"
        // indicator on the left of the same row. Docking is what keeps the row
        // visible when the tab content grows (the placeholder accordion); the
        // content is the `CentralPanel` below, which shrinks instead.
        // Save (Apply/OK) is gated by TWO draft checks (§4.4): every row needs
        // a key (`draft_savable` — Stage 2) AND the triggers must be unique
        // (`duplicate_bindings` — Stage 3, one combo or pad must trigger one
        // action); Cancel discards, so it stays enabled. The disabled-button
        // tooltip names whichever save gate blocks.
        // `right_to_left` lays widgets out from the right edge, so they are
        // added rightmost-first: Cancel, OK, Apply. Clicks are collected in the
        // layout pass and acted on after it, so the draft borrow ends before
        // anything mutates it.
        // egui reserves panels outermost-first and the `CentralPanel` must be
        // added last, so the bottom panel comes before the tab content.
        egui::Panel::bottom(chrome::BOTTOM_BAR_PANEL_ID).show(ui, |ui| {
            let unbound = !binding::draft_savable(&self.draft);
            let duplicates = binding::duplicate_bindings(&self.draft);
            let savable = !unbound && duplicates.is_empty();
            let buttons = chrome::bottom_buttons_enabled(
                chrome::is_dirty(&self.draft, &self.applied),
                savable,
            );
            let blocked_tooltip = if unbound {
                Some(binding::unbound_save_tooltip())
            } else if !duplicates.is_empty() {
                Some(binding::duplicate_save_tooltip())
            } else {
                None
            };
            let mut apply_clicked = false;
            let mut ok_clicked = false;
            let mut cancel_clicked = false;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                cancel_clicked = ui
                    .add_enabled(
                        buttons.cancel_enabled,
                        egui::Button::new(rust_i18n::t!("bottom.cancel")),
                    )
                    .clicked();
                let ok_response = ui.add_enabled(
                    buttons.ok_enabled,
                    egui::Button::new(rust_i18n::t!("bottom.ok")),
                );
                let apply_response = ui.add_enabled(
                    buttons.apply_enabled,
                    egui::Button::new(rust_i18n::t!("bottom.apply")),
                );
                // Only a blocked save gets a hint: a clean draft is disabled
                // for dirtiness alone and needs no tooltip. The tooltip method
                // consumes the response, so it is rebound.
                let ok_response = if let Some(tooltip) = &blocked_tooltip {
                    ok_response.on_disabled_hover_text(tooltip)
                } else {
                    ok_response
                };
                let apply_response = if let Some(tooltip) = &blocked_tooltip {
                    apply_response.on_disabled_hover_text(tooltip)
                } else {
                    apply_response
                };
                ok_clicked = ok_response.clicked();
                apply_clicked = apply_response.clicked();
                if chrome::is_dirty(&self.draft, &self.applied) {
                    ui.label(rust_i18n::t!("bottom.unsaved_changes"));
                }
            });

            if apply_clicked {
                // Apply: save + re-register + publish (§4.4); the window stays open.
                self.apply();
            }
            if ok_clicked {
                // OK: the same `apply()` as Apply, then a real close (§4.4) —
                // not the hide-to-tray the X button performs.
                self.apply();
                self.request_window_exit(ui.ctx());
            }
            if cancel_clicked {
                // Cancel: the draft is discarded — nothing is saved or
                // published — any bind capture is cancelled, and the window
                // closes the same way as OK (§4.4).
                self.draft = chrome::cancel_draft(&self.draft, &self.applied);
                self.cancel_active_capture();
                self.request_window_exit(ui.ctx());
            }
        });

        // Tab content fills whatever height the docked bottom bar left.
        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                for tab in [Tab::General, Tab::Hotkeys, Tab::Overlay] {
                    if ui.selectable_label(self.tab == tab, tab.label()).clicked() {
                        self.tab = tab;
                    }
                }
            });
            ui.separator();

            match self.tab {
                Tab::General => self.general_tab(ui),
                Tab::Hotkeys => self.hotkeys_tab(ui),
                Tab::Overlay => self.overlay_tab(ui),
            }
        });
    }
}

#[cfg(test)]
#[path = "app.tests.rs"]
mod tests;
