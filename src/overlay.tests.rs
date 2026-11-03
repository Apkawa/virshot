use super::ERROR_HOLD_MS;
use super::FADE_MS;
use super::Notification;
use super::ToastPhase;
use super::ToastState;
use super::new_mailbox;
use super::push;
use super::should_show;
use super::take_latest;
use super::toast_text;

use crate::config::Overlay;
use crate::config::OverlayPosition;

use std::path::PathBuf;

fn overlay_with(enabled: bool, duration_ms: u64) -> Overlay {
    Overlay {
        enabled,
        position: OverlayPosition::BottomRight,
        duration_ms,
        on_all_monitors: true,
    }
}

fn success(path: &str) -> Notification {
    Notification::Success {
        path: PathBuf::from(path),
    }
}

fn failure(message: &str) -> Notification {
    Notification::Error {
        message: message.to_owned(),
    }
}

// ---------- lifecycle phases ----------

#[test]
fn toast_state_at_start_is_hold() {
    let state = ToastState::new(success("/tmp/shot.png"), &overlay_with(true, 2000), 100.0);
    assert_eq!(state.phase(100.0), ToastPhase::Hold);
    assert_eq!(
        state.phase(100.0 + FADE_MS as f64 / 1000.0),
        ToastPhase::Hold
    );
}

#[test]
fn toast_state_until_hold_end_is_hold() {
    let state = ToastState::new(success("/tmp/shot.png"), &overlay_with(true, 2000), 100.0);
    assert_eq!(state.phase(100.0 + 1.9), ToastPhase::Hold);
    assert_eq!(state.alpha(100.0 + 1.9), 1.0);
}

#[test]
fn toast_state_after_hold_is_fade_out() {
    let state = ToastState::new(success("/tmp/shot.png"), &overlay_with(true, 2000), 100.0);
    assert_eq!(state.phase(100.0 + 2.0), ToastPhase::FadeOut);
    assert_eq!(
        state.phase(100.0 + 2.0 + FADE_MS as f64 / 1000.0 - 0.01),
        ToastPhase::FadeOut
    );
}

#[test]
fn toast_state_after_fade_out_is_gone() {
    let state = ToastState::new(success("/tmp/shot.png"), &overlay_with(true, 2000), 100.0);
    let gone_at = 100.0 + 2.0 + FADE_MS as f64 / 1000.0;
    assert_eq!(state.phase(gone_at), ToastPhase::Gone);
    assert!(state.is_gone(gone_at));
    assert!(!state.is_gone(gone_at - 0.01));
}

#[test]
fn toast_state_negative_clock_difference_stays_legible() {
    let state = ToastState::new(success("/tmp/shot.png"), &overlay_with(true, 2000), 100.0);
    assert_eq!(state.phase(99.5), ToastPhase::Hold);
    assert_eq!(state.alpha(99.5), 1.0);
}

#[test]
fn toast_state_replace_restarts_lifecycle_at_full_alpha() {
    let mut state = ToastState::new(success("/tmp/old.png"), &overlay_with(true, 2000), 100.0);
    assert!(state.is_gone(200.0));
    state.replace(failure("capture failed"), &overlay_with(true, 2000), 200.0);
    assert_eq!(state.phase(200.0), ToastPhase::Hold);
    assert_eq!(state.alpha(200.0), 1.0);
    assert_eq!(state.text(), "capture failed");
}

// ---------- fade alpha ----------

/// Spec §4.3 v6.0/v6.1: the chip is legible on the spawn frame itself —
/// there is no fade-in phase and no ramp leaving the toast window painted
/// empty for its first hundred milliseconds.
#[test]
fn toast_state_no_fade_in_legible_first_frame() {
    let state = ToastState::new(success("/tmp/shot.png"), &overlay_with(true, 2000), 100.0);
    assert_eq!(state.phase(100.0), ToastPhase::Hold);
    assert_eq!(state.alpha(100.0), 1.0);
    // ...and stays legible for the whole hold.
    let fade = FADE_MS as f64 / 1000.0;
    for elapsed in [0.0, fade / 2.0, fade, 1.9] {
        assert_eq!(state.alpha(100.0 + elapsed), 1.0, "legible at +{elapsed}s");
    }
}

#[test]
fn toast_alpha_fades_out_at_end() {
    let fade = FADE_MS as f64 / 1000.0;
    let state = ToastState::new(success("/tmp/shot.png"), &overlay_with(true, 2000), 100.0);
    let fade_out_start = 100.0 + 2.0;
    assert_eq!(state.alpha(fade_out_start), 1.0);
    assert!(
        (state.alpha(fade_out_start + fade / 2.0) - 0.5).abs() < 1e-6,
        "mid-fade alpha should be halfway"
    );
    assert_eq!(state.alpha(fade_out_start + fade), 0.0);
    assert!(state.is_gone(fade_out_start + fade));
}

#[test]
fn alpha_is_full_during_hold() {
    let state = ToastState::new(success("/tmp/shot.png"), &overlay_with(true, 2000), 100.0);
    assert_eq!(state.alpha(101.0), 1.0);
    assert_eq!(state.alpha(101.9), 1.0);
}

#[test]
fn alpha_falls_monotonically_during_fade_out() {
    let state = ToastState::new(success("/tmp/shot.png"), &overlay_with(true, 2000), 100.0);
    let fade_out_start = 100.0 + 2.0;
    let samples = [
        fade_out_start,
        fade_out_start + 0.05,
        fade_out_start + 0.125,
        fade_out_start + 0.2,
        fade_out_start + FADE_MS as f64 / 1000.0,
    ];
    let alphas: Vec<f32> = samples.iter().map(|now| state.alpha(*now)).collect();
    assert_eq!(alphas.first(), Some(&1.0));
    assert_eq!(alphas.last(), Some(&0.0));
    for pair in alphas.windows(2) {
        assert!(pair[1] <= pair[0], "alpha must not increase: {alphas:?}");
    }
}

#[test]
fn alpha_is_zero_after_gone() {
    let state = ToastState::new(success("/tmp/shot.png"), &overlay_with(true, 2000), 100.0);
    assert_eq!(state.alpha(200.0), 0.0);
}

// ---------- durations and visibility decisions ----------

#[test]
fn hold_ms_success_uses_configured_duration() {
    assert_eq!(
        super::hold_ms(&success("/tmp/shot.png"), &overlay_with(true, 750)),
        750
    );
}

#[test]
fn hold_ms_error_overrides_configured_duration() {
    assert_eq!(
        super::hold_ms(&failure("boom"), &overlay_with(true, 100)),
        ERROR_HOLD_MS
    );
}

#[test]
fn hold_ms_error_ignores_disabled_overlay() {
    assert_eq!(
        super::hold_ms(&failure("boom"), &overlay_with(false, 100)),
        ERROR_HOLD_MS
    );
}

#[test]
fn should_show_success_hidden_when_overlay_disabled() {
    assert!(!should_show(
        &success("/tmp/shot.png"),
        &overlay_with(false, 2000)
    ));
}

#[test]
fn should_show_success_when_overlay_enabled() {
    assert!(should_show(
        &success("/tmp/shot.png"),
        &overlay_with(true, 2000)
    ));
}

#[test]
fn should_show_error_even_when_overlay_disabled() {
    assert!(should_show(&failure("boom"), &overlay_with(false, 2000)));
}

// ---------- toast text ----------

#[test]
fn toast_text_success_shows_file_name_only() {
    assert_eq!(
        toast_text(&success("/home/u/pics/shot-2.png")),
        "Saved: shot-2.png"
    );
}

#[test]
fn toast_text_success_falls_back_to_full_path_without_file_name() {
    assert_eq!(toast_text(&success("/")), "Saved: /");
}

#[test]
fn toast_text_error_is_message_verbatim() {
    assert_eq!(
        toast_text(&failure("failed to capture monitor: no monitors found")),
        "failed to capture monitor: no monitors found"
    );
}

// ---------- mailbox ----------

#[test]
fn mailbox_push_replaces_previous_notification() {
    let mailbox = new_mailbox();
    push(&mailbox, success("/tmp/a.png"));
    push(&mailbox, failure("boom"));
    assert_eq!(take_latest(&mailbox), Some(failure("boom")));
    assert_eq!(
        take_latest(&mailbox),
        None,
        "mailbox is drained after polling"
    );
}
