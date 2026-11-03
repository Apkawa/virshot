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
#[path = "overlay.tests.rs"]
mod tests;
