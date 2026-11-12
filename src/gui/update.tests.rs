use super::*;

use std::time::Duration;

// ── Auto-update UI decisions (task 2026-11-11_auto-update, Stage 6) ──
// Pure state decisions only: injected values, no network, no self-replace,
// no threads. The wiring in `SettingsApp` (row visibility, dialog buttons,
// discovery toast gate, install-thread result transitions) goes through
// exactly these functions, so each one mirrors one decision in `logic`.

/// A discovery snapshot for the decision tests — every field is a fixed
/// literal, nothing is read from the environment.
fn sample_update_info() -> crate::update::UpdateInfo {
    crate::update::UpdateInfo {
        version: "0.7.0".to_owned(),
        release_date: "2026-11-11T12:00:00.000Z".to_owned(),
        commit_sha: "abc1234".to_owned(),
        archive_url:
            "https://github.com/virshot/virshot/releases/download/v0.7.0/virshot-v0.7.0-win-x64.zip"
                .to_owned(),
        sha512: "AAAA".to_owned(),
        size: 4_200_000,
    }
}

#[test]
fn update_row_visible_with_discovery_shows_row() {
    assert!(
        update_row_visible(Some(&sample_update_info())),
        "a held discovery must show the row"
    );
}

#[test]
fn update_row_visible_never_interval_forced_discovery_shows_row() {
    // A discovery can come from the forced «Проверить обновления» button,
    // which works with ANY interval (spec §4.7), so the interval no longer
    // gates the row.
    assert!(
        update_row_visible(Some(&sample_update_info())),
        "a discovery from a forced check must show the row with the interval off"
    );
}

#[test]
fn update_row_visible_dev_build_shows_row() {
    // A `-dev` build checks updates on the same terms as a release build
    // (spec §4.7), so its discovery gets the row too.
    assert!(
        update_row_visible(Some(&sample_update_info())),
        "a dev build's discovery must show the update row"
    );
}

#[test]
fn update_row_visible_no_info_hides_row() {
    assert!(
        !update_row_visible(None),
        "without a discovery there is nothing to show"
    );
}

#[test]
fn check_now_button_enabled_when_idle() {
    assert!(
        check_now_button_enabled(false),
        "the forced-check button is enabled while no check runs"
    );
}

#[test]
fn check_now_button_enabled_while_in_flight_disabled() {
    assert!(
        !check_now_button_enabled(true),
        "exactly one check may be in flight, so the button is disabled while one runs"
    );
}

#[test]
fn force_check_requested_never_interval_still_requests() {
    // The button ignores the interval setting (spec §4.7): `Never` still runs
    // a forced check.
    assert!(
        force_check_requested(crate::config::UpdateCheck::Never, false),
        "a forced check must run even when scheduled checks are off"
    );
    assert!(
        force_check_requested(crate::config::UpdateCheck::Week, false),
        "a forced check must run on every interval"
    );
}

#[test]
fn force_check_requested_while_in_flight_skips() {
    assert!(
        !force_check_requested(crate::config::UpdateCheck::Never, true),
        "a second check must not be spawned while one is in flight"
    );
}

#[test]
fn install_button_enabled_when_idle() {
    assert!(
        install_button_enabled(&UpdateStatus::Idle),
        "idle is the only state that offers the install action"
    );
}

#[test]
fn install_button_disabled_while_installing() {
    assert!(
        !install_button_enabled(&UpdateStatus::Installing),
        "a running install must not offer a second one"
    );
}

#[test]
fn install_button_disabled_after_failure() {
    let message = "failed to replace running binary: Permission denied (os error 5)";
    let status = UpdateStatus::Failed(message.to_owned());
    assert!(
        !install_button_enabled(&status),
        "a failed install must not re-offer the button in the same state"
    );
    assert_eq!(
        install_failure_hint(&status),
        Some(message),
        "the row must show the verbatim English error text instead of the button"
    );
    assert_eq!(
        install_failure_hint(&UpdateStatus::Idle),
        None,
        "idle has no failure text to show"
    );
    assert_eq!(
        install_failure_hint(&UpdateStatus::Installing),
        None,
        "a running install has no failure text yet"
    );
    assert_eq!(
        install_failure_hint(&UpdateStatus::RestartPending),
        None,
        "a finished install shows no failure text"
    );
}

#[test]
fn dialog_cancel_dismisses_without_install() {
    assert!(
        matches!(
            dialog_outcome(
                UpdateDialogButton::Cancel,
                &UpdateStatus::Idle,
                Some(&sample_update_info())
            ),
            Some(UpdateDialogOutcome::Dismiss)
        ),
        "Cancel closes the dialog and installs nothing"
    );
    assert!(
        matches!(
            dialog_outcome(
                UpdateDialogButton::Cancel,
                &UpdateStatus::Installing,
                Some(&sample_update_info())
            ),
            Some(UpdateDialogOutcome::Dismiss)
        ),
        "Cancel closes regardless of the install state"
    );
}

#[test]
fn dialog_install_request_only_when_idle() {
    // `UpdateInfo` ships `Debug, Clone` with no `PartialEq` (Stage 3), so the
    // transfer is checked by variant plus the short fields the install thread
    // actually uses.
    match dialog_outcome(
        UpdateDialogButton::Install,
        &UpdateStatus::Idle,
        Some(&sample_update_info()),
    ) {
        Some(UpdateDialogOutcome::Install(info)) => {
            assert_eq!(info.version, "0.7.0");
            assert_eq!(info.commit_sha, "abc1234");
            assert_eq!(
                info.archive_url,
                "https://github.com/virshot/virshot/releases/download/v0.7.0/virshot-v0.7.0-win-x64.zip"
            );
        }
        other => {
            panic!("idle + discovery must hand the update info to the install path, got {other:?}")
        }
    }
    assert!(
        dialog_outcome(
            UpdateDialogButton::Install,
            &UpdateStatus::Installing,
            Some(&sample_update_info())
        )
        .is_none(),
        "a second Install press while Installing must request nothing"
    );
    assert!(
        dialog_outcome(UpdateDialogButton::Install, &UpdateStatus::Idle, None).is_none(),
        "Install with no held discovery requests nothing"
    );
    assert!(
        dialog_outcome(
            UpdateDialogButton::Install,
            &UpdateStatus::Failed("boom".to_owned()),
            Some(&sample_update_info())
        )
        .is_none(),
        "Install after a failure requests nothing (the row shows the error instead)"
    );
}

#[test]
fn auto_install_on_discovery_true_only_when_auto_enabled() {
    assert!(
        auto_install_on_discovery(true, true),
        "auto-install ON installs straight away on discovery (spec §4.7)"
    );
    assert!(
        !auto_install_on_discovery(true, false),
        "auto-install OFF keeps the dialog path"
    );
    assert!(
        !auto_install_on_discovery(false, true),
        "nothing is installed without a discovery"
    );
    assert!(
        !auto_install_on_discovery(false, false),
        "nothing is installed without a discovery and without auto-install"
    );
}

#[test]
fn update_toast_enabled_follows_overlay_toggle() {
    assert!(
        update_toast_enabled(Some(&sample_update_info()), true),
        "the discovery chip shows while the overlay is enabled, like the save toast (§4.3)"
    );
    assert!(
        !update_toast_enabled(Some(&sample_update_info()), false),
        "overlay disabled suppresses the success-style discovery chip (§4.3 should_show)"
    );
    assert!(!update_toast_enabled(None, true), "no discovery, no chip");
}

#[test]
fn discovery_clears_after_successful_install() {
    assert_eq!(
        install_finished(&Ok(())),
        UpdateStatus::RestartPending,
        "a successful install ends in the restart-pending state"
    );
    assert_eq!(
        install_finished(&Err(
            "failed to replace running binary: os error 5".to_owned()
        )),
        UpdateStatus::Failed("failed to replace running binary: os error 5".to_owned()),
        "a failed install ends in the Failed state carrying the verbatim message"
    );
    assert!(
        update_info_after_install(Some(sample_update_info()), true).is_none(),
        "success clears the discovery, so the row disappears"
    );
    let kept = update_info_after_install(Some(sample_update_info()), false);
    assert!(
        kept.as_ref().is_some_and(|info| info.version == "0.7.0"
            && info.archive_url
                == "https://github.com/virshot/virshot/releases/download/v0.7.0/virshot-v0.7.0-win-x64.zip"),
        "failure keeps the discovery so the row can show the error"
    );
    assert!(
        update_info_after_install(None, true).is_none(),
        "the auto-install path has no held discovery to clear"
    );
}

#[test]
fn new_discovery_after_failure_restores_install_button() {
    // Stage 7 (spec §4.7): a NEW discovery arriving while a failed install is
    // shown makes the row actionable again — the failure text belongs to the
    // OLD attempt, so the state returns to Idle and Install is offered.
    assert_eq!(
        status_after_new_discovery(UpdateStatus::Failed("download failed".to_owned())),
        UpdateStatus::Idle,
        "a fresh discovery clears a stale install failure"
    );
    assert!(
        install_button_enabled(&status_after_new_discovery(UpdateStatus::Failed(
            "download failed".to_owned()
        ))),
        "the row must offer Install again after a fresh discovery"
    );
    assert_eq!(
        status_after_new_discovery(UpdateStatus::Idle),
        UpdateStatus::Idle,
        "Idle stays Idle"
    );
    assert_eq!(
        status_after_new_discovery(UpdateStatus::Installing),
        UpdateStatus::Installing,
        "a running install must not be interrupted by a new discovery"
    );
    assert_eq!(
        status_after_new_discovery(UpdateStatus::RestartPending),
        UpdateStatus::RestartPending,
        "a finished install stays restart-pending"
    );
}

// ── Background-check slot + schedule decisions (task 2026-11-11_auto-update,
// Stage 9 acceptance fixes) ──
//
// The pass-1 arm/drain race, the forced-press gate and the Apply-time schedule
// are pure data: the helpers take the flags plus the injected clock, so all
// three rules are pinned WITHOUT an eframe context (`App::logic` needs a
// `Context` + `Frame`, which unit tests never build).

#[test]
fn first_pass_outcome_drained_before_arm_leaves_button_enabled() {
    // The startup check spawned by `main.rs` fails fast (HTTP 404) or the
    // window init is slow, so its outcome drains on the FIRST logic pass.
    // Arming the deadline AFTER the drain put `check_in_flight` back ON with
    // nothing running: the slot stayed occupied forever — scheduled checks
    // never fired and «Проверить обновления» stayed disabled all session.
    let startup_deadline =
        update::next_check_deadline(Instant::now(), crate::config::UpdateCheck::Day.interval());
    assert!(
        startup_deadline.is_some(),
        "a scheduled interval arms a deadline on the first pass"
    );
    let after_drain = check_in_flight_after(false, true, startup_deadline);
    assert!(
        !after_drain,
        "a drained startup outcome frees the slot even though this pass armed a deadline"
    );
    assert!(
        check_now_button_enabled(after_drain),
        "«Проверить обновления» stays enabled after the pass-1 drain"
    );
    assert!(
        check_in_flight_after(false, false, startup_deadline),
        "with no outcome drained, the first-pass deadline IS the startup check (§4.7)"
    );
    assert!(
        !check_in_flight_after(false, false, None),
        "a Never interval arms nothing and spawns nothing, so the slot stays free"
    );
}

#[test]
fn scheduled_spawn_keeps_one_check_in_flight() {
    // A scheduled spawn consumes its deadline (`next_check = None`) and holds
    // the single slot until the outcome drains: no second spawn and no forced
    // check can start meanwhile (§4.7 — exactly one check in flight).
    let running = check_in_flight_after(true, false, None);
    assert!(
        running,
        "a check spawned by an earlier pass stays in flight on a pass with no outcome"
    );
    assert!(
        !check_now_button_enabled(running),
        "the forced-check button is disabled while a scheduled check runs"
    );
    assert!(
        !check_in_flight_after(false, false, None),
        "a deadline armed on a LATER pass is a future check, not a running one"
    );
    let freed = check_in_flight_after(true, true, None);
    assert!(
        !freed,
        "the drained outcome ends the running check, so the slot frees"
    );
    assert!(
        check_now_button_enabled(freed),
        "the button re-enables as soon as the outcome drains"
    );
}

#[test]
fn forced_outcome_does_not_rearm_deadline() {
    // §4.7: a FORCED outcome never moves or re-arms the interval deadline;
    // only a SCHEDULED outcome re-arms it to the applied interval.
    let now = Instant::now();
    let armed = update::next_check_deadline(now, crate::config::UpdateCheck::Hour.interval());
    let deadline_after_forced: Option<Instant> = if deadline_rearmed_by_outcome(true) {
        update::next_check_deadline(now, crate::config::UpdateCheck::Week.interval())
    } else {
        armed
    };
    assert_eq!(
        deadline_after_forced, armed,
        "a forced outcome leaves the armed 1h deadline exactly where it was"
    );
    assert!(
        deadline_rearmed_by_outcome(false),
        "a scheduled outcome re-arms the deadline to the applied interval"
    );
    assert!(
        update::next_check_deadline(now, crate::config::UpdateCheck::Never.interval()).is_none(),
        "a Never interval re-arms nothing"
    );
}

#[test]
fn forced_press_while_slot_busy_stays_pending() {
    // A press the paint recorded while a scheduled spawn took the slot on the
    // SAME logic pass must not be discarded: it stays pending and runs as soon
    // as the slot frees (the old code cleared the request before the gate).
    assert_eq!(
        forced_check_decision(crate::config::UpdateCheck::Day, true, true),
        ForcedCheckDecision {
            spawn: false,
            pending: true
        },
        "a busy slot defers the press instead of swallowing it"
    );
    assert_eq!(
        forced_check_decision(crate::config::UpdateCheck::Day, true, false),
        ForcedCheckDecision {
            spawn: true,
            pending: false
        },
        "a free slot honors the press on this pass"
    );
    assert_eq!(
        forced_check_decision(crate::config::UpdateCheck::Never, true, false),
        ForcedCheckDecision {
            spawn: true,
            pending: false
        },
        "«никогда» still runs a forced check — the interval is ignored (§4.7)"
    );
    assert_eq!(
        forced_check_decision(crate::config::UpdateCheck::Never, true, true),
        ForcedCheckDecision {
            spawn: false,
            pending: true
        },
        "a busy slot defers a Never-interval press too"
    );
    assert_eq!(
        forced_check_decision(crate::config::UpdateCheck::Day, false, false),
        ForcedCheckDecision {
            spawn: false,
            pending: false
        },
        "no press, no effect"
    );
}

#[test]
fn apply_never_interval_clears_deadline() {
    let now = Instant::now();
    assert!(
        schedule_after_apply(now, crate::config::UpdateCheck::Day).is_some(),
        "a scheduled interval keeps the deadline armed through Apply"
    );
    assert!(
        schedule_after_apply(now, crate::config::UpdateCheck::Never).is_none(),
        "switching to «никогда» clears an armed deadline, so no extra check fires (§4.7)"
    );
}

#[test]
fn apply_scheduled_interval_arms_deadline() {
    let now = Instant::now();
    assert_eq!(
        schedule_after_apply(now, crate::config::UpdateCheck::Day),
        Some(now + Duration::from_secs(86_400)),
        "switching from «никогда» to a scheduled interval arms the background schedule at Apply — no restart needed (§4.7)"
    );
    assert_eq!(
        schedule_after_apply(now, crate::config::UpdateCheck::Hour),
        Some(now + Duration::from_secs(3600)),
        "the armed deadline follows the APPLIED interval, not the draft"
    );
}

#[test]
fn apply_interval_change_rearms_deadline_to_new_interval() {
    let now = Instant::now();
    let old_deadline = schedule_after_apply(now, crate::config::UpdateCheck::Hour);
    let new_deadline = schedule_after_apply(now, crate::config::UpdateCheck::Week);
    assert_eq!(
        new_deadline,
        Some(now + Duration::from_secs(604_800)),
        "a 1h→1w switch re-arms the deadline to the NEW interval"
    );
    assert!(
        new_deadline > old_deadline,
        "the switch must push the deadline out, not keep the stale 1h one"
    );
}

// ── Forced-check result line + stale-discovery reset (task
// 2026-11-11_auto-update, Stage 11 sub-items 2+4) ──
//
// Pure state decisions again — injected outcomes, flags, and install status,
// no eframe context, no threads, no network. `SettingsApp::logic` drains
// through exactly these helpers (mirroring how the Stage 9 slot tests pin the
// drain rules without ever building a `Context`).

/// The verbatim `UpdateError` Display of a stable-channel 404 — the exact
/// string the failed-forced-check line must show (never localized, the
/// `capture::error_display` precedent).
fn stable_404_error() -> crate::update::UpdateError {
    crate::update::UpdateError::HttpStatus(404)
}

const STABLE_404_TEXT: &str = "release metadata fetch returned HTTP status 404";

#[test]
fn forced_check_status_after_forced_up_to_date_shows_no_updates_line() {
    assert_eq!(
        forced_check_status_after(None, &crate::update::CheckOutcome::UpToDate, true),
        Some(ForcedCheckStatus::NoUpdates),
        "a forced up-to-date outcome sets the «Обновлений нет» line"
    );
    // Before ANY forced check the slot stays empty — no status text ever.
    assert_eq!(
        forced_check_status_after(None, &crate::update::CheckOutcome::UpToDate, false),
        None,
        "a scheduled outcome never fills the slot before the first forced check"
    );
}

#[test]
fn forced_check_status_after_forced_failure_shows_verbatim_error_text() {
    assert_eq!(
        forced_check_status_after(
            None,
            &crate::update::CheckOutcome::Failed(stable_404_error()),
            true
        ),
        Some(ForcedCheckStatus::Failed(STABLE_404_TEXT.to_owned())),
        "the line must carry the exact `UpdateError` Display, never localized"
    );
}

#[test]
fn forced_check_status_after_forced_discovery_clears_slot() {
    // The discovery itself replaces the result line: a stale «Обновлений нет»
    // or error text must never sit next to a fresh «Доступно обновление».
    assert_eq!(
        forced_check_status_after(
            Some(ForcedCheckStatus::NoUpdates),
            &crate::update::CheckOutcome::Updated(sample_update_info()),
            true
        ),
        None,
        "a forced discovery clears the result line"
    );
    assert_eq!(
        forced_check_status_after(
            Some(ForcedCheckStatus::Failed(STABLE_404_TEXT.to_owned())),
            &crate::update::CheckOutcome::Updated(sample_update_info()),
            true
        ),
        None,
        "a forced discovery clears the previous error text too"
    );
}

#[test]
fn forced_check_status_after_scheduled_outcome_keeps_slot() {
    // Background/scheduled results keep using the toast + General row (§4.7);
    // they never touch the beside-button slot in either direction.
    let held = Some(ForcedCheckStatus::Failed(STABLE_404_TEXT.to_owned()));
    assert_eq!(
        forced_check_status_after(held.clone(), &crate::update::CheckOutcome::UpToDate, false),
        held,
        "a scheduled UpToDate leaves the held forced line untouched"
    );
    assert_eq!(
        forced_check_status_after(
            held.clone(),
            &crate::update::CheckOutcome::Failed(stable_404_error()),
            false
        ),
        held,
        "a scheduled failure leaves the held forced line untouched"
    );
    assert_eq!(
        forced_check_status_after(
            held.clone(),
            &crate::update::CheckOutcome::Updated(sample_update_info()),
            false
        ),
        held,
        "a scheduled discovery leaves the held forced line untouched"
    );
}

#[test]
fn forced_failure_replaces_earlier_no_updates_line() {
    assert_eq!(
        forced_check_status_after(
            Some(ForcedCheckStatus::NoUpdates),
            &crate::update::CheckOutcome::Failed(stable_404_error()),
            true
        ),
        Some(ForcedCheckStatus::Failed(STABLE_404_TEXT.to_owned())),
        "a later forced failure replaces the earlier «Обновлений нет»"
    );
}

#[test]
fn forced_up_to_date_replaces_earlier_failure_text() {
    assert_eq!(
        forced_check_status_after(
            Some(ForcedCheckStatus::Failed(STABLE_404_TEXT.to_owned())),
            &crate::update::CheckOutcome::UpToDate,
            true
        ),
        Some(ForcedCheckStatus::NoUpdates),
        "a later forced success replaces the earlier error text"
    );
}

#[test]
fn forced_check_in_flight_shows_no_status_text() {
    let slot = Some(ForcedCheckStatus::NoUpdates);
    assert_eq!(
        forced_check_status_shown(slot.as_ref(), true),
        None,
        "while a FORCED check runs, the row shows no status text (the slot stays untouched until its outcome drains)"
    );
    assert_eq!(
        forced_check_status_shown(slot.as_ref(), false),
        Some(&ForcedCheckStatus::NoUpdates),
        "once the slot is free, the held line renders"
    );
    assert_eq!(
        forced_check_status_shown(None, false),
        None,
        "before the first forced check there is nothing to render"
    );
}

#[test]
fn discovery_cleared_by_forced_up_to_date_outcome() {
    assert!(
        discovery_cleared_by_check_outcome(
            &crate::update::CheckOutcome::UpToDate,
            true,
            &UpdateStatus::Idle
        ),
        "an up-to-date forced outcome means nothing newer exists — the held discovery must go"
    );
}

#[test]
fn discovery_cleared_by_forced_failure_after_held_nightly_discovery() {
    // The user's bug: nightly discovery held, channel switched to `stable`,
    // the forced check answers 404 — «Доступно обновление» must disappear.
    assert!(
        discovery_cleared_by_check_outcome(
            &crate::update::CheckOutcome::Failed(stable_404_error()),
            true,
            &UpdateStatus::Idle
        ),
        "a failed FORCED check replaces the previous result and clears the stale discovery"
    );
}

#[test]
fn discovery_kept_by_scheduled_failure_outcome() {
    assert!(
        !discovery_cleared_by_check_outcome(
            &crate::update::CheckOutcome::Failed(stable_404_error()),
            false,
            &UpdateStatus::Idle
        ),
        "a transient scheduled blip (network/rate-limit) must not erase an actionable update"
    );
}

#[test]
fn discovery_cleared_by_scheduled_up_to_date_outcome() {
    assert!(
        discovery_cleared_by_check_outcome(
            &crate::update::CheckOutcome::UpToDate,
            false,
            &UpdateStatus::Idle
        ),
        "a scheduled up-to-date outcome clears the held discovery too (§4.7)"
    );
}

#[test]
fn discovery_never_cleared_while_install_in_progress() {
    // A running install owns its discovery: no check outcome — forced,
    // scheduled, success or failure — may drop it mid-install.
    assert!(
        !discovery_cleared_by_check_outcome(
            &crate::update::CheckOutcome::UpToDate,
            true,
            &UpdateStatus::Installing
        ),
        "no outcome clears while an install runs"
    );
    assert!(
        !discovery_cleared_by_check_outcome(
            &crate::update::CheckOutcome::UpToDate,
            false,
            &UpdateStatus::Installing
        ),
        "not even a scheduled up-to-date outcome"
    );
    assert!(
        !discovery_cleared_by_check_outcome(
            &crate::update::CheckOutcome::Failed(stable_404_error()),
            true,
            &UpdateStatus::Installing
        ),
        "a forced failure must not drop the discovery a live install needs"
    );
    assert!(
        !discovery_cleared_by_check_outcome(
            &crate::update::CheckOutcome::Updated(sample_update_info()),
            true,
            &UpdateStatus::Idle
        ),
        "`Updated` never clears here — the drain REPLACES the discovery (install failure keeps / install success clear stay in `update_info_after_install`, pinned by `discovery_clears_after_successful_install`)"
    );
}

#[test]
fn forced_failure_resets_stale_discovery_and_hides_update_row() {
    // End-to-end decision chain the drain applies: held nightly discovery +
    // forced 404 on `stable` → clear → «Доступно обновление» hidden.
    let held = Some(sample_update_info());
    assert!(
        discovery_cleared_by_check_outcome(
            &crate::update::CheckOutcome::Failed(stable_404_error()),
            true,
            &UpdateStatus::Idle
        ),
        "fixture: the forced failure resets the held discovery"
    );
    let cleared = if discovery_cleared_by_check_outcome(
        &crate::update::CheckOutcome::Failed(stable_404_error()),
        true,
        &UpdateStatus::Idle,
    ) {
        None
    } else {
        held
    };
    assert!(
        !update_row_visible(cleared.as_ref()),
        "the reset slot must hide the «Доступно обновление» row"
    );
}

#[test]
fn forced_discovery_keeps_row_visible_and_clears_status_line() {
    // The preserved behavior: a forced success WITH an update shows the row;
    // the beside-button line steps aside for it (sub-item 2 slot rule).
    let outcome = crate::update::CheckOutcome::Updated(sample_update_info());
    assert!(
        !discovery_cleared_by_check_outcome(&outcome, true, &UpdateStatus::Idle),
        "a forced discovery must not clear — it replaces the held discovery"
    );
    assert!(
        update_row_visible(Some(&sample_update_info())),
        "a forced success-with-update keeps the row visible"
    );
    assert_eq!(
        forced_check_status_after(
            Some(ForcedCheckStatus::Failed(STABLE_404_TEXT.to_owned())),
            &outcome,
            true
        ),
        None,
        "the stale error line must go as soon as the discovery row appears"
    );
}
