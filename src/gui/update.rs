//! Auto-update UI (task 2026-11-11_auto-update, Stages 6-11, spec §4.7): the
//! semantics behind the update area of the settings window — the General-tab
//! update row (visibility, the forced «Проверить обновления» button, the
//! beside-button result line, the install action), the install lifecycle state
//! (`UpdateStatus`), the update child viewport (release notes fetched by
//! `update::notes` with the `release.yaml` summary + release-page link as the
//! fallback, Install/Cancel presses RECORDED in the child pass and GATED in
//! the root pass), and the discovery/failure toast chips (success chips follow
//! the overlay toggle, danger chips always show, §4.3). All decisions are pure
//! functions so they are unit-testable without an `egui::Context`.
//!
//! The pump lives in `gui/app.rs`: `App::logic` drives the scheduled checks,
//! drains the check/install/notes channels, pumps the dialog viewport and
//! performs the install→restart contract. This module owns the rules, the
//! dialog window lifecycle and its paint, and the `SettingsApp` methods the
//! row and the pump call. Extracted from `gui/mod.rs` (GUI refactor).

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::Instant;

// The `gui::update` child module shadows nothing inside this file, but the
// crate module `crate::update` is imported explicitly so bare `update::` paths
// here mean the auto-update backend, not this GUI module.
use overlay::Corner;
use toast::MonitorSel;
use toast::Toast;
use toast::ToastColor;

use crate::capture;
use crate::config;
use crate::config::Overlay;
use crate::config::OverlayPosition;
use crate::update;
use crate::update::install;
use crate::update::notes;

use super::SettingsApp;

/// Install lifecycle state for the update row and dialog (task
/// 2026-11-11_auto-update Stage 6, spec §4.7). `Clone` is justified: the
/// deferred viewport callback is `'static` and needs an owned snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UpdateStatus {
    /// No install running; the install action is offered.
    Idle,
    /// The install thread is running; the button is disabled.
    Installing,
    /// The install failed; the row shows the verbatim English message.
    Failed(String),
    /// The binary was replaced; the process must restart to run it. Stage 6
    /// only RECORDS this — the restart wiring lands in Stage 7 (`main.rs`).
    RestartPending,
}

/// Which dialog button the paint pass recorded; `logic` gates the effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UpdateDialogButton {
    Install,
    Cancel,
}

/// The gated result of a dialog button press. No `PartialEq`: `UpdateInfo`
/// has no equality derive (Stage 3 shipped it as `Debug, Clone`), so tests
/// match on the variant and compare the short fields.
#[derive(Debug, Clone)]
pub(crate) enum UpdateDialogOutcome {
    /// Hand the discovered update to the install path (owned: the install
    /// thread needs it across the thread boundary).
    Install(update::UpdateInfo),
    /// Close the dialog, install nothing.
    Dismiss,
}

/// Visibility of the update row in the General tab (spec §4.7): a held
/// discovery is enough. Neither the interval nor dev-ness gates it — a
/// discovery may come from the forced «Проверить обновления» button, which
/// works with ANY interval including `Never`, and a `-dev` build checks
/// updates on the same terms as a release build.
pub(crate) fn update_row_visible(info: Option<&update::UpdateInfo>) -> bool {
    info.is_some()
}

/// Enabled state of the forced-check button (spec §4.7): exactly ONE check may
/// be in flight, so the button is disabled while `check_in_flight` is set.
pub(crate) fn check_now_button_enabled(check_in_flight: bool) -> bool {
    !check_in_flight
}

/// Whether the ONE background-check slot is occupied after a logic pass has
/// armed the interval deadline (if it had to) and drained the check outcomes
/// (spec §4.7: exactly one check in flight at a time, and the button gate
/// reads this flag).
///
/// - `slot_busy`: the slot was already occupied at the start of the pass — a
///   scheduled or forced check spawned by an earlier pass, whose deadline was
///   consumed at spawn (`next_check = None`).
/// - `outcome_drained`: this pass drained an outcome, which ENDS the check that
///   produced it, so the slot is free afterwards whatever was armed.
/// - `startup_deadline`: the deadline armed on the FIRST pass. It represents
///   the startup check `main.rs` spawned before the event loop (both are the
///   same interval gate: a `Never` interval arms nothing and spawns nothing),
///   so it means "in flight" ONLY on that pass — an arm of a later pass is a
///   FUTURE check, not a running one.
///
/// The pass-1 order matters: arming AFTER the drain put `check_in_flight` back
/// ON after the startup outcome had already drained (a fast failure such as an
/// HTTP 404, or a slow window init), and nothing ever cleared it again — no
/// scheduled check fired and «Проверить обновления» stayed disabled for the
/// whole session.
pub(crate) fn check_in_flight_after(
    slot_busy: bool,
    outcome_drained: bool,
    startup_deadline: Option<Instant>,
) -> bool {
    if outcome_drained {
        false
    } else {
        slot_busy || startup_deadline.is_some()
    }
}

/// Whether a button press starts a forced check (spec §4.7): the interval
/// setting is IGNORED — `Never` still runs a check — while an in-flight check
/// blocks a second one. `interval` is passed for the record, not for the
/// decision; the scheduled deadline stays untouched either way (`logic` never
/// re-arms it on a forced outcome).
pub(crate) fn force_check_requested(_interval: config::UpdateCheck, check_in_flight: bool) -> bool {
    !check_in_flight
}

/// Whether a drained outcome may re-arm the interval deadline (spec §4.7): only
/// a SCHEDULED outcome moves the deadline; a FORCED outcome leaves it exactly
/// where it was, so a manual «Проверить обновления» never shifts the background
/// schedule.
pub(crate) fn deadline_rearmed_by_outcome(forced_check_in_flight: bool) -> bool {
    !forced_check_in_flight
}

/// What one logic pass does with the forced-check press the paint recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ForcedCheckDecision {
    /// Spawn the forced check on this pass and clear the press record.
    pub(crate) spawn: bool,
    /// Keep the press recorded for a later pass: the single slot is busy — a
    /// scheduled check may have taken it on THIS very pass — so the press must
    /// not be dropped; it runs as soon as the slot frees.
    pub(crate) pending: bool,
}

/// Forced-check press decision (spec §4.7): honored while the single slot is
/// free (the interval is ignored — see [`force_check_requested`]) and kept
/// PENDING while the slot is busy, so a press recorded during paint survives a
/// pass in which the scheduled spawn consumed the slot.
pub(crate) fn forced_check_decision(
    interval: config::UpdateCheck,
    pressed: bool,
    check_in_flight: bool,
) -> ForcedCheckDecision {
    let honored = pressed && force_check_requested(interval, check_in_flight);
    ForcedCheckDecision {
        spawn: honored,
        pending: pressed && !honored,
    }
}

/// The background-check deadline after `apply()` (spec §4.7): the APPLIED
/// interval governs background checks, so saving settings moves the schedule —
/// a scheduled interval arms `now + interval` (an interval switch such as
/// 1h→1d re-arms to the NEW interval, and `никогда`→scheduled arms the
/// schedule without a restart), and `Never` clears the deadline so no extra
/// check fires after the user turned checks off. A check already in flight is
/// never cancelled: this decides the deadline only — the caller leaves
/// `check_in_flight`/`forced_check_in_flight` untouched.
pub(crate) fn schedule_after_apply(now: Instant, interval: config::UpdateCheck) -> Option<Instant> {
    update::next_check_deadline(now, interval.interval())
}

/// The install action is offered in exactly ONE state: Idle. `Installing`
/// must not start a second install thread; `Failed` keeps the button hidden
/// so the row shows the error text instead (spec §4.7).
pub(crate) fn install_button_enabled(status: &UpdateStatus) -> bool {
    matches!(status, UpdateStatus::Idle)
}

/// The failure text the row shows INSTEAD of the button: the verbatim
/// English `InstallError` display, no locale key — technical error text
/// stays English (precedent: `capture::chip_text`). `None` outside Failed.
pub(crate) fn install_failure_hint(status: &UpdateStatus) -> Option<&str> {
    match status {
        UpdateStatus::Failed(message) => Some(message.as_str()),
        UpdateStatus::Idle | UpdateStatus::Installing | UpdateStatus::RestartPending => None,
    }
}

/// The install request behind an Install press. The owned `UpdateInfo` is a
/// justified clone: the install thread moves it across the thread boundary
/// while app state keeps the discovery for the row. Nothing is requested
/// while `Installing`/`Failed`/`RestartPending` or without a held discovery.
pub(crate) fn install_request(
    status: &UpdateStatus,
    info: Option<&update::UpdateInfo>,
) -> Option<update::UpdateInfo> {
    if install_button_enabled(status) {
        info.cloned()
    } else {
        None
    }
}

/// Gate a dialog button press into its effect. Presses land in the outcome
/// slot during the CHILD paint pass; `logic` gates them in the root pass —
/// the same one-frame ordering as the About close flag. Cancel always
/// dismisses; Install only requests while Idle with a held discovery (the
/// button is disabled in other states; this gate is the second defense).
pub(crate) fn dialog_outcome(
    button: UpdateDialogButton,
    status: &UpdateStatus,
    info: Option<&update::UpdateInfo>,
) -> Option<UpdateDialogOutcome> {
    match button {
        UpdateDialogButton::Cancel => Some(UpdateDialogOutcome::Dismiss),
        UpdateDialogButton::Install => {
            install_request(status, info).map(UpdateDialogOutcome::Install)
        }
    }
}

/// Auto-install path (spec §4.7): with `auto_install` ON, a discovery starts
/// the install immediately — no dialog, no row. The caller passes the
/// APPLIED flag (§4.4: published settings drive behavior).
pub(crate) fn auto_install_on_discovery(info_present: bool, auto_install: bool) -> bool {
    info_present && auto_install
}

/// Gate for the discovery chip, mirroring `capture::should_show` (§4.3):
/// success-style chips show only while the overlay is enabled. Failure
/// chips are ALWAYS shown and never pass through this gate.
pub(crate) fn update_toast_enabled(
    info: Option<&update::UpdateInfo>,
    overlay_enabled: bool,
) -> bool {
    info.is_some() && overlay_enabled
}

/// State transition from the install thread's single outcome. `Ok` lands in
/// `RestartPending`: the binary IS replaced and the process must restart to
/// run it — Stage 6 only RECORDS that; Stage 7 wires it: `App::logic` sets
/// `restart_requested` and sends `ViewportCommand::Close`, and `main.rs`
/// drops the instance lock, spawns the new binary, and exits 0 (plus startup
/// `update::install::cleanup_leftovers` on every start).
/// `Err` keeps the verbatim message for the row and the danger chip.
pub(crate) fn install_finished(outcome: &Result<(), String>) -> UpdateStatus {
    match outcome {
        Ok(()) => UpdateStatus::RestartPending,
        Err(message) => UpdateStatus::Failed(message.clone()),
    }
}

/// Discovery after an install result: success clears it (the row disappears;
/// the restart banner is Stage 7's job), failure keeps it so the row can
/// show the error. Takes the discovery by value — returned or dropped,
/// never borrowed-and-cloned.
pub(crate) fn update_info_after_install(
    info: Option<update::UpdateInfo>,
    ok: bool,
) -> Option<update::UpdateInfo> {
    if ok { None } else { info }
}

/// State transition when a NEW discovery arrives (spec §4.7, Stage 7): a
/// discovery that lands while an install FAILURE is displayed re-opens the
/// install action — the failure text belongs to the attempt that is over, so
/// the state returns to `Idle` and the row offers Install again. `Installing`
/// is untouched (a running install must not be interrupted), `Idle` stays
/// `Idle`, and `RestartPending` stays: the binary is already replaced, so a
/// discovery there describes the very bytes the restart is about to run.
pub(crate) fn status_after_new_discovery(status: UpdateStatus) -> UpdateStatus {
    match status {
        UpdateStatus::Failed(_) => UpdateStatus::Idle,
        other => other,
    }
}

/// The result line painted next to «Проверить обновления» (task
/// 2026-11-11_auto-update Stage 11 sub-item 2, spec §4.7): ONLY forced-check
/// outcomes land here — scheduled/background results keep reporting through
/// the discovery chip and the General row as before. `None` before the first
/// forced check means no status text at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ForcedCheckStatus {
    /// The last forced check found nothing newer than the running build —
    /// the plain localized «Обновлений нет» (`update.no_updates_available`).
    NoUpdates,
    /// The last forced check failed: the verbatim English `UpdateError`
    /// Display, never localized (precedent: `capture::error_display`), drawn
    /// in the same warning styling as the install-failure hint.
    Failed(String),
}

/// The result line after ONE drained check outcome (sub-item 2): a
/// SCHEDULED outcome leaves the slot exactly as it was; a FORCED outcome
/// REPLACES it — `UpToDate` → «Обновлений нет», `Failed` → the verbatim
/// `UpdateError` Display, `Updated` → cleared, because the discovery row
/// shows that result and a stale line must never sit beside a fresh
/// «Доступно обновление». Takes the slot by value — returned or dropped,
/// never borrowed-and-cloned.
pub(crate) fn forced_check_status_after(
    slot: Option<ForcedCheckStatus>,
    outcome: &update::CheckOutcome,
    forced: bool,
) -> Option<ForcedCheckStatus> {
    if !forced {
        return slot;
    }
    match outcome {
        update::CheckOutcome::UpToDate => Some(ForcedCheckStatus::NoUpdates),
        update::CheckOutcome::Failed(err) => Some(ForcedCheckStatus::Failed(err.to_string())),
        update::CheckOutcome::Updated(_) => None,
    }
}

/// What the paint pass renders in the slot: NOTHING while a FORCED check is
/// in flight (the slot stays untouched until its outcome drains — no
/// half-result while the button is disabled), otherwise the held line.
pub(crate) fn forced_check_status_shown(
    slot: Option<&ForcedCheckStatus>,
    forced_check_in_flight: bool,
) -> Option<&ForcedCheckStatus> {
    if forced_check_in_flight { None } else { slot }
}

/// Stale-discovery reset (sub-item 4, spec §4.7): a check outcome REPLACES
/// the previous result. An up-to-date outcome — FORCED or scheduled — clears
/// the held discovery (nothing newer exists anymore); a failed FORCED check
/// clears it too (the user's bug: a nightly discovery kept «Доступно
/// обновление» after switching the channel to `stable` and the forced check
/// answering 404); a failed SCHEDULED check KEEPS it (a transient
/// network/rate-limit blip must not erase an actionable update — the next
/// interval retries). `Updated` never clears here: the drain replaces the
/// discovery with the new one (or auto-install consumes it). While an
/// install is in progress NOTHING ever clears: the running install owns its
/// discovery, and a failure that lands afterwards keeps the row for the
/// retry (`update_info_after_install`).
pub(crate) fn discovery_cleared_by_check_outcome(
    outcome: &update::CheckOutcome,
    forced: bool,
    install_status: &UpdateStatus,
) -> bool {
    if matches!(install_status, UpdateStatus::Installing) {
        return false;
    }
    match outcome {
        update::CheckOutcome::UpToDate => true,
        update::CheckOutcome::Failed(_) => forced,
        update::CheckOutcome::Updated(_) => false,
    }
}

/// Lifecycle of the update child viewport — the exact shape of
/// `about::AboutWindow` (task 2026-11-10 stage 10): an OS close request is
/// visible ONLY in the child viewport's input pass, so the paint records it
/// into a shared flag; the next logic pass sees the flag, stops
/// registration, egui GCs the viewport, and eframe drops the `Arc<Window>`
/// (`crates/overlay/src/host.rs:313-323`). Not reusing `about::AboutWindow`:
/// its name and tray semantics belong to the About module (SRP), and this
/// dialog additionally needs the Cancel-button close path.
pub(crate) struct UpdateDialogWindow {
    open: bool,
    close_flag: Arc<AtomicBool>,
}

impl UpdateDialogWindow {
    /// Closed state with an inert flag; opening replaces the flag, so a
    /// stale flag is never handed to a paint callback.
    pub(crate) fn new() -> Self {
        Self {
            open: false,
            close_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Open with a FRESH close flag; returns whether this call newly opened
    /// (a re-open of an already-open viewport must not inherit state — same
    /// rule as `about::AboutWindow::open`).
    fn open(&mut self) -> bool {
        if self.open {
            return false;
        }
        self.close_flag = Arc::new(AtomicBool::new(false));
        self.open = true;
        true
    }

    /// Cancel-button close: no OS request happened, but the next pass must
    /// stop registration like one did (egui GCs the viewport).
    fn close(&mut self) {
        self.open = false;
    }

    /// Observe the shared flag: true (and now closed) when the child pass
    /// recorded an OS close request since the last observation.
    pub(crate) fn observe_close(&mut self) -> bool {
        if self.open && self.close_flag.load(Ordering::Relaxed) {
            self.open = false;
            return true;
        }
        false
    }

    /// Whether the logic pass must call `show_viewport_deferred` for this
    /// viewport (stopping registration = GC, see the struct doc).
    pub(crate) fn wants_registration(&self) -> bool {
        self.open
    }

    /// Close-flag handle for the paint callback, `None` when closed. The
    /// `Arc` clone is O(1) — one refcount bump, shared with app state.
    pub(crate) fn close_flag(&self) -> Option<Arc<AtomicBool>> {
        if self.open {
            Some(Arc::clone(&self.close_flag))
        } else {
            None
        }
    }
}

/// What the update dialog paints on a pass, built from the snapshots the
/// deferred callback owns, so the callback stays `'static`.
pub(crate) struct UpdateDialogView<'a> {
    /// `None` = fetch pending (loading hint); `Some` = arrived result.
    pub(crate) notes: Option<&'a Result<Arc<notes::ReleaseNotes>, notes::NotesError>>,
    /// Held discovery: version line + fallback summary. `None` only in the
    /// auto-install path, which never opens the dialog.
    pub(crate) info: Option<&'a update::UpdateInfo>,
    /// APPLIED channel, for the release-page fallback URL.
    pub(crate) channel: config::UpdateChannel,
    /// Install-state snapshot driving the button's enabled flag.
    pub(crate) status: &'a UpdateStatus,
}

/// Paint the update dialog in its deferred child viewport — the exact About
/// pattern (task 2026-11-10 stage 10, verified against the locked egui 0.37
/// sources): record an OS close request into the shared flag (only the child
/// pass sees it), fill an OPAQUE background (the app's clear color is alpha-0
/// and `Frame::central_panel` covers the whole viewport), render notes with
/// the scrollable `egui_commonmark` viewer (`viewport_cache(true)`, viewer
/// `!Send` → constructed per paint), and end with the install/cancel buttons
/// (spec §4.7). NOT modal: the settings window keeps working while the
/// dialog is open.
pub(crate) fn paint_update_dialog(
    ui: &mut egui::Ui,
    cache: &mut egui_commonmark::CommonMarkCache,
    close_flag: &AtomicBool,
    slot: &Mutex<Option<UpdateDialogButton>>,
    view: &UpdateDialogView<'_>,
) {
    if ui.input(|i| i.viewport().close_requested()) {
        close_flag.store(true, Ordering::Relaxed);
    }
    let frame = egui::Frame::central_panel(ui.style()).inner_margin(egui::Margin::symmetric(16, 8));
    egui::CentralPanel::default().frame(frame).show(ui, |ui| {
        if let Some(info) = view.info {
            // The discovered version is a technical value kept literal
            // (About build-line convention), so no locale key here.
            ui.label(
                egui::RichText::new(info.version.as_str())
                    .size(18.0)
                    .strong(),
            );
        }
        ui.separator();
        match view.notes {
            None => {
                ui.label(rust_i18n::t!("update.notes_loading"));
            }
            Some(Ok(notes)) if !notes.body.is_empty() => {
                egui_commonmark::CommonMarkViewer::new().show_scrollable(
                    egui::Id::unique("virshot-update-notes"),
                    ui,
                    cache,
                    &egui_commonmark::CommonMarkScrollOptions::default().viewport_cache(true),
                    &notes.body,
                );
            }
            // Fallback (spec §4.7): an empty body or a fetch failure shows
            // the release.yaml summary plus the release-page link — the
            // technical values stay literal and the URL is verbatim (About
            // repo-link precedent).
            Some(Ok(_)) | Some(Err(_)) => {
                ui.label(rust_i18n::t!("update.notes_unavailable"));
                if let Some(info) = view.info {
                    ui.label(format!(
                        "{} · {} · {}",
                        info.version, info.release_date, info.commit_sha
                    ));
                }
                let url = view
                    .notes
                    .and_then(|result| result.as_ref().ok())
                    .map(|notes| notes.html_url.as_str())
                    .unwrap_or_else(|| notes::release_page_url(view.channel));
                ui.hyperlink(url);
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
            let install_clicked = ui
                .add_enabled(
                    install_button_enabled(view.status),
                    egui::Button::new(rust_i18n::t!("general.install_update_button")),
                )
                .clicked();
            let cancel_clicked = ui.button(rust_i18n::t!("bottom.cancel")).clicked();
            if install_clicked || cancel_clicked {
                // RECORDED, not acted on: the root logic pass gates the press
                // against current state (`dialog_outcome`).
                let button = if install_clicked {
                    UpdateDialogButton::Install
                } else {
                    UpdateDialogButton::Cancel
                };
                *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(button);
            }
        });
    });
}

/// Chip geometry from the APPLIED overlay settings: the same monitor /
/// corner / clamped-hold mapping `capture::toast_from_outcome` applies to
/// real chips. Those helpers are private in capture, so this duplicates the
/// three-line mapping locally instead of widening capture's interface
/// (precedent: `test_overlay_toast`).
fn update_chip_geometry(overlay: &Overlay) -> (MonitorSel, Corner, u32) {
    let monitor = if overlay.on_all_monitors {
        MonitorSel::All
    } else {
        MonitorSel::Index(0)
    };
    let corner = match overlay.position {
        OverlayPosition::TopLeft => Corner::TopLeft,
        OverlayPosition::TopRight => Corner::TopRight,
        OverlayPosition::BottomLeft => Corner::BottomLeft,
        OverlayPosition::BottomRight => Corner::BottomRight,
    };
    let duration_ms = u32::try_from(overlay.duration_ms).unwrap_or(u32::MAX);
    (monitor, corner, duration_ms)
}

/// Discovery chip (success style — gated by `update_toast_enabled` BEFORE
/// this is built): localized announcement with the version interpolated, the
/// same `%{...}` mechanism as the save chip's file name.
pub(crate) fn update_discovery_chip(overlay: &Overlay, info: &update::UpdateInfo) -> Toast {
    let (monitor, corner, duration_ms) = update_chip_geometry(overlay);
    Toast {
        monitor,
        text: rust_i18n::t!("update.discovered_message", version = info.version.as_str())
            .into_owned(),
        image: None,
        color: ToastColor::Success,
        corner,
        duration_ms,
    }
}

/// Install-failure chip: ALWAYS shown (danger chips ignore the overlay
/// toggle, §4.3 `should_show`) with the verbatim English error text — no
/// locale key for technical failures (capture::chip_text precedent).
pub(crate) fn update_failure_chip(overlay: &Overlay, message: &str) -> Toast {
    let (monitor, corner, duration_ms) = update_chip_geometry(overlay);
    Toast {
        monitor,
        text: message.to_owned(),
        image: None,
        color: ToastColor::Danger,
        corner,
        duration_ms,
    }
}

impl SettingsApp {
    /// Open the update dialog from the General row (spec §4.7): exactly the
    /// About lazy pattern — fresh close flag, fresh outcome slot, and the
    /// notes fetch thread is spawned HERE and nowhere else (a background
    /// check never pays for notes nobody reads). Re-opening an open dialog
    /// is a no-op (`UpdateDialogWindow::open` returns false).
    pub(crate) fn open_update_dialog(&mut self) {
        if !self.update_dialog.open() {
            return;
        }
        self.dialog_outcome_slot = Arc::new(Mutex::new(None));
        self.notes = None;
        let (tx, rx) = mpsc::channel::<Result<notes::ReleaseNotes, notes::NotesError>>();
        self.notes_rx = Some(rx);
        // Notes fetch (Stage 7 seam re-point, spec §4.7): the seam is
        // `Fn(&str) -> Result<ReleaseNotes, NotesError>`, so the production
        // fetcher is `update::notes::fetch_release_notes` itself — fetch AND
        // parse belong to `update::notes`, and this wiring point passes the fn
        // item with no error mapping (SRP: `update` owns the `release.yaml`
        // contract, `update::notes` owns the GitHub Releases API contract).
        notes::spawn_notes_fetch(
            self.applied.general.update_channel,
            tx,
            notes::fetch_release_notes,
        );
    }

    /// Close the dialog from the Cancel button (an OS close goes through
    /// `observe_close` in `logic` instead): stop registration next pass —
    /// egui GCs the viewport and eframe drops the `Arc<Window>` — and drop
    /// the notes state, so a still-pending fetch send is ignored by the
    /// closed receiver (module contract) and a re-open fetches fresh.
    pub(crate) fn dismiss_update_dialog(&mut self) {
        self.update_dialog.close();
        self.notes_rx = None;
        self.notes = None;
    }

    /// Start the install path (spec §4.7 — auto-install on discovery or a
    /// dialog Install press): ONE plain std thread (repo thread model, no
    /// async) running `update::install::install` with
    /// `self_replace::self_replace` as the replace seam. The single result
    /// arrives over a dedicated channel drained in `logic`; nothing here
    /// restarts or exits — `RestartPending` is the Stage 7 seam (see
    /// [`install_finished`]).
    pub(crate) fn start_install(&mut self, info: update::UpdateInfo) {
        let (tx, rx) = mpsc::channel::<Result<(), String>>();
        self.install_rx = Some(rx);
        self.update_status = UpdateStatus::Installing;
        std::thread::spawn(move || {
            // `InstallError`'s Display text travels verbatim: the row and
            // the danger chip show the real OS/network failure wording,
            // exactly as `capture::chip_text` does for capture failures.
            // A closure (not the bare fn item) is required here: `install`'s
            // replace seam is higher-ranked (`for<'a> Fn(&'a Path)`), and a
            // generic fn item instantiated at the call site is specialized to
            // one lifetime — the closure stays generic over the argument.
            let outcome = install::install(&info, |path| self_replace::self_replace(path))
                .map_err(|err| err.to_string());
            let _ = tx.send(outcome);
        });
    }

    /// Publish an update chip through the SAME hand-off slot real saves use
    /// (spec §4.3): the handle never blocks, and the chip spawns on the next
    /// `overlay_host.update` pass in `logic`. A missing handle (no toast
    /// service) or a closed channel is WARNed, never fatal (spec §5) — the
    /// update result itself is what app state keeps.
    pub(crate) fn publish_update_chip(&self, chip: Toast) {
        let Some(handle) = capture::handoff_handle(&self.handoff) else {
            tracing::warn!("Update chip skipped: no toast handle attached");
            return;
        };
        let text = chip.text.clone(); // `show` consumes the chip; the log quotes it
        if let Err(err) = handle.show(chip) {
            tracing::warn!("Update chip failed: {err}");
            return;
        }
        tracing::info!("Update chip shown: {text}");
    }
}

#[cfg(test)]
#[path = "update.tests.rs"]
mod tests;
