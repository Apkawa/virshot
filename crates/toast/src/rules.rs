//! Toast lifecycle rules (ported from `virshot/src/overlay.rs`): the phase
//! machine `Hold → FadeOut → Gone`, hold lengths, fade alpha, and the next
//! scheduler deadline. Pure logic with an injected clock (`now` in monotonic
//! seconds) — no threads, no I/O, so every rule is unit-testable.
//!
//! Ported semantics:
//! - no fade-IN (spec §4.3 v6.0): the chip is legible on its very first frame,
//!   the fade ramps only at the END of the lifecycle;
//! - `Danger` holds [`ERROR_HOLD_MS`] regardless of `duration_ms`;
//! - `duration_ms == 0` is sticky: never `Gone` until an external dismiss;
//! - negative clock differences stay legible instead of jumping to `Gone`.
//!
//! During `FadeOut` the scheduler wakes every [`FADE_TICK_MS`] so a host loop
//! sees intermediate alpha values (the overlay backend paints the latest state
//! it was given and owns no timers of its own).

use std::time::Duration;

use crate::color::ToastColor;

/// Fade-out length, spec §4.3 (~250 ms), ported from `virshot/src/overlay.rs`.
pub(crate) const FADE_MS: u64 = 250;

/// Hold length for a `Danger` toast regardless of `duration_ms`, spec §4.3
/// (errors stay visible ~3000 ms), ported from `virshot/src/overlay.rs`.
pub(crate) const ERROR_HOLD_MS: u64 = 3000;

/// Repaint cadence while fading out: the overlay backend has no timers, so the
/// toast thread pushes intermediate alphas at this interval.
pub(crate) const FADE_TICK_MS: u64 = 25;

/// Where a toast is in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToastPhase {
    Hold,
    FadeOut,
    Gone,
}

/// One toast's lifecycle timing: color (decides the hold length), configured
/// duration, and the moment it started (monotonic seconds).
#[derive(Debug, Clone, Copy)]
pub(crate) struct ToastLifecycle {
    color: ToastColor,
    duration_ms: u32,
    started_at: f64,
}

impl ToastLifecycle {
    /// A toast starting at `now` (monotonic seconds).
    pub(crate) fn start(color: ToastColor, duration_ms: u32, started_at: f64) -> Self {
        Self {
            color,
            duration_ms,
            started_at,
        }
    }

    /// Fully-visible hold length in milliseconds: `Danger` always uses
    /// [`ERROR_HOLD_MS`], other colors use `duration_ms`. `None` = sticky
    /// (`duration_ms == 0` on a non-danger toast: never fades on its own).
    pub(crate) fn hold_ms(&self) -> Option<u64> {
        if self.color == ToastColor::Danger {
            return Some(ERROR_HOLD_MS);
        }
        if self.duration_ms == 0 {
            return None;
        }
        Some(u64::from(self.duration_ms))
    }

    /// Seconds since the toast started, never negative (a clock that moved
    /// backwards keeps the toast legible instead of fading it).
    fn elapsed(&self, now: f64) -> f64 {
        (now - self.started_at).max(0.0)
    }

    /// Hold seconds; `None` for a sticky toast.
    fn hold_seconds(&self) -> Option<f64> {
        self.hold_ms().map(|ms| ms as f64 / 1000.0)
    }

    /// The phase at `now`; boundaries are inclusive on the later phase, as in
    /// the ported `ToastState::phase`.
    pub(crate) fn phase(&self, now: f64) -> ToastPhase {
        let Some(hold) = self.hold_seconds() else {
            return ToastPhase::Hold;
        };
        let elapsed = self.elapsed(now);
        if elapsed < hold {
            ToastPhase::Hold
        } else if elapsed < hold + fade_seconds() {
            ToastPhase::FadeOut
        } else {
            ToastPhase::Gone
        }
    }

    /// Opacity multiplier at `now`: 1 from the first frame through the whole
    /// hold, 1 → 0 across the fade, 0 once gone.
    pub(crate) fn alpha(&self, now: f64) -> f32 {
        let Some(hold) = self.hold_seconds() else {
            return 1.0;
        };
        let elapsed = self.elapsed(now);
        match self.phase(now) {
            ToastPhase::Hold => 1.0,
            ToastPhase::FadeOut => {
                ((1.0 - (elapsed - hold) / fade_seconds()) as f32).clamp(0.0, 1.0)
            }
            ToastPhase::Gone => 0.0,
        }
    }

    /// When the timer thread should next wake: the end of the hold, one fade
    /// tick, or never (sticky / gone).
    pub(crate) fn next_deadline(&self, now: f64) -> Option<Duration> {
        let hold = self.hold_seconds()?;
        let fade = fade_seconds();
        let elapsed = self.elapsed(now);
        match self.phase(now) {
            // Wake when the hold ends; a clock jump backwards waits the full hold.
            ToastPhase::Hold => Some(Duration::from_secs_f64(hold - elapsed)),
            // Wake at the next fade tick, but never past the fade's end.
            ToastPhase::FadeOut => Some(Duration::from_secs_f64(
                (hold + fade - elapsed).min(FADE_TICK_MS as f64 / 1000.0),
            )),
            ToastPhase::Gone => None,
        }
    }
}

/// Fade length in seconds.
fn fade_seconds() -> f64 {
    FADE_MS as f64 / 1000.0
}

#[cfg(test)]
mod tests {
    use super::Duration;
    use super::ERROR_HOLD_MS;
    use super::FADE_MS;
    use super::FADE_TICK_MS;
    use super::ToastLifecycle;
    use super::ToastPhase;
    use crate::color::ToastColor;

    fn success(duration_ms: u32, started_at: f64) -> ToastLifecycle {
        ToastLifecycle::start(ToastColor::Success, duration_ms, started_at)
    }

    fn danger(duration_ms: u32, started_at: f64) -> ToastLifecycle {
        ToastLifecycle::start(ToastColor::Danger, duration_ms, started_at)
    }

    // ---------- lifecycle phases ----------

    #[test]
    fn toast_state_at_start_is_hold() {
        let state = success(2000, 100.0);
        assert_eq!(state.phase(100.0), ToastPhase::Hold);
        assert_eq!(
            state.phase(100.0 + FADE_MS as f64 / 1000.0),
            ToastPhase::Hold
        );
    }

    #[test]
    fn toast_state_after_hold_is_fade_out() {
        let state = success(2000, 100.0);
        assert_eq!(state.phase(100.0 + 2.0), ToastPhase::FadeOut);
        assert_eq!(
            state.phase(100.0 + 2.0 + FADE_MS as f64 / 1000.0 - 0.01),
            ToastPhase::FadeOut
        );
    }

    #[test]
    fn toast_state_after_fade_out_is_gone() {
        let state = success(2000, 100.0);
        let gone_at = 100.0 + 2.0 + FADE_MS as f64 / 1000.0;
        assert_eq!(state.phase(gone_at), ToastPhase::Gone);
        assert!(state.phase(gone_at) == ToastPhase::Gone);
        assert_eq!(state.phase(gone_at - 0.01), ToastPhase::FadeOut);
    }

    #[test]
    fn toast_state_negative_clock_difference_stays_legible() {
        let state = success(2000, 100.0);
        assert_eq!(state.phase(99.5), ToastPhase::Hold);
        assert_eq!(state.alpha(99.5), 1.0);
    }

    #[test]
    fn toast_replace_restarts_lifecycle_at_full_alpha() {
        let old = success(2000, 100.0);
        assert_eq!(old.phase(200.0), ToastPhase::Gone);
        // A newer toast replaces the old one: fresh timing from `now`.
        let fresh = danger(100, 200.0);
        assert_eq!(fresh.phase(200.0), ToastPhase::Hold);
        assert_eq!(fresh.alpha(200.0), 1.0);
    }

    #[test]
    fn toast_state_zero_duration_is_sticky_until_deadline_none() {
        let state = success(0, 100.0);
        assert_eq!(state.phase(100.0), ToastPhase::Hold);
        assert_eq!(state.phase(100.0 + 600.0), ToastPhase::Hold);
        assert_eq!(state.alpha(100.0 + 600.0), 1.0);
        assert_eq!(
            state.next_deadline(100.0),
            None,
            "sticky: no timer to wake on"
        );
    }

    // ---------- fade alpha ----------

    #[test]
    fn toast_alpha_falls_monotonically_during_fade_out() {
        let state = success(2000, 100.0);
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
    fn toast_state_no_fade_in_legible_first_frame() {
        let state = success(2000, 100.0);
        assert_eq!(state.phase(100.0), ToastPhase::Hold);
        assert_eq!(state.alpha(100.0), 1.0);
        let fade = FADE_MS as f64 / 1000.0;
        for elapsed in [0.0, fade / 2.0, fade, 1.9] {
            assert_eq!(state.alpha(100.0 + elapsed), 1.0, "legible at +{elapsed}s");
        }
    }

    // ---------- hold lengths ----------

    #[test]
    fn toast_hold_ms_danger_overrides_configured_duration() {
        assert_eq!(danger(100, 0.0).hold_ms(), Some(ERROR_HOLD_MS));
        assert_eq!(danger(0, 0.0).hold_ms(), Some(ERROR_HOLD_MS));
    }

    #[test]
    fn toast_hold_ms_others_use_duration() {
        assert_eq!(
            ToastLifecycle::start(ToastColor::Default, 750, 0.0).hold_ms(),
            Some(750)
        );
        assert_eq!(
            ToastLifecycle::start(ToastColor::Warning, 1500, 0.0).hold_ms(),
            Some(1500)
        );
        assert_eq!(success(999, 0.0).hold_ms(), Some(999));
        assert_eq!(
            ToastLifecycle::start(ToastColor::Default, 0, 0.0).hold_ms(),
            None,
            "duration 0 is sticky, not an instant fade"
        );
    }

    // ---------- scheduler deadlines ----------

    #[test]
    fn toast_deadline_in_hold_is_hold_remaining() {
        assert_eq!(
            success(2000, 100.0).next_deadline(100.0),
            Some(Duration::from_millis(2000))
        );
        assert_eq!(
            success(2000, 100.0).next_deadline(101.5),
            Some(Duration::from_millis(500))
        );
        assert_eq!(
            danger(100, 100.0).next_deadline(100.0),
            Some(Duration::from_millis(ERROR_HOLD_MS))
        );
    }

    #[test]
    fn toast_deadline_during_fade_out_is_at_most_a_fade_tick() {
        let state = success(2000, 100.0);
        let fade_out_start = 100.0 + 2.0;
        assert_eq!(
            state.next_deadline(fade_out_start),
            Some(Duration::from_millis(FADE_TICK_MS))
        );
        // Late in the fade the deadline is the fade's end, not the full tick.
        let late = fade_out_start + FADE_MS as f64 / 1000.0 - 0.01;
        assert!(state.next_deadline(late) < Some(Duration::from_millis(FADE_TICK_MS)));
    }

    #[test]
    fn toast_deadline_none_after_gone() {
        let state = success(2000, 100.0);
        let gone_at = 100.0 + 2.0 + FADE_MS as f64 / 1000.0;
        assert_eq!(state.next_deadline(gone_at), None);
    }
}
