//! Overlay toast semantics (spec §4.3): a transient, transparent, click-through
//! toast reporting capture results — `Saved: <file name>` on success, the typed
//! error message on failure.
//!
//! This module owns the toast *rules* only: notification kinds, the latest-only
//! mailbox, the fade-in → hold → fade-out lifecycle, visibility decisions, hold
//! durations, and toast text. Window creation, placement, and painting live in
//! `gui`. Time is injected as monotonic seconds by the caller (`gui` uses
//! `egui::Context::input(|i| i.time)`), so every rule here is unit-testable
//! without a display.
//!
//! Mailbox: `ToastMailbox` is a latest-only hand-off between the capture thread
//! and the GUI, the same pattern `hotkeys` uses for live settings — it carries
//! one owned value at a time and is never shared capture state. A headless
//! `virshot shot` passes a mailbox nobody polls; pushes then simply accumulate
//! and are dropped with the process.
//!
//! A new notification replaces the current toast (spec §4.3 "one toast at a
//! time"): the lifecycle restarts at fade-in.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use crate::config::Overlay;

/// Fade-in and fade-out length (spec §4.3: ~250 ms each way).
pub(crate) const FADE_MS: u64 = 250;

/// Hold length for an error toast, regardless of `overlay.duration_ms`
/// (spec §4.3: errors stay visible ~3000 ms).
pub(crate) const ERROR_HOLD_MS: u64 = 3000;

/// What the toast announces. `Success` carries the full saved path — the toast
/// shows only the file name (spec §4.3), the log keeps the full path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Notification {
    Success { path: PathBuf },
    Error { message: String },
}

/// Latest-only toast mailbox: capture pushes, the GUI polls and drains.
pub(crate) type ToastMailbox = Arc<Mutex<Option<Notification>>>;

/// A mailbox with no notification in it.
pub(crate) fn new_mailbox() -> ToastMailbox {
    Arc::new(Mutex::new(None))
}

/// Poisoned-lock recovery: a panicking pusher leaves the stored value intact,
/// so the toast hand-off keeps working (same policy as `hotkeys`).
fn lock_mailbox(mailbox: &ToastMailbox) -> std::sync::MutexGuard<'_, Option<Notification>> {
    mailbox
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Record a notification, replacing whatever the GUI has not polled yet.
pub(crate) fn push(mailbox: &ToastMailbox, notification: Notification) {
    *lock_mailbox(mailbox) = Some(notification);
}

/// Take the newest notification, leaving the mailbox empty.
pub(crate) fn take_latest(mailbox: &ToastMailbox) -> Option<Notification> {
    lock_mailbox(mailbox).take()
}

/// Whether the notification gets a toast (spec §4.3): errors always, success
/// only while `overlay.enabled`.
pub(crate) fn should_show(notification: &Notification, overlay: &Overlay) -> bool {
    match notification {
        Notification::Success { .. } => overlay.enabled,
        Notification::Error { .. } => true,
    }
}

/// How long the toast stays fully visible between the fades: errors use
/// [`ERROR_HOLD_MS`], success uses `overlay.duration_ms`.
pub(crate) fn hold_ms(notification: &Notification, overlay: &Overlay) -> u64 {
    match notification {
        Notification::Success { .. } => overlay.duration_ms,
        Notification::Error { .. } => ERROR_HOLD_MS,
    }
}

/// The toast text (English, spec §4.3): `Saved: <file name>` for success —
/// never the directory — and the error message verbatim.
pub(crate) fn toast_text(notification: &Notification) -> String {
    match notification {
        Notification::Success { path } => match path.file_name() {
            Some(name) => format!("Saved: {}", name.to_string_lossy()),
            None => format!("Saved: {}", path.display()),
        },
        Notification::Error { message } => message.clone(),
    }
}

/// Where the toast is in its lifecycle.
///
/// There is no fade-IN phase (spec §4.3 v6.0): the chip must be legible on its
/// very first frame. A fade-in ramp starts at alpha 0, and `Color32::gamma_multiply`
/// scales the alpha channel too, so an early paint of the (transparent) toast
/// window renders nothing at all — an empty window instead of a toast. The fade
/// is therefore applied only at the END of the lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToastPhase {
    Hold,
    FadeOut,
    Gone,
}

/// One toast's lifecycle: the notification, when it started (monotonic
/// seconds), and its hold length.
#[derive(Debug, Clone)]
pub(crate) struct ToastState {
    notification: Notification,
    started_at: f64,
    hold: f64,
}

impl ToastState {
    /// A toast starting at `now` (monotonic seconds).
    pub(crate) fn new(notification: Notification, overlay: &Overlay, now: f64) -> Self {
        let hold = hold_ms(&notification, overlay) as f64 / 1000.0;
        Self {
            notification,
            started_at: now,
            hold,
        }
    }

    /// A newer notification replaces this toast (spec §4.3): the lifecycle
    /// restarts at full opacity from `now`.
    pub(crate) fn replace(&mut self, notification: Notification, overlay: &Overlay, now: f64) {
        *self = Self::new(notification, overlay, now);
    }

    /// The current toast text.
    pub(crate) fn text(&self) -> String {
        toast_text(&self.notification)
    }

    /// Elapsed seconds since the toast started, never negative.
    fn elapsed(&self, now: f64) -> f64 {
        (now - self.started_at).max(0.0)
    }

    /// The phase at `now`: held for `hold` seconds, then faded out over
    /// `FADE_MS`. Boundaries are inclusive on the later phase.
    pub(crate) fn phase(&self, now: f64) -> ToastPhase {
        let fade = FADE_MS as f64 / 1000.0;
        let elapsed = self.elapsed(now);
        if elapsed < self.hold {
            ToastPhase::Hold
        } else if elapsed < self.hold + fade {
            ToastPhase::FadeOut
        } else {
            ToastPhase::Gone
        }
    }

    /// Opacity multiplier for the toast paint at `now`: 1 from the very first
    /// frame (spec §4.3 v6.0), 1 while holding, 1 → 0 while fading out, 0 once gone.
    pub(crate) fn alpha(&self, now: f64) -> f32 {
        let fade = FADE_MS as f64 / 1000.0;
        let elapsed = self.elapsed(now);
        match self.phase(now) {
            ToastPhase::Hold => 1.0,
            ToastPhase::FadeOut => ((1.0 - (elapsed - self.hold) / fade) as f32).clamp(0.0, 1.0),
            ToastPhase::Gone => 0.0,
        }
    }

    /// Whether the toast window can be closed.
    pub(crate) fn is_gone(&self, now: f64) -> bool {
        self.phase(now) == ToastPhase::Gone
    }
}

#[cfg(test)]
mod tests {
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
}
