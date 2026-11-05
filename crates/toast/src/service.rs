//! The non-blocking façade and the plain `std` timer thread behind it.
//!
//! `ToastHandle::show`/`dismiss` only push a command into an inbox and return —
//! they never touch overlay windows, never block on a handshake, and never touch
//! an event loop. The spawned thread owns the lifecycle: it waits for the next
//! deadline computed by `rules`, repaints the chip while fading, hides it when
//! gone, and hides everything on `Dismiss`/`Quit`.
//!
//! No async, no tokio, no winit here: the overlay crate owns the one
//! `EventLoop` per process, and hosts drive `OverlayHost` on their own loop. The
//! optional `egui::Context` wake hook exists so a host loop that sleeps between
//! frames still repaints during a fade.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc;
use std::sync::mpsc::RecvTimeoutError;
use std::thread::JoinHandle;
use std::time::Duration;
use std::time::Instant;

use overlay::MonitorRect;
use overlay::OverlayWindowSpec;

use crate::chip::chip_content;
use crate::chip::chip_size;
use crate::error::ToastError;
use crate::placement::placements;
use crate::rules::ToastLifecycle;
use crate::rules::ToastPhase;
use crate::sink::ToastSink;
use crate::toast::Toast;

/// A command for the toast thread.
enum ToastCommand {
    /// Show this toast, replacing whatever is live (spec §4.3: replacement, not
    /// a stack).
    Show(Toast),
    /// Hide the current toast immediately; a no-op when nothing is live.
    Dismiss,
    /// Hide everything and exit the thread.
    Quit,
}

/// Handle for the toast thread. Clones share the inbox; sending never blocks.
#[derive(Clone)]
pub struct ToastHandle {
    sender: Arc<Mutex<mpsc::Sender<ToastCommand>>>,
}

impl ToastHandle {
    /// Show a toast. Latest wins: a toast that is still live is hidden and
    /// replaced, and the replacement restarts at full alpha (spec §4.3).
    /// Returns `ToastError::ChannelClosed` when the service is gone.
    pub fn show(&self, toast: Toast) -> Result<(), ToastError> {
        self.send(ToastCommand::Show(toast))
    }

    /// Same semantics as [`ToastHandle::show`]: the toast module has one
    /// notification semantics — replacement, not a stack — so `update` is an
    /// alias kept for the API surface spec §4.3 names.
    pub fn update(&self, toast: Toast) -> Result<(), ToastError> {
        self.show(toast)
    }

    /// Hide the live toast immediately. No-op when nothing is live.
    ///
    /// Deviation from the stage-3 API sketch (`dismiss(id)`): the service keeps
    /// one logical toast per monitor set, so there is no id to address —
    /// dismissing means "hide the current toast".
    pub fn dismiss(&self) -> Result<(), ToastError> {
        self.send(ToastCommand::Dismiss)
    }

    /// Push a command; the lock is held for the send only.
    fn send(&self, command: ToastCommand) -> Result<(), ToastError> {
        let Ok(sender) = self.sender.lock() else {
            return Err(ToastError::ChannelClosed);
        };
        sender.send(command).map_err(|_| ToastError::ChannelClosed)
    }
}

/// The toast thread: spawned by [`ToastService::spawn`], joined on drop.
pub struct ToastService {
    sender: Arc<Mutex<mpsc::Sender<ToastCommand>>>,
    thread: Option<JoinHandle<()>>,
}

impl ToastService {
    /// Start the timer thread.
    ///
    /// * `sink` — the overlay backend to drive (`OverlayHandle`, `OverlayHostHandle`,
    ///   or a host-side wrapper).
    /// * `monitors` — placement targets: PHYSICAL desktop rects with each
    ///   monitor's DPI scale attached (`overlay::MonitorRect`). An empty list
    ///   makes every toast a logged no-op.
    /// * `wake` — an `egui::Context` to `request_repaint()` after the thread
    ///   touches a window, so a host loop driven by a sleeping event loop still
    ///   shows fade frames.
    pub fn spawn(
        sink: Arc<dyn ToastSink>,
        monitors: Vec<MonitorRect>,
        wake: Option<egui::Context>,
    ) -> Self {
        let (sender, inbox) = mpsc::channel::<ToastCommand>();
        let shared = Arc::new(Mutex::new(sender));
        let thread = std::thread::spawn(move || timer_thread(sink, monitors, wake, inbox));
        Self {
            sender: shared,
            thread: Some(thread),
        }
    }

    /// A handle to the running thread. Cheap to clone; usable from any thread.
    pub fn handle(&self) -> ToastHandle {
        ToastHandle {
            sender: Arc::clone(&self.sender),
        }
    }
}

impl Drop for ToastService {
    /// Asks the thread to hide its windows and exit, then joins it so no toast
    /// outlives the service.
    fn drop(&mut self) {
        if let Ok(sender) = self.sender.lock() {
            let _ = sender.send(ToastCommand::Quit);
        }
        let Some(thread) = self.thread.take() else {
            return;
        };
        if thread.join().is_err() {
            eprintln!("toast: timer thread panicked");
        }
    }
}

/// A live toast: its windows and its timing.
struct LiveToast {
    toast: Toast,
    ids: Vec<overlay::OverlayId>,
    lifecycle: ToastLifecycle,
}

/// What the thread woke on.
enum Wake {
    Command(ToastCommand),
    /// The deadline elapsed: step the lifecycle.
    Tick,
    /// The inbox is gone.
    Exit,
}

/// Wait for the next command, or for `deadline` to elapse (forever when there
/// is nothing to time — a sticky toast, or nothing live at all).
fn wait(inbox: &mpsc::Receiver<ToastCommand>, deadline: Option<Duration>) -> Wake {
    match deadline {
        Some(duration) => match inbox.recv_timeout(duration) {
            Ok(command) => Wake::Command(command),
            Err(RecvTimeoutError::Timeout) => Wake::Tick,
            Err(RecvTimeoutError::Disconnected) => Wake::Exit,
        },
        None => match inbox.recv() {
            Ok(command) => Wake::Command(command),
            Err(_) => Wake::Exit,
        },
    }
}

/// Monotonic seconds since thread start — the clock handed to the pure rules.
fn now(anchor: Instant) -> f64 {
    anchor.elapsed().as_secs_f64()
}

fn timer_thread(
    sink: Arc<dyn ToastSink>,
    monitors: Vec<MonitorRect>,
    wake: Option<egui::Context>,
    inbox: mpsc::Receiver<ToastCommand>,
) {
    let anchor = Instant::now();
    let mut current: Option<LiveToast> = None;

    loop {
        let deadline = current
            .as_ref()
            .and_then(|live| live.lifecycle.next_deadline(now(anchor)));
        match wait(&inbox, deadline) {
            Wake::Exit => break,
            Wake::Command(ToastCommand::Quit) => break,
            Wake::Command(ToastCommand::Show(toast)) => {
                hide_all(&sink, current.take());
                current = show(&sink, &monitors, toast, now(anchor));
                request_wake(&wake);
            }
            Wake::Command(ToastCommand::Dismiss) => {
                hide_all(&sink, current.take());
                request_wake(&wake);
            }
            Wake::Tick => {
                let Some(live) = current.as_mut() else {
                    continue;
                };
                let at = now(anchor);
                if live.lifecycle.phase(at) == ToastPhase::Gone {
                    hide_all(&sink, current.take());
                    request_wake(&wake);
                    continue;
                }
                let alpha = live.lifecycle.alpha(at);
                redraw_all(&sink, &live.ids, &live.toast, alpha);
                request_wake(&wake);
            }
        }
    }

    hide_all(&sink, current.take());
}

/// Spawn one chip per selected monitor. `None` when there is nowhere to put it.
fn show(
    sink: &Arc<dyn ToastSink>,
    monitors: &[MonitorRect],
    toast: Toast,
    at: f64,
) -> Option<LiveToast> {
    if monitors.is_empty() {
        eprintln!(
            "toast: no monitors configured, toast not shown: {}",
            toast.text
        );
        return None;
    }
    let size = chip_size(&toast);
    let mut ids = Vec::new();
    for monitor in placements(monitors, toast.monitor, toast.corner, size) {
        let spec = OverlayWindowSpec {
            rect: monitor.rect,
            // The monitor's own DPI scale: the host backend converts the
            // physical rect with it, the thread backend ignores it.
            scale: monitor.scale,
            // Toast chips are transparent-background, mouse-passthrough windows;
            // content protection is the overlay's capture-exclusion request.
            transparent: true,
            click_through: true,
            content_protected: true,
        };
        // `OverlayContent` is not cloneable (it carries a boxed painter), so the
        // chip is rebuilt per window — cheap for a text chip.
        match sink.spawn_window(spec, chip_content(&toast), 1.0) {
            Ok(id) => ids.push(id),
            Err(error) => eprintln!("toast: window spawn failed: {error}"),
        }
    }
    if ids.is_empty() {
        eprintln!(
            "toast: nowhere to place the chip, toast not shown: {}",
            toast.text
        );
        return None;
    }
    let lifecycle = ToastLifecycle::start(toast.color, toast.duration_ms, at);
    Some(LiveToast {
        toast,
        ids,
        lifecycle,
    })
}

/// Push the current fade alpha to every live window.
fn redraw_all(sink: &Arc<dyn ToastSink>, ids: &[overlay::OverlayId], toast: &Toast, alpha: f32) {
    for id in ids {
        if let Err(error) = sink.redraw(*id, chip_content(toast), alpha) {
            eprintln!("toast: window repaint failed ({id}): {error}");
        }
    }
}

/// Close every window of a toast (if there is one).
fn hide_all(sink: &Arc<dyn ToastSink>, live: Option<LiveToast>) {
    let Some(live) = live else {
        return;
    };
    for id in live.ids {
        if let Err(error) = sink.hide(id) {
            eprintln!("toast: window hide failed ({id}): {error}");
        }
    }
}

/// Ask the host loop for another frame so fade alpha actually reaches the screen.
fn request_wake(wake: &Option<egui::Context>) {
    if let Some(ctx) = wake {
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::ToastHandle;
    use super::ToastService;
    use crate::color::ToastColor;
    use crate::sink::ToastSink;
    use crate::toast::MonitorSel;
    use crate::toast::Toast;
    use overlay::Corner;
    use overlay::MonitorRect;
    use overlay::OverlayContent;
    use overlay::OverlayError;
    use overlay::OverlayHost;
    use overlay::OverlayHostHandle;
    use overlay::OverlayId;
    use overlay::OverlayWindowSpec;
    use overlay::Rect;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::time::Duration;
    use std::time::Instant;

    /// One recorded sink call — the observable side of the service.
    #[derive(Debug, Clone)]
    struct Record {
        kind: &'static str,
        id: OverlayId,
        rect: Rect,
        alpha: f32,
    }

    /// Records every sink call while delegating to the real host backend, which
    /// queues commands without painting: `OverlayHost::update` is never called
    /// here, so no window and no display server are involved.
    struct RecordingSink {
        inner: OverlayHostHandle,
        records: Arc<Mutex<Vec<Record>>>,
    }

    impl ToastSink for RecordingSink {
        fn spawn_window(
            &self,
            spec: OverlayWindowSpec,
            content: OverlayContent,
            alpha: f32,
        ) -> Result<OverlayId, OverlayError> {
            let id = self.inner.spawn_window(spec, content, alpha)?;
            self.records.lock().unwrap().push(Record {
                kind: "spawn",
                id,
                rect: spec.rect,
                alpha,
            });
            Ok(id)
        }

        fn redraw(
            &self,
            id: OverlayId,
            content: OverlayContent,
            alpha: f32,
        ) -> Result<(), OverlayError> {
            self.inner.redraw(id, content, alpha)?;
            self.records.lock().unwrap().push(Record {
                kind: "redraw",
                id,
                rect: Rect::new(0.0, 0.0, 0.0, 0.0),
                alpha,
            });
            Ok(())
        }

        fn hide(&self, id: OverlayId) -> Result<(), OverlayError> {
            self.inner.hide(id)?;
            self.records.lock().unwrap().push(Record {
                kind: "hide",
                id,
                rect: Rect::new(0.0, 0.0, 0.0, 0.0),
                alpha: 0.0,
            });
            Ok(())
        }
    }

    struct Harness {
        /// Keeps the host-side channel open; never updated, so nothing paints.
        _host: OverlayHost,
        sink: Arc<RecordingSink>,
        records: Arc<Mutex<Vec<Record>>>,
    }

    impl Harness {
        fn new() -> Self {
            let host = OverlayHost::new();
            let records = Arc::new(Mutex::new(Vec::new()));
            let sink = Arc::new(RecordingSink {
                inner: host.handle(),
                records: Arc::clone(&records),
            });
            Self {
                _host: host,
                sink,
                records,
            }
        }

        fn service(&self, monitors: Vec<MonitorRect>) -> ToastService {
            ToastService::spawn(Arc::clone(&self.sink) as Arc<dyn ToastSink>, monitors, None)
        }

        fn kinds(&self) -> Vec<&'static str> {
            self.records
                .lock()
                .unwrap()
                .iter()
                .map(|r| r.kind)
                .collect()
        }

        fn spawns(&self) -> Vec<Record> {
            self.records
                .lock()
                .unwrap()
                .iter()
                .filter(|r| r.kind == "spawn")
                .cloned()
                .collect()
        }

        /// Ids the sink saw for a given call kind, in call order.
        fn ids(&self, kind: &str) -> Vec<OverlayId> {
            self.records
                .lock()
                .unwrap()
                .iter()
                .filter(|r| r.kind == kind)
                .map(|r| r.id)
                .collect()
        }

        /// Waits until `count` records of `kind` were written (the timer thread
        /// runs concurrently, so assertions poll instead of sleeping once).
        fn await_count(&self, kind: &str, count: usize) {
            let deadline = Instant::now() + Duration::from_millis(1500);
            while self.kinds().iter().filter(|k| **k == kind).count() < count {
                if Instant::now() >= deadline {
                    panic!("expected {count} `{kind}` records, got {:?}", self.kinds());
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        fn count(&self, kind: &str) -> usize {
            self.kinds().iter().filter(|k| **k == kind).count()
        }
    }

    /// Two PHYSICAL monitors at 100% DPI, so the placement numbers these tests
    /// assert are the pixels the backend receives.
    fn monitors() -> Vec<MonitorRect> {
        vec![
            MonitorRect::new(Rect::new(0.0, 0.0, 1920.0, 1080.0), 1.0),
            MonitorRect::new(Rect::new(1920.0, 0.0, 1280.0, 720.0), 1.0),
        ]
    }

    /// The same desktop with the primary monitor at 150% DPI.
    fn scaled_monitors() -> Vec<MonitorRect> {
        vec![
            MonitorRect::new(Rect::new(0.0, 0.0, 1920.0, 1080.0), 1.5),
            MonitorRect::new(Rect::new(1920.0, 0.0, 1280.0, 720.0), 1.0),
        ]
    }

    fn toast(color: ToastColor, duration_ms: u32) -> Toast {
        Toast {
            monitor: MonitorSel::All,
            text: "Saved: shot-2.png".to_owned(),
            image: None,
            color,
            corner: Corner::BottomRight,
            duration_ms,
        }
    }

    // ---------- API shape ----------

    #[test]
    fn toast_handle_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ToastHandle>();
        assert_send_sync::<Arc<ToastHandle>>();
        // Handles stay usable after the service is dropped (send returns an error).
        let handle = {
            let harness = Harness::new();
            let service = harness.service(monitors());
            service.handle()
        };
        assert!(
            handle.dismiss().is_err(),
            "a dead service reports ChannelClosed"
        );
    }

    // ---------- commands ----------

    #[test]
    fn dismiss_without_current_is_noop() {
        let harness = Harness::new();
        let service = harness.service(monitors());
        let handle = service.handle();
        assert!(
            handle.dismiss().is_ok(),
            "nothing to dismiss is not an error"
        );
        assert!(harness.records.lock().unwrap().is_empty(), "no sink calls");
    }

    #[test]
    fn show_on_empty_monitors_is_noop() {
        let harness = Harness::new();
        let service = harness.service(Vec::new());
        let handle = service.handle();
        assert!(handle.show(toast(ToastColor::Success, 2000)).is_ok());
        std::thread::sleep(Duration::from_millis(150));
        assert!(
            harness.records.lock().unwrap().is_empty(),
            "no monitors means no windows, and the call still succeeds"
        );
    }

    #[test]
    fn show_spawns_one_chip_per_monitor_at_full_alpha() {
        let harness = Harness::new();
        let service = harness.service(monitors());
        let handle = service.handle();
        handle.show(toast(ToastColor::Success, 0)).unwrap();
        harness.await_count("spawn", 2);

        let spawns = harness.spawns();
        assert_eq!(spawns.len(), 2, "one chip per monitor");
        for record in &spawns {
            assert_eq!(record.alpha, 1.0, "legible from the first frame");
            assert!(record.rect.w > 0.0 && record.rect.h > 0.0);
        }
        assert_eq!(spawns[0].rect.x, 1920.0 - spawns[0].rect.w - 30.0);
        assert_eq!(spawns[1].rect.x, 1920.0 + 1280.0 - spawns[1].rect.w - 30.0);
    }

    #[test]
    fn a_scaled_monitor_gets_a_chip_scaled_to_its_dpi() {
        let harness = Harness::new();
        let service = harness.service(scaled_monitors());
        let handle = service.handle();
        handle.show(toast(ToastColor::Success, 0)).unwrap();
        harness.await_count("spawn", 2);

        let spawns = harness.spawns();
        assert_eq!(spawns.len(), 2, "one chip per monitor, still");
        // The 220x36 logical chip becomes 330x54 PHYSICAL px on the 150% monitor.
        assert_eq!(spawns[0].rect.w, 330.0);
        assert_eq!(spawns[0].rect.h, 54.0);
        assert_eq!(spawns[0].rect.x, 1920.0 - 330.0 - 30.0);
        assert_eq!(spawns[0].rect.y, 1080.0 - 54.0 - 60.0);
        // The 100% monitor keeps the chip at its design size.
        assert_eq!(spawns[1].rect.w, 220.0);
        assert_eq!(spawns[1].rect.h, 36.0);
    }

    #[test]
    fn dismiss_hides_live_chips_immediately() {
        let harness = Harness::new();
        let service = harness.service(monitors());
        let handle = service.handle();
        handle.show(toast(ToastColor::Success, 0)).unwrap();
        harness.await_count("spawn", 2);

        handle.dismiss().unwrap();
        harness.await_count("hide", 2);

        // Dismiss closes exactly the windows it spawned — no stray ids.
        let spawned: Vec<OverlayId> = harness.ids("spawn");
        let hidden: Vec<OverlayId> = harness.ids("hide");
        assert_eq!(hidden.len(), spawned.len(), "every live chip is hidden");
        assert!(hidden.iter().all(|id| spawned.contains(id)));
    }

    #[test]
    fn sticky_toast_never_hides_on_its_own() {
        let harness = Harness::new();
        let service = harness.service(monitors());
        let handle = service.handle();
        handle.show(toast(ToastColor::Default, 0)).unwrap();
        harness.await_count("spawn", 2);
        std::thread::sleep(Duration::from_millis(400));

        assert_eq!(
            harness.count("hide"),
            0,
            "duration 0 survives until dismissed"
        );
        assert_eq!(
            harness.count("redraw"),
            0,
            "a sticky chip has no fade ticks"
        );
    }

    #[test]
    fn short_duration_toast_fades_then_hides() {
        let harness = Harness::new();
        let service = harness.service(monitors());
        let handle = service.handle();
        handle.show(toast(ToastColor::Warning, 40)).unwrap();

        // Hold 40 ms + fade 250 ms: gone long before this await expires.
        harness.await_count("hide", 2);
        assert_eq!(harness.count("spawn"), 2);

        let faded: Vec<f32> = harness
            .records
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.kind == "redraw")
            .map(|r| r.alpha)
            .collect();
        assert!(
            faded.iter().any(|alpha| *alpha < 1.0 && *alpha > 0.0),
            "the fade must reach the host loop with intermediate alphas: {faded:?}"
        );
    }

    #[test]
    fn danger_toast_holds_error_hold_before_fading() {
        let harness = Harness::new();
        let service = harness.service(monitors());
        let handle = service.handle();
        handle.show(toast(ToastColor::Danger, 40)).unwrap();
        harness.await_count("spawn", 2);

        // Its configured 40 ms duration must not fade it early.
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(harness.count("hide"), 0, "errors stay up ~3000 ms");
        assert_eq!(harness.count("redraw"), 0, "still in the hold phase");
    }

    #[test]
    fn latest_wins_replace_hides_the_previous_chip() {
        let harness = Harness::new();
        let service = harness.service(monitors());
        let handle = service.handle();
        handle.show(toast(ToastColor::Success, 0)).unwrap();
        handle.show(toast(ToastColor::Danger, 0)).unwrap();

        harness.await_count("spawn", 4);
        harness.await_count("hide", 2);
        let records = harness.records.lock().unwrap().clone();
        let last_spawn = records
            .iter()
            .rev()
            .find(|r| r.kind == "spawn")
            .unwrap_or_else(|| panic!("a replacement must spawn a fresh chip"));
        assert_eq!(
            last_spawn.alpha, 1.0,
            "the replacement restarts at full alpha"
        );
        assert_eq!(records.iter().filter(|r| r.kind == "hide").count(), 2);

        // The hidden ids are the first toast's windows, not the replacement's.
        let spawned = harness.ids("spawn");
        assert_eq!(harness.ids("hide"), vec![spawned[0], spawned[1]]);
    }

    #[test]
    fn update_replaces_like_show() {
        let harness = Harness::new();
        let service = harness.service(monitors());
        let handle = service.handle();
        handle.update(toast(ToastColor::Default, 0)).unwrap();
        harness.await_count("spawn", 2);
        assert_eq!(harness.count("hide"), 0);
    }

    #[test]
    fn dropping_the_service_hides_live_chips() {
        let harness = Harness::new();
        let service = harness.service(monitors());
        let handle = service.handle();
        handle.show(toast(ToastColor::Success, 0)).unwrap();
        harness.await_count("spawn", 2);

        drop(service);

        assert_eq!(
            harness.count("hide"),
            2,
            "the toast thread hides its windows before it exits"
        );
    }
}
